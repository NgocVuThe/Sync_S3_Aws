use crate::*;
use tracing::{error, info};

pub fn setup_select_log_path_handler(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_select_log_path(move || {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            let path_str = path.to_string_lossy().to_string();

            // Validate that the path is writable
            let test_file = path.join(".s3sync_write_test");
            match std::fs::File::create(&test_file) {
                Ok(_) => {
                    // Clean up test file
                    let _ = std::fs::remove_file(&test_file);
                }
                Err(e) => {
                    error!("Log path is not writable: {:?}", e);
                    crate::utils::update_status(
                        &ui_handle,
                        format!("Thư mục log không có quyền ghi: {}", e),
                        0.0,
                        true,
                    );
                    return;
                }
            }

            // Save to config file
            let mut config = crate::config::load_config();
            config.log_path = path_str.clone();
            if let Err(e) = crate::config::save_config(&config) {
                error!("Failed to save config: {:?}", e);
                crate::utils::update_status(
                    &ui_handle,
                    format!("Lỗi lưu cấu hình: {}", e),
                    0.0,
                    true,
                );
            } else {
                info!("Config saved: log_path = {}", path_str);
                crate::utils::update_status(
                    &ui_handle,
                    "Đã lưu đường dẫn log".to_string(),
                    0.0,
                    false,
                );
            }

            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_log_path(path_str.into());
            });
        }
    });
}

pub fn setup_open_log_folder_handler(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_open_log_folder(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let log_path = ui.get_log_path().to_string();
            if !log_path.is_empty() {
                let ui_handle_cloned = ui_handle.clone();
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                    ui.set_is_opening_log(true);
                });

                let spawn_result;
                #[cfg(target_os = "windows")]
                {
                    spawn_result = std::process::Command::new("explorer")
                        .arg(&log_path)
                        .spawn();
                }
                #[cfg(target_os = "macos")]
                {
                    spawn_result = std::process::Command::new("open").arg(&log_path).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    spawn_result = std::process::Command::new("xdg-open")
                        .arg(&log_path)
                        .spawn();
                }

                // Reset button state immediately after spawn attempt
                let ui_handle_for_reset = ui_handle.clone();
                match spawn_result {
                    Ok(_) => {
                        info!("Opened log folder: {}", log_path);
                    }
                    Err(e) => {
                        error!("Failed to open log folder: {:?}", e);
                        crate::utils::update_status(
                            &ui_handle_for_reset,
                            format!("Không thể mở thư mục: {}", e),
                            0.0,
                            true,
                        );
                    }
                }

                // Reset is_opening_log immediately (no arbitrary delay)
                let _ = ui_handle_for_reset.upgrade_in_event_loop(|ui| {
                    ui.set_is_opening_log(false);
                });
            }
        }
    });
}
