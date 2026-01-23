use crate::*;
use aws_sdk_s3::config::Credentials;
use rfd;
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use tokio;
use tokio::time;
use tracing::{error, info};

use crate::s3_client::{create_s3_client, sync_to_s3, test_bucket_access, find_best_s3_prefix, get_preview_prefix};

/// Sets up the test access handler for the UI.
pub fn setup_test_access_handler(ui: &AppWindow) {
    ui.on_test_access({
        let ui_handle = ui.as_weak();
        move |acc_key, sec_key, sess_token, region, bucket| {
            let bucket_name = bucket.to_string();
            let region_str = region.to_string();

            // Validate inputs
            if let Some(err) = crate::utils::validate_credentials(&acc_key, &sec_key, &bucket_name)
            {
                crate::utils::update_status(&ui_handle, err.clone(), 0.0, true);
                let _ = ui_handle.upgrade_in_event_loop(|ui| ui.set_test_access_error(err.into()));
                return;
            }

            let _credentials = Credentials::new(
                acc_key.to_string(),
                sec_key.to_string(),
                if sess_token.is_empty() {
                    None
                } else {
                    Some(sess_token.to_string())
                },
                None,
                "manual",
            );

            let ui_handle_cloned = ui_handle.clone();
            
            tokio::spawn(async move {
                crate::utils::update_status(
                    &ui_handle_cloned,
                    "Đang kiểm tra kết nối...".to_string(),
                    0.1,
                    false,
                );
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| ui.set_test_access_error("".into()));
                match create_s3_client(
                    acc_key.to_string(),
                    sec_key.to_string(),
                    if sess_token.is_empty() {
                        None
                    } else {
                        Some(sess_token.to_string())
                    },
                    region_str,
                )
                .await
                {
                    Ok(client) => match test_bucket_access(&client, &bucket_name).await {
                        Ok(_) => {
                            info!("Test Access thành công: {}", bucket_name);
                            let _ = ui_handle_cloned
                                .upgrade_in_event_loop(|ui| ui.set_show_config(false));
                            crate::utils::update_status(
                                &ui_handle_cloned,
                                "Kết nối thành công!".to_string(),
                                1.0,
                                false,
                            );
                            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| ui.set_test_access_error("".into()));
                        }
                        Err(e) => {
                            error!("Test Access thất bại: {:?}", e);
                            crate::utils::update_status(
                                &ui_handle_cloned,
                                format!("Lỗi: {}", e),
                                0.0,
                                true,
                            );
                            let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui| ui.set_test_access_error(format!("Lỗi: {}", e).into()));
                        }
                    },
                    Err(e) => {
                        error!("Failed to create S3 client: {:?}", e);
                        crate::utils::update_status(
                            &ui_handle_cloned,
                            format!("Lỗi tạo client: {}", e),
                            0.0,
                            true,
                        );
                        let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui| ui.set_test_access_error(format!("Lỗi tạo client: {}", e).into()));
                    }
                }
            });
        }
    });
}

/// Sets up the folder selection handler.
pub fn setup_select_folder_handler(ui: &AppWindow) {
    ui.on_select_folder({
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            // Get current AWS config
            let acc_key = ui.get_access_key().to_string();
            let sec_key = ui.get_secret_key().to_string();
            let sess_token = ui.get_session_token().to_string();
            let region = ui.get_region().to_string();
            let bucket = ui.get_bucket_name().to_string();
            let s3_base_path = ui.get_s3_base_path().to_string();

            let ui_handle_cloned = ui_handle.clone();
            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                ui.set_is_selecting_folder(true);
            });

            if let Some(paths) = rfd::FileDialog::new().pick_folders() {
                let ui_handle_task = ui_handle.clone();
                tokio::spawn(async move {
                    let mut results = Vec::new();
                    let base_path_buf = std::path::PathBuf::from(&s3_base_path);

                    // Try to create S3 client for accurate calculation
                    let client = if !acc_key.is_empty() && !sec_key.is_empty() && !bucket.is_empty() {
                        match create_s3_client(
                            acc_key,
                            sec_key,
                            if sess_token.is_empty() { None } else { Some(sess_token) },
                            region
                        ).await {
                            Ok(c) => Some(c),
                            Err(e) => {
                                error!("Failed to create S3 client for path preview: {:?}", e);
                                crate::utils::update_status(&ui_handle_task, "Cảnh báo: Không thể kết nối S3, sử dụng đường dẫn xem trước".to_string(), 0.0, false);
                                time::sleep(time::Duration::from_secs(2)).await; // Show message briefly
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let cache: crate::s3_client::GlobalPrefixCache = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

                    for p in paths {
                        let local_path = p.to_string_lossy().to_string();
                        
                        let s3_path = if !base_path_buf.as_os_str().is_empty() && p.starts_with(&base_path_buf) {
                            let rel = p.strip_prefix(&base_path_buf).unwrap_or(&p);
                            let rel_str = rel.to_string_lossy().replace('\\', "/");
                            if rel_str.is_empty() {
                                p.file_name().unwrap_or_default().to_string_lossy().to_string()
                            } else {
                                rel_str
                            }
                        } else if let Some(ref c) = client {
                            find_best_s3_prefix(c, &bucket, &p.to_path_buf(), &cache).await
                        } else {
                            get_preview_prefix(&p)
                        };

                        results.push(PathItem {
                            local_path: local_path.into(),
                            s3_path: s3_path.into(),
                        });
                    }

                    let _ = ui_handle_task.upgrade_in_event_loop(move |ui| {
                        let mut current_items: Vec<PathItem> = ui.get_local_paths().iter().collect();
                        current_items.extend(results);
                        let model = Rc::new(VecModel::from(current_items));
                        ui.set_local_paths(ModelRc::from(model));
                        ui.set_is_selecting_folder(false);
                    });
                });
            } else {
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                    ui.set_is_selecting_folder(false);
                });
            }
        }
    });
}

