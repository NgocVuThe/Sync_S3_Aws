use crate::*;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use chrono::{Local, Datelike, Timelike};
use slint::Weak;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, Mutex};
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::utils::{get_mime_type, update_status};

const MAX_RETRIES: u32 = 3;
const RETRY_DELAYS: [u64; 3] = [1, 3, 5]; // seconds between retries

#[derive(Debug, Clone)]
struct FileError {
    file_path: String,
    s3_key: String,
    error_type: String,
    error_message: String,
    timestamp: String,
    file_size: Option<u64>,
    retry_count: u32,
}

fn get_file_size(path: &PathBuf) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn is_retryable_error(error: &str) -> bool {
    let retryable_keywords = [
        "connection",
        "timeout",
        "network",
        "transient",
        "temporary",
        "503",
        "504",
        "429",
        "slow",
        "throttling",
        "rate",
    ];
    
    let lower_error = error.to_lowercase();
    retryable_keywords.iter().any(|keyword| lower_error.contains(keyword))
}

async fn upload_with_retry(
    client: &Arc<Client>,
    path: &PathBuf,
    key: &str,
    bucket_name: &str,
    ui_handle: &Weak<AppWindow>,
    display_name: &str,
    total_files: usize,
    completed_count: &Arc<Mutex<u32>>,
) -> Result<(), String> {
    let mut last_error = String::new();
    
    for attempt in 1..=MAX_RETRIES {
        match ByteStream::from_path(path).await {
            Ok(stream) => {
                let mime_type = get_mime_type(path);
                
                match client
                    .put_object()
                    .bucket(bucket_name)
                    .key(key)
                    .content_type(mime_type)
                        .cache_control("no-cache, no-store, must-revalidate, max-age=0")
                    .body(stream)
                    .send()
                    .await
                {
                    Ok(_) => {
                        let mut count = completed_count.lock().await;
                        *count += 1;
                        let progress = *count as f32 / total_files as f32;
                        update_status(
                            ui_handle,
                            format!(
                                "Đang upload: {} ({}/{})",
                                display_name, *count, total_files
                            ),
                            progress,
                            false,
                        );
                        debug!("Uploaded: {}", key);
                        return Ok(());
                    }
                    Err(e) => {
                        last_error = format!("AWS S3 upload failed: {}", e);
                        
                        if attempt < MAX_RETRIES && is_retryable_error(&last_error) {
                            let delay = RETRY_DELAYS[(attempt - 1) as usize];
                            warn!(
                                "Upload failed (attempt {}/{}): {}. Retrying in {}s...",
                                attempt, MAX_RETRIES, key, delay
                            );
                            tokio::time::sleep(Duration::from_secs(delay)).await;
                        } else {
                            return Err(format!("Lỗi upload {}: {}", key, e));
                        }
                    }
                }
            }
            Err(e) => {
                last_error = format!("Could not read file from disk: {}", e);
                
                if attempt < MAX_RETRIES {
                    let delay = RETRY_DELAYS[(attempt - 1) as usize];
                    warn!(
                        "File read failed (attempt {}/{}): {}. Retrying in {}s...",
                        attempt, MAX_RETRIES, path.display(), delay
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                } else {
                    return Err(format!("Lỗi mở file {}: {}", path.display(), e));
                }
            }
        }
    }
    
    Err(last_error)
}

fn detailed_file_error_log(error: &FileError) -> String {
    let mut log = String::new();
    log.push_str(&format!("╔══════════════════════════════════════════════════════════════╗\n"));
    log.push_str(&format!("║                    CHI TIẾT LỖI FILE                         ║\n"));
    log.push_str(&format!("╠══════════════════════════════════════════════════════════════╣\n"));
    log.push_str(&format!("║ Thời gian: {}                                     ║\n", error.timestamp));
    log.push_str(&format!("╠══════════════════════════════════════════════════════════════╣\n"));
    log.push_str(&format!("║ Đường dẫn file: {}                    ║\n", error.file_path));
    log.push_str(&format!("║ S3 Key: {}                                          ║\n", error.s3_key));
    log.push_str(&format!("║ Loại lỗi: {}                                           ║\n", error.error_type));
    if let Some(size) = error.file_size {
        log.push_str(&format!("║ Kích thước file: {:>40} bytes ║\n", size));
    }
    log.push_str(&format!("║ Số lần thử lại: {:>37}       ║\n", error.retry_count));
    log.push_str(&format!("╠══════════════════════════════════════════════════════════════╣\n"));
    log.push_str(&format!("║ Chi tiết lỗi:                                              ║\n"));
    log.push_str(&format!("║ {}                                                  ║\n", error.error_message));
    log.push_str(&format!("╚══════════════════════════════════════════════════════════════╝\n"));
    log
}

fn write_detailed_error_log(log_file: &str, error: &FileError) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    
    let log_entry = detailed_file_error_log(error);
    writeln!(file, "{}", log_entry)?;
    Ok(())
}

