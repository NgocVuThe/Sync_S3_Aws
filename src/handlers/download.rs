use crate::*;
use crate::s3::{create_s3_client, download_from_s3};
use crate::utils::{validate_credentials, update_status};
use std::sync::Arc;
use slint::ComponentHandle;
use rfd::FileDialog;

pub fn setup_download_handler(ui: &AppWindow) {
    let ui_handle = ui.as_weak();

    // 1. Handler for selecting download folder
    let ui_handle_select = ui_handle.clone();
    ui.on_select_download_folder(move || {
        let ui = ui_handle_select.unwrap();
        
        let folder = FileDialog::new()
            .set_title("Chọn thư mục để lưu file tải từ S3")
            .pick_folder();

        if let Some(local_dir) = folder {
            let local_dir_str = local_dir.to_string_lossy().to_string();
            ui.set_download_target_path(local_dir_str.clone().into());
            update_status(&ui_handle_select, format!("Đã chọn thư mục: {}. Nhấn 'Start Download' để bắt đầu.", local_dir_str), 0.0, false);
        }
    });

    // 2. Handler for executing download
    let ui_handle_execute = ui_handle.clone();
    ui.on_execute_download(move || {
        let ui = ui_handle_execute.unwrap();
        
        let acc_key = ui.get_access_key().to_string();
        let sec_key = ui.get_secret_key().to_string();
        let sess_token = ui.get_session_token().to_string();
        let region = ui.get_region().to_string();
        let bucket = ui.get_bucket_name().to_string();
        let prefix = ui.get_s3_base_path().to_string();
        let local_dir_str = ui.get_download_target_path().to_string();

        // Warning if prefix is empty (downloading entire bucket)
        if prefix.trim().is_empty() {
            let confirmed = rfd::MessageDialog::new()
                .set_title("Xác nhận tải toàn bộ Bucket")
                .set_description(&format!("Bạn đang để trống Prefix. Ứng dụng sẽ tải TOÀN BỘ nội dung của bucket '{}'. Việc này có thể tốn nhiều thời gian và dung lượng. Bạn có muốn tiếp tục không?", bucket))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();

            if confirmed != rfd::MessageDialogResult::Yes {
                return;
            }
        }

        // Validate
        if let Some(err) = validate_credentials(&acc_key, &sec_key, &bucket) {
            update_status(&ui_handle_execute, err, 0.0, true);
            return;
        }

        if local_dir_str.is_empty() {
            update_status(&ui_handle_execute, "Vui lòng chọn thư mục lưu trữ trước.".to_string(), 0.0, true);
            return;
        }

        let ui_handle_clone = ui_handle_execute.clone();
        
        tokio::spawn(async move {
            let sess_token_opt = if sess_token.is_empty() { None } else { Some(sess_token) };
            
            match create_s3_client(acc_key, sec_key, sess_token_opt, region).await {
                Ok(client) => {
                    let client = Arc::new(client);
                    if let Err(e) = download_from_s3(client, bucket, prefix, local_dir_str, ui_handle_clone.clone()).await {
                        update_status(&ui_handle_clone, e, 0.0, true);
                    } else {
                        // Reset download path on success to revert button text
                        let ui_weak = ui_handle_clone.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.set_download_target_path(slint::SharedString::from(""));
                            }
                        });
                    }
                }
                Err(e) => {
                    update_status(&ui_handle_clone, format!("Lỗi khởi tạo S3 Client: {}", e), 0.0, true);
                }
            }
        });
    });
}