/// Sets up the file selection handler.
pub fn setup_select_files_handler(ui: &AppWindow) {
    ui.on_select_files({
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            // Get current AWS config
            let acc_key = ui.get_access_key().to_string();
            let sec_key = ui.get_secret_key().to_string();
            let sess_token = ui.get_session_token().to_string();
            let region = ui.get_region().to_string();
            let bucket = ui.get_bucket_name().to_string();
            let s3_base_path = ui.get_s3_base_path().to_string();

            let ui_handle_cloned = ui_handle.clone();
            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                ui.set_is_selecting_folder(true);
            });

            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                let ui_handle_task = ui_handle.clone();
                tokio::spawn(async move {
                    let mut results = Vec::new();
                    let base_path_buf = std::path::PathBuf::from(&s3_base_path);

                    // Try to create S3 client for accurate calculation
                    let client = if !acc_key.is_empty() && !sec_key.is_empty() && !bucket.is_empty() {
                        match create_s3_client(
                            acc_key,
                            sec_key,
                            if sess_token.is_empty() { None } else { Some(sess_token) },
                            region
                        ).await {
                            Ok(c) => Some(c),
                            Err(e) => {
                                error!("Failed to create S3 client for path preview: {:?}", e);
                                crate::utils::update_status(&ui_handle_task, "Cảnh báo: Không thể kết nối S3, sử dụng đường dẫn xem trước".to_string(), 0.0, false);
                                time::sleep(time::Duration::from_secs(2)).await; // Show message briefly
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let cache: crate::s3_client::GlobalPrefixCache = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

                    for p in paths {
                        let local_path = p.to_string_lossy().to_string();
                        
                        let s3_path = if !base_path_buf.as_os_str().is_empty() && p.starts_with(&base_path_buf) {
                            let rel = p.strip_prefix(&base_path_buf).unwrap_or(&p);
                            let rel_str = rel.to_string_lossy().replace('\\', "/");
                            if rel_str.is_empty() {
                                p.file_name().unwrap_or_default().to_string_lossy().to_string()
                            } else {
                                rel_str
                            }
                        } else if let Some(ref c) = client {
                            find_best_s3_prefix(c, &bucket, &p.to_path_buf(), &cache).await
                        } else {
                            get_preview_prefix(&p)
                        };

                        results.push(PathItem {
                            local_path: local_path.into(),
                            s3_path: s3_path.into(),
                        });
                    }

                    let _ = ui_handle_task.upgrade_in_event_loop(move |ui| {
                        let mut current_items: Vec<PathItem> = ui.get_local_paths().iter().collect();
                        current_items.extend(results);
                        let model = Rc::new(VecModel::from(current_items));
                        ui.set_local_paths(ModelRc::from(model));
                        ui.set_is_selecting_folder(false);
                    });
                });
            } else {
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                    ui.set_is_selecting_folder(false);
                });
            }
        }
    });
}

/// Sets up the clear folders handler.
pub fn setup_clear_folders_handler(ui: &AppWindow) {
    ui.on_clear_folders({
        let ui_handle = ui.as_weak();
        move || {
            let _ = ui_handle.upgrade_in_event_loop(|ui| {
                let model = Rc::new(VecModel::from(vec![]));
                ui.set_local_paths(ModelRc::from(model));
            });
        }
    });
}