fn write_error_summary(log_file: &str, errors: &[FileError]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    
    let now = Local::now();
    writeln!(file, "\n")?;
    writeln!(file, "═══════════════════════════════════════════════════════════════════")?;
    writeln!(file, "                    BÁO CÁO TỔNG HỢP LỖI                         ")?;
    writeln!(file, "═══════════════════════════════════════════════════════════════════")?;
    writeln!(file, "Thời gian: {:04}-{:02}-{:02} {:02}:{:02}:{:02}", 
             now.year(), now.month(), now.day(), now.hour(), now.minute(), now.second())?;
    writeln!(file, "Tổng số lỗi: {}", errors.len())?;
    writeln!(file, "───────────────────────────────────────────────────────────────────")?;
    
    for (i, error) in errors.iter().enumerate() {
        writeln!(file, "\n[{}] {}", i + 1, error.file_path)?;
        writeln!(file, "     S3 Key: {}", error.s3_key)?;
        writeln!(file, "     Loại lỗi: {}", error.error_type)?;
        writeln!(file, "     Số lần thử lại: {}", error.retry_count)?;
        writeln!(file, "     Chi tiết: {}", error.error_message)?;
    }
    
    writeln!(file, "\n═══════════════════════════════════════════════════════════════════")?;
    writeln!(file, "                      KẾT THÚC BÁO CÁO                           ")?;
    writeln!(file, "═══════════════════════════════════════════════════════════════════\n")?;
    
    Ok(())
}

