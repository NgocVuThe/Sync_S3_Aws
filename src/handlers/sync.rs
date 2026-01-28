use crate::*;
use slint::Model;
use tracing::error;
use crate::s3::{create_s3_client, sync_to_s3};

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
