use crate::error::{PagerunnerError, Result};
use nix::unistd::{close, dup2, pipe};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
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
    /// Write end — we send CDP commands; Chrome reads via fd3
    pub cmd_write: tokio::fs::File,
    /// Read end — we receive CDP responses; Chrome writes via fd4
    pub evt_read: tokio::fs::File,
}

impl ChromeProcess {
    pub async fn spawn(user_data_dir: &str, _stealth: bool) -> Result<SpawnResult> {
        // pipe1: parent writes (cmd_w), Chrome reads (cmd_r → fd3)
        let (cmd_r_owned, cmd_w_owned) =
            pipe().map_err(|e| PagerunnerError::Chrome(e.to_string()))?;
        // pipe2: Chrome writes (evt_w → fd4), parent reads (evt_r)
        let (evt_r_owned, evt_w_owned) =
            pipe().map_err(|e| PagerunnerError::Chrome(e.to_string()))?;

        // Convert to RawFd (Copy) before the move closure
        let (cmd_r, cmd_w): (RawFd, RawFd) = (cmd_r_owned.into_raw_fd(), cmd_w_owned.into_raw_fd());
        let (evt_r, evt_w): (RawFd, RawFd) = (evt_r_owned.into_raw_fd(), evt_w_owned.into_raw_fd());

        let user_data_dir = user_data_dir.to_string();
        let mut cmd = Command::new(chrome_binary_path());

        let mut args: Vec<String> = vec![
            "--remote-debugging-pipe".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            format!("--user-data-dir={}", user_data_dir),
        ];

        args.push("--disable-blink-features=AutomationControlled".into());

        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // pre_exec runs in the child after fork, before exec.
        // Wires fd3 and fd4 then closes the originals.
        unsafe {
            cmd.pre_exec(move || {
                dup2(cmd_r, 3)
                    .map_err(|e: nix::errno::Errno| std::io::Error::other(e.to_string()))?;
                dup2(evt_w, 4)
                    .map_err(|e: nix::errno::Errno| std::io::Error::other(e.to_string()))?;
                let _ = close(cmd_r);
                let _ = close(cmd_w);
                let _ = close(evt_r);
                let _ = close(evt_w);
                Ok(())
            });
        }

        // Remove stale Chrome singleton files left behind by a previous unclean exit
        // (kill -9, crash). Chrome refuses to start if these exist.
        for name in &["SingletonLock", "SingletonCookie", "SingletonSocket"] {
            let path = std::path::Path::new(&user_data_dir).join(name);
            let _ = std::fs::remove_file(&path);
        }

        let child = cmd
            .spawn()
            .map_err(|e| PagerunnerError::Chrome(format!("Failed to spawn Chrome: {}", e)))?;

        // Parent: close the ends the child now owns
        let _ = close(cmd_r);
        let _ = close(evt_w);

        // Wrap the parent's ends as async files
        let cmd_write = unsafe { tokio::fs::File::from_raw_fd(cmd_w) };
        let evt_read = unsafe { tokio::fs::File::from_raw_fd(evt_r) };

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
