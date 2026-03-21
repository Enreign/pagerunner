use serde_json::Value;

#[derive(Debug, Clone)]
pub struct DetectedProfile {
    /// Chrome subdirectory name, e.g. "Default" or "Profile 1"
    pub dir: String,
    pub display_name: String,
    pub email: Option<String>,
    /// Suggested slug for the `name` field in config
    pub suggested_name: String,
    /// Absolute path to this profile's user data dir
    pub user_data_dir: String,
}

/// Returns the Chrome user-data root directory for the current OS, or None on unsupported OS.
pub fn chrome_user_data_root() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    #[cfg(target_os = "macos")]
    {
        return Some(home.join("Library/Application Support/Google/Chrome"));
    }
    #[cfg(target_os = "linux")]
    {
        return Some(home.join(".config/google-chrome"));
    }
    #[allow(unreachable_code)]
    None
}

/// Read Chrome's Local State file and return detected profiles.
pub fn detect_profiles(
    chrome_root: &std::path::Path,
) -> crate::error::Result<Vec<DetectedProfile>> {
    let local_state_path = chrome_root.join("Local State");
    let content = std::fs::read_to_string(&local_state_path).map_err(|e| {
        crate::error::PagerunnerError::Config(format!(
            "Cannot read Chrome Local State at {}: {}",
            local_state_path.display(),
            e
        ))
    })?;
    parse_local_state(&content, chrome_root.to_str().unwrap_or(""))
}

pub fn parse_local_state(
    json: &str,
    chrome_root: &str,
) -> crate::error::Result<Vec<DetectedProfile>> {
    let v: Value = serde_json::from_str(json).map_err(|e| {
        crate::error::PagerunnerError::Config(format!("Invalid Local State JSON: {}", e))
    })?;

    let info_cache = &v["profile"]["info_cache"];
    let obj = info_cache.as_object().ok_or_else(|| {
        crate::error::PagerunnerError::Config("No profile.info_cache in Local State".into())
    })?;

    let mut profiles: Vec<DetectedProfile> = obj
        .iter()
        .map(|(dir, info)| {
            let display_name = info["name"].as_str().unwrap_or(dir).to_string();
            let email = info["user_name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let suggested_name = email
                .as_deref()
                .map(slugify)
                .unwrap_or_else(|| slugify(dir));
            DetectedProfile {
                dir: dir.clone(),
                display_name,
                email,
                suggested_name,
                user_data_dir: format!("{}/{}", chrome_root, dir),
            }
        })
        .collect();

    // Stable order: Default first, then alphabetical
    profiles.sort_by(|a, b| {
        if a.dir == "Default" {
            return std::cmp::Ordering::Less;
        }
        if b.dir == "Default" {
            return std::cmp::Ordering::Greater;
        }
        a.dir.cmp(&b.dir)
    });

    Ok(profiles)
}

/// Convert an email or directory name to a config-friendly slug.
/// "alice@corp.com" → "alice", "my.name+tag@x.com" → "my_name_tag", "Default" → "default"
pub fn slugify(input: &str) -> String {
    let base = input.split('@').next().unwrap_or(input);
    let slug: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    slug.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_state_json() {
        let json = r#"{
            "profile": {
                "info_cache": {
                    "Default": { "name": "Person 1", "user_name": "alice@gmail.com" },
                    "Profile 1": { "name": "Work", "user_name": "alice@corp.com" }
                }
            }
        }"#;
        let profiles = parse_local_state(json, "/tmp/fake-chrome").unwrap();
        assert_eq!(profiles.len(), 2);
        let default = profiles.iter().find(|p| p.dir == "Default").unwrap();
        assert_eq!(default.email, Some("alice@gmail.com".into()));
        assert_eq!(default.display_name, "Person 1");
        assert_eq!(default.user_data_dir, "/tmp/fake-chrome/Default");
    }

    #[test]
    fn slugifies_name_from_email() {
        assert_eq!(slugify("alice@gmail.com"), "alice");
        assert_eq!(slugify("my.name+tag@corp.com"), "my_name_tag");
        assert_eq!(slugify("Default"), "default");
    }

    #[test]
    fn handles_missing_email() {
        let json = r#"{"profile":{"info_cache":{"Default":{"name":"Person 1","user_name":""}}}}"#;
        let profiles = parse_local_state(json, "/tmp/c").unwrap();
        assert_eq!(profiles[0].suggested_name, "default");
    }

    #[test]
    fn returns_err_on_malformed_json() {
        assert!(parse_local_state("not json", "/tmp/c").is_err());
    }
}
