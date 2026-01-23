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
    
    let ui = AppWindow::new()?;
    
    // Apply saved log_path to UI
    if !app_config.log_path.is_empty() {
        ui.set_log_path(app_config.log_path.into());
    }

    ui_handlers::setup_all_handlers(&ui);

    ui.run()?;
    Ok(())
}
