# Conduit MVP Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a macOS stdio MCP server (`conduit`) that manages Chrome browser instances via CDP over pipes, exposing browser control to AI agents through Claude Code.

**Architecture:** Conduit runs as a stdio MCP server (Claude Code spawns it as a subprocess). It manages Chrome processes launched with `--remote-debugging-pipe`, owns the pipe file descriptors as the sole gatekeeper, and proxies CDP JSON-RPC messages to Chrome. Agents call MCP tools; Conduit translates them to CDP and returns results. Chrome processes are owned by Conduit and cleaned up on exit.

**Tech Stack:** Rust 2021, tokio (async runtime), clap (CLI), serde/serde_json (CDP messages), toml (config), anyhow/thiserror (errors), tracing (logging), nix (fd manipulation for pipe setup)

**Not in MVP:** daemon mode, permissions scoping, multi-user, port-based CDP, Windows/Linux support

---

## File Map

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Dependencies |
| `src/main.rs` | CLI entry, wires components together |
| `src/error.rs` | `ConduitError` enum, `Result` alias |
| `src/config.rs` | `ConduitConfig`, `ChromeProfile`, load from `~/.conduit/config.toml` |
| `src/chrome.rs` | Spawn Chrome with `--remote-debugging-pipe`, pipe fd setup, kill |
| `src/cdp.rs` | Send/receive CDP messages over pipe (null-byte framing) |
| `src/session.rs` | `Session`, `SessionManager` — lifecycle of Chrome+CDP pairs |
| `src/browser.rs` | High-level browser ops: list_tabs, navigate, screenshot, get_content |
| `src/mcp_server.rs` | stdio JSON-RPC 2.0 MCP server, tool dispatch |
| `config.example.toml` | Example profile config |

---

## Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/error.rs`
- Create: `config.example.toml`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "conduit"
version = "0.1.0"
edition = "2021"
description = "Chrome CDP manager and MCP server for AI agents"

[[bin]]
name = "conduit"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
dirs = "6"
nix = { version = "0.29" }
```

- [ ] **Step 2: Create src/error.rs**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConduitError {
    #[error("Profile not found: {0}")]
    ProfileNotFound(String),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Chrome error: {0}")]
    Chrome(String),
    #[error("CDP error: {0}")]
    Cdp(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ConduitError>;
```

- [ ] **Step 3: Create src/main.rs stub**

```rust
mod config;
mod error;
mod chrome;
mod cdp;
mod session;
mod browser;
mod mcp_server;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "conduit", about = "Chrome CDP manager and MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start as MCP server (stdio transport)
    Mcp,
    /// List configured profiles
    Profiles,
    /// Print example config
    ExampleConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Mcp => mcp_server::run().await?,
        Commands::Profiles => {
            let config = config::ConduitConfig::load()?;
            for p in &config.profiles {
                println!("{}: {}", p.name, p.user_data_dir);
            }
        }
        Commands::ExampleConfig => {
            println!("{}", include_str!("../config.example.toml"));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Create config.example.toml**

```toml
# ~/.conduit/config.toml

[[profiles]]
name = "luminary"
display_name = "Stas @ Luminary"
user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Profile 6"

[[profiles]]
name = "personal"
display_name = "Personal"
user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Default"
```

- [ ] **Step 5: Verify it compiles**

```bash
cd /Users/stas/Code/conduit
cargo check
```
Expected: errors about missing modules (config, chrome, etc.) — that's fine, stubs needed.

- [ ] **Step 6: Add empty module stubs so it compiles**

Create `src/config.rs`, `src/chrome.rs`, `src/cdp.rs`, `src/session.rs`, `src/browser.rs`, `src/mcp_server.rs` each with just:
```rust
// TODO
```

- [ ] **Step 7: Verify cargo check passes**

```bash
cargo check
```
Expected: `Finished` with no errors (warnings OK).

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat: project scaffold"
```

---

## Task 2: Config

