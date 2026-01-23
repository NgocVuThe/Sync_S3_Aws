use serde::{Deserialize, Serialize};

const APP_NAME: &str = "S3SyncTool";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub log_path: String,
}

/// Load config from file. Returns default if file doesn't exist or is invalid.
pub fn load_config() -> AppConfig {
    confy::load(APP_NAME, None).unwrap_or_default()
}

/// Save config to file.
pub fn save_config(config: &AppConfig) -> Result<(), confy::ConfyError> {
    confy::store(APP_NAME, None, config)
}

/// Get the config file path for debugging purposes.
pub fn get_config_path() -> Option<std::path::PathBuf> {
    confy::get_configuration_file_path(APP_NAME, None).ok()
}
