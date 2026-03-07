use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;

use crate::stream::handler::StreamChunk;
use crate::Result;

use super::openai::OpenAiProvider;
use super::{ChatRequest, ChatResponse, Provider};

pub struct OpenAiCompatibleProvider {
    inner: OpenAiProvider,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            inner: OpenAiProvider::with_base_url(base_url, api_key, model),
        }
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        self.inner.chat(request).await
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        self.inner.chat_stream(request).await
    }
}
