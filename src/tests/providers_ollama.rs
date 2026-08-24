use serde_json::json;

use super::{ollama_base_url, supports_completion};

#[test]
fn normalizes_configured_ollama_host() {
    assert_eq!(
        ollama_base_url(Some("127.0.0.1:2345/")),
        "http://127.0.0.1:2345"
    );
    assert_eq!(
        ollama_base_url(Some("https://ollama.example/")),
        "https://ollama.example"
    );
}

#[test]
fn rejects_models_with_explicit_non_completion_capabilities() {
    assert!(supports_completion(
        &json!({ "capabilities": ["completion", "tools"] })
    ));
    assert!(!supports_completion(
        &json!({ "capabilities": ["embedding"] })
    ));
    assert!(supports_completion(&json!({})));
}
