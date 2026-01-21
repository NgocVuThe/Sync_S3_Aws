# Auto-kill script for Rust S3 Sync Tool
$ProcessName = "rust_project"

Write-Host "Checking for existing $ProcessName.exe processes..." -ForegroundColor Cyan
$processes = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue

if ($processes) {
    Write-Host "Found running instances. Killing them..." -ForegroundColor Yellow
    $processes | Stop-Process -Force
    # Wait a bit to ensure files are released
    Start-Sleep -Milliseconds 500
} else {
    Write-Host "No instances found." -ForegroundColor Gray
}

if ($args[0] -eq "run") {
    cargo run
} elseif ($args[0] -eq "build") {
    cargo build
} else {
    Write-Host "Usage: .\dev.ps1 [run|build]" -ForegroundColor Cyan
}
