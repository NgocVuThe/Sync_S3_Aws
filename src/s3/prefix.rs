use aws_sdk_s3::Client;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

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
    local_path: &std::path::Path,
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
