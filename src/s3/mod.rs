pub mod aws_profiles;
pub mod aws_sso;
pub mod client;
pub mod cloudfront;
pub mod prefix;
pub mod sync;
pub mod download;

use crate::config;

/// Sentinel value in the AWS profile ComboBox that represents manual credential entry.
/// Chosen to be extremely unlikely as an actual AWS profile name.
pub const MANUAL_SENTINEL: &str = "— Nhập tay —";

/// Source of credentials: manual (fallback) or AWS profile.
pub enum AwsCredSource {
    Manual {
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
    },
    Profile { name: String },
}

/// Determine whether to use Manual or Profile based on saved config.
/// This is the SINGLE call site to use at every client-creation point,
/// avoiding the need to change signatures of 5 Slint callbacks.
pub fn resolve_cred_source(
    manual_access_key: String,
    manual_secret_key: String,
    manual_session_token: Option<String>,
) -> AwsCredSource {
    let profile = config::load_config().selected_profile;
    if profile.is_empty() {
        AwsCredSource::Manual {
            access_key: manual_access_key,
            secret_key: manual_secret_key,
            session_token: manual_session_token,
        }
    } else {
        AwsCredSource::Profile { name: profile }
    }
}

/// Like `resolve_cred_source`, but accepts the profile name directly
/// (avoids loading config from disk on every call, useful for preview clients).
pub fn resolve_cred_source_with_profile(
    profile: String,
    manual_access_key: String,
    manual_secret_key: String,
    manual_session_token: Option<String>,
) -> AwsCredSource {
    if profile.is_empty() || profile == MANUAL_SENTINEL {
        AwsCredSource::Manual {
            access_key: manual_access_key,
            secret_key: manual_secret_key,
            session_token: manual_session_token,
        }
    } else {
        AwsCredSource::Profile { name: profile }
    }
}

pub use aws_profiles::{list_aws_profiles};
pub use aws_sso::run_sso_login;
pub use client::{create_s3_client, test_bucket_access, try_create_preview_client};
pub use cloudfront::{create_cloudfront_client, invalidate_cache, test_cloudfront_access};
pub use prefix::{find_best_s3_prefix, get_preview_prefix, GlobalPrefixCache};
pub use sync::sync_to_s3;
pub use download::download_from_s3;
