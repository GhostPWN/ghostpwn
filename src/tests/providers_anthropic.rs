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
