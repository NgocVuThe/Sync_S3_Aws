use crate::AppWindow;
use crate::s3::{create_cloudfront_client, create_s3_client, test_bucket_access, test_cloudfront_access};
use slint::ComponentHandle;
use tracing::{error, info};

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
                let _ = ui_handle.upgrade_in_event_loop(|ui: AppWindow| ui.set_test_access_error(err.into()));
                return;
            }

            let ui_handle_cloned = ui_handle.clone();
            
            tokio::spawn(async move {
                crate::utils::update_status(
                    &ui_handle_cloned,
                    "Đang kiểm tra kết nối S3...".to_string(),
                    0.1,
                    false,
                );
                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui: AppWindow| ui.set_test_access_error("".into()));
                
                let sess_token_opt = if sess_token.is_empty() { None } else { Some(sess_token.to_string()) };
                
                match create_s3_client(
                    acc_key.to_string(),
                    sec_key.to_string(),
                    sess_token_opt.clone(),
                    region_str.clone(),
                )
                .await
                {
                    Ok(client) => match test_bucket_access(&client, &bucket_name).await {
                        Ok(_) => {
                            info!("S3 Access thành công: {}", bucket_name);
                            
                            // Check for CloudFront if distribution ID exists
                            let config = crate::config::load_config();
                            if let Some(dist_id) = config.get_distribution_id(&bucket_name) {
                                crate::utils::update_status(
                                    &ui_handle_cloned,
                                    "S3 OK! Đang kiểm tra CloudFront...".to_string(),
                                    0.5,
                                    false,
                                );
                                
                                match create_cloudfront_client(
                                    acc_key.to_string(),
                                    sec_key.to_string(),
                                    sess_token_opt,
                                    region_str,
                                ).await {
                                    Ok(cf_client) => {
                                        match test_cloudfront_access(&cf_client, &dist_id).await {
                                            Ok(_) => {
                                                info!("CloudFront Access thành công: {}", dist_id);
                                                crate::utils::update_status(
                                                    &ui_handle_cloned,
                                                    "Kết nối S3 & CloudFront thành công!".to_string(),
                                                    1.0,
                                                    false,
                                                );
                                                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui: AppWindow| ui.set_is_cf_connected(true));
                                            }
                                            Err(e) => {
                                                error!("CloudFront Access thất bại: {}", e);
                                                crate::utils::update_status(
                                                    &ui_handle_cloned,
                                                    format!("S3 OK nhưng CloudFront lỗi: {}", e),
                                                    0.7,
                                                    true,
                                                );
                                                let _ = ui_handle_cloned.upgrade_in_event_loop(|ui: AppWindow| ui.set_is_cf_connected(false));
                                                let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui: AppWindow| ui.set_test_access_error(format!("CloudFront: {}", e).into()));
                                                return;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to create CF client: {}", e);
                                        crate::utils::update_status(
                                            &ui_handle_cloned,
                                            format!("S3 OK nhưng lỗi CloudFront client: {}", e),
                                            0.7,
                                            true,
                                        );
                                        return;
                                    }
                                }
                            } else {
                                crate::utils::update_status(
                                    &ui_handle_cloned,
                                    "Kết nối S3 thành công!".to_string(),
                                    1.0,
                                    false,
                                );
                            }

                            let _ = ui_handle_cloned
                                .upgrade_in_event_loop(|ui: AppWindow| ui.set_show_config(false));
                            let _ = ui_handle_cloned.upgrade_in_event_loop(|ui: AppWindow| ui.set_test_access_error("".into()));
                        }
                        Err(e) => {
                            error!("Test Access thất bại: {:?}", e);
                            crate::utils::update_status(
                                &ui_handle_cloned,
                                format!("Lỗi: {}", e),
                                0.0,
                                true,
                            );
                            let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui: AppWindow| ui.set_test_access_error(format!("Lỗi: {}", e).into()));
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
                        let _ = ui_handle_cloned.upgrade_in_event_loop(move |ui: AppWindow| ui.set_test_access_error(format!("Lỗi tạo client: {}", e).into()));
                    }
                }
            });
        }
    });
}

/// Sets up the bucket selected handler to auto-fill distribution ID
pub fn setup_bucket_selected_handler(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_bucket_selected({
        move |bucket_name: slint::SharedString| {
            let bucket_str = bucket_name.to_string();
            let mut config = crate::config::load_config();
            
            let distribution_id = config.get_distribution_id(&bucket_str).unwrap_or_default();
            config.selected_bucket = bucket_str;
            let _ = crate::config::save_config(&config);
            
            let _ = ui_handle.upgrade_in_event_loop(move |ui: AppWindow| {
                ui.set_distribution_id(distribution_id.into());
                ui.set_is_cf_connected(false);
            });
        }
    });
}

