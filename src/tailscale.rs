//! Thin wrapper around the `tailscale` CLI's `whois` subcommand.
//!
//! Used by the HTTP API when `auth = "tailscale"`: instead of a shared bearer
//! token, each incoming request is identified by the tailnet identity of its
//! peer. The `tailscale` CLI talks to the local `tailscaled` LocalAPI socket
//! and returns a JSON record containing the peer's node info, user profile,
//! and capability map.
//!
//! We shell out rather than embed `tsnet` because the CLI is present on every
//! Tailscale install (GUI App Store build, Homebrew, system package) while
//! `tsnet` would pull in the Go runtime at link time.

use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WhoisResult {
    #[serde(default)]
    pub user_profile: UserProfile,
    #[serde(default)]
    pub node: Node,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UserProfile {
    #[serde(default)]
    pub login_name: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Node {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Locate the `tailscale` CLI. On macOS the App Store build ships the binary
/// inside the app bundle; Homebrew puts it on PATH. We try PATH first, then
/// fall back to the GUI app bundle.
fn tailscale_binary() -> PathBuf {
    if let Ok(path) = which::which("tailscale") {
        return path;
    }
    let app_bundle = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    if app_bundle.exists() {
        return app_bundle;
    }
    PathBuf::from("tailscale")
}

/// Identify the tailnet peer at `peer` via the local Tailscale daemon.
///
/// Returns `Ok(Some(_))` when Tailscale recognises the address, `Ok(None)`
/// when the address is not part of the tailnet, and `Err` if `tailscale`
/// itself is unreachable (daemon down, binary missing).
pub async fn whois(peer: SocketAddr) -> Result<Option<WhoisResult>, String> {
    let bin = tailscale_binary();
    let output = tokio::process::Command::new(&bin)
        .args(["whois", "--json", &peer.to_string()])
        .output()
        .await
        .map_err(|e| format!("tailscale whois: {}", e))?;

    if !output.status.success() {
        // `tailscale whois` exits non-zero when the peer isn't in the tailnet,
        // which is an expected "not authenticated" signal rather than an error.
        return Ok(None);
    }

    serde_json::from_slice::<WhoisResult>(&output.stdout)
        .map(Some)
        .map_err(|e| format!("tailscale whois parse: {}", e))
}
