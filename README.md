# RustProAI - S3 Sync Tool

A desktop GUI application for syncing local files and folders to AWS S3 buckets.

## Features

- Upload local files and folders to S3
- Concurrent uploads with progress tracking
- AWS credential testing
- MIME type detection for web assets
- Multi-language support (English/Vietnamese UI)

## Installation

1. Clone the repository
2. Install Rust (https://rustup.rs/)
3. Run `cargo build --release`

## Usage

1. Launch the application
2. Enter AWS credentials and bucket details
3. Test connection
4. Select files/folders to upload
5. Start sync

## Architecture

- **UI**: Slint for modern GUI
- **Async**: Tokio for concurrency
- **AWS**: AWS SDK for S3 operations
- **Modules**:
  - `utils`: Utility functions (MIME types, UI updates)
  - `s3_client`: S3 operations
  - `ui_handlers`: Event handlers

## Development

- `cargo run` to run in debug mode
- `cargo test` to run tests
- `cargo fmt` and `cargo clippy` for code quality

## Security Note

Credentials are handled in memory. For production, consider secure credential providers.