/// Sets up the auto-invalidate toggle handler
pub fn setup_auto_invalidate_handler(ui: &AppWindow) {
    ui.on_update_auto_invalidate({
        move |checked| {
            let mut config = crate::config::load_config();
            config.auto_invalidate = checked;
            if let Err(e) = crate::config::save_config(&config) {
                error!("Failed to save auto-invalidate config: {:?}", e);
            }
        }
    });
}

/// Sets up the clear cache handler
pub fn setup_clear_cache_handler(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_clear_cache({
        let ui_handle_inner = ui_handle.clone();
        move || {
            // Read ALL UI properties on UI thread BEFORE spawning
            let Some(ui_ref) = ui_handle_inner.upgrade() else { return; };
            let acc_key = ui_ref.get_access_key().to_string();
            let sec_key = ui_ref.get_secret_key().to_string();
            let sess_token = ui_ref.get_session_token().to_string();
            let region = ui_ref.get_region().to_string();
            let bucket_name = ui_ref.get_bucket_name().to_string();
            let log_path = ui_ref.get_log_path().to_string();
            drop(ui_ref);

            let distribution_id = {
                let config = crate::config::load_config();
                config.get_distribution_id(&bucket_name).unwrap_or_default()
            };

            if distribution_id.is_empty() {
                let _ = ui_handle_inner.upgrade_in_event_loop(|ui: AppWindow| {
                    ui.set_status_text("Chưa cấu hình Distribution ID".into());
                });
                return;
            }

            // Set loading state on UI thread
            let _ = ui_handle_inner.upgrade_in_event_loop(|ui: AppWindow| {
                ui.set_status_text("Đang xóa cache CloudFront...".into());
                ui.set_is_clearing_cache(true);
            });

            let sess_token_opt = if sess_token.is_empty() { None } else { Some(sess_token) };
            let ui_handle_for_spawn = ui_handle.clone();

            // Spawn with already-captured data, NO upgrade() inside
            tokio::spawn(async move {
                match crate::s3::create_cloudfront_client(
                    acc_key,
                    sec_key,
                    sess_token_opt,
                    region,
                ).await {
                    Ok(client) => {
                        match crate::s3::invalidate_cache(&client, &distribution_id, vec!["/*".to_string()]).await {
                            Ok(inv_id) => {
                                let now = chrono::Local::now();
                                let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
                                let log_msg = format!(
                                    "[{}] CloudFront Invalidation - Bucket: {}, Distribution ID: {}, Invalidation ID: {}",
                                    timestamp,
                                    bucket_name,
                                    distribution_id,
                                    inv_id
                                );
                                tracing::info!("{}", log_msg);
                                
                                crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                                
                                let _ = ui_handle_for_spawn.upgrade_in_event_loop(move |ui: AppWindow| {
                                    ui.set_status_text(format!("Đã tạo invalidation: {} - {}", bucket_name, inv_id).into());
                                    ui.set_is_clearing_cache(false);
                                });
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                let now = chrono::Local::now();
                                let log_msg = format!(
                                    "[{}] CloudFront Invalidation Error - Bucket: {}, Distribution ID: {}, Error: {}",
                                    now.format("%Y-%m-%d %H:%M:%S"),
                                    bucket_name,
                                    distribution_id,
                                    err_msg
                                );
                                tracing::error!("{}", log_msg);
                                
                                crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                                
                                let _ = ui_handle_for_spawn.upgrade_in_event_loop(move |ui: AppWindow| {
                                    ui.set_status_text(format!("Lỗi invalidation: {}", err_msg).into());
                                    ui.set_is_clearing_cache(false);
                                });
                            }
                        }
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        tracing::error!("CloudFront Client Error - Bucket: {}, Error: {}", bucket_name, err_msg);
                        
                        let now = chrono::Local::now();
                        let log_msg = format!(
                            "[{}] CloudFront Client Error - Bucket: {}, Error: {}",
                            now.format("%Y-%m-%d %H:%M:%S"),
                            bucket_name,
                            err_msg
                        );
                        crate::utils::log_cloudfront_to_file(&log_path, &log_msg);
                        
                        let _ = ui_handle_for_spawn.upgrade_in_event_loop(move |ui: AppWindow| {
                            ui.set_status_text(format!("Lỗi tạo CloudFront client: {}", err_msg).into());
                            ui.set_is_clearing_cache(false);
                        });
                    }
                }
            });
        }
    });
}
