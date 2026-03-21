use crate::chrome_detect::DetectedProfile;

/// Serialization wrapper — only the profiles section, no security defaults.
#[derive(serde::Serialize)]
struct InitConfig {
    profiles: Vec<crate::config::ChromeProfile>,
}

/// Generate config TOML from detected Chrome profiles.
/// Deduplicates suggested names by appending `_2`, `_3`, etc.
pub fn generate_config_toml(profiles: &[DetectedProfile]) -> String {
    let mut used_names: Vec<String> = Vec::new();
    let mut chrome_profiles: Vec<crate::config::ChromeProfile> = Vec::new();

    for p in profiles {
        let mut name = p.suggested_name.clone();
        if used_names.contains(&name) {
            let mut n = 2u32;
            loop {
                let candidate = format!("{}_{}", p.suggested_name, n);
                if !used_names.contains(&candidate) {
                    name = candidate;
                    break;
                }
                n += 1;
            }
        }
        used_names.push(name.clone());

        let display_name = match &p.email {
            Some(e) => format!("{} ({})", p.display_name, e),
            None => p.display_name.clone(),
        };

        chrome_profiles.push(crate::config::ChromeProfile {
            name,
            display_name,
            user_data_dir: p.user_data_dir.clone(),
        });
    }

    toml::to_string(&InitConfig {
        profiles: chrome_profiles,
    })
    .unwrap_or_else(|e| format!("# serialization error: {}\n", e))
}

/// Run `pagerunner init`. Detects Chrome profiles and writes ~/.pagerunner/config.toml.
pub fn run(force: bool) -> crate::error::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| {
        crate::error::PagerunnerError::Config("Cannot find home directory".into())
    })?;
    let config_path = home.join(".pagerunner/config.toml");

    if config_path.exists() && !force {
        return Err(crate::error::PagerunnerError::Config(format!(
            "Config already exists at {}.\nUse --force to overwrite.",
            config_path.display()
        )));
    }

    let chrome_root = crate::chrome_detect::chrome_user_data_root().ok_or_else(|| {
        crate::error::PagerunnerError::Config(
            "Unsupported OS — Chrome detection works on macOS and Linux only.".into(),
        )
    })?;

    if !chrome_root.exists() {
        return Err(crate::error::PagerunnerError::Config(format!(
            "Chrome user data directory not found at {}.\nIs Google Chrome installed?",
            chrome_root.display()
        )));
    }

    let profiles = crate::chrome_detect::detect_profiles(&chrome_root)?;

    if profiles.is_empty() {
        return Err(crate::error::PagerunnerError::Config(format!(
            "No Chrome profiles found in {}.\nOpen Chrome at least once to create a profile.",
            chrome_root.display()
        )));
    }

    let toml_content = generate_config_toml(&profiles);
    std::fs::create_dir_all(
        config_path
            .parent()
            .ok_or_else(|| crate::error::PagerunnerError::Config("Invalid config path".into()))?,
    )
    .map_err(crate::error::PagerunnerError::Io)?;
    std::fs::write(&config_path, &toml_content).map_err(crate::error::PagerunnerError::Io)?;

    println!(
        "Wrote {} profile{} to {}\n",
        profiles.len(),
        if profiles.len() == 1 { "" } else { "s" },
        config_path.display()
    );
    for p in &profiles {
        let label = p.email.as_deref().unwrap_or(&p.display_name);
        println!("  {} — {}", p.suggested_name, label);
    }
    println!("\nRun `pagerunner status` to verify, then register with Claude Code:");
    println!("  claude mcp add pagerunner $(which pagerunner) mcp");
    #[cfg(feature = "ner")]
    println!("  pagerunner download-model   # enable PERSON/ORG name detection (~65MB model)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_detect::DetectedProfile;

    fn make_profile(dir: &str, name: &str, email: &str, slug: &str, path: &str) -> DetectedProfile {
        DetectedProfile {
            dir: dir.into(),
            display_name: name.into(),
            email: if email.is_empty() {
                None
            } else {
                Some(email.into())
            },
            suggested_name: slug.into(),
            user_data_dir: path.into(),
        }
    }

    #[test]
    fn generates_toml_from_detected_profiles() {
        let profiles = vec![
            make_profile(
                "Default",
                "Person 1",
                "alice@gmail.com",
                "alice",
                "/c/Default",
            ),
            make_profile(
                "Profile 1",
                "Work",
                "alice@corp.com",
                "alice_corp",
                "/c/Profile 1",
            ),
        ];
        let toml = generate_config_toml(&profiles);
        assert!(toml.contains("[[profiles]]"));
        assert!(toml.contains(r#"name = "alice""#));
        assert!(toml.contains("alice@gmail.com"));
        assert!(toml.contains(r#"name = "alice_corp""#));
        assert!(toml.contains(r#"display_name = "Work (alice@corp.com)""#));
        // Round-trip: output must be valid TOML
        let parsed: crate::config::PagerunnerConfig =
            toml::from_str(&toml).expect("generate_config_toml must produce valid TOML");
        assert_eq!(parsed.profiles.len(), 2);
        assert_eq!(parsed.profiles[0].name, "alice");
    }

    #[test]
    fn deduplicates_suggested_names() {
        let profiles = vec![
            make_profile(
                "Default",
                "Person 1",
                "alice@gmail.com",
                "alice",
                "/c/Default",
            ),
            make_profile(
                "Profile 1",
                "Person 2",
                "alice@work.com",
                "alice",
                "/c/Profile 1",
            ),
        ];
        let toml = generate_config_toml(&profiles);
        assert!(toml.contains(r#"name = "alice""#));
        assert!(toml.contains(r#"name = "alice_2""#));
    }

    #[test]
    fn profile_without_email_uses_display_name_only() {
        let profiles = vec![make_profile(
            "Default",
            "Person 1",
            "",
            "default",
            "/c/Default",
        )];
        let toml = generate_config_toml(&profiles);
        assert!(toml.contains(r#"display_name = "Person 1""#));
        assert!(!toml.contains("()")); // no trailing empty parens
    }
}
