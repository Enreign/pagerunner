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
    chrome: ChromeProcess,
    pub cdp: CdpConn,
    /// Cache of target_id → CDP sessionId to reuse attached sessions
    pub cdp_sessions: HashMap<String, String>,
    pub security_policy: Option<crate::security::SecurityPolicy>,
    pub nav_count: u32,
    /// Last navigated URL per target_id — used for untrusted-content domain labeling
    pub tab_urls: HashMap<String, String>,
    pub anon_config: Option<crate::anonymizer::AnonConfig>,
    pub _reader_task: tokio::task::JoinHandle<()>,
    /// Reverse map: CDP sessionId → target_id (populated by fresh_attach)
    pub cdp_sessions_rev: std::sync::Arc<std::sync::RwLock<HashMap<String, String>>>,
    /// Network event processor task handle
    pub _network_processor: Option<tokio::task::JoinHandle<()>>,
    /// True once Network.enable has been successfully called for at least one tab in this session.
    pub network_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
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
        db: std::sync::Arc<crate::db::Db>,
        network_config: &crate::config::NetworkConfig,
    ) -> Result<SessionId> {
        let result = crate::chrome::ChromeProcess::spawn(&profile.user_data_dir, stealth).await?;
        let (cdp, reader_task) = CdpConn::new(result.cmd_write, result.evt_read);
        let id = Uuid::new_v4().to_string();
        let cdp_sessions_rev = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));

        let events_rx = cdp.subscribe_events();
        let cdp_for_processor = cdp.clone();
        let session_id_for_processor = id.clone();
        let db_for_processor = db;
        let rev_map = cdp_sessions_rev.clone();
        let capacity = network_config.buffer_capacity;

        let processor_handle = tokio::spawn(crate::network_log::network_event_processor(
            events_rx,
            cdp_for_processor,
            session_id_for_processor,
            db_for_processor,
            rev_map,
            capacity,
        ));

        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: profile.name.clone(),
                profile_display_name: profile.display_name.clone(),
                stealth,
                chrome: result.process,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls: HashMap::new(),
                anon_config: None,
                _reader_task: reader_task,
                cdp_sessions_rev,
                _network_processor: Some(processor_handle),
                network_enabled: false,
            },
        );
        Ok(id)
    }

    pub async fn close(&mut self, id: &str, db: &crate::db::Db) -> Result<()> {
        let mut session = self
            .sessions
            .remove(id)
            .ok_or_else(|| PagerunnerError::SessionNotFound(id.into()))?;
        // Graceful shutdown: Browser.close lets Chrome write session state cleanly.
        // Fall back to kill if it doesn't exit within 3 seconds.
        let _ = session
            .cdp
            .send("Browser.close", serde_json::json!({}))
            .await;
        let graceful =
            tokio::time::timeout(std::time::Duration::from_secs(3), session.chrome.wait()).await;
        if graceful.is_err() {
            session.chrome.kill().await?;
        }
        let _ = crate::network_log::delete_session_entries(db, id);
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
            })
            .collect()
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
        let (cdp, _reader_handle) = crate::cdp::CdpConn::new(cmd_write, evt_read);

        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: "stub".into(),
                profile_display_name: "Stub".into(),
                stealth: false,
                chrome,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls: HashMap::new(),
                anon_config: None,
                _reader_task: tokio::spawn(async {}),
                cdp_sessions_rev: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
                _network_processor: None,
                network_enabled: false,
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