/// Sets up the remove single folder handler.
pub fn setup_remove_folder_handler(ui: &AppWindow) {
    ui.on_remove_folder({
        let ui_handle = ui.as_weak();
        move |index| {
            let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                let model = ui.get_local_paths();
                if let Some(vec_model) = model
                    .as_any()
                    .downcast_ref::<VecModel<PathItem>>()
                {
                    vec_model.remove(index as usize);
                } else {
                    let mut current_items: Vec<PathItem> =
                        ui.get_local_paths().iter().collect();
                    if (index as usize) < current_items.len() {
                        current_items.remove(index as usize);
                        let new_model = Rc::new(VecModel::from(current_items));
                        ui.set_local_paths(ModelRc::from(new_model));
                    }
                }
            });
        }
    });
}

/// Sets up the start sync handler.
pub fn setup_start_sync_handler(ui: &AppWindow) {
    ui.on_start_sync({
        let ui_handle = ui.as_weak();
        move |acc_key, sec_key, sess_token, region, bucket, local_dirs| {
            let bucket_name = bucket.to_string();
            let region_str = region.to_string();
            let mappings: Vec<(String, String)> = local_dirs
                .iter()
                .map(|item: PathItem| (item.local_path.to_string(), item.s3_path.to_string()))
                .collect();
            let log_path = ui_handle.upgrade().map(|ui| ui.get_log_path().to_string()).unwrap_or_default();

            // Validate inputs
            if let Some(err) = crate::utils::validate_credentials(&acc_key, &sec_key, &bucket_name)
            {
                crate::utils::update_status(&ui_handle, err, 0.0, true);
                return;
            }

            if mappings.is_empty() {
                crate::utils::update_status(
                    &ui_handle,
                    "Không có file hoặc thư mục nào để upload".to_string(),
                    0.0,
                    true,
                );
                return;
            }

            let ui_handle_cloned = ui_handle.clone();

            tokio::spawn(async move {
                match create_s3_client(
                    acc_key.to_string(),
                    sec_key.to_string(),
                    if sess_token.is_empty() {
                        None
                    } else {
                        Some(sess_token.to_string())
                    },
                    region_str,
                )
                .await
                {
                    Ok(client) => {
                        let client = std::sync::Arc::new(client);
                        if let Err(e) =
                            sync_to_s3(client, bucket_name, mappings, ui_handle_cloned, log_path).await
                        {
                            error!("Sync failed: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to create S3 client for sync: {:?}", e);
                        crate::utils::update_status(
                            &ui_handle_cloned,
                            format!("Lỗi tạo client: {}", e),
                            0.0,
                            true,
                        );
                    }
                }
            });
        }
    });
}

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
                crate::utils::update_status(&ui_handle, format!("Lỗi lưu cấu hình: {}", e), 0.0, true);
            } else {
                info!("Config saved: log_path = {}", path_str);
                crate::utils::update_status(&ui_handle, "Đã lưu đường dẫn log".to_string(), 0.0, false);
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
                    spawn_result = std::process::Command::new("explorer").arg(&log_path).spawn();
                }
                #[cfg(target_os = "macos")]
                {
                    spawn_result = std::process::Command::new("open").arg(&log_path).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    spawn_result = std::process::Command::new("xdg-open").arg(&log_path).spawn();
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

/// Sets up the base path selection handler.
pub fn setup_select_base_path_handler(ui: &AppWindow) {
    ui.on_select_base_path({
        let ui_handle = ui.as_weak();
        move || {
            let ui_handle_cloned = ui_handle.clone();
            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                ui.set_is_selecting_base_path(true);
            });

            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let path_str = path.to_string_lossy().to_string();

                // Save to config file
                let mut config = crate::config::load_config();
                config.s3_base_path = path_str.clone();
                if let Err(e) = crate::config::save_config(&config) {
                    error!("Failed to save config: {:?}", e);
                    crate::utils::update_status(&ui_handle_cloned, format!("Lỗi lưu cấu hình: {}", e), 0.0, true);
                } else {
                    info!("Config saved: s3_base_path = {}", path_str);
                }

                let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui| {
                    ui.set_s3_base_path(path_str.into());
                    ui.set_is_selecting_base_path(false);
                });
            } else {
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                    ui.set_is_selecting_base_path(false);
                });
            }
        }
    });
}

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
}
