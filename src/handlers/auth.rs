use crate::*;
use aws_sdk_s3::config::Credentials;
use tracing::{error, info};
use crate::s3::{create_s3_client, test_bucket_access};

/// Sets up the test access handler for the UI.
pub fn setup_test_access_handler(ui: &AppWindow) {
    ui.on_test_access({
        let ui_handle = ui.as_weak();
        move |acc_key, sec_key, sess_token, region, bucket| {
            let bucket_name = bucket.to_string();
            let region_str = region.to_string();

            // Save selected bucket and region to config
            let mut config = crate::config::load_config();
            config.selected_bucket = bucket_name.clone();
            config.selected_region = region_str.clone();
            if let Err(e) = crate::config::save_config(&config) {
                error!("Failed to save config: {:?}", e);
            }

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
