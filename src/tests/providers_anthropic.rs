use serde_json::json;

use super::parse_claude_models;

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
