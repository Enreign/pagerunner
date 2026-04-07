//! LLM provider abstraction for Pagerunner agent system.

pub mod anthropic;
pub mod error;
pub mod factory;
pub mod ollama;
pub mod openai_compat;
pub mod provider;
pub mod types;

// Re-export the most commonly used items at the crate root.
pub use error::{LlmError, Result};
pub use factory::{create_default_provider, create_provider, AgentLlmConfig, ProviderConfig};
pub use provider::{BoxStream, LlmProvider};
pub use types::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, Role, StopReason, StreamChunk,
    ToolSchema, Usage,
};
