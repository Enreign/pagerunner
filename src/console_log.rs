use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

const CONSOLE_CAPACITY: usize = 200;
const EXCEPTION_CAPACITY: usize = 200;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
    pub url: Option<String>,
    pub line: Option<u32>,
    pub timestamp_ms: u64,
    pub tab_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExceptionEntry {
    pub text: String,
    pub url: Option<String>,
    pub line: Option<u32>,
    pub timestamp_ms: u64,
    pub tab_id: String,
}

pub struct ConsoleBuffers {
    pub console: VecDeque<ConsoleEntry>,
    pub exceptions: VecDeque<ExceptionEntry>,
}

impl ConsoleBuffers {
    pub fn new() -> Self {
        Self {
            console: VecDeque::with_capacity(CONSOLE_CAPACITY),
            exceptions: VecDeque::with_capacity(EXCEPTION_CAPACITY),
        }
    }
}

pub type ConsoleBuffer = Arc<Mutex<ConsoleBuffers>>;

pub fn new_buffer() -> ConsoleBuffer {
    Arc::new(Mutex::new(ConsoleBuffers::new()))
}

pub async fn console_event_processor(
    mut events: broadcast::Receiver<serde_json::Value>,
    buffer: ConsoleBuffer,
    cdp_sessions_rev: Arc<RwLock<HashMap<String, String>>>,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                let cdp_session = event
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let method = event.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let params = &event["params"];

                let tab_id = cdp_sessions_rev
                    .read()
                    .ok()
                    .and_then(|m| m.get(&cdp_session).cloned())
                    .unwrap_or_else(|| cdp_session.clone());

                match method {
                    "Runtime.consoleAPICalled" => {
                        let level = params["type"].as_str().unwrap_or("log").to_string();
                        let text = params["args"]
                            .as_array()
                            .map(|args| {
                                args.iter()
                                    .map(|a| {
                                        a.get("value")
                                            .and_then(|v| {
                                                if v.is_string() {
                                                    v.as_str().map(String::from)
                                                } else {
                                                    Some(v.to_string())
                                                }
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default();
                        let url = params["stackTrace"]["callFrames"][0]["url"]
                            .as_str()
                            .map(String::from);
                        let line = params["stackTrace"]["callFrames"][0]["lineNumber"]
                            .as_u64()
                            .map(|n| n as u32);
                        let timestamp_ms = params["timestamp"]
                            .as_f64()
                            .map(|t| (t * 1000.0) as u64)
                            .unwrap_or_else(|| {
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64
                            });

                        if let Ok(mut buf) = buffer.lock() {
                            if buf.console.len() >= CONSOLE_CAPACITY {
                                buf.console.pop_front();
                            }
                            buf.console.push_back(ConsoleEntry {
                                level,
                                text,
                                url,
                                line,
                                timestamp_ms,
                                tab_id,
                            });
                        }
                    }
                    "Runtime.exceptionThrown" => {
                        let details = &params["exceptionDetails"];
                        let text = details["text"]
                            .as_str()
                            .or_else(|| details["exception"]["description"].as_str())
                            .unwrap_or("Exception thrown")
                            .to_string();
                        let url = details["url"].as_str().map(String::from);
                        let line = details["lineNumber"].as_u64().map(|n| n as u32);
                        let timestamp_ms = params["timestamp"]
                            .as_f64()
                            .map(|t| (t * 1000.0) as u64)
                            .unwrap_or_else(|| {
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64
                            });

                        if let Ok(mut buf) = buffer.lock() {
                            if buf.exceptions.len() >= EXCEPTION_CAPACITY {
                                buf.exceptions.pop_front();
                            }
                            buf.exceptions.push_back(ExceptionEntry {
                                text,
                                url,
                                line,
                                timestamp_ms,
                                tab_id,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Console event processor lagged, dropped {} events", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Get the last `limit` console entries for a tab (most recent first).
pub fn get_tab_console(buffer: &ConsoleBuffer, tab_id: &str, limit: usize) -> Vec<ConsoleEntry> {
    buffer
        .lock()
        .ok()
        .map(|buf| {
            buf.console
                .iter()
                .rev()
                .filter(|e| e.tab_id == tab_id)
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Get the last `limit` exception entries for a tab (most recent first).
pub fn get_tab_exceptions(
    buffer: &ConsoleBuffer,
    tab_id: &str,
    limit: usize,
) -> Vec<ExceptionEntry> {
    buffer
        .lock()
        .ok()
        .map(|buf| {
            buf.exceptions
                .iter()
                .rev()
                .filter(|e| e.tab_id == tab_id)
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_respects_capacity() {
        let buffer = new_buffer();
        for i in 0..(CONSOLE_CAPACITY + 5) {
            let mut buf = buffer.lock().unwrap();
            if buf.console.len() >= CONSOLE_CAPACITY {
                buf.console.pop_front();
            }
            buf.console.push_back(ConsoleEntry {
                level: "log".into(),
                text: format!("msg {}", i),
                url: None,
                line: None,
                timestamp_ms: i as u64,
                tab_id: "tab1".into(),
            });
        }
        let buf = buffer.lock().unwrap();
        assert_eq!(buf.console.len(), CONSOLE_CAPACITY);
        // most recent should be the last entry (index CONSOLE_CAPACITY+4)
        assert!(buf
            .console
            .back()
            .unwrap()
            .text
            .contains(&(CONSOLE_CAPACITY + 4).to_string()));
    }

    #[test]
    fn test_get_tab_console_filters_by_tab() {
        let buffer = new_buffer();
        {
            let mut buf = buffer.lock().unwrap();
            buf.console.push_back(ConsoleEntry {
                level: "log".into(),
                text: "tab1 msg".into(),
                url: None,
                line: None,
                timestamp_ms: 1,
                tab_id: "tab1".into(),
            });
            buf.console.push_back(ConsoleEntry {
                level: "log".into(),
                text: "tab2 msg".into(),
                url: None,
                line: None,
                timestamp_ms: 2,
                tab_id: "tab2".into(),
            });
        }
        let tab1 = get_tab_console(&buffer, "tab1", 10);
        assert_eq!(tab1.len(), 1);
        assert_eq!(tab1[0].text, "tab1 msg");
    }

    #[test]
    fn test_get_tab_exceptions_most_recent_first() {
        let buffer = new_buffer();
        {
            let mut buf = buffer.lock().unwrap();
            buf.exceptions.push_back(ExceptionEntry {
                text: "first".into(),
                url: None,
                line: None,
                timestamp_ms: 1,
                tab_id: "tab1".into(),
            });
            buf.exceptions.push_back(ExceptionEntry {
                text: "second".into(),
                url: None,
                line: None,
                timestamp_ms: 2,
                tab_id: "tab1".into(),
            });
        }
        let results = get_tab_exceptions(&buffer, "tab1", 10);
        assert_eq!(results[0].text, "second"); // most recent first
        assert_eq!(results[1].text, "first");
    }
}
