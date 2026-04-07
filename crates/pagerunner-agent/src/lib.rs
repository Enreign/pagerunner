//! Autonomous agent loop for Pagerunner.

pub mod autonomy;
pub mod events;

pub use autonomy::{AutonomyPolicy, ToolDecision};
pub use events::{AgentEvent, AgentOutcome, AgentResult};
