use crate::*;
use aws_sdk_s3::config::Credentials;
use rfd;
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use tokio;
use tracing::{error, info};

use crate::s3_client::{create_s3_client, sync_to_s3, test_bucket_access};

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
                crate::utils::update_status(&ui_handle, err, 0.0);
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
                );
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
                            );
                        }
                        Err(e) => {
                            error!("Test Access thất bại: {:?}", e);
                            crate::utils::update_status(
                                &ui_handle_cloned,
                                format!("Lỗi: {}", e),
                                0.0,
                            );
                        }
                    },
                    Err(e) => {
                        error!("Failed to create S3 client: {:?}", e);
                        crate::utils::update_status(
                            &ui_handle_cloned,
                            format!("Lỗi tạo client: {}", e),
                            0.0,
                        );
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
            if let Some(paths) = rfd::FileDialog::new().pick_folders() {
                let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                    let mut current_paths: Vec<slint::SharedString> = ui
                        .get_local_paths()
                        .iter()
                        .map(|s: slint::SharedString| s)
                        .collect();
                    for p in paths {
                        current_paths.push(p.to_string_lossy().to_string().into());
                    }
                    let model = Rc::new(VecModel::from(current_paths));
                    ui.set_local_paths(ModelRc::from(model));
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
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                let _ = ui_handle.upgrade_in_event_loop(move |ui| {
                    let mut current_paths: Vec<slint::SharedString> = ui
                        .get_local_paths()
                        .iter()
                        .map(|s: slint::SharedString| s)
                        .collect();
                    for p in paths {
                        current_paths.push(p.to_string_lossy().to_string().into());
                    }
                    let model = Rc::new(VecModel::from(current_paths));
                    ui.set_local_paths(ModelRc::from(model));
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
                    .downcast_ref::<VecModel<slint::SharedString>>()
                {
                    vec_model.remove(index as usize);
                } else {
                    let mut current_paths: Vec<slint::SharedString> =
                        ui.get_local_paths().iter().collect();
                    if (index as usize) < current_paths.len() {
                        current_paths.remove(index as usize);
                        let new_model = Rc::new(VecModel::from(current_paths));
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
            let folders: Vec<String> = local_dirs
                .iter()
                .map(|s: slint::SharedString| s.to_string())
                .collect();

            // Validate inputs
            if let Some(err) = crate::utils::validate_credentials(&acc_key, &sec_key, &bucket_name)
            {
                crate::utils::update_status(&ui_handle, err, 0.0);
                return;
            }

            if folders.is_empty() {
                crate::utils::update_status(
                    &ui_handle,
                    "Không có file hoặc thư mục nào để upload".to_string(),
                    0.0,
                );
                return;
            }

            let ui_handle_cloned = ui_handle.clone();

            let region_for_sync = region_str.clone();
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
                            sync_to_s3(client, bucket_name, folders, acc_key.to_string(), sec_key.to_string(), sess_token.to_string(), region_for_sync, ui_handle_cloned).await
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
                        );
                    }
                }
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
}
