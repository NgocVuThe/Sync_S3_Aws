---
name: rust-s3-sync
description: Hướng dẫn phát triển và mở rộng ứng dụng Rust S3 Sync Tool. Sử dụng khi cần thêm tính năng mới, sửa lỗi, hoặc tối ưu hiệu năng cho ứng dụng đồng bộ S3.
---

# Rust S3 Sync Tool - Development Skill

## Tổng quan dự án

Ứng dụng desktop sử dụng **Rust** + **Slint UI** để đồng bộ files/folders lên AWS S3.

## Cấu trúc dự án

```
rust_project/
├── src/
│   └── main.rs           # Entry point, AWS S3 logic, UI handlers
├── ui/
│   └── app_window.slint  # Slint UI definition
├── Cargo.toml            # Dependencies
└── build.rs              # Build script cho Slint
```

## Tech Stack

- **Rust**: tokio, aws-sdk-s3, walkdir, tracing
- **UI Framework**: Slint
- **File Dialog**: rfd crate
- **AWS SDK**: aws-sdk-s3, aws-config

## Các callback chính trong UI

| Callback | Mô tả |
|----------|-------|
| `select-folder()` | Mở dialog chọn nhiều folders |
| `select-files()` | Mở dialog chọn nhiều files |
| `clear-folders()` | Xóa tất cả paths đã chọn |
| `remove-folder(int)` | Xóa path tại index |
| `test-access(...)` | Kiểm tra kết nối AWS |
| `start-sync(...)` | Bắt đầu upload lên S3 |

## Logic Upload S3

### Folder
- S3 Key: `folder_name/relative_path`
- Ví dụ: `assets/images/logo.png`

### File đơn lẻ
- S3 Key: `filename` (upload vào root bucket)
- Ví dụ: `index.html`

## Hướng dẫn thêm tính năng mới

### 1. Thêm callback mới trong UI
```slint
// ui/app_window.slint
callback my-new-action(string);
```

### 2. Implement handler trong Rust
```rust
// src/main.rs
ui.on_my_new_action({
    let ui_handle = ui.as_weak();
    move |param| {
        // Logic xử lý
    }
});
```

### 3. Build và test
```bash
cargo build --release
cargo run
```

## MIME Types

Xử lý tự động trong hàm `get_mime_type()`:
- Fonts: woff2, woff, ttf, otf, eot
- Web: css, js, html
- Fallback: `mime_guess` crate

## Best Practices

1. **Concurrent uploads**: Sử dụng `Semaphore` giới hạn 10 concurrent uploads
2. **Progress tracking**: Cập nhật UI qua `update_status()`
3. **Error handling**: Log errors và hiển thị cho user
4. **Path normalization**: Replace `\\` với `/` cho S3 keys