**Files:**
- Create: `src/config.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test Profile"
user_data_dir = "/tmp/chrome-test"
"#;
        let config: ConduitConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.profiles[0].name, "test");
        assert_eq!(config.profiles[0].user_data_dir, "/tmp/chrome-test");
    }

    #[test]
    fn test_find_profile() {
        let config = ConduitConfig {
            profiles: vec![
                ChromeProfile {
                    name: "a".into(),
                    display_name: "A".into(),
                    user_data_dir: "/tmp/a".into(),
                },
            ],
        };
        assert!(config.find_profile("a").is_some());
        assert!(config.find_profile("missing").is_none());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test config
```
Expected: compile error — types not defined yet.

- [ ] **Step 3: Implement config.rs**

```rust
use serde::{Deserialize, Serialize};
use crate::error::{ConduitError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChromeProfile {
    pub name: String,
    pub display_name: String,
    pub user_data_dir: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConduitConfig {
    #[serde(default)]
    pub profiles: Vec<ChromeProfile>,
}

impl ConduitConfig {
    pub fn load() -> Result<Self> {
        let path = dirs::home_dir()
            .ok_or_else(|| ConduitError::Config("Cannot find home dir".into()))?
            .join(".conduit/config.toml");

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ConduitError::Config(format!("Cannot read {}: {}", path.display(), e)))?;

        toml::from_str(&content)
            .map_err(|e| ConduitError::Config(format!("Invalid config: {}", e)))
    }

    pub fn find_profile(&self, name: &str) -> Option<&ChromeProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

- [ ] **Step 4: Run tests, verify pass**

```bash
cargo test config
```
Expected: `test config::tests::test_parse_valid_config ... ok`, `test config::tests::test_find_profile ... ok`

- [ ] **Step 5: Commit**

```bash
git add src/config.rs && git commit -m "feat: config parsing with named Chrome profiles"
```

---

## Task 3: Chrome Process Management

**Files:**
- Create: `src/chrome.rs`

This is the most platform-specific task. Chrome is launched with `--remote-debugging-pipe`, which uses fd 3 (Chrome reads CDP commands) and fd 4 (Chrome writes CDP responses). We use `pre_exec` to wire up pipes before exec.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_kill_chrome() {
        let profile_dir = tempfile::tempdir().unwrap();
        let mut result = ChromeProcess::spawn(profile_dir.path().to_str().unwrap())
            .await
            .expect("Chrome should spawn");
        assert!(result.process.is_running());
        result.process.kill().await.expect("Chrome should die");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!result.process.is_running());
    }
}
```

Add `tempfile = "3"` to `[dev-dependencies]` in Cargo.toml.

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test chrome
```
Expected: compile error.

- [ ] **Step 3: Implement chrome.rs**

Note: In nix 0.29, `pipe()` returns `(OwnedFd, OwnedFd)`. Call `.into_raw_fd()` immediately so the closure captures `Copy` integers, and the parent retains usable values after the `move` closure.

```rust
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::process::Stdio;
use tokio::process::{Child, Command};
use nix::unistd::{close, dup2, pipe};
use crate::error::{ConduitError, Result};

const CHROME_PATH: &str =
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

pub struct ChromeProcess {
    child: Child,
}

pub struct SpawnResult {
    pub process: ChromeProcess,
    /// Write end — we send CDP commands; Chrome reads via fd3
    pub cmd_write: tokio::fs::File,
    /// Read end — we receive CDP responses; Chrome writes via fd4
    pub evt_read: tokio::fs::File,
}

