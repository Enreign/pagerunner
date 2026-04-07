//! Budget tracking — enforces per-run token and step limits.

use serde::{Deserialize, Serialize};

use pagerunner_llm::Usage;

// ---------------------------------------------------------------------------
// BudgetConfig
// ---------------------------------------------------------------------------

fn default_max_steps() -> u32 {
    50
}

fn default_max_tokens_per_step() -> u32 {
    4096
}

fn default_total_token_budget() -> u64 {
    0 // 0 = unlimited
}

/// Limits applied to an agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Maximum number of LLM completion steps.  Default: 50.
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,

    /// Maximum tokens a single step may consume.  Default: 4096.
    #[serde(default = "default_max_tokens_per_step")]
    pub max_tokens_per_step: u32,

    /// Total cumulative token budget for the run.  0 = unlimited.  Default: 0.
    #[serde(default = "default_total_token_budget")]
    pub total_token_budget: u64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
            max_tokens_per_step: default_max_tokens_per_step(),
            total_token_budget: default_total_token_budget(),
        }
    }
}

// ---------------------------------------------------------------------------
// BudgetExceeded
// ---------------------------------------------------------------------------

/// Reason the budget was exceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetExceeded {
    /// Step count reached or exceeded the limit.
    MaxSteps { limit: u32, current: u32 },
    /// Total token usage reached or exceeded the budget.
    TokenBudget { limit: u64, used: u64 },
}

// ---------------------------------------------------------------------------
// BudgetTracker
// ---------------------------------------------------------------------------

/// Tracks resource usage accumulated over an agent run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BudgetTracker {
    /// Number of LLM completion steps taken so far.
    pub steps: u32,
    /// Total input tokens consumed.
    pub total_input_tokens: u64,
    /// Total output tokens consumed.
    pub total_output_tokens: u64,
}

impl BudgetTracker {
    /// Create a new tracker with all counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate token usage from a single step.
    pub fn record_usage(&mut self, usage: &Usage) {
        self.total_input_tokens += usage.input_tokens;
        self.total_output_tokens += usage.output_tokens;
    }

    /// Increment the step counter by one.
    pub fn record_step(&mut self) {
        self.steps += 1;
    }

    /// Total tokens consumed (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Check whether any budget limit has been exceeded.
    ///
    /// Returns `Some(BudgetExceeded)` describing the first violation found,
    /// or `None` if all limits are within bounds.
    ///
    /// Checks (in order): max_steps, then total_token_budget.
    pub fn check(&self, config: &BudgetConfig) -> Option<BudgetExceeded> {
        if self.steps >= config.max_steps {
            return Some(BudgetExceeded::MaxSteps {
                limit: config.max_steps,
                current: self.steps,
            });
        }
        if config.total_token_budget > 0 && self.total_tokens() >= config.total_token_budget {
            return Some(BudgetExceeded::TokenBudget {
                limit: config.total_token_budget,
                used: self.total_tokens(),
            });
        }
        None
    }

