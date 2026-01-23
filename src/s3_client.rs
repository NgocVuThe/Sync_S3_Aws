use crate::*;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use chrono::{Local, Datelike};
use slint::Weak;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, error, info};
use walkdir::WalkDir;

use crate::utils::{get_mime_type, update_status};

/// Creates an S3 client with provided credentials and region.
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

/// Tests access to S3 bucket by attempting to head the bucket.
pub async fn test_bucket_access(client: &Client, bucket: &str) -> Result<(), aws_sdk_s3::Error> {
    client.head_bucket().bucket(bucket).send().await?;
    Ok(())
}

/// Cache structure for S3 prefix lookups to avoid redundant requests
struct PrefixCache {
    prefixes: HashSet<String>,
    cache_time: std::time::Instant,
}

impl PrefixCache {
    fn new() -> Self {
        Self {
            prefixes: HashSet::new(),
            cache_time: std::time::Instant::now(),
        }
    }

    fn is_expired(&self, ttl_secs: u64) -> bool {
        self.cache_time.elapsed().as_secs() > ttl_secs
    }
}

/// Global cache for S3 prefixes per bucket
type GlobalPrefixCache = Arc<Mutex<HashMap<String, PrefixCache>>>;

/// Checks if a prefix (folder) exists in S3 bucket using cache.
async fn is_s3_prefix_exists_cached(
    client: &Client,
    bucket: &str,
    prefix: &str,
    cache: &GlobalPrefixCache,
) -> bool {
    let prefix_normalized = if prefix.ends_with('/') || prefix.is_empty() {
        prefix.to_string()
    } else {
        format!("{}/", prefix)
    };

    let mut cache_guard = cache.lock().await;

    let cache_entry = cache_guard.get(bucket);
    let needs_refresh = cache_entry.is_none() || cache_entry.unwrap().is_expired(300);

    if needs_refresh {
        if let Ok(resp) = client
            .list_objects_v2()
            .bucket(bucket)
            .delimiter("/")
            .max_keys(1000)
            .send()
            .await
        {
            let mut new_cache = PrefixCache::new();
            for cp in resp.common_prefixes() {
                if let Some(prefix) = cp.prefix() {
                    new_cache.prefixes.insert(
                        prefix
                            .trim_end_matches('/')
                            .trim_start_matches('/')
                            .to_string(),
                    );
                }
            }
            for obj in resp.contents() {
                if let Some(key) = obj.key() {
                    if let Some((parent, _)) = key.rsplit_once('/') {
                        new_cache.prefixes.insert(
                            parent
                                .trim_end_matches('/')
                                .trim_start_matches('/')
                                .to_string(),
                        );
                    }
                }
            }
            cache_guard.insert(bucket.to_string(), new_cache);
        }
    }

    if let Some(entry) = cache_guard.get(bucket) {
        let trimmed = prefix_normalized.trim_end_matches('/');
        return entry.prefixes.contains(trimmed);
    }

    false
}

/// Finds best S3 prefix for a local folder by matching its hierarchy.
/// Uses cached S3 data for better performance.
async fn find_best_s3_prefix(
    client: &Client,
    bucket: &str,
    local_path: &PathBuf,
    cache: &GlobalPrefixCache,
) -> String {
    let folder_name = local_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let normalized = local_path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter_map(|s| {
            let s = s.trim();
            if !s.is_empty() && !s.contains(':') {
                Some(s)
            } else {
                None
            }
        })
        .collect();

    if parts.is_empty() {
        return folder_name;
    }

    let n = parts.len();
    for i in 0..n {
        let candidate = parts[i..].join("/");
        debug!("Checking S3 prefix candidate: '{}'", candidate);
        if is_s3_prefix_exists_cached(client, bucket, &candidate, cache).await {
            info!("Match found! Using S3 prefix: '{}'", candidate);
            return candidate;
        }
    }

    info!(
        "No matching prefix found on S3. Defaulting to folder name: '{}'",
        folder_name
    );
    folder_name
}

/// Performs sync operation: uploads all files from the provided local paths to the S3 bucket.
/// Supports both files and folders, with concurrent uploads limited by a semaphore.
pub async fn sync_to_s3(
    client: Arc<Client>,
    bucket_name: String,
    local_paths: Vec<String>,
    ui_handle: Weak<AppWindow>,
    log_path: String,
) -> Result<(), String> {
    update_status(&ui_handle, "Khởi tạo Sync...".to_string(), 0.0, false);

    let should_log = !log_path.is_empty() && !cfg!(debug_assertions);
    let start_time = Local::now();
    let mut mappings: Vec<String> = Vec::new();

    let prefix_cache: GlobalPrefixCache = Arc::new(Mutex::new(HashMap::new()));
    let mut all_files: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for local_path in &local_paths {
        let base_path = PathBuf::from(local_path);

        if base_path.is_file() {
            let file_name = base_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            all_files.push((base_path.clone(), base_path.clone(), file_name));
        } else {
            let s3_prefix =
                find_best_s3_prefix(&client, &bucket_name, &base_path, &prefix_cache).await;
            info!(
                "Smart Prefix Detection: Found '{}' for local path {:?}",
                s3_prefix, base_path
            );
            mappings.push(format!("Local: {} -> S3 Folder: {}", local_path, s3_prefix));

            let files = WalkDir::new(local_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| (e.path().to_path_buf(), base_path.clone(), s3_prefix.clone()));
            all_files.extend(files);
        }
    }

    if should_log && !mappings.is_empty() {
        let log_file = format!("{}/sync_log_{:02}_{:02}_{}.log", log_path, start_time.day(), start_time.month(), start_time.year());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_file) {
            for mapping in &mappings {
                let _ = writeln!(file, "{}", mapping);
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

    for (path, base_path, prefix) in all_files {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let ui_handle = ui_handle.clone();
        let bucket_name = bucket_name.clone();
        let completed_count = Arc::clone(&completed_count);

        set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let key = if path == base_path {
                prefix
            } else {
                let relative_path = path.strip_prefix(&base_path).unwrap_or(&path);
                let clean_path = relative_path.to_string_lossy().replace('\\', "/");
                let clean_path = clean_path.trim_start_matches('/');
                format!("{}/{}", prefix, clean_path)
            };

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

    let mut has_error = false;
    while let Some(res) = set.join_next().await {
        if let Ok(Err(e)) = res {
            error!("{}", e);
            update_status(&ui_handle, format!("Lỗi: {}", e), 0.0, true);
            has_error = true;
            set.abort_all();
            break;
        }
    }

    if !has_error {
        update_status(&ui_handle, "Đồng bộ hoàn tất!".to_string(), 1.0, false);
    }

    if should_log {
        let end_time = Local::now();
        let status = if !has_error { "success" } else { "failed" };
        let log_file = format!("{}/sync_log_{:02}_{:02}_{}.log", log_path, start_time.day(), start_time.month(), start_time.year());
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_file) {
            let _ = writeln!(file, "Time Upload: {}, Status: {}", end_time.format("%Y-%m-%d %H:%M:%S"), status);
            let _ = writeln!(file, "--------------------------------------------------");
        }
    }

    Ok(())
}
