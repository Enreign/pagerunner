use crate::cdp::CdpConn;
use crate::chrome::ChromeProcess;
use crate::config::ChromeProfile;
use crate::error::{PagerunnerError, Result};
use crate::session_health::SessionHealth;
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
    pub health: SessionHealth,
    // TCP port Chrome listens on for reattach
    pub debug_port: u16,
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
    pub status: String,
}

/// Poll Chrome's TCP debug endpoint until it's ready, then return the WebSocket URL.
/// Retries up to 50 times with 100ms intervals (5 seconds total).
async fn wait_for_chrome_ws_url(debug_port: u16) -> crate::error::Result<String> {
    let version_url = format!("http://127.0.0.1:{}/json/version", debug_port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;

    for attempt in 0..50 {
        if let Ok(resp) = client.get(&version_url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(ws_url) = json["webSocketDebuggerUrl"].as_str() {
                    tracing::debug!(
                        port = debug_port,
                        attempts = attempt + 1,
                        "Chrome TCP endpoint ready"
                    );
                    return Ok(ws_url.to_string());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    Err(crate::error::PagerunnerError::Config(format!(
        "Chrome did not respond on port {} after 5 seconds",
        debug_port
    )))
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
                if primary.health == SessionHealth::Alive {
                    let cdp = primary.cdp.clone();
                    let db_for_processor = db.clone();
                    let network_config_capacity = network_config.buffer_capacity;
                    let site_store_clone = site_store.clone();

                    // Open a new Chrome window within the existing process
                    let result = cdp
                        .send(
                            "Target.createTarget",
                            serde_json::json!({
                                "url": "about:blank",
                                "newWindow": true
                            }),
                        )
                        .await?;
                    let root_target_id = result["targetId"]
                        .as_str()
                        .ok_or_else(|| {
                            crate::error::PagerunnerError::Cdp(
                                "No targetId from createTarget newWindow".into(),
                            )
                        })?
                        .to_string();

                    let id = Uuid::new_v4().to_string();
                    let cdp_sessions_rev =
                        std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
                    let mut owned_targets = std::collections::HashSet::new();
                    owned_targets.insert(root_target_id);

                    let events_rx = cdp.subscribe_events();
                    let cdp_for_processor = cdp.clone();
                    let session_id_for_processor = id.clone();
                    let rev_map = cdp_sessions_rev.clone();
                    let processor_handle =
                        tokio::spawn(crate::network_log::network_event_processor(
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
                    let console_processor_handle =
                        tokio::spawn(crate::console_log::console_event_processor(
                            events_rx2,
                            console_buffer_for_proc,
                            rev_map2,
                        ));

                    let tab_urls: TabUrlMap = Arc::new(RwLock::new(HashMap::new()));
                    let events_rx3 = cdp.subscribe_events();
                    let rev_map3 = cdp_sessions_rev.clone();
                    let tab_urls_for_proc = tab_urls.clone();
                    let frame_nav_handle =
                        tokio::spawn(frame_nav_processor(events_rx3, rev_map3, tab_urls_for_proc));

                    self.sessions.insert(
                        id.clone(),
                        Session {
                            id: id.clone(),
                            profile_name: profile.name.clone(),
                            profile_display_name: profile.display_name.clone(),
                            stealth,
                            health: SessionHealth::Alive,
                            debug_port: 0,
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
                        },
                    );
                    return Ok(id);
                }
            }
        }

        let user_data_dir = profile.user_data_dir.as_deref().ok_or_else(|| {
            crate::error::PagerunnerError::Config("Profile has no user_data_dir".into())
        })?;
        let result = crate::chrome::ChromeProcess::spawn(user_data_dir, stealth).await?;
        // Connect to Chrome via TCP WebSocket (same path as attach_session)
        let ws_url = wait_for_chrome_ws_url(result.debug_port).await?;
        let (cdp, reader_task) = crate::cdp::CdpConn::connect_ws(&ws_url).await?;
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

        let frame_nav_processor_handle =
            tokio::spawn(frame_nav_processor(events_rx3, rev_map3, tab_urls_for_proc));

        // Collect initial tabs before inserting the session
        let initial_tabs = crate::browser::list_tabs(&cdp).await.unwrap_or_default();
        let owned_targets: std::collections::HashSet<String> =
            initial_tabs.iter().map(|t| t.target_id.clone()).collect();

        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name: profile.name.clone(),
                profile_display_name: profile.display_name.clone(),
                stealth,
                health: SessionHealth::Alive,
                debug_port: result.debug_port,
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
        self.profile_primary
            .insert(profile.name.clone(), id.clone());
        Ok(id)
    }

    /// Attach to an already-running Chrome instance started with `--remote-debugging-port`.
    /// `debug_url` is the HTTP base URL, e.g. `"http://localhost:9222"`.
    /// `profile_label` overrides the display name (defaults to Chrome version string).
    pub async fn attach(
        &mut self,
        debug_url: &str,
        profile_name: Option<String>,
        display_name: Option<String>,
        db: std::sync::Arc<crate::db::Db>,
        network_config: &crate::config::NetworkConfig,
        site_store: Option<std::sync::Arc<crate::site_knowledge::SiteKnowledgeStore>>,
    ) -> Result<SessionId> {
        // Fetch browser info + WebSocket URL
        let version_url = format!("{}/json/version", debug_url.trim_end_matches('/'));
        let version: serde_json::Value = reqwest::get(&version_url)
            .await
            .map_err(|e| {
                crate::error::PagerunnerError::Config(format!(
                    "Cannot reach Chrome at {}: {}",
                    debug_url, e
                ))
            })?
            .json()
            .await
            .map_err(|e| {
                crate::error::PagerunnerError::Config(format!(
                    "Bad response from Chrome at {}: {}",
                    debug_url, e
                ))
            })?;

        let ws_url_raw = version["webSocketDebuggerUrl"].as_str().ok_or_else(|| {
            crate::error::PagerunnerError::Config(
                "No webSocketDebuggerUrl in Chrome /json/version response — \
                 is Chrome running with --remote-debugging-port?"
                    .into(),
            )
        })?;

        // Chrome behind a proxy (VM, gvproxy) returns a webSocketDebuggerUrl
        // with no port (e.g. `ws://localhost/devtools/browser/...`). Rewrite
        // the authority to match the host:port we actually connected to.
        let ws_url = if let Ok(parsed_debug) = url::Url::parse(debug_url) {
            if let Ok(mut parsed_ws) = url::Url::parse(ws_url_raw) {
                let host = parsed_debug.host_str().unwrap_or("localhost");
                let port = parsed_debug.port();
                let _ = parsed_ws.set_host(Some(host));
                let _ = parsed_ws.set_port(port);
                parsed_ws.to_string()
            } else {
                ws_url_raw.to_string()
            }
        } else {
            ws_url_raw.to_string()
        };

        let browser_label = version["Browser"]
            .as_str()
            .unwrap_or("Chrome (attached)")
            .to_string();
        let display_name = display_name.unwrap_or_else(|| browser_label.clone());
        let profile_name = profile_name.unwrap_or_else(|| "_attached".to_string());

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
        let frame_nav_handle =
            tokio::spawn(frame_nav_processor(events_rx3, rev_map3, tab_urls_for_proc));

        self.sessions.insert(
            id.clone(),
            Session {
                id: id.clone(),
                profile_name,
                profile_display_name: display_name,
                stealth: false,
                health: SessionHealth::Alive,
                debug_port: 0,
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
            },
        );

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
                let _ = session
                    .cdp
                    .send(
                        "Target.closeTarget",
                        serde_json::json!({ "targetId": target_id }),
                    )
                    .await;
            }
        } else {
            // Primary session: remove all secondary sessions for this profile first
            let profile = session.profile_name.clone();
            let secondary_ids: Vec<String> = self
                .sessions
                .values()
                .filter(|s| s.profile_name == profile && !s.owns_process)
                .map(|s| s.id.clone())
                .collect();
            for sec_id in secondary_ids {
                self.sessions.remove(&sec_id);
            }
            self.profile_primary.remove(&profile);

            // If Chrome has already crashed, skip the Browser.close CDP call (pipe is dead).
            if session.health == SessionHealth::Alive && session.is_chrome_running() {
                // Graceful shutdown: Browser.close lets Chrome write session state cleanly.
                // Fall back to kill if it doesn't exit within 3 seconds.
                let _ = session
                    .cdp
                    .send("Browser.close", serde_json::json!({}))
                    .await;
                if let Some(ref mut chrome) = session.chrome {
                    let graceful =
                        tokio::time::timeout(std::time::Duration::from_secs(3), chrome.wait())
                            .await;
                    if graceful.is_err() {
                        chrome.kill().await?;
                    }
                }
            }
        }

        let _ = crate::network_log::delete_session_entries(db, id);
        Ok(())
    }

    /// Kill all owned Chrome processes. Called on daemon/MCP shutdown to prevent orphans.
    /// With TCP-only transport, Chrome survives process exit — this ensures cleanup.
    pub async fn kill_all_chrome(&mut self) {
        for session in self.sessions.values_mut() {
            if session.owns_process {
                if let Some(ref mut chrome) = session.chrome {
                    let _ = chrome.kill().await;
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn list(&mut self) -> Vec<SessionInfo> {
        // Proactively update alive flag via OS process check (no CDP — never hangs).
        // Abort background tasks for newly-detected crashed sessions so their
        // event-loop awaits (events.recv()) are cancelled immediately.
        for session in self.sessions.values_mut() {
            if session.health == SessionHealth::Dead {
                continue;
            }
            // Detect disconnection: either the Chrome process is gone (for owned processes)
            // or the WebSocket reader task has finished (for all session types).
            let ws_dropped = session._reader_task.is_finished();
            let chrome_dead = session.owns_process && !session.is_chrome_running();

            if chrome_dead {
                // Chrome process truly gone → Dead
                session.health = SessionHealth::Dead;
                session._reader_task.abort();
                if let Some(ref h) = session._network_processor {
                    h.abort();
                }
                if let Some(ref h) = session._console_processor {
                    h.abort();
                }
                if let Some(ref h) = session._frame_nav_processor {
                    h.abort();
                }
            } else if ws_dropped && session.health == SessionHealth::Alive {
                // WebSocket dropped but Chrome may still be running → Reconnecting
                session.health = SessionHealth::Reconnecting;
            }
        }
        // Secondary sessions whose primary is dead also become dead.
        let dead_primaries: std::collections::HashSet<String> = self
            .sessions
            .values()
            .filter(|s| s.health == SessionHealth::Dead && s.owns_process)
            .map(|s| s.id.clone())
            .collect();
        for session in self.sessions.values_mut() {
            if session.health != SessionHealth::Dead && !session.owns_process {
                if let Some(ref pid) = session.primary_session_id {
                    if dead_primaries.contains(pid) {
                        session.health = SessionHealth::Dead;
                    }
                }
            }
        }
        self.sessions
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                profile_name: s.profile_name.clone(),
                profile_display_name: s.profile_display_name.clone(),
                stealth: s.stealth,
                alive: s.health.is_alive_or_reconnecting(),
                status: s.health.status_str().to_string(),
            })
            .collect()
    }

    /// Attempt to reconnect a session that is in the `Reconnecting` state.
    /// On success: replaces the reader task, sets health back to Alive, clears CDP session cache.
    /// On failure: sets health to Dead and aborts all background tasks.
    pub async fn attempt_reconnect(&mut self, id: &str) -> Result<()> {
        let session = match self.sessions.get(id) {
            Some(s) => s,
            None => return Err(PagerunnerError::SessionNotFound(id.into())),
        };

        // Only reconnect sessions in Reconnecting state
        if session.health != SessionHealth::Reconnecting {
            return Ok(());
        }

        let debug_port = session.debug_port;
        if debug_port == 0 {
            // Secondary session with no port — cannot reconnect independently
            let session = self.sessions.get_mut(id).unwrap();
            session.health = SessionHealth::Dead;
            session._reader_task.abort();
            if let Some(ref h) = session._network_processor {
                h.abort();
            }
            if let Some(ref h) = session._console_processor {
                h.abort();
            }
            if let Some(ref h) = session._frame_nav_processor {
                h.abort();
            }
            return Err(PagerunnerError::Cdp(
                "Cannot reconnect session with no debug port".into(),
            ));
        }

        let cdp = session.cdp.clone();

        // Attempt reconnection (this may take up to RECONNECT_TIMEOUT)
        match crate::cdp_reconnect::reconnect_cdp(debug_port, &cdp).await {
            Ok(new_reader_handle) => {
                let session = self.sessions.get_mut(id).unwrap();
                session._reader_task.abort();
                session._reader_task = new_reader_handle;
                session.health = SessionHealth::Alive;
                session.cdp_sessions.clear();
                tracing::info!(session_id = id, "Session reconnected successfully");
                Ok(())
            }
            Err(e) => {
                let session = self.sessions.get_mut(id).unwrap();
                session.health = SessionHealth::Dead;
                session._reader_task.abort();
                if let Some(ref h) = session._network_processor {
                    h.abort();
                }
                if let Some(ref h) = session._console_processor {
                    h.abort();
                }
                if let Some(ref h) = session._frame_nav_processor {
                    h.abort();
                }
                tracing::warn!(session_id = id, error = %e, "Session reconnection failed, marked dead");
                Err(e)
            }
        }
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

        // Check health state
        match session.health {
            SessionHealth::Dead => {
                return Err(crate::error::PagerunnerError::SessionDead(id.into()));
            }
            SessionHealth::Reconnecting => {
                return Err(crate::error::PagerunnerError::SessionReconnecting(id.into()));
            }
            SessionHealth::Alive | SessionHealth::Recovering => {
                // proceed
            }
        }

        // Attached sessions have no owned process and no primary — they stay alive
        // until explicitly closed or the WebSocket drops.
        if session.is_attached {
            return Ok(self.sessions.get_mut(id).unwrap());
        }

        // For secondary sessions, check if the primary session is still alive
        if !session.owns_process {
            let primary_alive = session
                .primary_session_id
                .as_ref()
                .and_then(|pid| self.sessions.get(pid))
                .map(|p| p.health.is_alive_or_reconnecting())
                .unwrap_or(false);
            if !primary_alive {
                self.sessions.get_mut(id).unwrap().health = SessionHealth::Dead;
                return Err(crate::error::PagerunnerError::SessionDead(id.into()));
            }
            return Ok(self.sessions.get_mut(id).unwrap());
        }

        // For primary sessions, lazy check: is Chrome still running?
        let chrome_running = self.sessions.get_mut(id).unwrap().is_chrome_running();
        if !chrome_running {
            self.sessions.get_mut(id).unwrap().health = SessionHealth::Dead;
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
                health: SessionHealth::Alive,
                debug_port: 0,
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
                tracing::warn!("Frame nav processor lagged, dropped {} events", n);
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
