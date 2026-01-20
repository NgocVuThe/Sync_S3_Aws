use crate::*;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use slint::Weak;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, error, info};
use walkdir::WalkDir;

use crate::utils::{get_mime_type, update_status};

/// Creates an S3 client with the provided credentials and region.
pub async fn create_s3_client(
    acc_key: String,
    sec_key: String,
    sess_token: Option<String>,
    region: String,
) -> Result<Client, aws_sdk_s3::Error> {
    let credentials = Credentials::new(acc_key, sec_key, sess_token, None, "manual");
    let config = aws_config::from_env()
        .credentials_provider(credentials)
        .region(Region::new(region))
        .load()
        .await;
    Ok(Client::new(&config))
}

/// Tests access to the S3 bucket by attempting to head the bucket.
pub async fn test_bucket_access(client: &Client, bucket: &str) -> Result<(), aws_sdk_s3::Error> {
    client.head_bucket().bucket(bucket).send().await?;
    Ok(())
}

/// Performs the sync operation: uploads all files from the provided local paths to the S3 bucket.
/// Supports both files and folders, with concurrent uploads limited by a semaphore.
pub async fn sync_to_s3(
    client: Arc<Client>,
    bucket_name: String,
    local_paths: Vec<String>,
    ui_handle: Weak<AppWindow>,
) -> Result<(), String> {
    update_status(&ui_handle, "Khởi tạo Sync...".to_string(), 0.0);

    // Collect all files to upload
    let mut all_files: Vec<(PathBuf, Option<(PathBuf, String)>)> = Vec::new();
    for local_path in &local_paths {
        let base_path = PathBuf::from(local_path);

        if base_path.is_file() {
            all_files.push((base_path, None));
        } else {
            let folder_name = base_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let files: Vec<_> = WalkDir::new(local_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| {
                    (
                        e.path().to_path_buf(),
                        Some((base_path.clone(), folder_name.clone())),
                    )
                })
                .collect();
            all_files.extend(files);
        }
    }

    let total_files = all_files.len();
    if total_files == 0 {
        update_status(&ui_handle, "Không có file nào để upload!".to_string(), 1.0);
        return Ok(());
    }

    // Concurrent uploads (configurable via S3_SYNC_CONCURRENCY env var, default 10)
    let concurrency = std::env::var("S3_SYNC_CONCURRENCY")
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut set = JoinSet::new();
    let completed_count = Arc::new(tokio::sync::Mutex::new(0));

    for (path, folder_info) in all_files {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let ui_handle = ui_handle.clone();
        let bucket_name = bucket_name.clone();
        let completed_count = Arc::clone(&completed_count);

        set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let key = if let Some((base_path, folder_name)) = folder_info {
                let relative_path = path.strip_prefix(&base_path).unwrap_or(&path);
                // Ensure forward slashes for S3
                let normalized_path = relative_path.to_string_lossy().replace('\\', "/");
                // Remove leading slash if present in relative path to avoid double slashes//
                let clean_path = normalized_path.trim_start_matches('/');
                format!("{}/{}", folder_name, clean_path)
            } else {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            };

            // Log mapping for user visibility
            info!("Map local file: {:?} -> S3 Key: {}", path, key);
            let normalized_path = path
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
                                    normalized_path, *count, total_files
                                ),
                                progress,
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

    // Wait for completion
    let mut has_error = false;
    while let Some(res) = set.join_next().await {
        if let Ok(Err(e)) = res {
            error!("{}", e);
            update_status(&ui_handle, format!("Lỗi: {}", e), 0.0);
            has_error = true;
            set.abort_all();
            break;
        }
    }

    if !has_error {
        update_status(&ui_handle, "Đồng bộ hoàn tất!".to_string(), 1.0);
    }

    Ok(())
}
