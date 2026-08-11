use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AwsProfile {
    pub name: String,
    pub is_sso: bool,
}

fn aws_config_path() -> Option<PathBuf> {
    // Prioritize AWS_CONFIG_FILE env var (standard AWS SDK behavior)
    if let Ok(p) = std::env::var("AWS_CONFIG_FILE") {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            let p = std::path::Path::new(&p).join(".aws").join("config");
            if p.exists() {
                return Some(p);
            }
        }
    }
    if let Ok(h) = std::env::var("HOME") {
        let p = std::path::Path::new(&h).join(".aws").join("config");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Parse raw AWS config file content and return list of profiles.
///
/// - `[default]` (no prefix) → profile named "default".
/// - `[profile NAME]` → profile named "NAME".
/// - `[sso-session ...]` → skip (not a selectable profile).
/// - Set `is_sso=true` when a line under a profile starts with `sso_start_url` or `sso_session`.
pub(crate) fn parse_aws_config(content: &str) -> Vec<AwsProfile> {
    let mut out: Vec<AwsProfile> = Vec::new();
    let mut current: Option<usize> = None;

    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let header = &t[1..t.len() - 1];
            let lower = header.to_ascii_lowercase();
            if lower == "default" {
                out.push(AwsProfile {
                    name: "default".to_string(),
                    is_sso: false,
                });
                current = Some(out.len() - 1);
            } else if lower.strip_prefix("profile ").is_some() {
                let original_name = header["profile ".len()..].trim().to_string();
                out.push(AwsProfile {
                    name: original_name,
                    is_sso: false,
                });
                current = Some(out.len() - 1);
            } else {
                current = None; // [sso-session ...] or unknown section — skip
            }
            continue;
        }
        if let Some(idx) = current {
            if t.starts_with("sso_start_url") || t.starts_with("sso_session") {
                out[idx].is_sso = true;
            }
        }
    }
    out
}

/// Parse ~/.aws/config, return list of profiles.
pub fn list_aws_profiles() -> Vec<AwsProfile> {
    let path = match aws_config_path() {
        Some(p) => p,
        None => return vec![],
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    parse_aws_config(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_aws_config: core parsing logic (no disk IO) ----

    #[test]
    fn test_sso_profile_detected() {
        let content = r#"
[profile dev-sso]
sso_session = my-sso
sso_account_id = 123456789012
sso_role_name = DeveloperAccess
region = ap-northeast-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "dev-sso");
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_sso_profile_via_start_url() {
        let content = r#"
[profile sso-user]
sso_start_url = https://my-sso-portal.awsapps.com/start
region = us-east-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "sso-user");
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_default_profile() {
        let content = r#"
[default]
region = ap-northeast-1
output = json
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
        // No sso_start_url or sso_session → is_sso should be false
        assert!(!profiles[0].is_sso);
    }

    #[test]
    fn test_default_profile_with_sso_session() {
        let content = r#"
[default]
sso_session = my-sso
region = us-east-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_sso_session_block_skipped() {
        let content = r#"
[sso-session mysso]
sso_region = ap-northeast-1
sso_start_url = https://my-sso-portal.awsapps.com/start
sso_registration_scopes = sso:account:access

[profile dev-sso]
sso_session = mysso
sso_account_id = 123456789012
region = ap-northeast-1
"#;
        let profiles = parse_aws_config(content);
        // sso-session block should NOT appear; only dev-sso should be present
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "dev-sso");
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_mixed_sso_and_iam_profiles() {
        let content = r#"
[profile dev-sso]
sso_session = mysso
sso_account_id = 123456789012
region = ap-northeast-1

[profile static]
region = us-west-2
output = json

[default]
region = ap-northeast-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 3);

        // dev-sso: is_sso = true
        assert_eq!(profiles[0].name, "dev-sso");
        assert!(profiles[0].is_sso);

        // static: no sso keys → is_sso = false
        assert_eq!(profiles[1].name, "static");
        assert!(!profiles[1].is_sso);

        // default: no sso keys → is_sso = false
        assert_eq!(profiles[2].name, "default");
        assert!(!profiles[2].is_sso);
    }

    #[test]
    fn test_empty_content() {
        let profiles = parse_aws_config("");
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_whitespace_only_content() {
        let profiles = parse_aws_config("   \n  \n   ");
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_profile_name_preserves_original_casing() {
        let content = "[profile MyDevSSO]\nsso_session = x\n";
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "MyDevSSO");
    }

    #[test]
    fn test_unknown_section_skipped() {
        let content = r#"
[something-weird]
key = value

[default]
region = us-east-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
    }

    #[test]
    fn test_comments_and_blank_lines() {
        let content = r#"
# This is a comment
[profile dev-sso]
# Another comment
sso_session = mysso

  # indented comment
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "dev-sso");
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_sso_session_key_value_with_equals() {
        // sso_session = value should trigger is_sso
        let content = "[profile test]\nsso_session = my-session\n";
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_sso_start_url_key_value_with_equals() {
        // sso_start_url = value should trigger is_sso
        let content = "[profile test]\nsso_start_url = https://portal.awsapps.com/start\n";
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_sso);
    }

    #[test]
    fn test_sso_inside_default_braces_not_matched() {
        // "sso" or "sso_start" without _url should NOT match
        let content = "[default]\nsso = foo\nsso_start = bar\nregion = us-east-1\n";
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].is_sso);
    }

    #[test]
    fn test_multiple_sso_sessions_all_skipped() {
        let content = r#"
[sso-session a]
sso_start_url = https://a.example.com

[sso-session b]
sso_start_url = https://b.example.com

[profile dev]
sso_session = a
region = us-east-1
"#;
        let profiles = parse_aws_config(content);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "dev");
    }

    // ---- list_aws_profiles: disk-IO path (just checks it doesn't panic) ----

    #[test]
    fn test_list_aws_profiles_does_not_panic() {
        // list_aws_profiles reads from real ~/.aws/config — just verify it doesn't crash
        let _profiles = list_aws_profiles();
    }
}
