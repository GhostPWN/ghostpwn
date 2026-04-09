use anyhow::Result;
use futures_util::StreamExt;
use reqwest::Response;

pub async fn consume_sse(
    response: Response,
    mut on_data: impl FnMut(&str) -> Result<bool>,
) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        if buffer.contains('\r') {
            buffer = buffer.replace("\r\n", "\n").replace('\r', "\n");
        }

        while let Some(end_index) = buffer.find("\n\n") {
            let block = buffer[..end_index].to_string();
            buffer = buffer[end_index + 2..].to_string();

            if let Some(data) = extract_data_lines(&block)
                && !on_data(&data)?
            {
                return Ok(());
            }
        }
    }

    if !buffer.trim().is_empty()
        && let Some(data) = extract_data_lines(buffer.trim())
    {
        let _ = on_data(&data)?;
    }

    Ok(())
}

fn extract_data_lines(block: &str) -> Option<String> {
    let mut data_lines = Vec::<String>::new();

    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}
