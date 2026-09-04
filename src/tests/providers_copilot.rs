use serde_json::json;

use std::sync::Arc;

use super::{map_messages, map_response_messages, parse_models_for_chat_completions};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType};

#[test]
fn keeps_models_even_when_only_responses_endpoint_is_present() {
    let body = json!({
        "data": [
            { "id": "gpt-4o", "supported_endpoints": ["/chat/completions"] },
            { "id": "gpt-5", "supported_endpoints": ["/responses"] }
        ]
    });

    let models = parse_models_for_chat_completions(&body);
    assert_eq!(models, vec!["gpt-4o", "gpt-5"]);
}

#[test]
fn maps_images_for_both_copilot_endpoints() {
    let messages = vec![ConversationMessage::user_with_parts(vec![
        ConversationPart::Text("inspect".to_string()),
        ConversationPart::Image(ImageAttachment {
            media_type: ImageMediaType::Png,
            data: Arc::from(*b"png"),
            name: "shot.png".to_string(),
        }),
    ])];

    let chat = map_messages("system", &messages);
    assert_eq!(
        chat[1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,cG5n"
    );

    let responses = map_response_messages("system", &messages);
    assert_eq!(responses[1]["content"][1]["type"], "input_image");
    assert_eq!(
        responses[1]["content"][1]["image_url"],
        "data:image/png;base64,cG5n"
    );
}

#[test]
fn hides_models_disabled_by_picker_policy_or_endpoint_capability() {
    let body = json!({
        "data": [
            {
                "id": "available",
                "model_picker_enabled": true,
                "policy": { "state": "enabled" },
                "supported_endpoints": ["/responses"]
            },
            {
                "id": "hidden",
                "model_picker_enabled": false,
                "supported_endpoints": ["/chat/completions"]
            },
            {
                "id": "blocked",
                "policy": { "state": "disabled" },
                "supported_endpoints": ["/chat/completions"]
            },
            {
                "id": "embedding-only",
                "type": "embedding",
                "supported_endpoints": ["/embeddings"]
            }
        ]
    });

    assert_eq!(parse_models_for_chat_completions(&body), vec!["available"]);
}

#[test]
fn falls_back_to_unfiltered_when_no_hints_exist() {
    let body = json!({
        "models": [
            { "id": "gpt-4o" },
            { "name": "gpt-5.3-codex" }
        ]
    });

    let models = parse_models_for_chat_completions(&body);
    assert_eq!(models, vec!["gpt-4o", "gpt-5.3-codex"]);
}

#[test]
fn ignores_router_entries_and_keeps_real_models() {
    let body = json!({
        "data": [
            { "id": "accounts/msft/routers" },
            { "id": "accounts/msft/models/gpt-4.1" },
            { "name": "gpt-4o" }
        ]
    });

    let models = parse_models_for_chat_completions(&body);
    assert_eq!(models, vec!["gpt-4.1", "gpt-4o"]);
}

#[test]
fn canonicalizes_official_display_names() {
    let body = json!({
        "models": [
            { "name": "GPT-5 mini" },
            { "name": "GPT-5.4 mini" },
            { "name": "Claude Opus 4.6 (fast mode) (preview)" },
            { "name": "Gemini 3.1 Pro" },
            { "name": "Raptor mini" }
        ]
    });

    let models = parse_models_for_chat_completions(&body);
    assert_eq!(
        models,
        vec![
            "gpt-5-mini",
            "gpt-5.4-mini",
            "claude-opus-4.6-fast-mode-preview",
            "gemini-3.1-pro",
            "raptor-mini"
        ]
    );
}
