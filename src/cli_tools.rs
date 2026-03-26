use crate::config::PagerunnerConfig;
use base64::Engine;

pub enum ScreenshotMode {
    File,
    Base64,
}

/// Convert a base64 PNG string (from `call_tool`) to CLI output.
/// File mode: decode and write to a temp file, return `{"file":"..."}`.
/// Base64 mode: return `{"base64":"..."}` inline.
pub(crate) fn handle_screenshot_output(b64: &str, mode: &ScreenshotMode) -> anyhow::Result<String> {
    match mode {
        ScreenshotMode::Base64 => Ok(serde_json::json!({"base64": b64}).to_string()),
        ScreenshotMode::File => {
            use std::io::Write;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| anyhow::anyhow!("Base64 decode error: {}", e))?;
            let path = std::env::temp_dir().join(format!(
                "pagerunner-screenshot-{}.png",
                uuid::Uuid::new_v4()
            ));
            let mut f = std::fs::File::create(&path)?;
            f.write_all(&bytes)?;
            let path_str = path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Temp path is not valid UTF-8"))?;
            Ok(serde_json::json!({"file": path_str}).to_string())
        }
    }
}

/// Call a tool and print its JSON result to stdout.
/// On error: return `Err` (caller prints to stderr and exits 1).
pub async fn run_tool(
    tool: &str,
    args: serde_json::Value,
    screenshot_mode: ScreenshotMode,
    config: &PagerunnerConfig,
) -> anyhow::Result<()> {
    let tool_response = crate::mcp_server::call_tool(tool, args, config)
        .await
        .map_err(anyhow::Error::from)?;

    // Check if this is a screenshot response ({"ok":true,"data":"data:image/png;base64,..."})
    let parsed_result: serde_json::Value =
        serde_json::from_str(&tool_response.result).unwrap_or(serde_json::Value::Null);
    let screenshot_data_uri = parsed_result
        .get("data")
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("data:image/png;base64,"));

    let output = if let Some(data_uri) = screenshot_data_uri {
        let b64 = data_uri
            .strip_prefix("data:image/png;base64,")
            .unwrap_or(data_uri);
        handle_screenshot_output(b64, &screenshot_mode)?
    } else {
        // If there's metadata, wrap result and metadata together in JSON
        if let Some(meta) = tool_response.metadata {
            let output_obj = serde_json::json!({
                "result": serde_json::from_str::<serde_json::Value>(&tool_response.result).unwrap_or_else(|_| serde_json::Value::String(tool_response.result.clone())),
                "_metadata": meta
            });
            serde_json::to_string_pretty(&output_obj)?
        } else {
            tool_response.result
        }
    };

    println!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_screenshot_base64_mode_returns_inline() {
        let result = handle_screenshot_output("abc123", &ScreenshotMode::Base64).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["base64"].as_str().unwrap(), "abc123");
    }

    #[test]
    fn test_handle_screenshot_file_mode_writes_file() {
        use base64::Engine;
        let bytes = b"\x89PNG\r\n\x1a\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let result = handle_screenshot_output(&b64, &ScreenshotMode::File).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        let path = v["file"].as_str().expect("file key missing");
        assert!(
            std::path::Path::new(path).exists(),
            "file not created at {}",
            path
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_handle_screenshot_file_mode_invalid_b64_returns_err() {
        let result = handle_screenshot_output("!!!not_base64!!!", &ScreenshotMode::File);
        assert!(result.is_err());
    }
}
