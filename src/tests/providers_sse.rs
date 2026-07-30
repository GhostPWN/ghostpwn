use super::{Utf8StreamDecoder, extract_data_lines};

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
