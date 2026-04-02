use crate::cdp::CdpConn;
use crate::error::{PagerunnerError, Result};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Backoff schedule for reconnection attempts.
const BACKOFF_SCHEDULE: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

/// Maximum total time to spend on reconnection attempts.
pub const RECONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Returns the backoff duration for the given attempt number.
/// Clamps to the last value in the schedule for attempts beyond its length.
pub fn backoff_duration(attempt: usize) -> Duration {
    if attempt < BACKOFF_SCHEDULE.len() {
        BACKOFF_SCHEDULE[attempt]
    } else {
        BACKOFF_SCHEDULE[BACKOFF_SCHEDULE.len() - 1]
    }
}

/// Fetch the `webSocketDebuggerUrl` from Chrome's `/json/version` endpoint.
/// Uses a short (500ms) timeout since Chrome might not be ready yet.
pub async fn fetch_ws_url(version_url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .ok()?;
    let resp = client.get(version_url).send().await.ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json["webSocketDebuggerUrl"].as_str().map(|s| s.to_string())
}

/// Attempt to reconnect a CDP connection to Chrome on the given debug port.
/// Loops with exponential backoff until either:
/// - The WebSocket transport is successfully replaced (returns the new reader handle).
/// - The total timeout is exceeded (returns an error).
pub async fn reconnect_cdp(debug_port: u16, cdp: &CdpConn) -> Result<JoinHandle<()>> {
    let version_url = format!("http://127.0.0.1:{}/json/version", debug_port);
    let start = Instant::now();
    let mut attempt: usize = 0;

    loop {
        if start.elapsed() >= RECONNECT_TIMEOUT {
            return Err(PagerunnerError::Cdp(format!(
                "Reconnection timed out after {:?} ({} attempts)",
                start.elapsed(),
                attempt
            )));
        }

        // Try reconnection first, sleep after failure
        if let Some(ws_url) = fetch_ws_url(&version_url).await {
            match cdp.replace_ws_transport(&ws_url).await {
                Ok(handle) => {
                    tracing::info!(
                        port = debug_port,
                        attempts = attempt + 1,
                        elapsed_ms = start.elapsed().as_millis() as u64,
                        "CDP reconnection succeeded"
                    );
                    return Ok(handle);
                }
                Err(e) => {
                    tracing::debug!(
                        port = debug_port,
                        attempt = attempt + 1,
                        error = %e,
                        "CDP reconnection attempt failed"
                    );
                }
            }
        } else {
            tracing::debug!(
                port = debug_port,
                attempt = attempt + 1,
                "Chrome /json/version not ready yet"
            );
        }

        tokio::time::sleep(backoff_duration(attempt)).await;
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_duration_returns_schedule_values() {
        assert_eq!(backoff_duration(0), Duration::from_millis(100));
        assert_eq!(backoff_duration(1), Duration::from_millis(200));
        assert_eq!(backoff_duration(2), Duration::from_millis(500));
        assert_eq!(backoff_duration(3), Duration::from_secs(1));
        assert_eq!(backoff_duration(4), Duration::from_secs(2));
    }

    #[test]
    fn backoff_duration_clamps_beyond_schedule() {
        assert_eq!(backoff_duration(5), Duration::from_secs(2));
        assert_eq!(backoff_duration(100), Duration::from_secs(2));
        assert_eq!(backoff_duration(usize::MAX), Duration::from_secs(2));
    }

    #[test]
    fn reconnect_timeout_is_30_seconds() {
        assert_eq!(RECONNECT_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn backoff_schedule_is_monotonically_non_decreasing() {
        for i in 1..BACKOFF_SCHEDULE.len() {
            assert!(BACKOFF_SCHEDULE[i] >= BACKOFF_SCHEDULE[i - 1]);
        }
    }

    #[test]
    fn backoff_never_decreases() {
        let mut prev = Duration::ZERO;
        for i in 0..20 {
            let d = backoff_duration(i);
            assert!(d >= prev, "backoff_duration({}) = {:?} < prev {:?}", i, d, prev);
            prev = d;
        }
    }
}
