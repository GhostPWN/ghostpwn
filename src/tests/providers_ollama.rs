use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::{MODEL_PROBE_CONCURRENCY, OllamaProvider, ollama_base_url, supports_completion};
use crate::providers::Provider;

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

#[tokio::test]
async fn model_probes_are_bounded_and_preserve_discovery_order() {
    let model_count = MODEL_PROBE_CONCURRENCY + 4;
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("listener address");
    let models = (0..model_count)
        .map(|index| json!({ "name": format!("model-{index}") }))
        .collect::<Vec<_>>();
    let response_body = Arc::new(
        serde_json::to_string(&json!({ "models": models })).expect("models response json"),
    );
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let server_active = Arc::clone(&active);
    let server_peak = Arc::clone(&peak);
    let server = thread::spawn(move || {
        let mut handlers = Vec::with_capacity(model_count + 1);
        for _ in 0..=model_count {
            let (stream, _) = listener.accept().expect("request");
            let active = Arc::clone(&server_active);
            let peak = Arc::clone(&server_peak);
            let response_body = Arc::clone(&response_body);
            handlers.push(thread::spawn(move || {
                handle_request(stream, active, peak, &response_body)
            }));
        }
        for handler in handlers {
            handler.join().expect("request handler");
        }
    });

    let provider = OllamaProvider {
        model: "model-0".to_string(),
        base_url: format!("http://{address}"),
        client: crate::providers::provider_http_client(),
    };
    let discovered = provider.list_models().await.expect("models");
    server.join().expect("server");

    assert_eq!(
        discovered,
        (0..model_count)
            .map(|index| format!("model-{index}"))
            .collect::<Vec<_>>()
    );
    assert!(peak.load(Ordering::SeqCst) <= MODEL_PROBE_CONCURRENCY);
    assert!(peak.load(Ordering::SeqCst) > 1);
}

fn handle_request(
    mut stream: TcpStream,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    models_response: &str,
) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).expect("read request");
        assert!(read > 0, "request ended before headers");
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let is_tags = request.starts_with("GET /api/tags ");
    let body = if is_tags {
        models_response
    } else {
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(current, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(50));
        active.fetch_sub(1, Ordering::SeqCst);
        r#"{"capabilities":["completion"]}"#
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write response");
}
