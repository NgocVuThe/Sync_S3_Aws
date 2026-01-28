use crate::*;
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use tokio::time;
use tracing::{error, warn};
use crate::s3::{create_s3_client, find_best_s3_prefix, get_preview_prefix};

/// Calculates the S3 path for a given local path and base path.
fn calculate_s3_path(p: &std::path::Path, base_path_buf: &std::path::Path) -> String {
    if !base_path_buf.as_os_str().is_empty() && p.starts_with(base_path_buf) {
        let rel = p.strip_prefix(base_path_buf).unwrap_or(p);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            rel_str
        }
    } else {
        get_preview_prefix(p)
    }
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

                    let cache: crate::s3::prefix::GlobalPrefixCache = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

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
                            find_best_s3_prefix(c, &bucket, p.as_path(), &cache).await
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

                    let cache: crate::s3::prefix::GlobalPrefixCache = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

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
                            find_best_s3_prefix(c, &bucket, p.as_path(), &cache).await
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

/// Sets up the base path selection handler.
pub fn setup_select_base_path_handler(ui: &AppWindow) {
    ui.on_select_base_path({
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            // Get current AWS config for accurate calculation
            let acc_key = ui.get_access_key().to_string();
            let sec_key = ui.get_secret_key().to_string();
            let sess_token = ui.get_session_token().to_string();
            let region = ui.get_region().to_string();
            let bucket = ui.get_bucket_name().to_string();

            let ui_handle_cloned = ui_handle.clone();
            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                ui.set_is_selecting_base_path(true);
            });

            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let path_str = path.to_string_lossy().to_string();
                let ui_handle_task = ui_handle.clone();

                tokio::spawn(async move {
                    // 1. Save to config file
                    let mut config = crate::config::load_config();
                    config.s3_base_path = path_str.clone();
                    if let Err(e) = crate::config::save_config(&config) {
                        error!("Failed to save config: {:?}", e);
                    }

                    crate::utils::update_status(
                        &ui_handle_task,
                        "Đang tính toán lại đường dẫn S3...".to_string(),
                        0.0,
                        false,
                    );

                    // 2. Try to create S3 client
                    let client = if !acc_key.is_empty() && !sec_key.is_empty() && !bucket.is_empty() {
                        match create_s3_client(
                            acc_key,
                            sec_key,
                            if sess_token.is_empty() {
                                None
                            } else {
                                Some(sess_token)
                            },
                            region,
                        )
                        .await
                        {
                            Ok(c) => Some(c),
                            Err(e) => {
                                warn!("S3 client creation failed, using offline calculation: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let base_path_buf = std::path::PathBuf::from(&path_str);
                    let mut updated_items = Vec::new();
                    let cache: crate::s3::GlobalPrefixCache = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));


                    // 3. Recalculate each item (possibly calling S3)
                    let current_items: Vec<PathItem> = if let Some(ui) = ui_handle_task.upgrade() {
                        ui.get_local_paths().iter().collect()
                    } else {
                        Vec::new()
                    };

                    for mut item in current_items {
                        let p = std::path::Path::new(item.local_path.as_str());

                        let new_s3_path = if !base_path_buf.as_os_str().is_empty()
                            && p.starts_with(&base_path_buf)
                        {
                            // Priority 1: Relative to Base Path
                            calculate_s3_path(p, &base_path_buf)
                        } else if let Some(ref c) = client {
                            // Priority 2: Accurate S3 Lookup (if outside base path)
                            find_best_s3_prefix(c, &bucket, p, &cache).await
                        } else {
                            // Priority 3: General Preview
                            calculate_s3_path(p, &base_path_buf)
                        };

                        item.s3_path = new_s3_path.into();
                        updated_items.push(item);
                    }

                    // 4. Update UI
                    let ui_handle_final = ui_handle_task.clone();
                    let _ = ui_handle_task.upgrade_in_event_loop(move |ui| {
                        ui.set_s3_base_path(path_str.into());
                        let model = Rc::new(VecModel::from(updated_items));
                        ui.set_local_paths(ModelRc::from(model));
                        ui.set_is_selecting_base_path(false);
                        crate::utils::update_status(
                            &ui_handle_final,
                            "Đã cập nhật đường dẫn S3".to_string(),
                            0.0,
                            false,
                        );
                    });
                });
            } else {
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui| {
                    ui.set_is_selecting_base_path(false);
                });
            }
        }
    });
}
