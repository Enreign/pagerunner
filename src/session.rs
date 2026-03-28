use crate::cdp::CdpConn;
use crate::chrome::ChromeProcess;
use crate::config::ChromeProfile;
use crate::error::{PagerunnerError, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use uuid::Uuid;

pub type SessionId = String;
/// Shared map of target_id → current URL, updated on navigate and Page.frameNavigated.
pub type TabUrlMap = Arc<RwLock<HashMap<String, String>>>;

pub struct Session {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
    pub alive: bool,
    chrome: Option<ChromeProcess>,
    /// True if this session owns the Chrome process (primary). False for secondary sessions.
    pub owns_process: bool,
    /// Set of CDP target_ids owned by this session (tabs opened within this session).
    pub owned_targets: std::collections::HashSet<String>,
    /// For secondary sessions: the session_id of the primary session sharing the Chrome process.
    pub primary_session_id: Option<SessionId>,
    /// True for sessions created via `attach_session` (connected to a user-launched Chrome).
    /// Closing an attached session disconnects pagerunner without touching Chrome tabs.
    pub is_attached: bool,
    pub cdp: CdpConn,
    /// Cache of target_id → CDP sessionId to reuse attached sessions
    pub cdp_sessions: HashMap<String, String>,
    pub security_policy: Option<crate::security::SecurityPolicy>,
    pub nav_count: u32,
    /// Current URL per target_id — updated on navigate and Page.frameNavigated events.
    pub tab_urls: TabUrlMap,
    pub anon_config: Option<crate::anonymizer::AnonConfig>,
    pub _reader_task: tokio::task::JoinHandle<()>,
    /// Reverse map: CDP sessionId → target_id (populated by fresh_attach)
    pub cdp_sessions_rev: std::sync::Arc<std::sync::RwLock<HashMap<String, String>>>,
    /// Network event processor task handle
    pub _network_processor: Option<tokio::task::JoinHandle<()>>,
    /// True once Network.enable has been successfully called for at least one tab in this session.
    pub network_enabled: bool,
    /// In-memory ring buffer for Runtime console/exception events (no persistence needed).
    pub console_buffer: crate::console_log::ConsoleBuffer,
    /// Console event processor task handle
    pub _console_processor: Option<tokio::task::JoinHandle<()>>,
    /// Frame navigation event processor task handle
    pub _frame_nav_processor: Option<tokio::task::JoinHandle<()>>,
}

impl Session {
    /// Check if the underlying Chrome process is still running (non-blocking).
    pub fn is_chrome_running(&mut self) -> bool {
        match self.chrome.as_mut() {
            Some(chrome) => chrome.is_running(),
            None => true, // secondary: no owned process; Chrome liveness checked via primary
        }
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
    /// Maps profile_name → session_id of the primary (process-owning) session for that profile.
    profile_primary: HashMap<String, SessionId>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            profile_primary: HashMap::new(),
        }
    }

    pub async fn open(
        &mut self,
        profile: &ChromeProfile,
        stealth: bool,
        security_policy: Option<crate::security::SecurityPolicy>,
        db: std::sync::Arc<crate::db::Db>,
        network_config: &crate::config::NetworkConfig,
        site_store: Option<std::sync::Arc<crate::site_knowledge::SiteKnowledgeStore>>,
    ) -> Result<SessionId> {
        // If an alive primary session exists for this profile, open a new window in it
        if let Some(primary_id) = self.profile_primary.get(&profile.name).cloned() {
            if let Some(primary) = self.sessions.get(&primary_id) {
                if primary.alive {
                    let cdp = primary.cdp.clone();
                    let db_for_processor = db.clone();
                    let network_config_capacity = network_config.buffer_capacity;
                    let site_store_clone = site_store.clone();

                    // Open a new Chrome window within the existing process
                    let result = cdp.send("Target.createTarget", serde_json::json!({
                        "url": "about:blank",
                        "newWindow": true
                    })).await?;
                    let root_target_id = result["targetId"]
                        .as_str()
                        .ok_or_else(|| crate::error::PagerunnerError::Cdp("No targetId from createTarget newWindow".into()))?
                        .to_string();

                    let id = Uuid::new_v4().to_string();
                    let cdp_sessions_rev = std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
                    let mut owned_targets = std::collections::HashSet::new();
                    owned_targets.insert(root_target_id);

                    let events_rx = cdp.subscribe_events();
                    let cdp_for_processor = cdp.clone();
                    let session_id_for_processor = id.clone();
                    let rev_map = cdp_sessions_rev.clone();
                    let processor_handle = tokio::spawn(crate::network_log::network_event_processor(
                        events_rx,
                        cdp_for_processor,
                        session_id_for_processor,
                        db_for_processor,
                        rev_map,
                        network_config_capacity,
                        site_store_clone,
                    ));

                    let events_rx2 = cdp.subscribe_events();
                    let console_buffer = crate::console_log::new_buffer();
                    let console_buffer_for_proc = console_buffer.clone();
                    let rev_map2 = cdp_sessions_rev.clone();
                    let console_processor_handle = tokio::spawn(crate::console_log::console_event_processor(
                        events_rx2,
                        console_buffer_for_proc,
                        rev_map2,
                    ));

                    let tab_urls: TabUrlMap = Arc::new(RwLock::new(HashMap::new()));
                    let events_rx3 = cdp.subscribe_events();
                    let rev_map3 = cdp_sessions_rev.clone();
                    let tab_urls_for_proc = tab_urls.clone();
                    let frame_nav_handle = tokio::spawn(frame_nav_processor(
                        events_rx3,
                        rev_map3,
                        tab_urls_for_proc,
                    ));

                    self.sessions.insert(id.clone(), Session {
                        id: id.clone(),
                        profile_name: profile.name.clone(),
                        profile_display_name: profile.display_name.clone(),
                        stealth,
                        alive: true,
                        chrome: None,
                        owns_process: false,
                        is_attached: false,
                        owned_targets,
                        primary_session_id: Some(primary_id),
                        cdp,
                        cdp_sessions: HashMap::new(),
                        security_policy,
                        nav_count: 0,
                        tab_urls,
                        anon_config: None,
                        _reader_task: tokio::spawn(async {}), // secondary: reader already running in primary
                        cdp_sessions_rev,
                        _network_processor: Some(processor_handle),
                        network_enabled: false,
                        console_buffer,
                        _console_processor: Some(console_processor_handle),
                        _frame_nav_processor: Some(frame_nav_handle),
                    });
                    return Ok(id);
                }
            }
        }

        let user_data_dir = profile.user_data_dir.as_deref()
            .ok_or_else(|| crate::error::PagerunnerError::Config("Profile has no user_data_dir".into()))?;
        let result = crate::chrome::ChromeProcess::spawn(user_data_dir, stealth).await?;
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
            site_store,
        ));

        let events_rx2 = cdp.subscribe_events();
        let console_buffer = crate::console_log::new_buffer();
        let console_buffer_for_proc = console_buffer.clone();
        let rev_map2 = cdp_sessions_rev.clone();

        let console_processor_handle = tokio::spawn(crate::console_log::console_event_processor(
            events_rx2,
            console_buffer_for_proc,
            rev_map2,
        ));

        let tab_urls: TabUrlMap = Arc::new(RwLock::new(HashMap::new()));
        let events_rx3 = cdp.subscribe_events();
        let rev_map3 = cdp_sessions_rev.clone();
        let tab_urls_for_proc = tab_urls.clone();

        let frame_nav_processor_handle = tokio::spawn(frame_nav_processor(
            events_rx3,
            rev_map3,
            tab_urls_for_proc,
        ));

        // Collect initial tabs before inserting the session
        let initial_tabs = crate::browser::list_tabs(&cdp).await.unwrap_or_default();
        let owned_targets: std::collections::HashSet<String> = initial_tabs.iter().map(|t| t.target_id.clone()).collect();

        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: profile.name.clone(),
                profile_display_name: profile.display_name.clone(),
                stealth,
                alive: true,
                chrome: Some(result.process),
                owns_process: true,
                is_attached: false,
                owned_targets,
                primary_session_id: None,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls,
                anon_config: None,
                _reader_task: reader_task,
                cdp_sessions_rev,
                _network_processor: Some(processor_handle),
                network_enabled: false,
                console_buffer,
                _console_processor: Some(console_processor_handle),
                _frame_nav_processor: Some(frame_nav_processor_handle),
            },
        );
        self.profile_primary.insert(profile.name.clone(), id.clone());
        Ok(id)
    }

    /// Attach to an already-running Chrome instance started with `--remote-debugging-port`.
    /// `debug_url` is the HTTP base URL, e.g. `"http://localhost:9222"`.
    /// `profile_label` overrides the display name (defaults to Chrome version string).
    pub async fn attach(
        &mut self,
        debug_url: &str,
        profile_label: Option<String>,
        db: std::sync::Arc<crate::db::Db>,
        network_config: &crate::config::NetworkConfig,
        site_store: Option<std::sync::Arc<crate::site_knowledge::SiteKnowledgeStore>>,
    ) -> Result<SessionId> {
        // Fetch browser info + WebSocket URL
        let version_url = format!("{}/json/version", debug_url.trim_end_matches('/'));
        let version: serde_json::Value = reqwest::get(&version_url)
            .await
            .map_err(|e| crate::error::PagerunnerError::Config(
                format!("Cannot reach Chrome at {}: {}", debug_url, e)
            ))?
            .json()
            .await
            .map_err(|e| crate::error::PagerunnerError::Config(
                format!("Bad response from Chrome at {}: {}", debug_url, e)
            ))?;

        let ws_url = version["webSocketDebuggerUrl"]
            .as_str()
            .ok_or_else(|| crate::error::PagerunnerError::Config(
                "No webSocketDebuggerUrl in Chrome /json/version response — \
                 is Chrome running with --remote-debugging-port?".into()
            ))?
            .to_string();

        let browser_label = version["Browser"]
            .as_str()
            .unwrap_or("Chrome (attached)")
            .to_string();
        let display_name = profile_label.clone().unwrap_or(browser_label);
        let profile_name = profile_label.unwrap_or_else(|| "_attached".to_string());

        // Connect WebSocket
        let (cdp, reader_task) = crate::cdp::CdpConn::connect_ws(&ws_url).await?;

        // Discover existing page targets
        let targets_result = cdp.send("Target.getTargets", serde_json::json!({})).await?;
        let owned_targets: std::collections::HashSet<String> = targets_result["targetInfos"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        if t["type"].as_str() == Some("page") {
                            t["targetId"].as_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

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
            site_store,
        ));

        let events_rx2 = cdp.subscribe_events();
        let console_buffer = crate::console_log::new_buffer();
        let console_buffer_for_proc = console_buffer.clone();
        let rev_map2 = cdp_sessions_rev.clone();
        let console_processor_handle = tokio::spawn(crate::console_log::console_event_processor(
            events_rx2,
            console_buffer_for_proc,
            rev_map2,
        ));

        let tab_urls: TabUrlMap = Arc::new(RwLock::new(HashMap::new()));
        let events_rx3 = cdp.subscribe_events();
        let rev_map3 = cdp_sessions_rev.clone();
        let tab_urls_for_proc = tab_urls.clone();
        let frame_nav_handle = tokio::spawn(frame_nav_processor(
            events_rx3,
            rev_map3,
            tab_urls_for_proc,
        ));

        self.sessions.insert(id.clone(), Session {
            id: id.clone(),
            profile_name,
            profile_display_name: display_name,
            stealth: false,
            alive: true,
            chrome: None,
            owns_process: false,
            is_attached: true,
            owned_targets,
            primary_session_id: None,
            cdp,
            cdp_sessions: HashMap::new(),
            security_policy: None,
            nav_count: 0,
            tab_urls,
            anon_config: None,
            _reader_task: reader_task,
            cdp_sessions_rev,
            _network_processor: Some(processor_handle),
            network_enabled: false,
            console_buffer,
            _console_processor: Some(console_processor_handle),
            _frame_nav_processor: Some(frame_nav_handle),
        });

        Ok(id)
    }

    pub async fn close(&mut self, id: &str, db: &crate::db::Db) -> Result<()> {
        let mut session = self
            .sessions
            .remove(id)
            .ok_or_else(|| PagerunnerError::SessionNotFound(id.into()))?;

        if session.is_attached {
            // Attached session: just disconnect — don't touch the user's Chrome tabs
        } else if !session.owns_process {
            // Secondary session: close all owned tabs, don't kill Chrome
            for target_id in &session.owned_targets {
                let _ = session.cdp
                    .send("Target.closeTarget", serde_json::json!({ "targetId": target_id }))
                    .await;
            }
        } else {
            // Primary session: remove all secondary sessions for this profile first
            let profile = session.profile_name.clone();
            let secondary_ids: Vec<String> = self.sessions
                .values()
                .filter(|s| s.profile_name == profile && !s.owns_process)
                .map(|s| s.id.clone())
                .collect();
            for sec_id in secondary_ids {
                self.sessions.remove(&sec_id);
            }
            self.profile_primary.remove(&profile);

            // If Chrome has already crashed, skip the Browser.close CDP call (pipe is dead).
            if session.alive && session.is_chrome_running() {
                // Graceful shutdown: Browser.close lets Chrome write session state cleanly.
                // Fall back to kill if it doesn't exit within 3 seconds.
                let _ = session
                    .cdp
                    .send("Browser.close", serde_json::json!({}))
                    .await;
                if let Some(ref mut chrome) = session.chrome {
                    let graceful = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        chrome.wait(),
                    ).await;
                    if graceful.is_err() {
                        chrome.kill().await?;
                    }
                }
            }
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
                alive: s.alive,
            })
            .collect()
    }

    /// Look up a session and verify it's alive.
    /// Returns `SessionNotFound` if the ID doesn't exist.
    /// Returns `SessionDead` if Chrome has crashed.
    /// Marks the session `alive = false` when Chrome is detected as dead.
    pub fn get_live(&mut self, id: &str) -> crate::error::Result<&mut Session> {
        // Check exists
        if !self.sessions.contains_key(id) {
            return Err(crate::error::PagerunnerError::SessionNotFound(id.into()));
        }

        let session = self.sessions.get(id).unwrap();

        // Already marked dead
        if !session.alive {
            return Err(crate::error::PagerunnerError::SessionDead(id.into()));
        }

        // Attached sessions have no owned process and no primary — they stay alive
        // until explicitly closed or the WebSocket drops.
        if session.is_attached {
            return Ok(self.sessions.get_mut(id).unwrap());
        }

        // For secondary sessions, check if the primary session is still alive
        if !session.owns_process {
            let primary_alive = session.primary_session_id
                .as_ref()
                .and_then(|pid| self.sessions.get(pid))
                .map(|p| p.alive)
                .unwrap_or(false);
            if !primary_alive {
                self.sessions.get_mut(id).unwrap().alive = false;
                return Err(crate::error::PagerunnerError::SessionDead(id.into()));
            }
            return Ok(self.sessions.get_mut(id).unwrap());
        }

        // For primary sessions, lazy check: is Chrome still running?
        let chrome_running = self.sessions.get_mut(id).unwrap().is_chrome_running();
        if !chrome_running {
            self.sessions.get_mut(id).unwrap().alive = false;
            return Err(crate::error::PagerunnerError::SessionDead(id.into()));
        }

        Ok(self.sessions.get_mut(id).unwrap())
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
                alive: true,
                chrome: Some(chrome),
                owns_process: true,
                is_attached: false,
                owned_targets: std::collections::HashSet::new(),
                primary_session_id: None,
                cdp,
                cdp_sessions: HashMap::new(),
                security_policy,
                nav_count: 0,
                tab_urls: Arc::new(RwLock::new(HashMap::new())),
                anon_config: None,
                _reader_task: tokio::spawn(async {}),
                cdp_sessions_rev: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
                _network_processor: None,
                network_enabled: false,
                console_buffer: crate::console_log::new_buffer(),
                _console_processor: None,
                _frame_nav_processor: None,
            },
        );
        id
    }
}