    /// Return a snapshot of cumulative usage.
    pub fn usage(&self) -> Usage {
        Usage {
            input_tokens: self.total_input_tokens,
            output_tokens: self.total_output_tokens,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Defaults ---

    #[test]
    fn budget_config_defaults() {
        let cfg = BudgetConfig::default();
        assert_eq!(cfg.max_steps, 50);
        assert_eq!(cfg.max_tokens_per_step, 4096);
        assert_eq!(cfg.total_token_budget, 0);
    }

    #[test]
    fn budget_tracker_starts_at_zero() {
        let tracker = BudgetTracker::new();
        assert_eq!(tracker.steps, 0);
        assert_eq!(tracker.total_input_tokens, 0);
        assert_eq!(tracker.total_output_tokens, 0);
        assert_eq!(tracker.total_tokens(), 0);
    }

    // --- Accumulation ---

    #[test]
    fn record_usage_accumulates_tokens() {
        let mut tracker = BudgetTracker::new();
        tracker.record_usage(&Usage {
            input_tokens: 100,
            output_tokens: 50,
        });
        tracker.record_usage(&Usage {
            input_tokens: 200,
            output_tokens: 75,
        });
        assert_eq!(tracker.total_input_tokens, 300);
        assert_eq!(tracker.total_output_tokens, 125);
        assert_eq!(tracker.total_tokens(), 425);
    }

    #[test]
    fn usage_snapshot_is_correct() {
        let mut tracker = BudgetTracker::new();
        tracker.record_usage(&Usage {
            input_tokens: 10,
            output_tokens: 20,
        });
        let u = tracker.usage();
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 20);
    }

    // --- Step counting ---

    #[test]
    fn record_step_increments_counter() {
        let mut tracker = BudgetTracker::new();
        tracker.record_step();
        tracker.record_step();
        tracker.record_step();
        assert_eq!(tracker.steps, 3);
    }

    // --- Max steps exceeded ---

    #[test]
    fn max_steps_exceeded_when_at_limit() {
        let cfg = BudgetConfig {
            max_steps: 3,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new();
        tracker.record_step();
        tracker.record_step();
        assert!(tracker.check(&cfg).is_none());
        tracker.record_step();
        match tracker.check(&cfg) {
            Some(BudgetExceeded::MaxSteps { limit, current }) => {
                assert_eq!(limit, 3);
                assert_eq!(current, 3);
            }
            other => panic!("expected MaxSteps, got {other:?}"),
        }
    }

    #[test]
    fn max_steps_exceeded_beyond_limit() {
        let cfg = BudgetConfig {
            max_steps: 2,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new();
        for _ in 0..5 {
            tracker.record_step();
        }
        assert!(matches!(
            tracker.check(&cfg),
            Some(BudgetExceeded::MaxSteps { .. })
        ));
    }

    // --- Token budget exceeded ---

    #[test]
    fn token_budget_exceeded_when_at_limit() {
        let cfg = BudgetConfig {
            total_token_budget: 500,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new();
        tracker.record_usage(&Usage {
            input_tokens: 300,
            output_tokens: 199,
        });
        assert!(tracker.check(&cfg).is_none());
        tracker.record_usage(&Usage {
            input_tokens: 1,
            output_tokens: 0,
        });
        match tracker.check(&cfg) {
            Some(BudgetExceeded::TokenBudget { limit, used }) => {
                assert_eq!(limit, 500);
                assert_eq!(used, 500);
            }
            other => panic!("expected TokenBudget, got {other:?}"),
        }
    }

    // --- Unlimited budget (total_token_budget = 0) ---

    #[test]
    fn unlimited_budget_never_exceeds_token_limit() {
        let cfg = BudgetConfig {
            max_steps: 1000,
            total_token_budget: 0, // unlimited
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new();
        tracker.record_usage(&Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
        });
        // Should not report token budget exceeded.
        assert!(tracker.check(&cfg).is_none());
    }

    // --- TOML parsing ---

    #[test]
    fn budget_config_toml_defaults() {
        let cfg: BudgetConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.max_steps, 50);
        assert_eq!(cfg.max_tokens_per_step, 4096);
        assert_eq!(cfg.total_token_budget, 0);
    }

    #[test]
    fn budget_config_toml_partial_override() {
        let toml = r#"
            max_steps = 10
            total_token_budget = 20000
        "#;
        let cfg: BudgetConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.max_steps, 10);
        assert_eq!(cfg.max_tokens_per_step, 4096); // default preserved
        assert_eq!(cfg.total_token_budget, 20000);
    }

    // --- Serde roundtrip ---

    #[test]
    fn budget_config_serde_roundtrip() {
        let original = BudgetConfig {
            max_steps: 100,
            max_tokens_per_step: 8192,
            total_token_budget: 50000,
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: BudgetConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }
}
