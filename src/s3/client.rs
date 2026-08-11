use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use crate::s3::{AwsCredSource, resolve_cred_source_with_profile};

/// Creates an S3 client with the given credential source and region.
pub async fn create_s3_client(
    cred: AwsCredSource,
    region: String,
) -> Result<Client, aws_sdk_s3::Error> {
    let mut builder = aws_config::from_env().region(Region::new(region));
    builder = match cred {
        AwsCredSource::Manual {
            access_key,
            secret_key,
            session_token,
        } => {
            let creds =
                Credentials::new(access_key, secret_key, session_token, None, "manual");
            builder.credentials_provider(creds)
        }
        AwsCredSource::Profile { name } => builder.profile_name(name),
    };
    let config = builder.load().await;
    Ok(Client::new(&config))
}

/// Creates a preview S3 client using profile or manual credentials.
/// Returns `Ok(None)` if insufficient credentials or empty bucket.
pub async fn try_create_preview_client(
    profile: String,
    acc_key: String,
    sec_key: String,
    sess_token: Option<String>,
    region: String,
    bucket: String,
) -> Result<Option<Client>, aws_sdk_s3::Error> {
    let has_creds = (!acc_key.is_empty() && !sec_key.is_empty()) || !profile.is_empty();
    if !has_creds || bucket.is_empty() {
        return Ok(None);
    }
    let cred = resolve_cred_source_with_profile(profile, acc_key, sec_key, sess_token);
    match create_s3_client(cred, region).await {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(e),
    }
}

/// Tests access to S3 bucket by attempting to head the bucket.
pub async fn test_bucket_access(client: &Client, bucket: &str) -> Result<(), aws_sdk_s3::Error> {
    client.head_bucket().bucket(bucket).send().await?;
    Ok(())
}