/// Performs sync operation: uploads all files from the provided mappings to the S3 bucket.
pub async fn sync_to_s3(
    client: Arc<Client>,
    bucket_name: String,
    mappings: Vec<(String, String)>, // (local_path, s3_path)
    ui_handle: Weak<AppWindow>,
    log_path: String,
) -> Result<(), String> {
    update_status(&ui_handle, "Khởi tạo Sync...".to_string(), 0.0, false);

    let should_log = !log_path.is_empty();
    let start_time = Local::now();
    let mut log_mappings: Vec<String> = Vec::new();
    
    // Pre-compute log file path to avoid duplication
    let log_file_path = if should_log {
        Some(format!(
            "{}/sync_log_{:02}_{:02}_{}.log",
            log_path,
            start_time.day(),
            start_time.month(),
            start_time.year()
        ))
    } else {
        None
    };

    // Load filter config
    let filter_config = crate::config::load_config().filter_config;
    let mut all_files: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    let mut filtered_files = 0u64;
    
    for (local_path, s3_prefix) in mappings {
        let local_path_buf = PathBuf::from(&local_path);

        if local_path_buf.is_file() {
            if crate::utils::should_include_file(&local_path_buf, local_path_buf.parent().unwrap_or(&local_path_buf), &filter_config) {
                log_mappings.push(format!("File: {} -> S3: {}", local_path, s3_prefix));
                all_files.push((local_path_buf.clone(), local_path_buf.clone(), s3_prefix));
            } else {
                filtered_files += 1;
                info!("Filtered out file: {}", local_path);
            }
        } else {
            log_mappings.push(format!("Folder: {} -> S3 Folder: {}", local_path, s3_prefix));
            let files = WalkDir::new(&local_path_buf)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| {
                    let file_path = e.path().to_path_buf();
                    if crate::utils::should_include_file(&file_path, &local_path_buf, &filter_config) {
                        Some(e)
                    } else {
                        filtered_files += 1;
                        info!("Filtered out file: {}", file_path.display());
                        None
                    }
                })
                .map(|e| {
                    let file_path = e.path().to_path_buf();
                    let relative = file_path.strip_prefix(&local_path_buf).unwrap_or(&file_path);
                    let clean_rel = relative.to_string_lossy().replace('\\', "/");
                    let final_key = if clean_rel.is_empty() {
                        s3_prefix.clone()
                    } else {
                        format!("{}/{}", s3_prefix.trim_end_matches('/'), clean_rel.trim_start_matches('/'))
                    };
                    (file_path, local_path_buf.clone(), final_key)
                });
            all_files.extend(files);
        }
    }

    // Update status if files were filtered
    if filtered_files > 0 {
        update_status(
            &ui_handle,
            format!("Đã lọc {} files, chuẩn bị upload {} files...", filtered_files, all_files.len()),
            0.05,
            false,
        );
    }

    if should_log && !log_mappings.is_empty() {
        if let Some(ref log_file) = log_file_path {
            match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(mut file) => {
                    if writeln!(file, "--------------------------------------------------").is_err()
                        || writeln!(file, "Sync Session Started - Bucket: {}", bucket_name).is_err()
                    {
                        warn!("Failed to write sync session header to log file: {}", log_file);
                    }
                    for mapping in &log_mappings {
                        if writeln!(file, "{}", mapping).is_err() {
                            warn!("Failed to write mapping to log file: {}", log_file);
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open log file '{}': {}", log_file, e);
                }
            }
        }
    }

    let total_files = all_files.len();
    if total_files == 0 {
        update_status(&ui_handle, "Không có file nào để upload!".to_string(), 1.0, false);
        return Ok(());
    }

    let concurrency = std::env::var("S3_SYNC_CONCURRENCY")
        .unwrap_or_else(|_| "50".to_string())
        .parse()
        .unwrap_or(50);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut set = JoinSet::new();
    let completed_count = Arc::new(tokio::sync::Mutex::new(0));
    let failed_files: Arc<Mutex<Vec<FileError>>> = Arc::new(Mutex::new(Vec::new()));

    for (path, _base_path, key) in all_files {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let ui_handle = ui_handle.clone();
        let bucket_name = bucket_name.clone();
        let completed_count = Arc::clone(&completed_count);
        let failed_files = Arc::clone(&failed_files);

        set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            info!("Map local file: {:?} -> S3 Key: {}", path, key);
            let display_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let _mime_type = get_mime_type(&path);
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let file_size = get_file_size(&path);

            match upload_with_retry(
                &client,
                &path,
                &key,
                &bucket_name,
                &ui_handle,
                &display_name,
                total_files,
                &completed_count,
            ).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    let file_error = FileError {
                        file_path: path.display().to_string(),
                        s3_key: key.clone(),
                        error_type: if e.contains("read") || e.contains("disk") {
                            "File Read Error".to_string()
                        } else {
                            "S3 Upload Error".to_string()
                        },
                        error_message: e.clone(),
                        timestamp,
                        file_size,
                        retry_count: MAX_RETRIES,
                    };
                    failed_files.lock().await.push(file_error);
                    Err(e)
                }
            }
        });
    }

    let mut failed_files_list: Vec<FileError> = Vec::new();
    let mut simple_error_messages: Vec<String> = Vec::new();
    
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Err(e)) => {
                error!("{}", e);
                simple_error_messages.push(e);
            }
            Err(e) => {
                let err_msg = format!("Task Join Error: {}", e);
                error!("{}", err_msg);
                simple_error_messages.push(err_msg);
            }
            _ => {}
        }
    }
    
    // Transfer detailed errors from Arc<Mutex> to Vec
    let detailed_errors = failed_files.lock().await.clone();
    failed_files_list.extend(detailed_errors);
    
    let has_error = !failed_files_list.is_empty() || !simple_error_messages.is_empty();
    let success_count = total_files - failed_files_list.len();
    
    // Update status for simple errors that don't have FileError
    for err in &simple_error_messages {
        let file_error = FileError {
            file_path: "Unknown".to_string(),
            s3_key: "Unknown".to_string(),
            error_type: "Unknown Error".to_string(),
            error_message: err.clone(),
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            file_size: None,
            retry_count: 0,
        };
        failed_files_list.push(file_error);
    }

    if !has_error {
        update_status(&ui_handle, "Đồng bộ hoàn tất!".to_string(), 1.0, false);
    } else {
        update_status(
            &ui_handle,
            format!("Hoàn tất {}/{}, {} lỗi", success_count, total_files, failed_files_list.len()),
            1.0,
            true,
        );
    }

    if should_log {
        if let Some(ref log_file) = log_file_path {
            let end_time = Local::now();
            let status = if !has_error {
                "success".to_string()
            } else {
                format!("failed ({} errors)", failed_files_list.len())
            };
            
            // Write detailed error logs
            if !failed_files_list.is_empty() {
                let detailed_log_file = format!("{}/errors_{:02}_{:02}_{:02}_{:02}_{:02}.log",
                    log_path, end_time.day(), end_time.month(), end_time.hour(), end_time.minute(), end_time.second());
                
                for error in &failed_files_list {
                    if let Err(e) = write_detailed_error_log(&detailed_log_file, error) {
                        warn!("Failed to write detailed error log: {}", e);
                    }
                }
                
                // Write error summary
                if let Err(e) = write_error_summary(&detailed_log_file, &failed_files_list) {
                    warn!("Failed to write error summary: {}", e);
                }
            }
            
            match OpenOptions::new().create(true).append(true).open(log_file) {
                Ok(mut file) => {
                    // Summary line
                    let _ = writeln!(
                        file,
                        "Uploaded: {}/{} files | Time: {} | Bucket: {} | Status: {}",
                        success_count,
                        total_files,
                        end_time.format("%Y-%m-%d %H:%M:%S"),
                        bucket_name,
                        status
                    );

                    // Error details (if any)
                    if !failed_files_list.is_empty() {
                        let _ = writeln!(file, "Errors:");
                        for err in &failed_files_list {
                            let _ = writeln!(file, "  - [{}] {} -> {}: {}", 
                                err.error_type, err.file_path, err.s3_key, err.error_message);
                        }
                    }

                    let _ = writeln!(file, "--------------------------------------------------");
                }
                Err(e) => {
                    warn!("Failed to open log file '{}': {}", log_file, e);
                }
            }
        }
    }

    Ok(())
}
