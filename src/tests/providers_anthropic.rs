use serde_json::json;

use std::sync::Arc;

use super::{map_messages, parse_claude_models};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType};

#[test]
fn parses_catalog_page_in_provider_order() {
    let body = json!({
        "data": [
            { "id": "claude-sonnet-4-6" },
            { "id": "not-a-claude-model" },
            { "id": "claude-haiku-4-5" }
        ],
        "has_more": true,
        "last_id": "claude-haiku-4-5"
    });

    assert_eq!(
        parse_claude_models(&body),
        vec!["claude-sonnet-4-6", "claude-haiku-4-5"]
    );
}

#[test]
fn maps_image_inputs_to_anthropic_blocks() {
    let messages = vec![ConversationMessage::user_with_parts(vec![
        ConversationPart::Image(ImageAttachment {
            media_type: ImageMediaType::Jpeg,
            data: Arc::from(*b"jpg"),
            name: "shot.jpg".to_string(),
        }),
        ConversationPart::Text("inspect".to_string()),
    ])];
    let mapped = map_messages(&messages);

    assert_eq!(mapped[0]["content"][0]["type"], "image");
    assert_eq!(
        mapped[0]["content"][0]["source"]["media_type"],
        "image/jpeg"
    );
    assert_eq!(mapped[0]["content"][0]["source"]["data"], "anBn");
    assert_eq!(mapped[0]["content"][1]["text"], "inspect");
}

#[test]
fn batches_results_after_native_tool_use_and_preserves_signatures() {
    use crate::models::{NativeContent, ToolResult};
    let blocks = vec![
        json!({"type":"thinking", "thinking":"reason", "signature":"opaque"}),
        json!({"type":"tool_use", "id":"a", "name":"readFile", "input":{"path":"a"}}),
        json!({"type":"tool_use", "id":"b", "name":"readFile", "input":{"path":"b"}}),
    ];
    let mut assistant = ConversationMessage::assistant("");
    assistant.native_content = Some(NativeContent::Anthropic(blocks.clone()));
    let mut history = vec![assistant];
    for id in ["a", "b"] {
        let mut message = ConversationMessage::tool("");
        message.tool_result = Some(ToolResult {
            call_id: id.to_string(),
            provider_id: Some(id.to_string()),
            name: "readFile".to_string(),
            output: json!({"error":"denied"}),
            is_error: true,
        });
        history.push(message);
    }
    let mapped = map_messages(&history);
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[0]["content"], json!(blocks));
    assert_eq!(mapped[1]["content"].as_array().unwrap().len(), 2);
    assert_eq!(mapped[1]["content"][0]["tool_use_id"], "a");
    assert_eq!(mapped[1]["content"][1]["tool_use_id"], "b");
    assert_eq!(mapped[1]["content"][1]["is_error"], true);
}
