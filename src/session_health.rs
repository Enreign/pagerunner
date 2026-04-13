/// Health state machine for browser sessions.
///
/// Transitions:
/// - `Alive` → `Reconnecting` (WebSocket drops, Chrome still alive)
/// - `Alive` → `Recovering` (Chrome process exits, owned process)
/// - `Reconnecting` → `Alive` (reconnection succeeds)
/// - `Reconnecting` → `Dead` (reconnection fails or Chrome exits)
/// - `Recovering` → `Alive` (new Chrome spawned + checkpoint restored)
/// - `Recovering` → `Dead` (recovery fails)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHealth {
    Alive,
    Reconnecting,
    /// Chrome process died. Daemon is spawning new Chrome + restoring checkpoint.
    Recovering,
    Dead,
}

impl SessionHealth {
    /// True only when the session is fully operational.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Alive)
    }

    /// True when the session is alive or temporarily in a transient state
    /// (reconnecting or recovering) — i.e. not permanently dead.
    pub fn is_alive_or_reconnecting(&self) -> bool {
        matches!(self, Self::Alive | Self::Reconnecting | Self::Recovering)
    }

    /// Human-readable status string for JSON responses.
    pub fn status_str(&self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Reconnecting => "reconnecting",
            Self::Recovering => "recovering",
            Self::Dead => "crashed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_usable_only_for_alive() {
        assert!(SessionHealth::Alive.is_usable());
        assert!(!SessionHealth::Reconnecting.is_usable());
        assert!(!SessionHealth::Recovering.is_usable());
        assert!(!SessionHealth::Dead.is_usable());
    }

    #[test]
    fn is_alive_or_reconnecting_covers_transient_states() {
        assert!(SessionHealth::Alive.is_alive_or_reconnecting());
        assert!(SessionHealth::Reconnecting.is_alive_or_reconnecting());
        assert!(SessionHealth::Recovering.is_alive_or_reconnecting());
        assert!(!SessionHealth::Dead.is_alive_or_reconnecting());
    }

    #[test]
    fn status_str_values() {
        assert_eq!(SessionHealth::Alive.status_str(), "alive");
        assert_eq!(SessionHealth::Reconnecting.status_str(), "reconnecting");
        assert_eq!(SessionHealth::Recovering.status_str(), "recovering");
        assert_eq!(SessionHealth::Dead.status_str(), "crashed");
    }

    #[test]
    fn equality_and_clone() {
        let a = SessionHealth::Alive;
        let b = a;
        assert_eq!(a, b);
        let c = SessionHealth::Dead;
        assert_ne!(a, c);
    }

    #[test]
    fn test_transition_alive_to_reconnecting_to_alive() {
        let mut health = SessionHealth::Alive;
        assert!(health.is_usable());

        // Simulate disconnect
        health = SessionHealth::Reconnecting;
        assert!(!health.is_usable());
        assert!(health.is_alive_or_reconnecting());
        assert_eq!(health.status_str(), "reconnecting");

        // Simulate successful reconnection
        health = SessionHealth::Alive;
        assert!(health.is_usable());
        assert_eq!(health.status_str(), "alive");
    }

    #[test]
    fn test_transition_alive_to_reconnecting_to_dead() {
        let mut health = SessionHealth::Reconnecting;

        assert!(health.is_alive_or_reconnecting());

        // Reconnection failed
        health = SessionHealth::Dead;
        assert!(!health.is_usable());
        assert!(!health.is_alive_or_reconnecting());
        assert_eq!(health.status_str(), "crashed");
    }

    #[test]
    fn test_transition_alive_to_dead_directly() {
        let health = SessionHealth::Dead;
        assert!(!health.is_usable());
        assert_eq!(health.status_str(), "crashed");
    }

    #[test]
    fn test_transition_alive_to_recovering_to_alive() {
        let mut health = SessionHealth::Recovering;
        assert!(!health.is_usable());
        assert!(health.is_alive_or_reconnecting());
        assert_eq!(health.status_str(), "recovering");

        // Recovery succeeded — new Chrome spawned + checkpoint restored
        health = SessionHealth::Alive;
        assert!(health.is_usable());
        assert_eq!(health.status_str(), "alive");
    }

    #[test]
    fn test_transition_alive_to_recovering_to_dead() {
        let mut health = SessionHealth::Recovering;
        assert!(!health.is_usable());
        assert!(health.is_alive_or_reconnecting());

        // Recovery failed
        health = SessionHealth::Dead;
        assert!(!health.is_usable());
        assert!(!health.is_alive_or_reconnecting());
        assert_eq!(health.status_str(), "crashed");
    }
}