/// Async task that watches CDP events for `Page.frameNavigated` and updates
/// `tab_urls` for the main frame (no `parentId`) so SPA and OAuth redirects
/// are reflected without requiring an explicit `navigate` call.
pub async fn frame_nav_processor(
    mut events: broadcast::Receiver<serde_json::Value>,
    cdp_sessions_rev: Arc<RwLock<HashMap<String, String>>>,
    tab_urls: TabUrlMap,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                if event.get("method").and_then(|m| m.as_str()) != Some("Page.frameNavigated") {
                    continue;
                }
                let params = &event["params"];
                // Only update for the main frame — ignore iframes (which have a parentId).
                // serde_json returns Value::Null for both absent keys and JSON null, so
                // is_null() correctly identifies the main frame whether parentId is absent
                // or explicitly set to null.
                if !params["frame"]["parentId"].is_null() {
                    continue;
                }
                let url = match params["frame"]["url"].as_str() {
                    Some(u) => u.to_string(),
                    None => continue,
                };
                let cdp_session = event
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                // cdp_sessions_rev is populated by fresh_attach, which runs only when a
                // tool first attaches to a tab. The very first frameNavigated event (at
                // Chrome launch) may fire before any attachment, in which case the map is
                // empty and we fall back to the CDP session ID as the key. Subsequent
                // navigations after a fresh_attach will use the correct target_id.
                let target_id = cdp_sessions_rev
                    .read()
                    .ok()
                    .and_then(|m| m.get(&cdp_session).cloned())
                    .unwrap_or_else(|| cdp_session.clone());
                if let Ok(mut map) = tab_urls.write() {
                    map.insert(target_id, url);
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(
                    "Frame nav processor lagged, dropped {} events",
                    n
                );
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> ChromeProfile {
        ChromeProfile {
            name: "test".into(),
            display_name: "Test".into(),
            user_data_dir: Some("/tmp/pagerunner-test-profile".into()),
            debug_port: None,
            kind: None,
        }
    }
}
