use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;
use tracing::{info, error, debug};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use tracing_appender;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use slint::Model;
slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize logging
    let file_appender = tracing_appender::rolling::never(".", "s3_debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(fmt::layer().with_writer(non_blocking))
        .with(fmt::layer())
        .init();

    info!("Ứng dụng S3 Sync Tool đang khởi động...");
    let ui = AppWindow::new()?;

    // Handle Test Access
    ui.on_test_access({
        let ui_handle = ui.as_weak();
        move |acc_key, sec_key, sess_token, region, bucket| {
            let bucket_name = bucket.to_string();
            let region_str = region.to_string();
            let credentials = Credentials::new(
                acc_key.to_string(),
                sec_key.to_string(),
                if sess_token.is_empty() { None } else { Some(sess_token.to_string()) },
                None, "manual"
            );

            let ui_handle_cloned = ui_handle.clone();
            tokio::spawn(async move {
                update_status(&ui_handle_cloned, "Đang kiểm tra kết nối...".to_string(), 0.1);
                let config = aws_config::from_env()
                    .credentials_provider(credentials)
                    .region(Region::new(region_str))
                    .load().await;
                let client = Client::new(&config);

                match client.head_bucket().bucket(&bucket_name).send().await {
                    Ok(_) => {
                        info!("Test Access thành công: {}", bucket_name);
                        let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| ui.set_show_config(false));
                        update_status(&ui_handle_cloned, "Kết nối thành công!".to_string(), 1.0);
                    }
                    Err(e) => {
                        error!("Test Access thất bại: {:?}", e);
                        update_status(&ui_handle_cloned, format!("Lỗi: {}", e), 0.0);
                    }
                }
            });
        }
    });

    // Handle Folder Selection
    ui.on_select_folder({
        let ui_handle = ui.as_weak();
        move || {
            if let Some(paths) = rfd::FileDialog::new().pick_folders() {
                let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                    let mut current_paths: Vec<slint::SharedString> = ui.get_local_paths().iter().map(|s: slint::SharedString| s).collect();
                    for p in paths {
                        current_paths.push(p.to_string_lossy().to_string().into());
                    }
                    let model = std::rc::Rc::new(slint::VecModel::from(current_paths));
                    ui.set_local_paths(slint::ModelRc::from(model));
                });
            }
        }
    });

    // Handle Clear Folders
    ui.on_clear_folders({
        let ui_handle = ui.as_weak();
        move || {
            let _ = ui_handle.upgrade_in_event_loop(|ui| {
                let model = std::rc::Rc::new(slint::VecModel::from(vec![]));
                ui.set_local_paths(slint::ModelRc::from(model));
            });
        }
    });

    // Handle Sync Action (Multi-folder Support)
    ui.on_start_sync({
        let ui_handle = ui.as_weak();
        move |acc_key, sec_key, sess_token, region, bucket, local_dirs| {
            let bucket_name = bucket.to_string();
            let region_str = region.to_string();
            let credentials = Credentials::new(
                acc_key.to_string(),
                sec_key.to_string(),
                if sess_token.is_empty() { None } else { Some(sess_token.to_string()) },
                None, "manual"
            );

            let folders: Vec<String> = local_dirs.iter().map(|s: slint::SharedString| s.to_string()).collect();
            let ui_handle_cloned = ui_handle.clone();
            
            tokio::spawn(async move {
                update_status(&ui_handle_cloned, "Khởi tạo Sync...".to_string(), 0.0);
                
                let config = aws_config::from_env()
                    .credentials_provider(credentials)
                    .region(Region::new(region_str))
                    .load().await;
                let client = Arc::new(Client::new(&config));

                let mut all_files = Vec::new();
                for local_path in &folders {
                    let base_path = PathBuf::from(local_path);
                    let folder_name = base_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    
                    let files: Vec<_> = WalkDir::new(local_path)
                        .into_iter().filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_file())
                        .map(|e| (e.path().to_path_buf(), base_path.clone(), folder_name.clone()))
                        .collect();
                    all_files.extend(files);
                }

                let total_files = all_files.len();
                if total_files == 0 {
                    update_status(&ui_handle_cloned, "Không có file nào để upload!".to_string(), 1.0);
                    return;
                }

                // Optimization: Concurrent Uploads
                let semaphore = Arc::new(Semaphore::new(10)); // Max 10 concurrent uploads
                let mut set = JoinSet::new();
                let completed_count = Arc::new(tokio::sync::Mutex::new(0));

                for (path, base_path, folder_name) in all_files {
                    let client = Arc::clone(&client);
                    let semaphore = Arc::clone(&semaphore);
                    let ui_handle = ui_handle_cloned.clone();
                    let bucket_name = bucket_name.clone();
                    let completed_count = Arc::clone(&completed_count);

                    set.spawn(async move {
                        let _permit = semaphore.acquire().await.unwrap();
                        let relative_path = path.strip_prefix(&base_path).unwrap_or(&path);
                        let normalized_path = relative_path.to_string_lossy().replace('\\', "/");
                        let key = format!("{}/{}", folder_name, normalized_path);
                        let mime_type = get_mime_type(&path);

                        match ByteStream::from_path(&path).await {
                            Ok(stream) => {
                                match client.put_object()
                                    .bucket(&bucket_name)
                                    .key(&key)
                                    .content_type(mime_type)
                                    .body(stream)
                                    .send().await 
                                {
                                    Ok(_) => {
                                        let mut count = completed_count.lock().await;
                                        *count += 1;
                                        let progress = *count as f32 / total_files as f32;
                                        update_status(&ui_handle, format!("Đang upload: {} ({}/{})", normalized_path, *count, total_files), progress);
                                        debug!("Uploaded: {}", key);
                                        Ok(())
                                    }
                                    Err(e) => Err(format!("Lỗi upload {}: {}", key, e)),
                                }
                            }
                            Err(e) => Err(format!("Lỗi mở file {}: {}", path.display(), e)),
                        }
                    });
                }

                // Wait for all uploads to complete
                let mut has_error = false;
                while let Some(res) = set.join_next().await {
                    if let Ok(Err(e)) = res {
                        error!("{}", e);
                        update_status(&ui_handle_cloned, format!("Lỗi: {}", e), 0.0);
                        has_error = true;
                        set.abort_all();
                        break;
                    }
                }

                if !has_error {
                    info!("Đồng bộ hoàn tất {} file.", total_files);
                    update_status(&ui_handle_cloned, "Đồng bộ hoàn tất!".to_string(), 1.0);
                }
            });
        }
    });

    ui.run()?;
    Ok(())
}

fn get_mime_type(path: &Path) -> &'static str {
    let extension = path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "css" => "text/css",
        "js" => "application/javascript",
        "html" | "htm" => "text/html",
        _ => mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream"),
    }
}

fn update_status(ui_handle: &slint::Weak<AppWindow>, text: String, progress: f32) {
    let _ = ui_handle.upgrade_in_event_loop(move |ui| {
        ui.set_status_text(text.into());
        ui.set_progress(progress);
    });
}
