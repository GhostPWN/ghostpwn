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

#[test]
fn batches_function_responses_and_preserves_optional_ids_and_signatures() {
    use crate::models::{NativeContent, ToolResult};
    let parts = vec![
        json!({"functionCall":{"id":"a", "name":"readFile", "args":{"path":"a"}}, "thoughtSignature":"opaque"}),
        json!({"functionCall":{"name":"readFile", "args":{"path":"b"}}}),
    ];
    let mut assistant = ConversationMessage::assistant("");
    assistant.native_content = Some(NativeContent::Google(parts.clone()));
    let mut history = vec![assistant];
    for provider_id in [Some("a".to_string()), None] {
        let mut message = ConversationMessage::tool("");
        message.tool_result = Some(ToolResult {
            call_id: "internal".to_string(),
            provider_id,
            name: "readFile".to_string(),
            output: json!({"content":"code"}),
            is_error: false,
        });
        history.push(message);
    }
    let mapped = map_messages(&history);
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[0]["parts"], json!(parts));
    assert_eq!(mapped[1]["parts"].as_array().unwrap().len(), 2);
    assert_eq!(mapped[1]["parts"][0]["functionResponse"]["id"], "a");
    assert!(
        mapped[1]["parts"][1]["functionResponse"]
            .get("id")
            .is_none()
    );
}

#[test]
fn parameterless_tools_omit_empty_google_object_schema() {
    let declarations = super::tool_declarations();
    let list = declarations
        .iter()
        .find(|tool| tool["name"] == "listSkills")
        .unwrap();
    assert!(list.get("parameters").is_none());
    let multi = declarations
        .iter()
        .find(|tool| tool["name"] == "multiEdit")
        .unwrap();
    assert!(multi["parameters"].get("additionalProperties").is_none());
    assert!(
        multi["parameters"]["properties"]["edits"]["items"]
            .get("additionalProperties")
            .is_none()
    );
}
