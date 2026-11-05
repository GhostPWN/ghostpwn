use std::sync::Arc;

use serde_json::json;

use super::{build_provider_with_secret_store, dedup_preserve_order, map_chat_messages};
use crate::config::{ProviderKeys, ProviderKind};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType};
use crate::secrets::SecretStore;

#[test]
fn chat_messages_preserve_roles_text_and_image_order() {
    let history = vec![
        ConversationMessage::user_with_parts(vec![
            ConversationPart::Text("first".to_string()),
            ConversationPart::Text(" second".to_string()),
        ]),
        ConversationMessage::assistant("answer"),
        ConversationMessage::tool("result"),
        ConversationMessage::user_with_parts(vec![
            ConversationPart::Text("before".to_string()),
            ConversationPart::Image(ImageAttachment {
                media_type: ImageMediaType::Png,
                data: Arc::from(*b"png"),
                name: "shot.png".to_string(),
            }),
            ConversationPart::Text("after".to_string()),
        ]),
    ];

    assert_eq!(
        map_chat_messages("system", &history),
        vec![
            json!({ "role": "system", "content": "system" }),
            json!({ "role": "user", "content": "first second" }),
            json!({ "role": "assistant", "content": "answer" }),
            json!({ "role": "user", "content": "[tool] result" }),
            json!({ "role": "user", "content": [
                { "type": "text", "text": "before" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,cG5n" } },
                { "type": "text", "text": "after" },
            ] }),
        ]
    );
}

#[test]
fn model_deduplication_keeps_first_seen_order() {
    let mut models = ["second", "first", "second", "third", "first"]
        .map(String::from)
        .to_vec();
    dedup_preserve_order(&mut models);
    assert_eq!(models, ["second", "first", "third"]);
}

#[tokio::test]
async fn builder_preserves_connected_and_disconnected_providers() {
    let directory = tempfile::tempdir().unwrap();
    let store = SecretStore::file_only(directory.path().join("state.json"));
    for kind in ProviderKind::all().iter().copied() {
        let mut keys = ProviderKeys::default();
        let provider =
            build_provider_with_secret_store(kind, "model".to_string(), &keys, store.clone());
        let name = format!("{} / model", kind.as_str());
        if kind == ProviderKind::Ollama {
            assert_eq!(provider.display_name(), name);
            continue;
        }

        assert_eq!(provider.display_name(), format!("{name} (disconnected)"));
        assert!(provider.list_models().await.unwrap().is_empty());
        let mut deltas = Vec::new();
        let error = provider
            .stream_complete("system", &[], &mut |delta| deltas.push(delta))
            .await
            .unwrap_err();
        assert!(deltas.is_empty());
        assert_eq!(
            error.to_string(),
            format!(
                "No API key connected for {}. open /model and press c on the {} tab",
                kind.as_str(),
                kind.as_str()
            )
        );

        keys.set(kind, "credential".to_string());
        let provider =
            build_provider_with_secret_store(kind, "model".to_string(), &keys, store.clone());
        assert_eq!(provider.display_name(), name);
    }
}

#[test]
fn chat_and_legacy_mappings_keep_call_result_pairs() {
    use crate::models::{ToolCall, ToolRequest, ToolResult};
    let mut assistant = ConversationMessage::assistant("Checking");
    assistant.tool_calls.push(ToolRequest {
        id: "a".to_string(),
        provider_id: Some("a".to_string()),
        call: ToolCall {
            name: "readFile".to_string(),
            arguments: json!({"path":"a.rs"}),
        },
    });
    let mut result = ConversationMessage::tool("");
    result.tool_result = Some(ToolResult {
        call_id: "a".to_string(),
        provider_id: Some("a".to_string()),
        name: "readFile".to_string(),
        output: json!({"content":"code"}),
        is_error: false,
    });
    let history = [assistant, result];
    let mapped = map_chat_messages("system", &history);
    assert_eq!(mapped[1]["tool_calls"][0]["id"], "a");
    assert_eq!(
        mapped[1]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"a.rs\"}"
    );
    assert_eq!(mapped[2]["role"], "tool");
    assert_eq!(mapped[2]["tool_call_id"], "a");
    let legacy = super::map_legacy_chat_messages("system", &history);
    assert!(legacy[1].get("tool_calls").is_none());
    let envelope: serde_json::Value =
        serde_json::from_str(legacy[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["tool_calls"][0]["name"], "readFile");
    assert!(
        legacy[2]["content"]
            .as_str()
            .unwrap()
            .starts_with("[tool] tool_result readFile:")
    );
}
