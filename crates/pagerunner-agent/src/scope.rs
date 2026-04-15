//! Scope — agent-visible, thread-scoped multi-tab context passed in from the
//! iOS client. Feeds the system prompt and receives write-back events.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scope {
    #[serde(default)]
    pub tabs: Vec<ScopeTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub turn_log: Vec<TurnLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeTab {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl ScopeTab {
    /// Derived id matching the iOS `ScopeTab.id` format:
    /// `"{sessionId}-{targetId ?? "first"}"`.
    pub fn id(&self) -> String {
        format!(
            "{}-{}",
            self.session_id,
            self.target_id.as_deref().unwrap_or("first")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnLogEntry {
    pub user_goal: String,
    pub summary: String,
    #[serde(default)]
    pub touched_tab_ids: Vec<String>,
    #[serde(with = "time::serde::iso8601")]
    pub timestamp: OffsetDateTime,
}

// ---------------------------------------------------------------------------
// System-prompt formatter
// ---------------------------------------------------------------------------

/// Render the Scope as a block of system-prompt text. Empty `Scope` returns
/// an empty string — callers can append unconditionally.
pub fn build_scope_prompt(scope: &Scope) -> String {
    if scope.tabs.is_empty()
        && scope.goal.is_none()
        && scope.notes.is_none()
        && scope.turn_log.is_empty()
    {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("\n\nSCOPE: ");
    if let Some(goal) = &scope.goal {
        out.push_str(goal);
    } else {
        out.push_str(&format!("{} tab(s)", scope.tabs.len()));
    }

    if let Some(notes) = &scope.notes {
        out.push_str("\nNotes: ");
        out.push_str(notes);
    }

    if !scope.tabs.is_empty() {
        out.push_str("\nTabs (the user has pinned these — you may call tools on any):");
        for (i, tab) in scope.tabs.iter().enumerate() {
            out.push_str(&format!(
                "\n  {}. @{} (session_id={} target_id={})",
                i + 1,
                if tab.label.is_empty() {
                    "unnamed"
                } else {
                    tab.label.as_str()
                },
                tab.session_id,
                tab.target_id.as_deref().unwrap_or("(first)"),
            ));
            if let Some(purpose) = &tab.purpose {
                out.push_str(&format!(" — purpose: {}", purpose));
            }
            if let Some(digest) = &tab.digest {
                out.push_str(&format!("\n     Last observed: {}", digest));
            }
        }
    }

    if !scope.turn_log.is_empty() {
        out.push_str("\nRecent turns (most recent last):");
        for entry in &scope.turn_log {
            out.push_str(&format!("\n  - [{}] {}", entry.user_goal, entry.summary));
        }
    }

    out.push_str(
        "\nINSTRUCTIONS:\n\
         - After any successful tool call on one of the Scope tabs, call the special \
         `_scope_digest` tool with { session_id, target_id, digest } where digest is ≤ 500 chars \
         summarising what you now know about that tab.\n\
         - Right before emitting `done`, call `_turn_summary` with { summary, touched_tab_ids } \
         listing the Scope tab ids you actually interacted with this turn.\n\
         - Do NOT call `_scope_digest` or `_turn_summary` for tabs outside the Scope.",
    );

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn sample_tab() -> ScopeTab {
        ScopeTab {
            session_id: "s-1".into(),
            target_id: Some("t-a".into()),
            label: "Notion".into(),
            purpose: Some("source".into()),
            digest: Some("47 rows".into()),
        }
    }

    #[test]
    fn scope_tab_id_format_with_target() {
        let tab = sample_tab();
        assert_eq!(tab.id(), "s-1-t-a");
    }

    #[test]
    fn scope_tab_id_format_without_target() {
        let tab = ScopeTab {
            session_id: "s-1".into(),
            target_id: None,
            label: "x".into(),
            purpose: None,
            digest: None,
        };
        assert_eq!(tab.id(), "s-1-first");
    }

    #[test]
    fn scope_roundtrip_matches_ios_shape() {
        let json = r#"{
            "tabs": [{"session_id": "s-1", "target_id": "t-a", "label": "Notion", "purpose": "source", "digest": "47 rows"}],
            "goal": "weekly review",
            "notes": "header row is 2",
            "turn_log": [{"user_goal": "g", "summary": "s", "touched_tab_ids": ["s-1-t-a"], "timestamp": "2026-04-14T10:00:00Z"}]
        }"#;
        let scope: Scope = serde_json::from_str(json).expect("decode");
        assert_eq!(scope.tabs.len(), 1);
        assert_eq!(scope.tabs[0].session_id, "s-1");
        assert_eq!(scope.goal.as_deref(), Some("weekly review"));
        assert_eq!(scope.turn_log.len(), 1);
        assert_eq!(scope.turn_log[0].touched_tab_ids, vec!["s-1-t-a"]);
        let reserialised = serde_json::to_string(&scope).unwrap();
        let back: Scope = serde_json::from_str(&reserialised).unwrap();
        assert_eq!(back, scope);
    }

    #[test]
    fn scope_allows_empty_fields() {
        let json = r#"{}"#;
        let scope: Scope = serde_json::from_str(json).expect("decode empty");
        assert!(scope.tabs.is_empty());
        assert!(scope.goal.is_none());
        assert!(scope.notes.is_none());
        assert!(scope.turn_log.is_empty());
    }

    #[test]
    fn scope_skips_nil_option_fields_on_serialise() {
        let scope = Scope::default();
        let json = serde_json::to_string(&scope).unwrap();
        assert!(!json.contains("\"goal\""));
        assert!(!json.contains("\"notes\""));
    }

    #[test]
    fn build_scope_prompt_empty_returns_empty_string() {
        assert_eq!(build_scope_prompt(&Scope::default()), "");
    }

    #[test]
    fn build_scope_prompt_with_tabs_includes_label_and_ids() {
        let scope = Scope {
            tabs: vec![sample_tab()],
            goal: Some("weekly review".into()),
            ..Default::default()
        };
        let p = build_scope_prompt(&scope);
        assert!(p.contains("SCOPE: weekly review"));
        assert!(p.contains("@Notion"));
        assert!(p.contains("session_id=s-1"));
        assert!(p.contains("target_id=t-a"));
        assert!(p.contains("purpose: source"));
        assert!(p.contains("Last observed: 47 rows"));
        assert!(p.contains("_scope_digest"));
        assert!(p.contains("_turn_summary"));
    }

    #[test]
    fn build_scope_prompt_notes_and_turn_log() {
        let scope = Scope {
            tabs: vec![sample_tab()],
            notes: Some("header is row 2".into()),
            turn_log: vec![TurnLogEntry {
                user_goal: "check budget".into(),
                summary: "pulled rows".into(),
                touched_tab_ids: vec!["s-1-t-a".into()],
                timestamp: datetime!(2026-04-14 10:00:00 UTC),
            }],
            ..Default::default()
        };
        let p = build_scope_prompt(&scope);
        assert!(p.contains("Notes: header is row 2"));
        assert!(p.contains("Recent turns"));
        assert!(p.contains("[check budget]"));
        assert!(p.contains("pulled rows"));
    }
}
