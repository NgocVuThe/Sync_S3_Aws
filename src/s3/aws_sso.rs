/// Run `aws sso login --profile <name>` (opens browser, waits for user auth).
/// Uses tokio::process::Command (async) — NOT std::process::Command.
pub async fn run_sso_login(profile: &str) -> Result<String, String> {
    let output = tokio::process::Command::new("aws")
        .args(["sso", "login", "--profile", profile])
        .output()
        .await
        .map_err(|e| format!("Không tìm thấy AWS CLI: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
