//! Autonomous agent loop for Pagerunner.

pub mod autonomy;
pub mod budget;
pub mod config;
pub mod events;
pub mod executor;

pub use autonomy::{AutonomyPolicy, ToolDecision};
pub use budget::{BudgetConfig, BudgetExceeded, BudgetTracker};
pub use config::AgentConfig;
pub use events::{AgentEvent, AgentOutcome, AgentResult};
pub use executor::{ToolExecutor, ToolResponse};
