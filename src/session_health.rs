/// Health state machine for browser sessions.
///
/// Transitions:
/// - `Alive` → `Reconnecting` (WebSocket drops, Chrome still alive)
/// - `Reconnecting` → `Alive` (reconnection succeeds)
/// - `Reconnecting` → `Dead` (reconnection fails or Chrome exits)
/// - `Alive` → `Dead` (Chrome process exits)
/// - `Dead` → `Recovering` (checkpoint restore in progress)
/// - `Recovering` → `Alive` (restore succeeds)
/// - `Recovering` → `Dead` (restore fails)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHealth {
    Alive,
    Reconnecting,
    Dead,
    Recovering,
}

impl SessionHealth {
    /// True only when the session is fully operational.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Alive)
    }

    /// True when the session is alive or temporarily reconnecting
    /// (i.e. not permanently dead or mid-recovery).
    pub fn is_alive_or_reconnecting(&self) -> bool {
        matches!(self, Self::Alive | Self::Reconnecting)
    }

    /// Human-readable status string for JSON responses.
    pub fn status_str(&self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Reconnecting => "reconnecting",
            Self::Dead => "crashed",
            Self::Recovering => "recovering",
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
        assert!(!SessionHealth::Dead.is_usable());
        assert!(!SessionHealth::Recovering.is_usable());
    }

    #[test]
    fn is_alive_or_reconnecting_covers_both() {
        assert!(SessionHealth::Alive.is_alive_or_reconnecting());
        assert!(SessionHealth::Reconnecting.is_alive_or_reconnecting());
        assert!(!SessionHealth::Dead.is_alive_or_reconnecting());
        assert!(!SessionHealth::Recovering.is_alive_or_reconnecting());
    }

    #[test]
    fn status_str_values() {
        assert_eq!(SessionHealth::Alive.status_str(), "alive");
        assert_eq!(SessionHealth::Reconnecting.status_str(), "reconnecting");
        assert_eq!(SessionHealth::Dead.status_str(), "crashed");
        assert_eq!(SessionHealth::Recovering.status_str(), "recovering");
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
        let mut health = SessionHealth::Alive;

        health = SessionHealth::Reconnecting;
        assert!(health.is_alive_or_reconnecting());

        // Reconnection failed
        health = SessionHealth::Dead;
        assert!(!health.is_usable());
        assert!(!health.is_alive_or_reconnecting());
        assert_eq!(health.status_str(), "crashed");
    }

    #[test]
    fn test_transition_alive_to_dead_directly() {
        let mut health = SessionHealth::Alive;

        // Chrome process died — skip Reconnecting
        health = SessionHealth::Dead;
        assert!(!health.is_usable());
        assert_eq!(health.status_str(), "crashed");
    }
}
