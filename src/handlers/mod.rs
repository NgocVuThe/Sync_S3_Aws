use crate::*;

pub mod auth;
pub mod file_picker;
pub mod sync;
pub mod log;
pub mod filter;
pub mod managers;

use self::auth::setup_test_access_handler;
use self::file_picker::{setup_select_folder_handler, setup_select_files_handler, setup_clear_folders_handler, setup_remove_folder_handler, setup_select_base_path_handler};
use self::sync::setup_start_sync_handler;
use self::log::{setup_select_log_path_handler, setup_open_log_folder_handler};
use self::filter::{setup_toggle_filter_config_handler, setup_save_filter_config_handler, setup_reset_filter_config_handler, setup_preview_filtering_handler};
use self::managers::{setup_bucket_handlers, setup_region_handlers};

/// Convenience function to set up all UI handlers.
pub fn setup_all_handlers(ui: &AppWindow) {
    setup_test_access_handler(ui);
    setup_select_folder_handler(ui);
    setup_select_files_handler(ui);
    setup_clear_folders_handler(ui);
    setup_remove_folder_handler(ui);
    setup_start_sync_handler(ui);
    setup_select_log_path_handler(ui);
    setup_open_log_folder_handler(ui);
    setup_select_base_path_handler(ui);
    setup_toggle_filter_config_handler(ui);
    setup_save_filter_config_handler(ui);
    setup_reset_filter_config_handler(ui);
    setup_preview_filtering_handler(ui);
    setup_bucket_handlers(ui);
    setup_region_handlers(ui);
}
