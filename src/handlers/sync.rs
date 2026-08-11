use crate::*;
use slint::Model;
use tracing::{error, info};
use crate::s3::{create_s3_client, sync_to_s3, create_cloudfront_client, invalidate_cache, resolve_cred_source};

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
            let auto_invalidate = ui_handle.upgrade().map(|ui| ui.get_auto_invalidate()).unwrap_or(false);
            let dist_id = ui_handle.upgrade().map(|ui| ui.get_distribution_id().to_string()).unwrap_or_default();
            let acc_key_clone = acc_key.to_string();
            let sec_key_clone = sec_key.to_string();
            let sess_token_opt = if sess_token.is_empty() { None } else { Some(sess_token.to_string()) };
            let region_str_for_cf = region_str.clone();

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
                crate::utils::reset_busy_flag(&ui_handle, |ui| ui.set_is_syncing(false));
                crate::utils::update_status(&ui_handle, err, 0.0, true);
                return;
            }

            if mappings.is_empty() {
                crate::utils::reset_busy_flag(&ui_handle, |ui| ui.set_is_syncing(false));
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
                let cred = resolve_cred_source(
                    acc_key.to_string(),
                    sec_key.to_string(),
                    if sess_token.is_empty() {
                        None
                    } else {
                        Some(sess_token.to_string())
                    },
                );
                match create_s3_client(
                    cred,
                    region_str,
                )
                .await
                {
                    Ok(client) => {
                        let client = std::sync::Arc::new(client);
                        let bucket_name_for_cf = bucket_name.clone();
                        if let Err(e) =
                            sync_to_s3(client, bucket_name, mappings, ui_handle_cloned.clone(), log_path.clone()).await
                        {
                            error!("Sync failed: {}", e);
                            crate::utils::reset_busy_flag(&ui_handle_cloned, |ui| ui.set_is_syncing(false));
                            return;
                        }

                        // Auto-invalidate CloudFront cache if enabled
                        if auto_invalidate && !dist_id.is_empty() {
                            crate::utils::update_status(
                                &ui_handle_cloned,
                                "Đang xóa cache CloudFront...".to_string(),
                                1.0,
                                false,
                            );

                            let cf_cred = resolve_cred_source(
                                acc_key_clone,
                                sec_key_clone,
                                sess_token_opt,
                            );
                            match create_cloudfront_client(
                                cf_cred,
                                region_str_for_cf,
                            ).await {
                                Ok(cf_client) => {
                                    match invalidate_cache(&cf_client, &dist_id, vec!["/*".to_string()]).await {
                                        Ok(inv_id) => {
                                            let now = chrono::Local::now();
                                            let log_msg = format!(
                                                "[{}] Auto CloudFront Invalidation - Bucket: {}, Distribution ID: {}, Invalidation ID: {}",
                                                now.format("%Y-%m-%d %H:%M:%S"),
                                                bucket_name_for_cf,
                                                dist_id,
                                                inv_id
                                            );
                                            info!("{}", log_msg);
                                            crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                                            
                                            crate::utils::update_status(
                                                &ui_handle_cloned,
                                                format!("Đã sync và tạo invalidation: {} - Bucket: {}", inv_id, bucket_name_for_cf).to_string(),
                                                1.0,
                                                false,
                                            );
                                        }
                                        Err(e) => {
                                            error!("CloudFront invalidation failed: {}", e);
                                            let now = chrono::Local::now();
                                            let log_msg = format!(
                                                "[{}] Auto CloudFront Invalidation FAILED - Bucket: {}, Distribution ID: {}, Error: {}",
                                                now.format("%Y-%m-%d %H:%M:%S"),
                                                bucket_name_for_cf,
                                                dist_id,
                                                e
                                            );
                                            error!("{}", log_msg);
                                            crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                                            
                                            crate::utils::update_status(
                                                &ui_handle_cloned,
                                                format!("Sync xong nhưng lỗi invalidation: {}", e).to_string(),
                                                1.0,
                                                true,
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    let err_msg = e.to_string();
                                    error!("Failed to create CloudFront client: {}", err_msg);
                                    let now = chrono::Local::now();
                                        let log_msg = format!(
                                            "[{}] CloudFront Client Creation FAILED - Bucket: {}, Error: {}",
                                            now.format("%Y-%m-%d %H:%M:%S"),
                                            bucket_name_for_cf,
                                            err_msg
                                        );
                                        error!("{}", log_msg);
                                        crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                                        crate::utils::update_status(
                                        &ui_handle_cloned,
                                        format!("Sync xong nhưng lỗi CloudFront: {}", err_msg),
                                        1.0,
                                        true,
                                    );
                                }
                            }
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
                // Reset is_syncing on every terminal path that reaches here:
                // - create_s3_client Err (falls through from above)
                // - sync_to_s3 OK + no auto-invalidate (skips CF block)
                // - sync_to_s3 OK + auto-invalidate: CF OK or CF Err (all CF branches fall through)
                crate::utils::reset_busy_flag(&ui_handle_cloned, |ui| ui.set_is_syncing(false));
            });
        }
    });
}