impl ChromeProcess {
    pub async fn spawn(user_data_dir: &str) -> Result<SpawnResult> {
        // pipe1: parent writes (cmd_w), Chrome reads (cmd_r → fd3)
        let (cmd_r_owned, cmd_w_owned) = pipe()
            .map_err(|e| ConduitError::Chrome(e.to_string()))?;
        // pipe2: Chrome writes (evt_w → fd4), parent reads (evt_r)
        let (evt_r_owned, evt_w_owned) = pipe()
            .map_err(|e| ConduitError::Chrome(e.to_string()))?;

        // Convert to RawFd (Copy) before the move closure
        let (cmd_r, cmd_w): (RawFd, RawFd) =
            (cmd_r_owned.into_raw_fd(), cmd_w_owned.into_raw_fd());
        let (evt_r, evt_w): (RawFd, RawFd) =
            (evt_r_owned.into_raw_fd(), evt_w_owned.into_raw_fd());

        let user_data_dir = user_data_dir.to_string();
        let mut cmd = Command::new(CHROME_PATH);
        cmd.args([
            "--remote-debugging-pipe",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            "--disable-extensions",
            &format!("--user-data-dir={}", user_data_dir),
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        // pre_exec runs in the child after fork, before exec.
        // Closes copies of fd that the child doesn't need.
        unsafe {
            cmd.pre_exec(move || {
                dup2(cmd_r, 3).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                dup2(evt_w, 4).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                let _ = close(cmd_r);
                let _ = close(cmd_w);
                let _ = close(evt_r);
                let _ = close(evt_w);
                Ok(())
            });
        }

        let child = cmd.spawn()
            .map_err(|e| ConduitError::Chrome(format!("Failed to spawn Chrome: {}", e)))?;

        // Parent: close the ends the child now owns
        unsafe {
            let _ = close(cmd_r);
            let _ = close(evt_w);
        }

        // Wrap the parent's ends as async files
        let cmd_write = unsafe { tokio::fs::File::from_raw_fd(cmd_w) };
        let evt_read  = unsafe { tokio::fs::File::from_raw_fd(evt_r) };

        // Give Chrome time to initialize
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(SpawnResult {
            process: ChromeProcess { child },
            cmd_write,
            evt_read,
        })
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await
            .map_err(|e| ConduitError::Chrome(format!("Failed to kill Chrome: {}", e)))
    }
}
```

- [ ] **Step 4: Run test**

```bash
cargo test chrome -- --nocapture
```
Expected: Chrome spawns (you may briefly see it in Activity Monitor), test passes.

If Chrome path is wrong, adjust `CHROME_PATH`.

- [ ] **Step 5: Commit**

```bash
git add src/chrome.rs Cargo.toml && git commit -m "feat: spawn Chrome with CDP pipe transport"
```

---

## Task 4: CDP I/O

**Files:**
- Create: `src/cdp.rs`

CDP messages over pipes are null-byte (`\0`) delimited JSON objects. Each message has `id`, `method`, `params`. Responses have `id` and `result` or `error`.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_message() {
        let msg = CdpMessage {
            id: 1,
            method: "Target.getTargets".into(),
            params: serde_json::json!({}),
            session_id: None,
        };
        let framed = frame(&msg).unwrap();
        assert!(framed.ends_with(b"\0"));
        let json: serde_json::Value = serde_json::from_slice(&framed[..framed.len()-1]).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "Target.getTargets");
    }

    #[test]
    fn test_parse_response() {
        let raw = br#"{"id":1,"result":{"targetInfos":[]}}"#;
        let resp: CdpResponse = serde_json::from_slice(raw).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.error.is_none());
    }
}
```

- [ ] **Step 2: Run to verify fails**

```bash
cargo test cdp
```

- [ ] **Step 3: Implement cdp.rs**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::error::{ConduitError, Result};

#[derive(Debug, Serialize)]
pub struct CdpMessage {
    pub id: u64,
    pub method: String,
    pub params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CdpResponse {
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<Value>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CdpEvent {
    pub method: String,
    pub params: Option<Value>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

pub fn frame(msg: &CdpMessage) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(msg)?;
    bytes.push(b'\0');
    Ok(bytes)
}

pub struct CdpConn {
    writer: tokio::fs::File,
    reader: tokio::io::BufReader<tokio::fs::File>,
    next_id: u64,
}

impl CdpConn {
    pub fn new(write_fd: tokio::fs::File, read_fd: tokio::fs::File) -> Self {
        Self {
            writer: write_fd,
            reader: tokio::io::BufReader::new(read_fd),
            next_id: 1,
        }
    }

    pub async fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        self.send_on_session(method, params, None).await
    }

    pub async fn send_on_session(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = CdpMessage { id, method: method.into(), params, session_id };
        let framed = frame(&msg)?;
        self.writer.write_all(&framed).await?;

        // Read responses until we get the one matching our id
        loop {
            let raw = self.read_message().await?;
            let v: Value = serde_json::from_slice(&raw)?;

            if v.get("id") == Some(&Value::Number(id.into())) {
                if let Some(err) = v.get("error") {
                    return Err(ConduitError::Cdp(err.to_string()));
                }
                return Ok(v["result"].clone());
            }
            // Events (no "id") are dropped for now — Task 6 can buffer them
        }
    }

    async fn read_message(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        // Read until null byte
        loop {
            let mut byte = [0u8; 1];
            self.reader.read_exact(&mut byte).await
                .map_err(|e| ConduitError::Cdp(format!("Pipe read error: {}", e)))?;
            if byte[0] == b'\0' {
                break;
            }
            buf.push(byte[0]);
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // (tests from Step 1)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test cdp
```
Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cdp.rs && git commit -m "feat: CDP message framing and pipe I/O"
```

---

## Task 5: Session Management

**Files:**
- Create: `src/session.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChromeProfile, ConduitConfig};

    fn test_profile() -> ChromeProfile {
        ChromeProfile {
            name: "test".into(),
            display_name: "Test".into(),
            user_data_dir: "/tmp/conduit-test-profile".into(),
        }
    }

    #[tokio::test]
    async fn test_open_and_close_session() {
        let mut manager = SessionManager::new();
        let id = manager.open(&test_profile()).await
            .expect("Should open session");
        assert!(manager.get(&id).is_some());
        manager.close(&id).await.expect("Should close session");
        assert!(manager.get(&id).is_none());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let mut manager = SessionManager::new();
        assert_eq!(manager.list().len(), 0);
        let id = manager.open(&test_profile()).await.unwrap();
        assert_eq!(manager.list().len(), 1);
        assert_eq!(manager.list()[0].id, id);
        manager.close(&id).await.unwrap();
    }
}
```

- [ ] **Step 2: Run to verify fails**

```bash
cargo test session
```

- [ ] **Step 3: Implement session.rs**

`ChromeProcess::spawn` already returns `SpawnResult` (from Task 3). Use it directly — no `todo!()`, no refactor needed.

```rust
use std::collections::HashMap;
use uuid::Uuid;
use crate::chrome::ChromeProcess;
use crate::cdp::CdpConn;
use crate::config::ChromeProfile;
use crate::error::{ConduitError, Result};

pub type SessionId = String;

pub struct Session {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    chrome: crate::chrome::ChromeProcess,
    pub cdp: CdpConn,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
}

pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    pub async fn open(&mut self, profile: &ChromeProfile) -> Result<SessionId> {
        let result = ChromeProcess::spawn(&profile.user_data_dir).await?;
        let cdp = CdpConn::new(result.cmd_write, result.evt_read);
        let id = Uuid::new_v4().to_string();
        self.sessions.insert(id.clone(), Session {
            id: id.clone(),
            profile_name: profile.name.clone(),
            profile_display_name: profile.display_name.clone(),
            chrome: result.process,
            cdp,
        });
        Ok(id)
    }

    pub async fn close(&mut self, id: &str) -> Result<()> {
        let mut session = self.sessions.remove(id)
            .ok_or_else(|| ConduitError::SessionNotFound(id.into()))?;
        session.chrome.kill().await
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.values().map(|s| SessionInfo {
            id: s.id.clone(),
            profile_name: s.profile_name.clone(),
            profile_display_name: s.profile_display_name.clone(),
        }).collect()
    }
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test session
```
Expected: both tests pass. Chrome briefly appears in Activity Monitor and is killed.

- [ ] **Step 6: Commit**

```bash
git add src/session.rs && git commit -m "feat: session manager for Chrome lifecycle"
```

---

## Task 6: Browser Operations

**Files:**
- Create: `src/browser.rs`

High-level ops built on CDP. CDP flow: `Target.getTargets` → pick a page target → `Target.attachToTarget` → use returned `sessionId` for page-scoped commands.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Integration test — requires Chrome
    #[tokio::test]
    #[ignore]
    async fn test_list_tabs_returns_at_least_one() {
        use crate::chrome::ChromeProcess;
        use crate::cdp::CdpConn;
        let result = ChromeProcess::spawn("/tmp/conduit-browser-test").await.unwrap();
        let mut cdp = CdpConn::new(result.cmd_write, result.evt_read);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let tabs = list_tabs(&mut cdp).await.unwrap();
        assert!(!tabs.is_empty());
    }
}
```

- [ ] **Step 2: Implement browser.rs**

```rust
use serde_json::{json, Value};
use crate::cdp::CdpConn;
use crate::error::{ConduitError, Result};

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub target_id: String,
    pub url: String,
    pub title: String,
}

pub async fn list_tabs(cdp: &mut CdpConn) -> Result<Vec<TabInfo>> {
    let result = cdp.send("Target.getTargets", json!({})).await?;
    let targets = result["targetInfos"].as_array()
        .ok_or_else(|| ConduitError::Cdp("No targetInfos".into()))?;

    Ok(targets.iter()
        .filter(|t| t["type"].as_str() == Some("page"))
        .map(|t| TabInfo {
            target_id: t["targetId"].as_str().unwrap_or("").into(),
            url: t["url"].as_str().unwrap_or("").into(),
            title: t["title"].as_str().unwrap_or("").into(),
        })
        .collect())
}

pub async fn navigate(cdp: &mut CdpConn, target_id: &str, url: &str) -> Result<()> {
    // Attach to target to get a session
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No sessionId from attachToTarget".into()))?
        .to_string();

    cdp.send_on_session("Page.navigate", json!({ "url": url }), Some(session_id)).await?;
    Ok(())
}

pub async fn get_content(cdp: &mut CdpConn, target_id: &str) -> Result<String> {
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No sessionId".into()))?
        .to_string();

    let result = cdp.send_on_session("Runtime.evaluate", json!({
        "expression": "document.body.innerText",
        "returnByValue": true
    }), Some(session_id)).await?;

    Ok(result["result"]["value"].as_str().unwrap_or("").into())
}

pub async fn screenshot(cdp: &mut CdpConn, target_id: &str) -> Result<String> {
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No sessionId".into()))?
        .to_string();

    let result = cdp.send_on_session("Page.captureScreenshot", json!({
        "format": "png"
    }), Some(session_id)).await?;

    result["data"].as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ConduitError::Cdp("No screenshot data".into()))
}
```

- [ ] **Step 3: Run unit tests (integration test is ignored)**

```bash
cargo test browser
```
Expected: compiles, 0 tests run (integration test is `#[ignore]`).

- [ ] **Step 4: Run integration test manually to verify**

```bash
cargo test browser -- --ignored --nocapture
```
Expected: lists at least one tab (`about:blank`).

- [ ] **Step 5: Commit**

```bash
git add src/browser.rs && git commit -m "feat: browser ops — list_tabs, navigate, get_content, screenshot"
```

---

## Task 7: MCP Server

**Files:**
- Create: `src/mcp_server.rs`

Implements the MCP protocol over stdio (JSON-RPC 2.0). Claude Code spawns `conduit mcp` and communicates over stdin/stdout. All tracing goes to stderr so it doesn't corrupt the JSON-RPC stream.

MCP tools exposed:
| Tool | Description |
|------|-------------|
| `list_profiles` | List configured Chrome profiles from config |
| `open_session` | Launch Chrome for a profile, return session_id |
| `close_session` | Kill Chrome session |
| `list_sessions` | List active sessions |
| `list_tabs` | List tabs in a session |
| `navigate` | Navigate a tab to a URL |
| `get_content` | Get page text content |
| `screenshot` | Capture tab as base64 PNG |

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tools_list_response() {
        let tools = all_tools();
        assert!(tools.iter().any(|t| t["name"] == "list_profiles"));
        assert!(tools.iter().any(|t| t["name"] == "open_session"));
        assert!(tools.iter().any(|t| t["name"] == "screenshot"));
    }
}
```

- [ ] **Step 2: Implement mcp_server.rs**

```rust
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::config::ConduitConfig;
use crate::session::SessionManager;
use crate::browser;
use crate::error::Result;

pub fn all_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_profiles",
            "description": "List configured Chrome profiles",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "open_session",
            "description": "Launch Chrome for a named profile. Returns session_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "description": "Profile name from list_profiles" }
                },
                "required": ["profile"]
            }
        }),
        json!({
            "name": "close_session",
            "description": "Kill a Chrome session",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "list_sessions",
            "description": "List active Chrome sessions",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_tabs",
            "description": "List open tabs in a Chrome session",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "navigate",
            "description": "Navigate a tab to a URL",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "target_id": { "type": "string", "description": "Tab target_id from list_tabs" },
                    "url": { "type": "string" }
                },
                "required": ["session_id", "target_id", "url"]
            }
        }),
        json!({
            "name": "get_content",
            "description": "Get text content of a tab",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "target_id": { "type": "string" }
                },
                "required": ["session_id", "target_id"]
            }
        }),
        json!({
            "name": "screenshot",
            "description": "Capture a tab as base64 PNG screenshot",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "target_id": { "type": "string" }
                },
                "required": ["session_id", "target_id"]
            }
        }),
    ]
}

