//! Autonomous agent loop for Pagerunner.

pub mod autonomy;
pub mod budget;
pub mod events;

pub use autonomy::{AutonomyPolicy, ToolDecision};
pub use budget::{BudgetConfig, BudgetExceeded, BudgetTracker};
pub use events::{AgentEvent, AgentOutcome, AgentResult};
