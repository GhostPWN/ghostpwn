use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: Vec<ConversationPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationPart {
    Text(String),
    Image(ImageAttachment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAttachment {
    pub media_type: ImageMediaType,
    pub data: Arc<[u8]>,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMediaType {
    Png,
    Jpeg,
    Webp,
}

impl ImageMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

impl ConversationMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self::with_parts(
            MessageRole::User,
            vec![ConversationPart::Text(content.into())],
        )
    }

    pub fn user_with_parts(content: Vec<ConversationPart>) -> Self {
        Self::with_parts(MessageRole::User, content)
    }

    fn with_parts(role: MessageRole, content: Vec<ConversationPart>) -> Self {
        Self { role, content }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::with_parts(
            MessageRole::Assistant,
            vec![ConversationPart::Text(content.into())],
        )
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self::with_parts(
            MessageRole::Tool,
            vec![ConversationPart::Text(content.into())],
        )
    }

    pub fn has_images(&self) -> bool {
        self.content
            .iter()
            .any(|part| matches!(part, ConversationPart::Image(_)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelEnvelope {
    #[serde(default)]
    pub assistant: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug)]
pub enum AgentEvent {
    Selector {
        id: u64,
        event: Box<AgentEvent>,
    },
    AssistantDelta(String),
    ApprovalRequired {
        name: String,
        args_summary: String,
        response: tokio::sync::oneshot::Sender<bool>,
    },
    ToolCall {
        name: String,
        args_summary: String,
    },
    ModelList {
        provider: crate::config::ProviderKind,
        models: Vec<String>,
        error: Option<String>,
    },
    ProviderStatus {
        provider: crate::config::ProviderKind,
        message: String,
        error: bool,
    },
    ProviderName(String),
    Error(String),
    Done,
}
