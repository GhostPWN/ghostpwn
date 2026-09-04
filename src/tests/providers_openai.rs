use serde_json::json;

use std::sync::Arc;

use super::{extract_response_text, map_messages, parse_chat_models};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType};
use crate::providers::sse::extract_error_message;

#[test]
fn model_catalog_keeps_active_text_models_only() {
    let body = json!({
        "data": [
            { "id": "gpt-5.6-sol" },
            { "id": "o3" },
            { "id": "gpt-image-2" },
            { "id": "gpt-realtime-2" },
            { "id": "gpt-5.3-chat", "shutdown_date": 1_800_000_000 },
            { "id": "text-embedding-3-large" },
            { "id": "gpt-5.6-sol" }
        ]
    });

    assert_eq!(parse_chat_models(&body), vec!["gpt-5.6-sol", "o3"]);
}

#[test]
fn extracts_buffered_responses_text() {
    let body = json!({
        "output": [{
            "content": [
                { "type": "output_text", "text": "hello " },
                { "type": "output_text", "text": "world" }
            ]
        }]
    });

    assert_eq!(extract_response_text(&body).as_deref(), Some("hello world"));
}

#[test]
fn extracts_stream_error_events() {
    assert_eq!(
        extract_error_message(&json!({
            "type": "error",
            "error": { "message": "rate limited" }
        }))
        .as_deref(),
        Some("rate limited")
    );
    assert_eq!(
        extract_error_message(&json!({
            "type": "response.failed",
            "response": { "error": { "message": "model unavailable" } }
        }))
        .as_deref(),
        Some("model unavailable")
    );
    assert!(
        extract_error_message(&json!({
            "type": "response.output_text.delta",
            "delta": "ok"
        }))
        .is_none()
    );
}

#[test]
fn maps_image_inputs_for_responses_api() {
    let messages = vec![ConversationMessage::user_with_parts(vec![
        ConversationPart::Text("inspect".to_string()),
        ConversationPart::Image(ImageAttachment {
            media_type: ImageMediaType::Png,
            data: Arc::from(*b"png"),
            name: "shot.png".to_string(),
        }),
    ])];

    assert_eq!(
        map_messages(&messages),
        vec![json!({
            "role": "user",
            "content": [
                { "type": "input_text", "text": "inspect" },
                { "type": "input_image", "image_url": "data:image/png;base64,cG5n" }
            ]
        })]
    );
}
