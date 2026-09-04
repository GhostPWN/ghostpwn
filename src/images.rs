use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::models::{ConversationPart, ImageAttachment, ImageMediaType};
use crate::tools::ToolRuntime;

pub const MAX_IMAGES_PER_MESSAGE: usize = 10;
pub const MAX_IMAGE_BYTES_PER_MESSAGE: usize = 15 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum PromptPart {
    Text(String),
    ImagePath(String),
}

pub async fn prepare_parts(
    tools: &ToolRuntime,
    input: &str,
    clipboard_images: Vec<ImageAttachment>,
) -> Result<Vec<ConversationPart>> {
    let parsed = parse_image_references(input)?;
    let path_count = parsed
        .iter()
        .filter(|part| matches!(part, PromptPart::ImagePath(_)))
        .count();
    if path_count + clipboard_images.len() > MAX_IMAGES_PER_MESSAGE {
        return Err(anyhow!(
            "A message can contain at most {MAX_IMAGES_PER_MESSAGE} images"
        ));
    }

    let mut total_bytes = clipboard_images
        .iter()
        .try_fold(0_usize, |total, image| total.checked_add(image.data.len()))
        .ok_or_else(|| anyhow!("Image attachment size overflow"))?;
    if total_bytes > MAX_IMAGE_BYTES_PER_MESSAGE {
        return Err(image_size_error());
    }

    let mut parts = Vec::with_capacity(parsed.len() + clipboard_images.len());
    for part in parsed {
        match part {
            PromptPart::Text(text) => {
                if !text.is_empty() {
                    parts.push(ConversationPart::Text(text));
                }
            }
            PromptPart::ImagePath(path) => {
                let remaining = MAX_IMAGE_BYTES_PER_MESSAGE - total_bytes;
                let (relative_path, bytes) = tools.read_workspace_binary(&path, remaining).await?;
                let media_type = detect_image_type(&relative_path, &bytes)?;
                total_bytes = total_bytes
                    .checked_add(bytes.len())
                    .ok_or_else(|| anyhow!("Image attachment size overflow"))?;
                parts.push(ConversationPart::Image(ImageAttachment {
                    media_type,
                    data: Arc::from(bytes),
                    name: relative_path.to_string_lossy().into_owned(),
                }));
            }
        }
    }

    parts.extend(clipboard_images.into_iter().map(ConversationPart::Image));
    if parts.is_empty() {
        return Err(anyhow!("Message is empty"));
    }
    Ok(parts)
}

fn image_size_error() -> anyhow::Error {
    anyhow!(
        "Image attachments exceed the {} MiB per-message limit",
        MAX_IMAGE_BYTES_PER_MESSAGE / (1024 * 1024)
    )
}

fn parse_image_references(input: &str) -> Result<Vec<PromptPart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut index = 0;

    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with("\\@") {
            text.push('@');
            index += 2;
            continue;
        }

        let Some(ch) = rest.chars().next() else {
            break;
        };
        let at_boundary = index == 0
            || input[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if ch != '@' || !at_boundary {
            text.push(ch);
            index += ch.len_utf8();
            continue;
        }

        let after_at = index + 1;
        let (candidate, end, quoted) = if input[after_at..].starts_with('"') {
            let path_start = after_at + 1;
            let Some(close_offset) = input[path_start..].find('"') else {
                return Err(anyhow!("Unclosed quoted image path"));
            };
            let close = path_start + close_offset;
            (&input[path_start..close], close + 1, true)
        } else {
            let end_offset = input[after_at..]
                .find(char::is_whitespace)
                .unwrap_or(input.len() - after_at);
            let end = after_at + end_offset;
            (&input[after_at..end], end, false)
        };

        if supported_extension(candidate) {
            if !text.is_empty() {
                parts.push(PromptPart::Text(std::mem::take(&mut text)));
            }
            parts.push(PromptPart::ImagePath(candidate.to_string()));
            index = end;
            continue;
        }
        if quoted || known_unsupported_image_extension(candidate) {
            return Err(anyhow!(
                "Unsupported image '{}'; use PNG, JPEG, or WebP",
                candidate
            ));
        }

        text.push('@');
        index += 1;
    }

    if !text.is_empty() {
        parts.push(PromptPart::Text(text));
    }
    Ok(parts)
}

