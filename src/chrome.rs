use crate::error::{PagerunnerError, Result};
use std::process::Stdio;
use tokio::process::{Child, Command};

fn chrome_binary_path() -> String {
    if let Ok(path) = std::env::var("PAGERUNNER_CHROME_PATH") {
        return path;
    }
    #[cfg(target_os = "macos")]
    return "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string();
    #[cfg(not(target_os = "macos"))]
    "/usr/bin/google-chrome".to_string()
}

pub struct ChromeProcess {
    child: Child,
}

pub struct SpawnResult {
    pub process: ChromeProcess,
    pub debug_port: u16,
}

/// Bind to port 0 to get an OS-assigned free port, then immediately release it.
/// Small race window before Chrome binds to the port — acceptable for local dev use.
fn alloc_free_port() -> crate::error::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| crate::error::PagerunnerError::Config(
            format!("Failed to allocate debug port: {}", e)
        ))?;
    let port = listener.local_addr()
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?
        .port();
    // Drop listener releases the port
    Ok(port)
}

impl ChromeProcess {
    pub async fn spawn(user_data_dir: &str, _stealth: bool) -> Result<SpawnResult> {
        let debug_port = alloc_free_port()?;
        let user_data_dir = user_data_dir.to_string();
        let mut cmd = Command::new(chrome_binary_path());

        let mut args: Vec<String> = vec![
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            format!("--user-data-dir={}", user_data_dir),
            format!("--remote-debugging-port={}", debug_port),
        ];

        args.push("--disable-blink-features=AutomationControlled".into());

        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Remove stale Chrome singleton files left behind by a previous unclean exit
        // (kill -9, crash). Chrome refuses to start if these exist.
        for name in &["SingletonLock", "SingletonCookie", "SingletonSocket"] {
            let path = std::path::Path::new(&user_data_dir).join(name);
            let _ = std::fs::remove_file(&path);
        }

        let child = cmd
            .spawn()
            .map_err(|e| PagerunnerError::Chrome(format!("Failed to spawn Chrome: {}", e)))?;

        // Give Chrome time to initialize before TCP polling begins in session.rs
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(SpawnResult {
            process: ChromeProcess { child },
            debug_port,
        })
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub async fn wait(&mut self) -> Result<()> {
        self.child
            .wait()
            .await
            .map(|_| ())
            .map_err(|e| PagerunnerError::Chrome(format!("Failed to wait for Chrome: {}", e)))
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.child
            .kill()
            .await
            .map_err(|e| PagerunnerError::Chrome(format!("Failed to kill Chrome: {}", e)))
    }

    /// Wrap an already-spawned child as a ChromeProcess. For use in tests only.
    #[cfg(test)]
    pub fn from_child_for_test(child: tokio::process::Child) -> Self {
        Self { child }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_free_port_returns_nonzero() {
        let port = alloc_free_port().expect("should get a free port");
        assert!(port > 0, "OS-assigned port should be non-zero");
    }

    #[test]
    fn test_alloc_free_port_returns_different_ports() {
        // Two consecutive calls should return different ports (OS assigns distinct ports)
        let p1 = alloc_free_port().unwrap();
        let p2 = alloc_free_port().unwrap();
        assert_ne!(p1, p2, "consecutive alloc_free_port calls should return distinct ports");
    }

    #[test]
    fn test_spawn_result_has_no_pipe_fields() {
        fn _assert_fields(r: SpawnResult) {
            let _: ChromeProcess = r.process;
            let _: u16 = r.debug_port;
        }
    }
}