pub async fn run() -> Result<()> {
    let config = ConduitConfig::load()?;
    let sessions = Arc::new(Mutex::new(SessionManager::new()));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    tracing::info!("Conduit MCP server ready");

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await
            .map_err(|e| crate::error::ConduitError::Io(e))?;
        if n == 0 { break; } // EOF

        let req: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Invalid JSON: {}", e);
                continue;
            }
        };

        // Notifications have no "id" — must not send a response (JSON-RPC 2.0)
        let has_id = req.get("id").is_some();
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req["method"].as_str().unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        if !has_id {
            // It's a notification (e.g. notifications/initialized) — no reply
            continue;
        }

        let response = handle_request(method, params, &config, Arc::clone(&sessions)).await;

        let reply = match response {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32000, "message": e.to_string() }
            }),
        };

        let mut out = serde_json::to_string(&reply)?;
        out.push('\n');
        writer.write_all(out.as_bytes()).await
            .map_err(|e| crate::error::ConduitError::Io(e))?;
        writer.flush().await
            .map_err(|e| crate::error::ConduitError::Io(e))?;
    }

    // Cleanup: close all sessions
    let mut mgr = sessions.lock().await;
    for info in mgr.list() {
        let _ = mgr.close(&info.id).await;
    }

    Ok(())
}

