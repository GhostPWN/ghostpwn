use super::{Utf8StreamDecoder, extract_data_lines, push_normalized_lines};

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