fn supported_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
}

fn known_unsupported_image_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "gif" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "avif"
            )
        })
}

fn detect_image_type(path: &Path, bytes: &[u8]) -> Result<ImageMediaType> {
    let detected = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageMediaType::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageMediaType::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageMediaType::Webp)
    } else {
        None
    };
    let expected = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => ImageMediaType::Png,
        Some("jpg" | "jpeg") => ImageMediaType::Jpeg,
        Some("webp") => ImageMediaType::Webp,
        _ => return Err(anyhow!("Unsupported image path '{}'", path.display())),
    };

    match detected {
        Some(actual) if actual == expected => Ok(actual),
        Some(actual) => Err(anyhow!(
            "Image '{}' has {} data but its extension declares {}",
            path.display(),
            actual.as_str(),
            expected.as_str()
        )),
        None => Err(anyhow!(
            "Image '{}' does not contain valid PNG, JPEG, or WebP data",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_ordered_and_quoted_image_references() {
        assert_eq!(
            parse_image_references("compare @one.png with @\"screens/two shot.jpg\"").unwrap(),
            vec![
                PromptPart::Text("compare ".to_string()),
                PromptPart::ImagePath("one.png".to_string()),
                PromptPart::Text(" with ".to_string()),
                PromptPart::ImagePath("screens/two shot.jpg".to_string()),
            ]
        );
    }

    #[test]
    fn preserves_escaped_at_and_mentions() {
        assert_eq!(
            parse_image_references(r"send to user@example.com or \@logo.png").unwrap(),
            vec![PromptPart::Text(
                "send to user@example.com or @logo.png".to_string()
            )]
        );
    }

    #[test]
    fn rejects_malformed_and_unsupported_references() {
        assert!(parse_image_references("@\"missing.png").is_err());
        assert!(parse_image_references("@capture.gif").is_err());
    }

    #[test]
    fn detects_supported_signatures_and_mismatches() {
        assert_eq!(
            detect_image_type(Path::new("shot.png"), b"\x89PNG\r\n\x1a\nrest").unwrap(),
            ImageMediaType::Png
        );
        assert!(detect_image_type(Path::new("shot.jpg"), b"\x89PNG\r\n\x1a\nrest").is_err());
    }

    #[tokio::test]
    async fn loads_workspace_images_and_preserves_part_order() {
        let workspace = tempdir().unwrap();
        fs::write(workspace.path().join("shot.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();

        let parts = prepare_parts(&tools, "inspect @shot.png now", Vec::new())
            .await
            .unwrap();

        assert!(matches!(&parts[0], ConversationPart::Text(text) if text == "inspect "));
        assert!(matches!(&parts[1], ConversationPart::Image(image) if image.name == "shot.png"));
        assert!(matches!(&parts[2], ConversationPart::Text(text) if text == " now"));
    }

    #[tokio::test]
    async fn rejects_paths_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_image = outside.path().join("outside.png");
        fs::write(&outside_image, b"\x89PNG\r\n\x1a\nrest").unwrap();
        let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();

        let error = prepare_parts(&tools, &format!("@{}", outside_image.display()), Vec::new())
            .await
            .unwrap_err();

        assert!(error.to_string().contains("outside workspace"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinked_images() {
        use std::os::unix::fs::symlink;

        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("outside.png");
        fs::write(&target, b"\x89PNG\r\n\x1a\nrest").unwrap();
        symlink(target, workspace.path().join("link.png")).unwrap();
        let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();

        assert!(
            prepare_parts(&tools, "@link.png", Vec::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn enforces_image_count_limit() {
        let workspace = tempdir().unwrap();
        let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();
        let input = (0..=MAX_IMAGES_PER_MESSAGE)
            .map(|index| format!("@{index}.png"))
            .collect::<Vec<_>>()
            .join(" ");

        let error = prepare_parts(&tools, &input, Vec::new()).await.unwrap_err();

        assert!(error.to_string().contains("at most 10 images"));
    }
}
