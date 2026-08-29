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
    pub content: String,
}

impl ConversationMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
        }
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