async fn handle_request(
    method: &str,
    params: Value,
    config: &ConduitConfig,
    sessions: Arc<Mutex<SessionManager>>,
) -> crate::error::Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": "conduit", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "tools": {} }
        })),

        "tools/list" => Ok(json!({ "tools": all_tools() })),

        "tools/call" => {
            let tool = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            dispatch_tool(tool, args, config, sessions).await
                .map(|text| json!({ "content": [{ "type": "text", "text": text }] }))
        }

        _ => Ok(json!({})),
    }
}

async fn dispatch_tool(
    tool: &str,
    args: Value,
    config: &ConduitConfig,
    sessions: Arc<Mutex<SessionManager>>,
) -> crate::error::Result<String> {
    match tool {
        "list_profiles" => {
            let list: Vec<Value> = config.profiles.iter().map(|p| json!({
                "name": p.name,
                "display_name": p.display_name,
            })).collect();
            Ok(serde_json::to_string_pretty(&list)?)
        }

        "open_session" => {
            let profile_name = args["profile"].as_str()
                .ok_or_else(|| crate::error::ConduitError::Config("Missing profile".into()))?;
            let profile = config.find_profile(profile_name)
                .ok_or_else(|| crate::error::ConduitError::ProfileNotFound(profile_name.into()))?
                .clone();
            let mut mgr = sessions.lock().await;
            let id = mgr.open(&profile).await?;
            Ok(format!("Session opened: {}", id))
        }

        "close_session" => {
            let id = args["session_id"].as_str()
                .ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
            let mut mgr = sessions.lock().await;
            mgr.close(id).await?;
            Ok(format!("Session {} closed", id))
        }

        "list_sessions" => {
            let mgr = sessions.lock().await;
            let list: Vec<Value> = mgr.list().iter().map(|s| json!({
                "id": s.id,
                "profile": s.profile_name,
                "display_name": s.profile_display_name,
            })).collect();
            Ok(serde_json::to_string_pretty(&list)?)
        }

        "list_tabs" => {
            let id = args["session_id"].as_str()
                .ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
            let mut mgr = sessions.lock().await;
            let session = mgr.get_mut(id)
                .ok_or_else(|| crate::error::ConduitError::SessionNotFound(id.into()))?;
            let tabs = browser::list_tabs(&mut session.cdp).await?;
            let list: Vec<Value> = tabs.iter().map(|t| json!({
                "target_id": t.target_id,
                "url": t.url,
                "title": t.title,
            })).collect();
            Ok(serde_json::to_string_pretty(&list)?)
        }

        "navigate" => {
            let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
            let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
            let url = args["url"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing url".into()))?;
            let mut mgr = sessions.lock().await;
            let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
            browser::navigate(&mut session.cdp, tid, url).await?;
            Ok(format!("Navigated {} to {}", tid, url))
        }

        "get_content" => {
            let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
            let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
            let mut mgr = sessions.lock().await;
            let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
            browser::get_content(&mut session.cdp, tid).await
        }

        "screenshot" => {
            let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
            let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
            let mut mgr = sessions.lock().await;
            let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
            let data = browser::screenshot(&mut session.cdp, tid).await?;
            Ok(format!("data:image/png;base64,{}", data))
        }

        _ => Err(crate::error::ConduitError::Cdp(format!("Unknown tool: {}", tool))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tools_list_response() {
        let tools = all_tools();
        assert!(tools.iter().any(|t| t["name"] == "list_profiles"));
        assert!(tools.iter().any(|t| t["name"] == "open_session"));
        assert!(tools.iter().any(|t| t["name"] == "screenshot"));
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test mcp_server
```
Expected: `test_tools_list_response ... ok`

- [ ] **Step 4: Build release binary**

```bash
cargo build --release
```
Expected: `Finished release profile`.

- [ ] **Step 5: Commit**

```bash
git add src/mcp_server.rs && git commit -m "feat: MCP stdio server with browser control tools"
```

---

## Task 8: Claude Code Integration

**Files:**
- Modify: `~/.claude/settings.json` (Claude Code MCP config)
- Create: `~/.conduit/config.toml`

- [ ] **Step 1: Create your config**

```bash
mkdir -p ~/.conduit
```

Create `~/.conduit/config.toml`:
```toml
[[profiles]]
name = "luminary"
display_name = "Stas @ Luminary"
user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Profile 6"
```

- [ ] **Step 2: Verify profile listing works**

```bash
/Users/stas/Code/conduit/target/release/conduit profiles
```
Expected:
```
luminary: /Users/stas/Library/Application Support/Google/Chrome/Profile 6
```

- [ ] **Step 3: Register with Claude Code**

Add to `~/.claude/settings.json` under `mcpServers`:
```json
"conduit": {
  "command": "/Users/stas/Code/conduit/target/release/conduit",
  "args": ["mcp"]
}
```

- [ ] **Step 4: Restart Claude Code and verify tools appear**

In a new Claude Code session, run:
```
/mcp
```
Expected: `conduit` server listed with 8 tools.

- [ ] **Step 5: End-to-end smoke test in Claude Code**

Ask Claude Code:
```
Use conduit to open a luminary session, list its tabs, and navigate the first tab to example.com
```

Expected: Chrome opens, tab listed, navigation succeeds.

- [ ] **Step 6: Commit**

```bash
cd /Users/stas/Code/conduit
git add -A && git commit -m "feat: Claude Code MCP integration complete"
```

---

## Known Limitations (post-MVP)

- Events from Chrome (page loads, console errors) are dropped — cdp.rs discards non-response messages
- Sessions die when `conduit mcp` process exits (no daemon persistence)
- No permissions scoping — any agent with access to conduit gets full browser control
- Chrome must be installed at the hardcoded path
- Single tab per session assumed for simplicity; multi-tab requires target session management
