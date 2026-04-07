//! Autonomous agent loop for Pagerunner.

pub mod agent_loop;
pub mod autonomy;
pub mod budget;
pub mod config;
pub mod context;
pub mod events;
pub mod executor;

pub use agent_loop::{build_system_prompt, extract_text, run_agent};
pub use autonomy::{AutonomyPolicy, ToolDecision};
pub use budget::{BudgetConfig, BudgetExceeded, BudgetTracker};
pub use config::AgentConfig;
pub use context::ContextConfig;
pub use events::{AgentEvent, AgentOutcome, AgentResult};
pub use executor::{ToolExecutor, ToolResponse};
