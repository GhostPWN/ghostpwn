# Image input

GhostPWN can send PNG, JPEG, and WebP images to vision-capable models. Images can come from files inside the active workspace or from the system clipboard.

## Attach workspace files

Reference an image path inline with `@`. Relative paths resolve from the active workspace.

```text
Review @screenshots/login.png for security issues
Compare @before.png with @"screenshots/after login.webp"
```

Quoted paths support spaces. Unicode paths and multiple image references are supported, and text and images keep their original order. An image-only prompt is also valid.

Use `\@` when an image-like reference should remain literal:

```text
Document the literal path \@screenshots/login.png
```

GhostPWN rejects malformed references, missing files, directories, unsupported formats, extension and signature mismatches, and paths that escape the workspace. Absolute paths outside the workspace, `..` traversal, symlink escapes, and remote image URLs are not allowed.

## Paste from the clipboard

Press `Ctrl`+`V` to inspect the system clipboard. Bitmap data takes priority when the clipboard exposes both an image and text. GhostPWN encodes clipboard bitmap data as PNG and queues it for the next message. When no bitmap is available, normalized clipboard text is inserted into the prompt.

Some terminals intercept `Ctrl`+`V`. Use `/paste-image` when you need an image-only fallback. Use `/clear-images` to remove queued clipboard images.

The input footer shows the queued image count and total size. After submission, the transcript shows safe filenames or placeholders, never Base64 data.

## Limits and retention

- Up to 10 images and 15 MiB of image data per user message
- Up to 60 MiB of retained image data per conversation
- A 20 MB serialized inline-request ceiling for Google Gemini

Workspace images are read and sent without resizing, recompression, metadata stripping, or silent eviction. Near-limit Google histories can exceed its serialized request ceiling because Base64 and JSON add overhead. GhostPWN reports a clear error instead of dropping or resizing images.

Images remain attached while their user message remains in conversation history. They are resent during tool-loop iterations and follow-up prompts. `/clear` removes conversation history and queued clipboard images.

Validation errors appear in the transcript while the prompt and queued images remain available for correction.

## Providers and privacy

Image payloads use each provider's native request format. OpenAI, Anthropic, Google, Ollama, Codex, and GitHub Copilot are supported.

The selected provider and model must accept image input. Model catalogs do not expose reliable modality metadata, so GhostPWN cannot reject every text-only model before a request. If a provider rejects image input, choose a vision-capable model and submit again.

Every retained image is sent to the selected provider again when conversation history is included in a request. Only attach files you intend to share with that provider. Clipboard images are explicit attachments even though they do not originate inside the workspace.
