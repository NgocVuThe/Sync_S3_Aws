use crate::*;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use slint::Weak;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::io::BufReader;
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
    acc_key: String,
    sec_key: String,
    sess_token: String,
    region: String,
    ui_handle: Weak<AppWindow>,
) -> Result<(), String> {
    update_status(&ui_handle, "Khởi tạo Sync...".to_string(), 0.0);

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

            let files = WalkDir::new(local_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| (e.path().to_path_buf(), base_path.clone(), s3_prefix.clone()));
            all_files.extend(files);
        }
    }

    let total_files = all_files.len();
    if total_files == 0 {
        update_status(&ui_handle, "Không có file nào để upload!".to_string(), 1.0);
        return Ok(());
    }

    // Group files by their base_path and prefix for AWS CLI sync
    use std::collections::HashMap;
    let mut sync_groups: HashMap<(PathBuf, String), Vec<PathBuf>> = HashMap::new();
    for (path, base_path, prefix) in all_files {
        sync_groups.entry((base_path, prefix)).or_insert(Vec::new()).push(path);
    }

    let mut set = JoinSet::new();
    let completed_count = Arc::new(tokio::sync::Mutex::new(0));
    let ui_handle_arc = Arc::new(ui_handle);

    for ((base_path, prefix), _files) in sync_groups {
        let bucket_name = bucket_name.clone();
        let completed_count = Arc::clone(&completed_count);
        let ui_handle = Arc::clone(&ui_handle_arc);

        let acc_key = acc_key.clone();
        let sec_key = sec_key.clone();
        let sess_token = sess_token.clone();
        let region = region.clone();

        set.spawn(async move {
            let s3_uri = format!("s3://{}/{}", bucket_name, prefix);
            let local_path_str = base_path.to_string_lossy();

            info!("Syncing {} to {}", local_path_str, s3_uri);

            let aws_path = if cfg!(windows) {
                r"C:\Program Files\Amazon\AWSCLIV2\aws.exe"
            } else {
                "aws"
            };
            let mut cmd = tokio::process::Command::new(aws_path);
            if base_path.is_file() {
                let mime_type = get_mime_type(&base_path);
                cmd.args(&[
                    "s3",
                    "cp",
                    &local_path_str,
                    &s3_uri,
                    "--content-type",
                    mime_type,
                ]);
            } else {
                cmd.args(&["s3", "sync", &local_path_str, &s3_uri, "--delete"]);
            }

            if !acc_key.is_empty() {
                cmd.env("AWS_ACCESS_KEY_ID", &acc_key);
            }
            if !sec_key.is_empty() {
                cmd.env("AWS_SECRET_ACCESS_KEY", &sec_key);
            }
            if !region.is_empty() {
                cmd.env("AWS_DEFAULT_REGION", &region);
            }
            if !sess_token.is_empty() {
                cmd.env("AWS_SESSION_TOKEN", &sess_token);
            }

            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            match cmd.spawn() {
                Ok(mut child) => {
                    let stdout = child.stdout.take().unwrap();
                    let stderr = child.stderr.take().unwrap();

                    let completed_count_clone = Arc::clone(&completed_count);
                    let ui_handle_clone = Arc::clone(&ui_handle);

                    tokio::spawn(async move {
                        use tokio::io::{AsyncBufReadExt, BufReader};
                        let mut stdout_reader = BufReader::new(stdout).lines();
                        while let Ok(Some(line)) = stdout_reader.next_line().await {
                            if line.starts_with("upload:") {
                                let mut count = completed_count_clone.lock().await;
                                *count += 1;
                                let progress = *count as f32 / total_files as f32;
                                update_status(
                                    &ui_handle_clone,
                                    format!("Đang upload: {} ({}/{})", line, *count, total_files),
                                    progress,
                                );
                            }
                        }
                    });

                    let stderr_handle = tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut stderr_buf = Vec::new();
                        let mut stderr_reader = BufReader::new(stderr);
                        let _ = stderr_reader.read_to_end(&mut stderr_buf).await;
                        String::from_utf8_lossy(&stderr_buf).to_string()
                    });

                    let status = child.wait().await;
                    let stderr_output = stderr_handle.await.unwrap_or_default();

                    match status {
                        Ok(exit_status) if exit_status.success() => {
                            info!("Synced {} successfully", local_path_str);
                            Ok(())
                        }
                        Ok(exit_status) => Err(format!("AWS CLI failed for {}: exit code {}, stderr: {}", local_path_str, exit_status, stderr_output)),
                        Err(e) => Err(format!("Failed to run AWS CLI for {}: {}", local_path_str, e)),
                    }
                }
                Err(e) => Err(format!("Failed to spawn AWS CLI for {}: {}", local_path_str, e)),
            }
        });
    }

    let mut has_error = false;
    while let Some(res) = set.join_next().await {
        if let Ok(Err(e)) = res {
            error!("{}", e);
            update_status(&ui_handle_arc, format!("Lỗi: {}", e), 0.0);
            has_error = true;
            set.abort_all();
            break;
        }
    }

    if !has_error {
        update_status(&ui_handle_arc, "Đồng bộ hoàn tất!".to_string(), 1.0);
    }

    Ok(())
}
