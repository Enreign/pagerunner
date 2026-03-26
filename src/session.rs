use crate::cdp::CdpConn;
use crate::chrome::ChromeProcess;
use crate::config::ChromeProfile;
use crate::error::{PagerunnerError, Result};
use std::collections::HashMap;
use uuid::Uuid;

pub type SessionId = String;

pub struct Session {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
    pub alive: bool,
    chrome: ChromeProcess,
    pub cdp: CdpConn,
    /// Cache of target_id → CDP sessionId to reuse attached sessions
    pub cdp_sessions: HashMap<String, String>,
    pub security_policy: Option<crate::security::SecurityPolicy>,
    pub nav_count: u32,
    /// Last navigated URL per target_id — used for untrusted-content domain labeling
    pub tab_urls: HashMap<String, String>,
    pub anon_config: Option<crate::anonymizer::AnonConfig>,
}

impl Session {
    /// Check if the underlying Chrome process is still running (non-blocking).
    pub fn is_chrome_running(&mut self) -> bool {
        self.chrome.is_running()
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
    pub alive: bool,
}

pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub async fn open(
        &mut self,
        profile: &ChromeProfile,
        stealth: bool,
        security_policy: Option<crate::security::SecurityPolicy>,
    ) -> Result<SessionId> {
        let result = crate::chrome::ChromeProcess::spawn(&profile.user_data_dir, stealth).await?;
        let cdp = CdpConn::new(result.cmd_write, result.evt_read);
        let id = Uuid::new_v4().to_string();
        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: profile.name.clone(),
                profile_display_name: profile.display_name.clone(),
                stealth,
                alive: true,
                chrome: result.process,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls: HashMap::new(),
                anon_config: None,
            },
        );
        Ok(id)
    }

    pub async fn close(&mut self, id: &str) -> Result<()> {
        let mut session = self
            .sessions
            .remove(id)
            .ok_or_else(|| PagerunnerError::SessionNotFound(id.into()))?;
        // If Chrome has already crashed, skip the Browser.close CDP call (pipe is dead).
        if session.alive && session.is_chrome_running() {
            // Graceful shutdown: Browser.close lets Chrome write session state cleanly.
            // Fall back to kill if it doesn't exit within 3 seconds.
            let _ = session
                .cdp
                .send("Browser.close", serde_json::json!({}))
                .await;
            let graceful =
                tokio::time::timeout(std::time::Duration::from_secs(3), session.chrome.wait())
                    .await;
            if graceful.is_err() {
                session.chrome.kill().await?;
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                profile_name: s.profile_name.clone(),
                profile_display_name: s.profile_display_name.clone(),
                stealth: s.stealth,
                alive: s.alive,
            })
            .collect()
    }

    /// Look up a session and verify it's alive.
    /// Returns `SessionNotFound` if the ID doesn't exist.
    /// Returns `SessionDead` if Chrome has crashed.
    /// Marks the session `alive = false` when Chrome is detected as dead.
    pub fn get_live(&mut self, id: &str) -> crate::error::Result<&mut Session> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| crate::error::PagerunnerError::SessionNotFound(id.into()))?;

        // Already marked dead
        if !session.alive {
            return Err(crate::error::PagerunnerError::SessionDead(id.into()));
        }

        // Lazy check: is Chrome still running?
        if !session.is_chrome_running() {
            session.alive = false;
            return Err(crate::error::PagerunnerError::SessionDead(id.into()));
        }

        Ok(session)
    }

    /// Insert a stub session (no real browser) for use in unit tests.
    /// The session has real pipe fds for CdpConn but a `/bin/true` child for
    /// ChromeProcess, so neither is ever used in tests that trigger early
    /// policy checks.
    #[cfg(test)]
    pub async fn insert_stub(
        &mut self,
        security_policy: Option<crate::security::SecurityPolicy>,
    ) -> SessionId {
        use nix::unistd::pipe;
        use std::os::unix::io::{FromRawFd, IntoRawFd};

        let (r1, w1) = pipe().expect("pipe");
        let (r2, w2) = pipe().expect("pipe");
        let cmd_write = unsafe { tokio::fs::File::from_raw_fd(w1.into_raw_fd()) };
        let evt_read = unsafe { tokio::fs::File::from_raw_fd(r2.into_raw_fd()) };
        // close unused ends
        drop(unsafe { tokio::fs::File::from_raw_fd(r1.into_raw_fd()) });
        drop(unsafe { tokio::fs::File::from_raw_fd(w2.into_raw_fd()) });

        let child = tokio::process::Command::new("/usr/bin/true")
            .spawn()
            .expect("spawn /usr/bin/true");
        let chrome = crate::chrome::ChromeProcess::from_child_for_test(child);
        let cdp = crate::cdp::CdpConn::new(cmd_write, evt_read);

        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: "stub".into(),
                profile_display_name: "Stub".into(),
                stealth: false,
                alive: true,
                chrome,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls: HashMap::new(),
                anon_config: None,
            },
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> ChromeProfile {
        ChromeProfile {
            name: "test".into(),
            display_name: "Test".into(),
            user_data_dir: "/tmp/pagerunner-test-profile".into(),
        }
    }
}
