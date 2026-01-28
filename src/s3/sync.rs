use crate::*;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use chrono::{Local, Datelike};
use slint::Weak;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{PathBuf};
use std::sync::Arc;
use tokio::sync::{Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::utils::{get_mime_type, update_status};

/// Performs sync operation: uploads all files from the provided mappings to the S3 bucket.
pub async fn sync_to_s3(
    client: Arc<Client>,
    bucket_name: String,
    mappings: Vec<(String, String)>, // (local_path, s3_path)
    ui_handle: Weak<AppWindow>,
    log_path: String,
) -> Result<(), String> {
    update_status(&ui_handle, "Khởi tạo Sync...".to_string(), 0.0, false);

    let should_log = !log_path.is_empty();
    let start_time = Local::now();
    let mut log_mappings: Vec<String> = Vec::new();
    
    // Pre-compute log file path to avoid duplication
    let log_file_path = if should_log {
        Some(format!(
            "{}/sync_log_{:02}_{:02}_{}.log",
            log_path,
            start_time.day(),
            start_time.month(),
            start_time.year()
        ))
    } else {
        None
    };

    // Load filter config
    let filter_config = crate::config::load_config().filter_config;
    let mut all_files: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    let mut filtered_files = 0u64;
    
    for (local_path, s3_prefix) in mappings {
        let local_path_buf = PathBuf::from(&local_path);

        if local_path_buf.is_file() {
            if crate::utils::should_include_file(&local_path_buf, local_path_buf.parent().unwrap_or(&local_path_buf), &filter_config) {
                log_mappings.push(format!("File: {} -> S3: {}", local_path, s3_prefix));
                all_files.push((local_path_buf.clone(), local_path_buf.clone(), s3_prefix));
            } else {
                filtered_files += 1;
                info!("Filtered out file: {}", local_path);
            }
        } else {
            log_mappings.push(format!("Folder: {} -> S3 Folder: {}", local_path, s3_prefix));
            let files = WalkDir::new(&local_path_buf)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| {
                    let file_path = e.path().to_path_buf();
                    if crate::utils::should_include_file(&file_path, &local_path_buf, &filter_config) {
                        Some(e)
                    } else {
                        filtered_files += 1;
                        info!("Filtered out file: {}", file_path.display());
                        None
                    }
                })
                .map(|e| {
                    let file_path = e.path().to_path_buf();
                    let relative = file_path.strip_prefix(&local_path_buf).unwrap_or(&file_path);
                    let clean_rel = relative.to_string_lossy().replace('\\', "/");
                    let final_key = if clean_rel.is_empty() {
                        s3_prefix.clone()
                    } else {
                        format!("{}/{}", s3_prefix.trim_end_matches('/'), clean_rel.trim_start_matches('/'))
                    };
                    (file_path, local_path_buf.clone(), final_key)
                });
            all_files.extend(files);
        }
    }

    // Update status if files were filtered
    if filtered_files > 0 {
        update_status(
            &ui_handle,
            format!("Đã lọc {} files, chuẩn bị upload {} files...", filtered_files, all_files.len()),
            0.05,
            false,
        );
    }

    if should_log && !log_mappings.is_empty() {
        if let Some(ref log_file) = log_file_path {
            match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(mut file) => {
                    if writeln!(file, "--------------------------------------------------").is_err()
                        || writeln!(file, "Sync Session Started - Bucket: {}", bucket_name).is_err()
                    {
                        warn!("Failed to write sync session header to log file: {}", log_file);
                    }
                    for mapping in &log_mappings {
                        if writeln!(file, "{}", mapping).is_err() {
                            warn!("Failed to write mapping to log file: {}", log_file);
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open log file '{}': {}", log_file, e);
                }
            }
        }
    }

    let total_files = all_files.len();
    if total_files == 0 {
        update_status(&ui_handle, "Không có file nào để upload!".to_string(), 1.0, false);
        return Ok(());
    }

    let concurrency = std::env::var("S3_SYNC_CONCURRENCY")
        .unwrap_or_else(|_| "50".to_string())
        .parse()
        .unwrap_or(50);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut set = JoinSet::new();
    let completed_count = Arc::new(tokio::sync::Mutex::new(0));

    for (path, _base_path, key) in all_files {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let ui_handle = ui_handle.clone();
        let bucket_name = bucket_name.clone();
        let completed_count = Arc::clone(&completed_count);

        set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            info!("Map local file: {:?} -> S3 Key: {}", path, key);
            let display_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mime_type = get_mime_type(&path);

            match ByteStream::from_path(&path).await {
                Ok(stream) => {
                    match client
                        .put_object()
                        .bucket(&bucket_name)
                        .key(&key)
                        .content_type(mime_type)
                        .cache_control("no-cache")
                        .body(stream)
                        .send()
                        .await
                    {
                        Ok(_) => {
                            let mut count = completed_count.lock().await;
                            *count += 1;
                            let progress = *count as f32 / total_files as f32;
                            update_status(
                                &ui_handle,
                                format!(
                                    "Đang upload: {} ({}/{})",
                                    display_name, *count, total_files
                                ),
                                progress,
                                false,
                            );
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

    let mut failed_files: Vec<String> = Vec::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Err(e)) => {
                error!("{}", e);
                failed_files.push(e);
            }
            Err(e) => {
                let err_msg = format!("Task Join Error: {}", e);
                error!("{}", err_msg);
                failed_files.push(err_msg);
            }
            _ => {}
        }
    }

    let has_error = !failed_files.is_empty();
    let success_count = total_files - failed_files.len();

    if !has_error {
        update_status(&ui_handle, "Đồng bộ hoàn tất!".to_string(), 1.0, false);
    } else {
        update_status(
            &ui_handle,
            format!("Hoàn tất {}/{}, {} lỗi", success_count, total_files, failed_files.len()),
            1.0,
            true,
        );
    }

    if should_log {
        if let Some(ref log_file) = log_file_path {
            let end_time = Local::now();
            let status = if !has_error {
                "success".to_string()
            } else {
                format!("failed ({} errors)", failed_files.len())
            };
            match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(mut file) => {
                    // Summary line
                    let _ = writeln!(
                        file,
                        "Uploaded: {}/{} files | Time: {} | Bucket: {} | Status: {}",
                        success_count,
                        total_files,
                        end_time.format("%Y-%m-%d %H:%M:%S"),
                        bucket_name,
                        status
                    );

                    // Error details (if any)
                    if !failed_files.is_empty() {
                        let _ = writeln!(file, "Errors:");
                        for err in &failed_files {
                            let _ = writeln!(file, "  - {}", err);
                        }
                    }

                    let _ = writeln!(file, "--------------------------------------------------");
                }
                Err(e) => {
                    warn!("Failed to open log file '{}': {}", log_file, e);
                }
            }
        }
    }

    Ok(())
}
