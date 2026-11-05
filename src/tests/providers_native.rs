use serde_json::{Value, json};

use super::{
    AnthropicStream, ChatStream, GoogleStream, ResponsesStream, response_completion,
    response_messages,
};
use crate::models::{ConversationMessage, NativeContent, ToolResult};

fn call() -> Value {
    json!({"type": "function_call", "call_id": "call_1", "name": "readFile", "arguments": "{\"path\":\"file.rs\"}"})
}

#[test]
fn responses_replay_reasoning_calls_and_matching_results() {
    let items = vec![
        json!({"type": "reasoning", "id": "r1", "encrypted_content": "opaque", "summary": []}),
        call(),
    ];
    let completion = response_completion(&json!({"status": "completed", "output": items})).unwrap();
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].call.arguments["path"], "file.rs");
    let mut assistant = ConversationMessage::assistant("");
    assistant.tool_calls = completion.tool_calls;
    assistant.native_content = completion.native_content;
    let mut result = ConversationMessage::tool("");
    result.tool_result = Some(ToolResult {
        call_id: "call_1".to_string(),
        provider_id: Some("call_1".to_string()),
        name: "readFile".to_string(),
        output: json!({"content": "code"}),
        is_error: false,
    });
    assert_eq!(
        response_messages(&[assistant, result]),
        vec![
            items[0].clone(),
            call(),
            json!({"type": "function_call_output", "call_id": "call_1", "output": "{\"content\":\"code\"}"})
        ]
    );
}

#[test]
fn responses_execute_only_after_completed_event() {
    let mut stream = ResponsesStream::default();
    stream
        .ingest(
            &json!({"type": "response.output_item.done", "item": call()}),
            &mut |_| {},
        )
        .unwrap();
    assert!(stream.finish().is_err());
    let mut stream = ResponsesStream::default();
    let mut deltas = Vec::new();
    stream
        .ingest(
            &json!({"type": "response.output_text.delta", "delta": "Checking"}),
            &mut |text| deltas.push(text),
        )
        .unwrap();
    stream.ingest(&json!({"type": "response.completed", "response": {"status": "completed", "output": [call()]}}), &mut |text| deltas.push(text)).unwrap();
    assert_eq!(stream.finish().unwrap().tool_calls[0].id, "call_1");
    assert_eq!(deltas, ["Checking"]);
}

#[test]
fn rejects_incomplete_or_invalid_native_arguments() {
    for body in [
        json!({"status": "incomplete", "output": [call()]}),
        json!({"status": "completed", "output": [{"type":"function_call", "name":"readFile", "call_id":"x", "arguments":"{\"path\":"}]}),
        json!({"status": "completed", "output": [{"type":"function_call", "name":"readFile", "call_id":"x", "arguments":"[]"}]}),
    ] {
        assert!(response_completion(&body).is_err());
    }
    let mut stream = ResponsesStream::default();
    assert!(
        stream
            .ingest(&json!({"type": "response.incomplete"}), &mut |_| {})
            .is_err()
    );
}

#[test]
fn chat_reassembles_interleaved_tool_arguments_by_index() {
    let mut stream = ChatStream::default();
    for event in [
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"readFile","arguments":"{\"path\":"}},{"index":1,"id":"b","function":{"name":"listDirectory","arguments":"{"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"}"}},{"index":0,"function":{"arguments":"\"a.rs\"}"}}]}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
    ] {
        stream.ingest(&event, &mut |_| {}).unwrap();
    }
    let completion = stream.finish().unwrap();
    assert_eq!(
        completion
            .tool_calls
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(
        completion.tool_calls[0].call.arguments,
        json!({"path":"a.rs"})
    );
    assert_eq!(completion.tool_calls[1].call.arguments, json!({}));
    assert!(ChatStream::default().finish().is_err());
    let mut stream = ChatStream::default();
    assert!(
        stream
            .ingest(
                &json!({"choices":[{"finish_reason":"length"}]}),
                &mut |_| {}
            )
            .is_err()
    );
}

#[test]
fn anthropic_reassembles_arguments_and_preserves_thinking_signature() {
    let mut stream = AnthropicStream::default();
    for event in [
        json!({"type":"content_block_start", "index":0, "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"thinking_delta","thinking":"private"}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"signature_delta","signature":"signed"}}),
        json!({"type":"content_block_stop", "index":0}),
        json!({"type":"content_block_start", "index":1, "content_block":{"type":"tool_use","id":"a","name":"readFile","input":{}}}),
        json!({"type":"content_block_delta", "index":1, "delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
        json!({"type":"content_block_delta", "index":1, "delta":{"type":"input_json_delta","partial_json":"\"a.rs\"}"}}),
        json!({"type":"content_block_stop", "index":1}),
        json!({"type":"message_delta", "delta":{"stop_reason":"tool_use"}}),
        json!({"type":"message_stop"}),
    ] {
        stream
            .ingest(&event, &mut |_| {
                panic!("thinking leaked into assistant text")
            })
            .unwrap();
    }
    let completion = stream.finish().unwrap();
    assert_eq!(completion.tool_calls[0].call.arguments["path"], "a.rs");
    let Some(NativeContent::Anthropic(blocks)) = completion.native_content else {
        panic!("missing native blocks")
    };
    assert_eq!(
        blocks[0],
        json!({"type":"thinking", "thinking":"private", "signature":"signed"})
    );
    assert!(AnthropicStream::default().finish().is_err());
}

#[test]
fn google_keeps_signed_parts_and_parallel_calls_in_order() {
    let parts = json!([
        {"functionCall":{"id":"a","name":"readFile","args":{"path":"a.rs"}},"thoughtSignature":"opaque"},
        {"functionCall":{"id":"b","name":"listDirectory","args":{}}},
    ]);
    let mut stream = GoogleStream::default();
    stream
        .ingest(
            &json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}]}),
            &mut |_| {},
        )
        .unwrap();
    let completion = stream.finish().unwrap();
    assert_eq!(
        completion
            .tool_calls
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    let Some(NativeContent::Google(native)) = completion.native_content else {
        panic!("missing signed parts")
    };
    assert_eq!(json!(native), parts);
    assert!(GoogleStream::default().finish().is_err());
    let mut stream = GoogleStream::default();
    assert!(
        stream
            .ingest(
                &json!({"candidates":[{"finishReason":"MAX_TOKENS"}]}),
                &mut |_| {}
            )
            .is_err()
    );
}

#[tokio::test]
async fn responses_transport_handles_json_and_buffered_sse_without_content_type() {
    use std::io::{Read, Write};
    let final_response = json!({"status":"completed", "output":[call()]});
    for body in [
        final_response.to_string(),
        format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"type":"response.completed", "response":final_response})
        ),
    ] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let completion = super::read_responses(response, &mut |_| {}).await.unwrap();
        assert_eq!(completion.tool_calls[0].id, "call_1");
        server.join().unwrap();
    }
}

#[test]
fn responses_mapping_skips_empty_assistant_text_before_chat_tool_calls() {
    let mut assistant = ConversationMessage::assistant("");
    assistant.tool_calls = response_completion(&json!({"status":"completed", "output":[call()]}))
        .unwrap()
        .tool_calls;
    assert_eq!(response_messages(&[assistant]), vec![call()]);
}
