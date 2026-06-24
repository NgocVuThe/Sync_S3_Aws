use aws_sdk_cloudfront::Client;
use aws_sdk_cloudfront::config::{Credentials, Region};
use aws_sdk_cloudfront::types::{InvalidationBatch, Paths};
use tracing::info;

pub async fn create_cloudfront_client(
    acc_key: String,
    sec_key: String,
    sess_token: Option<String>,
    region: String,
) -> Result<Client, aws_sdk_cloudfront::Error> {
    let credentials = Credentials::new(acc_key, sec_key, sess_token, None, "manual");
    let config = aws_config::from_env()
        .credentials_provider(credentials)
        .region(Region::new(region))
        .load()
        .await;
    Ok(Client::new(&config))
}

pub async fn invalidate_cache(
    client: &Client,
    distribution_id: &str,
    paths: Vec<String>,
) -> Result<String, String> {
    if distribution_id.is_empty() {
        return Err("Distribution ID is empty".to_string());
    }

    if paths.is_empty() {
        return Err("Paths cannot be empty".to_string());
    }

    let caller_ref = format!(
        "s3synctool-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_else(|_| 0)
    );

    let paths_obj = Paths::builder()
        .quantity(paths.len() as i32)
        .set_items(Some(paths))
        .build()
        .map_err(|e| format!("Failed to build Paths: {}", e))?;

    let batch = InvalidationBatch::builder()
        .paths(paths_obj)
        .caller_reference(caller_ref)
        .build()
        .map_err(|e| format!("Failed to build InvalidationBatch: {}", e))?;

    let result = client
        .create_invalidation()
        .distribution_id(distribution_id)
        .invalidation_batch(batch)
        .send()
        .await
        .map_err(|e| format!("CloudFront API error: {}", e))?;

    let invalidation_id = result
        .invalidation()
        .map(|inv| inv.id().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    info!("Created CloudFront invalidation: {} for distribution: {}", invalidation_id, distribution_id);
    Ok(invalidation_id)
}

pub async fn test_cloudfront_access(
    client: &Client,
    distribution_id: &str,
) -> Result<(), String> {
    if distribution_id.is_empty() {
        return Err("Distribution ID is empty".to_string());
    }

    match client
        .get_distribution()
        .id(distribution_id)
        .send()
        .await
    {
        Ok(_) => {
            info!("CloudFront access test successful for distribution: {}", distribution_id);
            Ok(())
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("AccessDenied") {
                Err("CloudFront Access Denied: Check permissions (GetDistribution)".to_string())
            } else if err_msg.contains("NoSuchDistribution") {
                Err(format!("CloudFront distribution '{}' not found", distribution_id))
            } else {
                Err(format!("CloudFront connection failed: {}", err_msg))
            }
        }
    }
}
