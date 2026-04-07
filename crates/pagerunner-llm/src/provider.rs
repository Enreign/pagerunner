//! The `LlmProvider` trait — the core abstraction all backends implement.

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;

use crate::error::Result;
use crate::types::{CompletionRequest, CompletionResponse, StreamChunk};

/// A pinned, boxed, `Send` stream of [`StreamChunk`] values.
pub type BoxStream = Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send + 'static>>;

/// An LLM provider that can generate completions (blocking and streaming).
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    /// Generate a complete (non-streaming) response.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;

    /// Generate a streaming response.
    ///
    /// Returns a stream of [`StreamChunk`] values that ends with
    /// [`StreamChunk::Done`] when the model has finished generating.
    async fn complete_stream(&self, request: CompletionRequest) -> Result<BoxStream>;

    /// The provider's human-readable name (e.g. `"anthropic"`, `"openai"`).
    fn name(&self) -> &str;

    /// Whether this provider supports tool/function calling.
    fn supports_tools(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, Message, StopReason, Usage};
    use futures_util::StreamExt;
    use std::sync::Arc;

    // A minimal mock provider used only in tests.
    struct EchoProvider;

    #[async_trait]
    impl LlmProvider for EchoProvider {
        async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
            // Echo the last user message back as the assistant reply.
            let text = request
                .messages
                .last()
                .and_then(|m| {
                    m.content.iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();

            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: format!("echo: {text}"),
                }],
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
            })
        }

        async fn complete_stream(&self, request: CompletionRequest) -> Result<BoxStream> {
            let text = request
                .messages
                .last()
                .and_then(|m| {
                    m.content.iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();

            let chunks: Vec<Result<StreamChunk>> = vec![
                Ok(StreamChunk::TextDelta {
                    text: format!("echo: {text}"),
                }),
                Ok(StreamChunk::Usage(Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                })),
                Ok(StreamChunk::Done),
            ];

            Ok(Box::pin(futures_util::stream::iter(chunks)))
        }

        fn name(&self) -> &str {
            "echo"
        }

        fn supports_tools(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn provider_complete() {
        let p = EchoProvider;
        let req = crate::types::CompletionRequest::new(
            vec![Message::user("hello")],
            "test-model",
            64,
        );
        let resp = p.complete(req).await.unwrap();
        assert_eq!(resp.text(), "echo: hello");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[tokio::test]
    async fn provider_complete_stream() {
        let p = EchoProvider;
        let req = crate::types::CompletionRequest::new(
            vec![Message::user("stream me")],
            "test-model",
            64,
        );
        let mut stream = p.complete_stream(req).await.unwrap();

        let first = stream.next().await.unwrap().unwrap();
        assert!(
            matches!(first, StreamChunk::TextDelta { ref text } if text == "echo: stream me"),
            "unexpected first chunk: {first:?}"
        );

        let second = stream.next().await.unwrap().unwrap();
        assert!(matches!(second, StreamChunk::Usage(_)));

        let third = stream.next().await.unwrap().unwrap();
        assert_eq!(third, StreamChunk::Done);

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn provider_metadata() {
        let p = EchoProvider;
        assert_eq!(p.name(), "echo");
        assert!(!p.supports_tools());
    }

    #[tokio::test]
    async fn provider_behind_arc() {
        // Verify the trait object can live behind Arc.
        let p: Arc<dyn LlmProvider> = Arc::new(EchoProvider);
        let req = crate::types::CompletionRequest::new(
            vec![Message::user("arc test")],
            "test-model",
            64,
        );
        let resp = p.complete(req).await.unwrap();
        assert!(resp.text().contains("arc test"));
    }
}
