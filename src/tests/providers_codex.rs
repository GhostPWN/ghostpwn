use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;

use super::{
    CodexCredentials, credentials_from_device_response, extract_account_id, extract_response_text,
    extract_response_text_from_body, extract_stream_delta, parse_credentials, persist_credentials,
    pkce_challenge, serialize_credentials,
};
use crate::secrets::SecretStore;

#[test]
fn pkce_challenge_is_s256_base64url() {
    assert_eq!(
        pkce_challenge("test-verifier"),
        "JBbiqONGWPaAmwXk_8bT6UnlPfrn65D32eZlJS-zGG0"
    );
}

#[test]
fn credential_expiry_saturates_instead_of_wrapping() {
    let credentials = credentials_from_device_response(
        "access".to_string(),
        Some("refresh".to_string()),
        Some(u64::MAX),
        None,
    )
    .expect("credentials");

    assert_eq!(credentials.expires_at, u64::MAX);
}

#[test]
fn refreshed_credentials_report_persistence_failure() {
    let dir = tempfile::tempdir().expect("temp dir");
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "not a directory").expect("blocker");
    let store = SecretStore::file_only(blocker.join("state.json"));
    let credentials = CodexCredentials {
        refresh_token: "refresh".to_string(),
        access_token: "access".to_string(),
        expires_at: u64::MAX,
        account_id: None,
    };

    let error = persist_credentials(&store, &credentials).expect_err("persistence must fail");

    assert!(
        error
            .to_string()
            .contains("failed to persist refreshed Codex credentials")
    );
}

#[test]
fn credential_json_round_trips() {
    let credentials = CodexCredentials {
        refresh_token: "refresh".to_string(),
        access_token: "access".to_string(),
        expires_at: 42,
        account_id: Some("account".to_string()),
    };

    let serialized = serialize_credentials(&credentials).unwrap();
    let parsed = parse_credentials(&serialized).unwrap();

    assert_eq!(parsed.refresh_token, "refresh");
    assert_eq!(parsed.access_token, "access");
    assert_eq!(parsed.expires_at, 42);
    assert_eq!(parsed.account_id.as_deref(), Some("account"));
}

#[test]
fn extracts_account_id_from_jwt_payload() {
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "https://api.openai.com/auth/account_id": "acct_123"
        }))
        .unwrap(),
    );
    let token = format!("header.{payload}.sig");

    assert_eq!(extract_account_id(&token).as_deref(), Some("acct_123"));
}

#[test]
fn parses_stream_text_delta_events() {
    let event = json!({
        "type": "response.output_text.delta",
        "delta": "hello"
    });

    assert_eq!(extract_stream_delta(&event).as_deref(), Some("hello"));
}

#[test]
fn parses_non_stream_response_text() {
    let body = json!({
        "output": [{
            "content": [
                { "text": "hello " },
                { "text": "world" }
            ]
        }]
    });

    assert_eq!(extract_response_text(&body).as_deref(), Some("hello world"));
}

#[test]
fn parses_buffered_sse_response_text() {
    let body = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "data: [DONE]\n\n",
    );

    assert_eq!(
        extract_response_text_from_body(body).unwrap(),
        "hello world".to_string()
    );
}

#[test]
fn parses_buffered_sse_final_response_text() {
    let body = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"output_text\":\"done\"}\n\n",
    );

    assert_eq!(
        extract_response_text_from_body(body).unwrap(),
        "done".to_string()
    );
}
