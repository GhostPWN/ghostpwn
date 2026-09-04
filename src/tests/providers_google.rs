use serde_json::json;

use std::sync::Arc;

use super::{ensure_inline_request_size, map_messages, parse_gemini_models};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType};

#[test]
fn keeps_only_generate_content_models() {
    let body = json!({
        "models": [
            {
                "name": "models/gemini-2.5-flash",
                "supportedGenerationMethods": ["generateContent", "countTokens"]
            },
            {
                "name": "models/gemini-embedding-001",
                "supportedGenerationMethods": ["embedContent"]
            },
            { "name": "models/text-bison" }
        ]
    });

    assert_eq!(parse_gemini_models(&body), vec!["gemini-2.5-flash"]);
}

#[test]
fn maps_image_inputs_to_inline_data() {
    let messages = vec![ConversationMessage::user_with_parts(vec![
        ConversationPart::Text("inspect".to_string()),
        ConversationPart::Image(ImageAttachment {
            media_type: ImageMediaType::Webp,
            data: Arc::from(*b"webp"),
            name: "shot.webp".to_string(),
        }),
    ])];
    let mapped = map_messages(&messages);

    assert_eq!(mapped[0]["parts"][0]["text"], "inspect");
    assert_eq!(
        mapped[0]["parts"][1]["inlineData"]["mimeType"],
        "image/webp"
    );
    assert_eq!(mapped[0]["parts"][1]["inlineData"]["data"], "d2VicA==");
}

#[test]
fn rejects_oversized_inline_request() {
    let payload = json!({ "data": "x".repeat(super::MAX_INLINE_REQUEST_BYTES) });
    assert!(ensure_inline_request_size(&payload).is_err());
}
