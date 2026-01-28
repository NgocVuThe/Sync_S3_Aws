use serde::{Deserialize, Serialize};
use tracing::warn;

const APP_NAME: &str = "S3SyncTool";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FilterConfig {
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default = "default_include_patterns")]
    pub include_patterns: Vec<String>,
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
    #[serde(default = "default_true")]
    pub enable_filtering: bool,
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "node_modules".to_string(),
        ".git".to_string(),
        "target".to_string(),
        ".vscode".to_string(),
        ".idea".to_string(),
        "dist".to_string(),
        "build".to_string(),
        "*.tmp".to_string(),
        "*.log".to_string(),
        ".DS_Store".to_string(),
        "Thumbs.db".to_string(),
    ]
}

fn default_include_patterns() -> Vec<String> {
    vec![
        "*.html".to_string(),
        "*.css".to_string(),
        "*.js".to_string(),
        "*.json".to_string(),
        "*.png".to_string(),
        "*.jpg".to_string(),
        "*.jpeg".to_string(),
        "*.gif".to_string(),
        "*.svg".to_string(),
        "*.ico".to_string(),
        "*.woff".to_string(),
        "*.woff2".to_string(),
        "*.ttf".to_string(),
        "*.otf".to_string(),
        "*.eot".to_string(),
    ]
}

fn default_max_file_size() -> u64 {
    100 * 1024 * 1024
}
fn default_true() -> bool {
    true
}

fn default_buckets() -> Vec<String> {
    vec![
        "ien-corp-dev-contents".to_string(),
        "i-ocean-global-stg-contents".to_string(),
        "i-ocean-global-prod-contents".to_string(),
        "ien-corp-prod-contents".to_string(),
    ]
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: default_exclude_patterns(),
            include_patterns: default_include_patterns(),
            max_file_size: default_max_file_size(),
            enable_filtering: default_true(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub log_path: String,
    #[serde(default)]
    pub s3_base_path: String,
    #[serde(default)]
    pub filter_config: FilterConfig,
    #[serde(default = "default_buckets")]
    pub buckets: Vec<String>,
    #[serde(default = "default_regions")]
    pub regions: Vec<String>,
    #[serde(default)]
    pub selected_bucket: String,
    #[serde(default = "default_region")]
    pub selected_region: String,
}

fn default_region() -> String {
    "ap-northeast-1".to_string()
}

fn default_regions() -> Vec<String> {
    vec![
        "ap-northeast-1".to_string(),
        "ap-southeast-1".to_string(),
        "us-east-1".to_string(),
        "us-west-2".to_string(),
        "eu-west-1".to_string(),
    ]
}

/// Load config from file. Returns default if file doesn't exist or is invalid.
pub fn load_config() -> AppConfig {
    match confy::load(APP_NAME, None) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(
                "Không thể load config (có thể file cũ/lỗi), sử dụng mặc định: {}",
                e
            );
            AppConfig::default()
        }
    }
}

/// Save config to file.
pub fn save_config(config: &AppConfig) -> Result<(), confy::ConfyError> {
    confy::store(APP_NAME, None, config)
}

/// Get the config file path for debugging purposes.
pub fn get_config_path() -> Option<std::path::PathBuf> {
    confy::get_configuration_file_path(APP_NAME, None).ok()
}
