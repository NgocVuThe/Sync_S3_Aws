use crate::*;
use slint::Model;
use tracing::{error, info};

/// Sets up the filter configuration toggle handler.
pub fn setup_toggle_filter_config_handler(ui: &AppWindow) {
    ui.on_toggle_filter_config({
        let ui_handle = ui.as_weak();
        move || {
            let _ = ui_handle.upgrade_in_event_loop(|ui| {
                ui.set_show_filter_config(!ui.get_show_filter_config());
            });
        }
    });
}

/// Sets up the save filter configuration handler.
pub fn setup_save_filter_config_handler(ui: &AppWindow) {
    ui.on_save_filter_config({
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            // Get current values from UI
            let enable_filtering = ui.get_enable_filtering();
            let exclude_patterns_text = ui.get_exclude_patterns_text().to_string();
            let include_patterns_text = ui.get_include_patterns_text().to_string();
            let max_file_size_text = ui.get_max_file_size_text().to_string();

            // Parse and validate max file size
            let max_file_size_mb = match max_file_size_text.parse::<u64>() {
                Ok(val) if val > 0 && val <= 10240 => val, // Giới hạn từ 1MB đến 10GB
                _ => {
                    crate::utils::update_status(&ui_handle, "Max file size phải là số từ 1 đến 10240 MB".to_string(), 0.0, true);
                    return;
                }
            };
            let max_file_size = max_file_size_mb.saturating_mul(1024 * 1024);

            // Validate exclude patterns
            let invalid_exclude = crate::utils::validate_glob_patterns(&exclude_patterns_text);
            if !invalid_exclude.is_empty() {
                crate::utils::update_status(&ui_handle, format!("Pattern không hợp lệ trong Exclude: {}", invalid_exclude.join(", ")), 0.0, true);
                return;
            }

            // Validate include patterns
            let invalid_include = crate::utils::validate_glob_patterns(&include_patterns_text);
            if !invalid_include.is_empty() {
                crate::utils::update_status(&ui_handle, format!("Pattern không hợp lệ trong Include: {}", invalid_include.join(", ")), 0.0, true);
                return;
            }

            // Parse patterns (comma-separated)
            let exclude_patterns: Vec<String> = exclude_patterns_text
                .split(',')
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();

            let include_patterns: Vec<String> = include_patterns_text
                .split(',')
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();

            // Create new filter config
            let filter_config = crate::config::FilterConfig {
                enable_filtering,
                exclude_patterns,
                include_patterns,
                max_file_size,
            };

            // Save to config
            let mut app_config = crate::config::load_config();
            app_config.filter_config = filter_config.clone();
            
            if let Err(e) = crate::config::save_config(&app_config) {
                error!("Failed to save filter config: {:?}", e);
                crate::utils::update_status(&ui_handle, format!("Lỗi lưu cấu hình lọc: {}", e), 0.0, true);
            } else {
                info!("Filter config saved successfully");
                crate::utils::update_status(&ui_handle, "Đã lưu cấu hình lọc file".to_string(), 0.0, false);
                
                // Hide config section after successful save
                let ui_handle_clone = ui_handle.clone();
                let _ = ui_handle_clone.upgrade_in_event_loop(|ui| {
                    ui.set_show_filter_config(false);
                });
            }
        }
    });
}

/// Sets up the reset filter configuration handler.
pub fn setup_reset_filter_config_handler(ui: &AppWindow) {
    ui.on_reset_filter_config({
        let ui_handle = ui.as_weak();
        move || {
            let default_config = crate::config::FilterConfig::default();
            let exclude_text = default_config.exclude_patterns.join(", ");
            let include_text = default_config.include_patterns.join(", ");
            let max_size_text = (default_config.max_file_size / (1024 * 1024)).to_string();
            let enable_filtering = default_config.enable_filtering;
            
            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_enable_filtering(enable_filtering);
                ui.set_exclude_patterns_text(exclude_text.into());
                ui.set_include_patterns_text(include_text.into());
                ui.set_max_file_size_text(max_size_text.into());
                ui.set_filter_stats("".into());
            });
            
            crate::utils::update_status(&ui_handle, "Đã reset cấu hình lọc file".to_string(), 0.0, false);
        }
    });
}

/// Sets up the preview filtering handler.
pub fn setup_preview_filtering_handler(ui: &AppWindow) {
    ui.on_preview_filtering({
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            // Get current paths
            let local_paths: Vec<crate::PathItem> = ui.get_local_paths().iter().collect();
            
            if local_paths.is_empty() {
                crate::utils::update_status(&ui_handle, "Vui lòng chọn thư mục/file trước".to_string(), 0.0, true);
                return;
            }

            // Get current filter config from UI
            let enable_filtering = ui.get_enable_filtering();
            let exclude_patterns_text = ui.get_exclude_patterns_text().to_string();
            let include_patterns_text = ui.get_include_patterns_text().to_string();
            let max_file_size_text = ui.get_max_file_size_text().to_string();

            // Parse max file size
            let max_file_size = max_file_size_text.parse::<u64>()
                .unwrap_or(100)
                .saturating_mul(1024 * 1024);

            // Parse patterns (comma-separated)
            let exclude_patterns: Vec<String> = exclude_patterns_text
                .split(',')
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();

            let include_patterns: Vec<String> = include_patterns_text
                .split(',')
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();

            let filter_config = crate::config::FilterConfig {
                enable_filtering,
                exclude_patterns,
                include_patterns,
                max_file_size,
            };

            let ui_handle_task = ui_handle.clone();
            tokio::spawn(async move {
                let mut total_stats = crate::utils::FilteringStats {
                    total_files: 0,
                    included_files: 0,
                    excluded_files: 0,
                    total_size: 0,
                    excluded_size: 0,
                };

                for item in &local_paths {
                    let local_path_str = item.local_path.to_string();
                    let path = std::path::Path::new(&local_path_str);
                    if path.is_dir() {
                        if let Ok(stats) = crate::utils::get_filtering_stats(path, &filter_config) {
                            total_stats.total_files += stats.total_files;
                            total_stats.included_files += stats.included_files;
                            total_stats.excluded_files += stats.excluded_files;
                            total_stats.total_size += stats.total_size;
                            total_stats.excluded_size += stats.excluded_size;
                        }
                    } else if path.is_file() {
                        total_stats.total_files += 1;
                        if crate::utils::should_include_file(path, path.parent().unwrap_or(path), &filter_config) {
                            total_stats.included_files += 1;
                            if let Ok(metadata) = std::fs::metadata(path) {
                                total_stats.total_size += metadata.len();
                            }
                        } else {
                            total_stats.excluded_files += 1;
                            if let Ok(metadata) = std::fs::metadata(path) {
                                total_stats.excluded_size += metadata.len();
                            }
                        }
                    }
                }

                let stats_text = format!(
                    "Tổng: {} files | Bao gồm: {} files | Loại trừ: {} files\nTổng kích thước: {} MB | Tiết kiệm: {} MB ({:.1}%)",
                    total_stats.total_files,
                    total_stats.included_files,
                    total_stats.excluded_files,
                    total_stats.total_size / (1024 * 1024),
                    total_stats.excluded_size / (1024 * 1024),
                    total_stats.exclusion_rate() * 100.0
                );

                let _ = ui_handle_task.upgrade_in_event_loop(|ui| {
                    ui.set_filter_stats(stats_text.into());
                });
            });
        }
    });
}
