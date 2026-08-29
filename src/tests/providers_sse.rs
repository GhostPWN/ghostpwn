use serde_json::json;

use super::{Utf8StreamDecoder, extract_data_lines, extract_error_message, push_normalized_lines};

#[test]
fn extracts_provider_stream_error_shapes() {
    assert_eq!(
        extract_error_message(&json!({
            "type": "error",
            "error": { "type": "overloaded_error", "message": "overloaded" }
        }))
        .as_deref(),
        Some("overloaded")
    );
    assert_eq!(
        extract_error_message(&json!({
            "error": { "code": 429, "message": "quota exceeded", "status": "RESOURCE_EXHAUSTED" }
        }))
        .as_deref(),
        Some("quota exceeded")
    );
    assert_eq!(
        extract_error_message(&json!({ "error": "stream failed" })).as_deref(),
        Some("stream failed")
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
            "delta": "ok",
            "error": { "message": "not an error event" }
        }))
        .is_none()
    );
    assert!(extract_error_message(&json!({ "error": null })).is_none());
    assert!(extract_error_message(&json!({ "response": { "error": null } })).is_none());
}

#[test]
fn utf8_decoder_waits_for_split_codepoint() {
    let message = "data: 🌋\n\n";
    let bytes = message.as_bytes();
    let split = message.find('🌋').expect("emoji offset") + 2;

    let mut decoder = Utf8StreamDecoder::default();
    assert!(
        decoder
            .push(&bytes[..split])
            .expect("first chunk")
            .is_none()
    );
    assert_eq!(
        decoder.push(&bytes[split..]).expect("second chunk"),
        Some(message.to_string())
    );
}

#[test]
fn data_lines_are_joined_without_event_metadata() {
    let block = "event: message\ndata: {\"a\":1}\ndata: {\"b\":2}";
    assert_eq!(
        extract_data_lines(block).as_deref(),
        Some("{\"a\":1}\n{\"b\":2}")
    );
}

#[test]
fn split_crlf_is_normalized_as_one_line_ending() {
    let mut buffer = String::new();
    let mut pending_cr = false;

    push_normalized_lines(&mut buffer, "data: one\r", &mut pending_cr);
    push_normalized_lines(&mut buffer, "\n\r\ndata: two\r\n\r\n", &mut pending_cr);

    assert_eq!(buffer, "data: one\n\ndata: two\n\n");
    assert!(!pending_cr);
}
