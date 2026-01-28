use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};

/// Creates an S3 client with provided credentials and region.
pub async fn create_s3_client(
    acc_key: String,
    sec_key: String,
    sess_token: Option<String>,
    region: String,
) -> Result<Client, aws_sdk_s3::Error> {
    let credentials = Credentials::new(acc_key, sec_key, sess_token, None, "manual");
    let config = aws_config::from_env()
        .credentials_provider(credentials)
        .region(Region::new(region))
        .load()
        .await;
    Ok(Client::new(&config))
}

/// Tests access to S3 bucket by attempting to head the bucket.
pub async fn test_bucket_access(client: &Client, bucket: &str) -> Result<(), aws_sdk_s3::Error> {
    client.head_bucket().bucket(bucket).send().await?;
    Ok(())
}
