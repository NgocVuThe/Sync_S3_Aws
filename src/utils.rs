use crate::*;
use glob::Pattern;
use std::fs;
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

/// Checks if a file should be included based on filtering rules.
/// Returns true if the file should be included, false if excluded.
pub fn should_include_file(
    file_path: &Path,
    base_path: &Path,
    filter_config: &crate::config::FilterConfig,
) -> bool {
    if !filter_config.enable_filtering {
        return true;
    }

    // Check file size
    if let Ok(metadata) = fs::metadata(file_path) {
        if metadata.len() > filter_config.max_file_size {
            return false;
        }
    }

    // Get relative path from base for pattern matching
    let relative_path = match file_path.strip_prefix(base_path) {
        Ok(path) => path,
        Err(_) => file_path,
    };

    let path_str = relative_path.to_string_lossy();
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();

    // Check exclude patterns first
    for pattern in &filter_config.exclude_patterns {
        if matches_pattern(&path_str, &file_name, pattern) {
            return false;
        }
    }

    // If include patterns are specified, check them
    if !filter_config.include_patterns.is_empty() {
        for pattern in &filter_config.include_patterns {
            if matches_pattern(&path_str, &file_name, pattern) {
                return true;
            }
        }
        // If include patterns exist but none matched, exclude
        return false;
    }

    true
}

/// Checks if a path matches a glob pattern.
fn matches_pattern(path_str: &str, file_name: &str, pattern: &str) -> bool {
    // Try to match the full path first
    if let Ok(full_pattern) = Pattern::new(pattern) {
        if full_pattern.matches(path_str) {
            return true;
        }
    }

    // Try to match just the filename
    if let Ok(file_pattern) = Pattern::new(pattern) {
        if file_pattern.matches(file_name) {
            return true;
        }
    }

    // Simple substring match for non-glob patterns
    if !pattern.contains('*') && !pattern.contains('?') {
        if path_str.contains(pattern) || file_name.contains(pattern) {
            return true;
        }
    }

    false
}

/// Gets filtering statistics for a directory.
pub fn get_filtering_stats(
    dir_path: &Path,
    filter_config: &crate::config::FilterConfig,
) -> Result<FilteringStats, std::io::Error> {
    let mut total_files = 0u64;
    let mut included_files = 0u64;
    let mut excluded_files = 0u64;
    let mut total_size = 0u64;
    let mut excluded_size = 0u64;

    for entry in walkdir::WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        total_files += 1;

        if let Ok(metadata) = fs::metadata(path) {
            let file_size = metadata.len();
            total_size += file_size;

            if should_include_file(path, dir_path, filter_config) {
                included_files += 1;
            } else {
                excluded_files += 1;
                excluded_size += file_size;
            }
        }
    }

    Ok(FilteringStats {
        total_files,
        included_files,
        excluded_files,
        total_size,
        excluded_size,
    })
}

#[derive(Debug, Clone)]
pub struct FilteringStats {
    pub total_files: u64,
    pub included_files: u64,
    pub excluded_files: u64,
    pub total_size: u64,
    pub excluded_size: u64,
}

impl FilteringStats {
    pub fn exclusion_rate(&self) -> f64 {
        if self.total_files == 0 {
            0.0
        } else {
            self.excluded_files as f64 / self.total_files as f64
        }
    }

    pub fn size_savings(&self) -> f64 {
        if self.total_size == 0 {
            0.0
        } else {
            self.excluded_size as f64 / self.total_size as f64
        }
    }
}

/// Validates if a string is a valid glob pattern.
pub fn is_valid_glob_pattern(pattern: &str) -> bool {
    glob::Pattern::new(pattern).is_ok()
}

/// Validates a list of comma-separated glob patterns.
/// Returns a list of invalid patterns.
pub fn validate_glob_patterns(patterns_str: &str) -> Vec<String> {
    patterns_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter(|s| glob::Pattern::new(s).is_err())
        .map(|s| s.to_string())
        .collect()
}

/// Updates the UI status text and progress bar.
/// Must be called from within an event loop.
pub fn update_status(
    ui_handle: &slint::Weak<AppWindow>,
    text: String,
    progress: f32,
    is_error: bool,
) {
    let _ = ui_handle.upgrade_in_event_loop(move |ui| {
        ui.set_status_text(text.into());
        ui.set_progress(progress);
        ui.set_is_error(is_error);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FilterConfig;
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

    #[test]
    fn test_should_include_file_disabled_filtering() {
        let config = FilterConfig {
            enable_filtering: false,
            ..Default::default()
        };

        // All files should be included when filtering is disabled
        assert!(should_include_file(
            Path::new("node_modules/package.json"),
            Path::new("."),
            &config
        ));
        assert!(should_include_file(
            Path::new("test.tmp"),
            Path::new("."),
            &config
        ));
    }

    #[test]
    fn test_exclude_patterns() {
        let config = FilterConfig {
            enable_filtering: true,
            exclude_patterns: vec!["node_modules".to_string(), "*.tmp".to_string()],
            include_patterns: vec![],
            max_file_size: 100 * 1024 * 1024,
        };

        assert!(!should_include_file(
            Path::new("node_modules/package.json"),
            Path::new("."),
            &config
        ));

        assert!(!should_include_file(
            Path::new("test.tmp"),
            Path::new("."),
            &config
        ));

        assert!(should_include_file(
            Path::new("index.html"),
            Path::new("."),
            &config
        ));
    }

    #[test]
    fn test_include_patterns() {
        let config = FilterConfig {
            enable_filtering: true,
            exclude_patterns: vec![],
            include_patterns: vec!["*.html".to_string(), "*.css".to_string()],
            max_file_size: 100 * 1024 * 1024,
        };

        assert!(should_include_file(
            Path::new("index.html"),
            Path::new("."),
            &config
        ));

        assert!(should_include_file(
            Path::new("styles.css"),
            Path::new("."),
            &config
        ));

        assert!(!should_include_file(
            Path::new("script.js"),
            Path::new("."),
            &config
        ));

        assert!(!should_include_file(
            Path::new("README.md"),
            Path::new("."),
            &config
        ));
    }

    #[test]
    fn test_max_file_size() {
        let config = FilterConfig {
            enable_filtering: true,
            exclude_patterns: vec![],
            include_patterns: vec![],
            max_file_size: 1024, // 1KB
        };

        // This test requires actual file size, which is hard to test without real files
        // For now, just test the pattern matching logic
        assert!(should_include_file(
            Path::new("index.html"),
            Path::new("."),
            &config
        ));
    }

    #[test]
    fn test_filtering_stats() {
        let _config = FilterConfig::default();

        // This test would require a real directory structure
        // For now, just test the default values
        let stats = FilteringStats {
            total_files: 100,
            included_files: 80,
            excluded_files: 20,
            total_size: 1000000,
            excluded_size: 200000,
        };

        assert_eq!(stats.exclusion_rate(), 0.2);
        assert_eq!(stats.size_savings(), 0.2);
    }

    #[test]
    fn test_pattern_matching() {
        assert!(matches_pattern("index.html", "index.html", "index.html"));
        assert!(matches_pattern(
            "node_modules/package.json",
            "package.json",
            "node_modules"
        ));
        assert!(matches_pattern("test.tmp", "test.tmp", "*.tmp"));
        assert!(matches_pattern("styles/main.css", "main.css", "*.css"));

        assert!(!matches_pattern("index.html", "index.html", "*.css"));
        assert!(!matches_pattern("main.js", "main.js", "node_modules"));
    }
}
