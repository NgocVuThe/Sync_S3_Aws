use crate::*;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use chrono::{Local, Datelike};
use slint::Weak;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};
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
pub struct PrefixCache {
    pub prefixes: HashSet<String>,
    pub cache_time: std::time::Instant,
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
pub type GlobalPrefixCache = Arc<Mutex<HashMap<String, PrefixCache>>>;

/// Checks if a prefix (folder) exists in S3 bucket using cache.
pub async fn is_s3_prefix_exists_cached(
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
    
    // FIXED: Use configurable TTL from env var, default to 5 minutes
    let ttl_secs = std::env::var("S3_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let needs_refresh = cache_entry.is_none() || cache_entry.unwrap().is_expired(ttl_secs);

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

/// Normalizes a path for S3 use by filtering out system and user-specific directories.
pub fn normalize_path_parts(path: &std::path::Path) -> Vec<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized
        .split('/')
        .filter_map(|s| {
            let s = s.trim();
            let s_lower = s.to_lowercase();
            // Filter out drive letters, system folders, and common user folders
            if s.is_empty()
                || s.contains(':')
                || [
                    "users",
                    "home",
                    "desktop",
                    "documents",
                    "downloads",
                    "appdata",
                    "local",
                    "temp",
                    "admin",
                ]
                .contains(&s_lower.as_str())
            {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

/// Simple preview: usually takes last 2-3 folder levels to provide safe context.
pub fn get_preview_prefix(path: &std::path::Path) -> String {
    let parts = normalize_path_parts(path);
    if parts.is_empty() {
        return path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
    }

    // Take last 2-3 levels to provide enough context
    let n = parts.len();
    if n >= 3 {
        format!("{}/{}/{}", parts[n - 3], parts[n - 2], parts[n - 1])
    } else if n >= 2 {
        format!("{}/{}", parts[n - 2], parts[n - 1])
    } else {
        parts[0].clone()
    }
}

/// Robust prefix detection: uses normalized path, and expands/merges
/// based on actual S3 structure to prevent production path errors.
pub async fn find_best_s3_prefix(
    client: &Client,
    bucket: &str,
    local_path: &Path,
    cache: &GlobalPrefixCache,
) -> String {
    let default_prefix = get_preview_prefix(local_path);

    // Try to find a longer match on S3 if possible, with FIXED logic
    let normalized = local_path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s: &&str| !s.is_empty() && !s.contains(':')).collect();
    let n = parts.len();
    
    for i in 0..n {
        let candidate = parts[i..].join("/");

        if is_s3_prefix_exists_cached(client, bucket, &candidate, cache).await {
            // FIXED: Check if candidate is a PROPER prefix of default
if candidate.split('/').count() == 1 && default_prefix.contains('/')
                && !default_prefix.starts_with(&candidate) && !default_prefix.contains(&format!("{}/", candidate)) {
                    continue;
                }
            info!("Smart Match found on S3: '{}'", candidate);
            return candidate;
        }
    }

    info!("Using prefix: '{}'", default_prefix);
    default_prefix
}

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
        if let Some(ref log_file) = log_file_path {
            let end_time = Local::now();
            let status = if !has_error { "success" } else { "failed" };
            match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(mut file) => {
                    if writeln!(
                        file,
                        "Time Upload: {}, Bucket: {}, Status: {}",
                        end_time.format("%Y-%m-%d %H:%M:%S"),
                        bucket_name,
                        status
                    )
                    .is_err()
                        || writeln!(file, "--------------------------------------------------").is_err()
                    {
                        warn!("Failed to write sync completion to log file: {}", log_file);
                    }
                }
                Err(e) => {
                    warn!("Failed to open log file '{}': {}", log_file, e);
                }
            }
        }
    }

    Ok(())
}
