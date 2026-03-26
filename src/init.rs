use crate::chrome_detect::DetectedProfile;
use std::path::PathBuf;

/// Serialization wrapper — only the profiles section, no security defaults.
#[derive(serde::Serialize)]
struct InitConfig {
    profiles: Vec<crate::config::ChromeProfile>,
}

/// JSON result returned in --json mode.
#[derive(serde::Serialize, Default)]
struct InitJsonResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    already_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet_appended: Option<bool>,
}

/// The pagerunner usage snippet, embedded at compile time.
const SNIPPET: &str = include_str!("../docs/examples/CLAUDE.md");

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

/// Walk up from `start` looking for CLAUDE.md or AGENTS.md.
/// Stops at a `.git` boundary or filesystem root.
fn find_project_file(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        for name in &["CLAUDE.md", "AGENTS.md"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        if dir.join(".git").exists() {
            break;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Run `pagerunner init`. Detects Chrome profiles and writes ~/.pagerunner/config.toml.
/// With `--json`, outputs a JSON result instead of interactive prompts.
pub fn run(force: bool, json: bool) -> crate::error::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| {
        crate::error::PagerunnerError::Config("Cannot find home directory".into())
    })?;
    let config_path = home.join(".pagerunner/config.toml");

    let mut json_result = InitJsonResult {
        ok: true,
        config_path: Some(config_path.display().to_string()),
        ..Default::default()
    };

    if config_path.exists() && !force {
        if !json {
            println!(
                "Config already exists at {}.\nUse --force to overwrite.",
                config_path.display()
            );
        }
        // config already present — skip chrome detection, still do project setup
    } else {
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

        if !json {
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
            println!(
                "  pagerunner download-model   # enable PERSON/ORG name detection (~65MB model)"
            );
        }
    }

    // --- Phase 2: Project CLAUDE.md / AGENTS.md setup ---
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let project_file_path = find_project_file(&cwd);

    match &project_file_path {
        None => {
            if !json {
                println!(
                    "\nTip: run `pagerunner init` from a project directory with a CLAUDE.md or \
                     AGENTS.md to add browser automation instructions for your agent."
                );
            }
        }
        Some(path) => {
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let already_present = content.contains("pagerunner");

            json_result.project_file = Some(file_name.clone());
            json_result.already_present = Some(already_present);
            json_result.snippet = Some(SNIPPET.to_string());
            json_result.snippet_appended = Some(false);

            if already_present {
                if !json {
                    println!("\n✓ pagerunner already mentioned in {}", path.display());
                }
            } else if json {
                // Non-interactive: just return the snippet in the JSON result
            } else {
                // Interactive
                println!("\nFound {} — pagerunner is not yet mentioned.", path.display());
                println!("\nSuggested snippet to add:\n");
                println!("---\n{}\n---", SNIPPET);
                print!("\nAppend this snippet to {}? [y/N] ", file_name);
                use std::io::Write as _;
                let _ = std::io::stdout().flush();

                let mut input = String::new();
                use std::io::BufRead as _;
                let appended = if std::io::stdin().lock().read_line(&mut input).is_ok() {
                    input.trim().eq_ignore_ascii_case("y")
                } else {
                    false
                };

                if appended {
                    let separator = if content.ends_with('\n') { "\n" } else { "\n\n" };
                    let new_content = format!("{}{}{}", content, separator, SNIPPET);
                    match std::fs::write(path, new_content) {
                        Ok(()) => {
                            println!("✓ Appended to {}", path.display());
                            json_result.snippet_appended = Some(true);
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to write {}: {e}", path.display());
                        }
                    }
                } else {
                    println!("Skipped. You can manually add the snippet above.");
                }
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string(&json_result).unwrap_or_default());
    }

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

    #[test]
    fn find_project_file_finds_claude_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "# test").unwrap();
        let found = find_project_file(tmp.path());
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().file_name().unwrap().to_str().unwrap(),
            "CLAUDE.md"
        );
    }

    #[test]
    fn find_project_file_finds_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# test").unwrap();
        let found = find_project_file(tmp.path());
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().file_name().unwrap().to_str().unwrap(),
            "AGENTS.md"
        );
    }

    #[test]
    fn find_project_file_returns_none_when_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a .git dir to stop the walk
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let found = find_project_file(tmp.path());
        assert!(found.is_none());
    }

    #[test]
    fn find_project_file_prefers_claude_md_over_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "# claude").unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# agents").unwrap();
        let found = find_project_file(tmp.path());
        assert_eq!(
            found.unwrap().file_name().unwrap().to_str().unwrap(),
            "CLAUDE.md"
        );
    }

    #[test]
    fn snippet_is_non_empty() {
        assert!(!SNIPPET.is_empty());
        assert!(SNIPPET.contains("pagerunner"));
    }
}
