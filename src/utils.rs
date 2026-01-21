use crate::*;
use mime_guess;
use std::path::Path;

/// Determines the MIME type of a file based on its extension.
/// Provides custom mappings for web assets and falls back to mime_guess.
pub fn get_mime_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "css" => "text/css",
        "js" => "application/javascript",
        "html" | "htm" => "text/html",
        _ => mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream"),
    }
}

/// Validates AWS credentials and bucket name.
/// Returns an error message if invalid, or None if valid.
pub fn validate_credentials(acc_key: &str, sec_key: &str, bucket: &str) -> Option<String> {
    if acc_key.trim().is_empty() {
        return Some("Access Key không được để trống".to_string());
    }
    if sec_key.trim().is_empty() {
        return Some("Secret Key không được để trống".to_string());
    }
    if bucket.trim().is_empty() {
        return Some("Bucket name không được để trống".to_string());
    }
    // Basic bucket name validation (AWS rules: 3-63 chars, lowercase, etc.)
    if bucket.len() < 3
        || bucket.len() > 63
        || !bucket
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || bucket.starts_with('-')
        || bucket.ends_with('-')
    {
        return Some(
            "Bucket name không hợp lệ (3-63 ký tự, chỉ chữ thường, số, dấu gạch ngang)".to_string(),
        );
    }
    None
}

/// Updates the UI status text and progress bar.
/// Must be called from within an event loop.
pub fn update_status(ui_handle: &slint::Weak<AppWindow>, text: String, progress: f32) {
    let _ = ui_handle.upgrade_in_event_loop(move |ui| {
        ui.set_status_text(text.into());
        ui.set_progress(progress);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_get_mime_type_custom() {
        assert_eq!(get_mime_type(Path::new("file.woff2")), "font/woff2");
        assert_eq!(get_mime_type(Path::new("file.css")), "text/css");
        assert_eq!(
            get_mime_type(Path::new("file.js")),
            "application/javascript"
        );
    }

    #[test]
    fn test_get_mime_type_fallback() {
        // Assuming mime_guess recognizes .txt as text/plain
        assert_eq!(get_mime_type(Path::new("file.txt")), "text/plain");
    }

    #[test]
    fn test_get_mime_type_unknown() {
        assert_eq!(
            get_mime_type(Path::new("file.unknown")),
            "application/octet-stream"
        );
    }
}
