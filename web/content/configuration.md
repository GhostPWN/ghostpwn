# Configuration

## Environment variables

- `GHOSTPWN_PROVIDER`: optional startup provider override (`anthropic` | `openai` | `google` | `copilot` | `codex` | `ollama`)
- `GHOSTPWN_MODEL`: optional startup model override for the selected provider
- `GHOSTPWN_WORKSPACE`: optional root path used as the filesystem-tool boundary and command working directory
- `GHOSTPWN_SKILLS_DIR`: optional directory for local skills; defaults to `skills`
- `GHOSTPWN_SKILLS_CACHE_DIR`: optional parent directory for extracted bundled skills
- `GHOSTPWN_STATE_FILE`: optional local state-file override
- `OLLAMA_HOST`: optional Ollama server URL or host; defaults to `http://localhost:11434`

## Provider keys

- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`
- `GOOGLE_GENERATIVE_AI_API_KEY`
- `GITHUB_COPILOT_TOKEN`
- `CODEX_OAUTH_TOKEN` (serialized Codex OAuth credentials; normally managed by `/model`)

API keys are loaded from environment first, then OS keychain entries under service `ghostpwn-rust`, then the local state-file fallback.

## State and persistence

- Local state fallback defaults to `%APPDATA%\ghostpwn\state.json` on Windows, `$XDG_CONFIG_HOME/ghostpwn/state.json` on Unix/macOS, then the user's home config path
- The latest provider/model selection is stored as `latest_provider` and `latest_model` in the OS keychain and local state file
- Ollama does not require an API key
