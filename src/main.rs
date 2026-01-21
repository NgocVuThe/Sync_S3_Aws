use tracing::info;
use tracing_appender;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use rust_project::*;

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
    let ui = AppWindow::new()?;

    ui_handlers::setup_all_handlers(&ui);

    ui.run()?;
    Ok(())
}
