use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use aws_sdk_s3::Client;
use slint::Weak;
use crate::AppWindow;
use crate::utils::update_status;
use tokio::io::AsyncWriteExt;
use std::path::Path;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn download_from_s3(
    client: Arc<Client>,
    bucket: String,
    prefix: String,
    local_dir: String,
    ui_handle: Weak<AppWindow>,
) -> Result<(), String> {
    update_status(&ui_handle, "Đang liệt kê file trên S3...".to_string(), 0.0, false);

    // Normalize prefix: S3 keys usually don't start with /
    let prefix = prefix.trim_start_matches('/').to_string();

    let mut objects = Vec::new();
    let mut continuation_token = None;

    loop {
        let mut list_request = client.list_objects_v2().bucket(&bucket).prefix(&prefix);
        if let Some(token) = continuation_token {
            list_request = list_request.continuation_token(token);
        }

        match list_request.send().await {
            Ok(output) => {
                if let Some(contents) = output.contents {
                    objects.extend(contents);
                }

                update_status(&ui_handle, format!("Đang liệt kê file trên S3... (Đã tìm thấy {} file)", objects.len()), 0.0, false);

                if output.is_truncated.unwrap_or(false) {
                    continuation_token = output.next_continuation_token;
                } else {
                    break;
                }
            }
            Err(e) => return Err(format!("Lỗi liệt kê S3: {}", e)),
        }
    }

    // Filter out folder markers and empty keys early
    let objects: Vec<_> = objects
        .into_iter()
        .filter(|obj| {
            let key = obj.key().unwrap_or("");
            !key.ends_with('/') && !key.is_empty()
        })
        .collect();

    let total_files = objects.len();
    if total_files == 0 {
        return Err("Không tìm thấy file nào để tải (đã lọc các thư mục trống).".to_string());
    }

    update_status(&ui_handle, format!("Đang chuẩn bị tải {} file...", total_files), 0.0, false);

    // Parallel download setup
    let concurrency = std::env::var("S3_SYNC_CONCURRENCY")
        .unwrap_or_else(|_| "50".to_string())
        .parse::<usize>()
        .unwrap_or(50);
    
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut join_set = JoinSet::new();
    let completed_count = Arc::new(AtomicUsize::new(0));

    for obj in objects {
        let key = obj.key().unwrap_or("").to_string();

        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let prefix = prefix.clone();
        let local_dir = local_dir.clone();
        let ui_handle = ui_handle.clone();
        let semaphore = Arc::clone(&semaphore);
        let completed_count = Arc::clone(&completed_count);
        let total_files = total_files;

        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            
            // Determine local path
            let relative_path = if !prefix.is_empty() {
                if key.starts_with(&prefix) {
                    key[prefix.len()..].trim_start_matches('/')
                } else {
                    &key
                }
            } else {
                &key
            };

            let full_local_path = Path::new(&local_dir).join(relative_path);

            // Ensure parent directory exists
            if let Some(parent) = full_local_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return Err(format!("Lỗi tạo thư mục cho {}: {}", key, e));
                }
            }

            // Update status before starting download
            {
                let current_count = completed_count.load(Ordering::Relaxed);
                let progress = (current_count as f32) / (total_files as f32);
                update_status(&ui_handle, format!("Đang tải: {} ({}/{})", key, current_count + 1, total_files), progress, false);
            }

            // Download file
            match client.get_object().bucket(&bucket).key(&key).send().await {
                Ok(output) => {
                    let mut file = tokio::fs::File::create(&full_local_path).await
                        .map_err(|e| format!("Lỗi tạo file {}: {}", key, e))?;
                    
                    let mut body = output.body;
                    while let Some(bytes) = body.next().await {
                        let bytes = bytes.map_err(|e| format!("Lỗi stream {}: {}", key, e))?;
                        file.write_all(&bytes).await.map_err(|e| format!("Lỗi ghi file {}: {}", key, e))?;
                    }
                }
                Err(e) => {
                    return Err(format!("Lỗi download {}: {}", key, e));
                }
            }

            let current_count = completed_count.fetch_add(1, Ordering::SeqCst) + 1;
            let progress = (current_count as f32) / (total_files as f32);
            update_status(&ui_handle, format!("Đã tải xong: {} ({}/{})", key, current_count, total_files), progress, false);
            
            Ok(())
        });
    }

    let mut errors = Vec::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Err(e)) => {
                tracing::error!("Download error: {}", e);
                errors.push(e);
            }
            Err(e) => {
                tracing::error!("Task panic: {}", e);
                errors.push(format!("Task panic: {}", e));
            }
            Ok(Ok(_)) => {}
        }
    }

    if !errors.is_empty() {
        update_status(&ui_handle, format!("Download hoàn tất với {} lỗi.", errors.len()), 1.0, true);
        return Err(format!("Có {} lỗi xảy ra trong quá trình download. Kiểm tra log để biết chi tiết.", errors.len()));
    }

    update_status(&ui_handle, "Download hoàn tất thành công!".to_string(), 1.0, false);
    Ok(())
}
