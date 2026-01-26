#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tracing::info;
use tracing_appender;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use rust_project::*;

mod config;
mod s3_client;
mod ui_handlers;
mod utils;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize logging
    let file_appender = tracing_appender::rolling::never(".", "s3_debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(fmt::layer().with_writer(non_blocking))
        .with(fmt::layer())
        .init();

    info!("Ứng dụng S3 Sync Tool đang khởi động...");
    
    // Load saved config
    let app_config = config::load_config();
    info!("Config loaded from: {:?}", config::get_config_path());
    info!("Loaded log_path: '{}'", app_config.log_path);
    
    let ui = AppWindow::new()?;
    
    // Apply saved config to UI
    if !app_config.log_path.is_empty() {
        ui.set_log_path(app_config.log_path.into());
    }
    if !app_config.s3_base_path.is_empty() {
        ui.set_s3_base_path(app_config.s3_base_path.into());
    }
    
    // Apply filter config to UI
    let exclude_text = app_config.filter_config.exclude_patterns.join(", ");
    let include_text = app_config.filter_config.include_patterns.join(", ");
    let max_size_text = (app_config.filter_config.max_file_size / (1024 * 1024)).to_string();
    
    ui.set_enable_filtering(app_config.filter_config.enable_filtering);
    ui.set_exclude_patterns_text(exclude_text.into());
    ui.set_include_patterns_text(include_text.into());
    ui.set_max_file_size_text(max_size_text.into());

    if !app_config.selected_bucket.is_empty() {
        ui.set_bucket_name(app_config.selected_bucket.into());
    }

    ui_handlers::setup_all_handlers(&ui);

    ui.run()?;
    Ok(())
}
