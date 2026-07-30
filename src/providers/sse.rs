use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use reqwest::Response;

pub async fn consume_sse(
    response: Response,
    mut on_data: impl FnMut(&str) -> Result<bool>,
) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut decoder = Utf8StreamDecoder::default();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let Some(text) = decoder.push(&chunk)? else {
            continue;
        };

        buffer.push_str(&text);

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

    decoder.finish()?;

    if !buffer.trim().is_empty()
        && let Some(data) = extract_data_lines(buffer.trim())
    {
        let _ = on_data(&data)?;
    }

    Ok(())
}

#[derive(Default)]
struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Option<String>> {
        self.pending.extend_from_slice(chunk);

        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let out = text.to_string();
                self.pending.clear();
                Ok(Some(out))
            }
            Err(err) if err.error_len().is_none() => Ok(None),
            Err(err) => Err(anyhow!(
                "SSE stream contained invalid UTF-8 at byte {}",
                err.valid_up_to()
            )),
        }
    }

    fn finish(&self) -> Result<()> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("SSE stream ended with incomplete UTF-8 sequence"))
        }
    }
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

#[cfg(test)]
#[path = "../tests/providers_sse.rs"]
mod tests;
