use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct TurnCancellation(watch::Sender<bool>);

impl Default for TurnCancellation {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}

impl TurnCancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

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
    pub tool_calls: Vec<ToolRequest>,
    pub tool_result: Option<ToolResult>,
    pub native_content: Option<NativeContent>,
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
        Self {
            role,
            content,
            tool_calls: Vec::new(),
            tool_result: None,
            native_content: None,
        }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub id: String,
    // Google can omit a wire ID. The local ID still links each call and result.
    pub provider_id: Option<String>,
    pub call: ToolCall,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub provider_id: Option<String>,
    pub name: String,
    pub output: Value,
    pub is_error: bool,
}

// Preserve provider-owned reasoning and signature fields when replaying assistant turns.
#[derive(Debug, Clone)]
pub enum NativeContent {
    Responses(Vec<Value>),
    Anthropic(Vec<Value>),
    Google(Vec<Value>),
}

#[derive(Debug, Default)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolRequest>,
    pub native_content: Option<NativeContent>,
}

impl Completion {
    pub fn text(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
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
        /// Why the execution policy asks for approval, shown apart from the arguments.
        reason: &'static str,
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
    Cancelled,
    Done,
}
