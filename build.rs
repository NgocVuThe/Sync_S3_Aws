fn main() {
    // Kill existing process to avoid file lock during build/run
    #[cfg(windows)]
    let _ = std::process::Command::new("powershell")
        .args(&[
            "-Command",
            "Get-Process -Name rust_project -ErrorAction SilentlyContinue | Stop-Process -Force",
        ])
        .output();

    slint_build::compile("ui/app_window.slint").unwrap();

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("ui/app_icon.ico");
        res.compile().unwrap();
    }
}
