# AGENTS.md

This file is guidance for coding agents working in `ghostpwn`.
It captures build/test commands, project conventions, and coding style.

## Repository Scope

- Primary target: `ghostpwn/` (Rust TUI application).
- Runtime: Rust (edition 2024), async with Tokio.
- UI: `ratatui` + `crossterm`.
- Providers: OpenAI, Anthropic, Google (streaming).
- Tool runtime: local FS + command tools with workspace boundaries.
- Secrets: keychain persistence support.

## External Agent Rules Check

- Checked for Cursor rules in `.cursor/rules/`: not found.
- Checked for `.cursorrules`: not found.
- Checked for Copilot rules in `.github/copilot-instructions.md`: not found.
- If these files are added later, treat them as higher-priority local policy.

## Working Directory and Invocation

- Preferred working directory: `ghostpwn/`.
- If running from parent directory, use:
- `cargo <cmd> --manifest-path ghostpwn/Cargo.toml`.

## Build, Lint, and Test Commands

- Fast compile check:
- `cargo check`
- Build debug binary:
- `cargo build`
- Build release binary:
- `cargo build --release`
- Run the TUI app:
- `cargo run`
- Format code:
- `cargo fmt`
- Verify formatting only:
- `cargo fmt -- --check`
- Lint with strict warnings:
- `cargo clippy -- -D warnings`
- Run full test suite:
- `cargo test`

## Single-Test Commands (Important)

- Run one test by fully-qualified name:
- `cargo test agent::tests::parse_envelope_reads_json_block`
- Run one test exactly (avoid substring matches):
- `cargo test agent::tests::parse_envelope_reads_json_block -- --exact`
- Run one test with output visible:
- `cargo test agent::tests::parse_envelope_reads_json_block -- --exact --nocapture`
- Run all tests in one module namespace:
- `cargo test agent::tests::`
- Run one tools test:
- `cargo test tools::tests::rejects_paths_outside_workspace -- --exact`
- Run tests serially (if debugging race/state issues):
- `cargo test -- --test-threads=1`

## Recommended Validation Pipeline Before Handoff

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`
- For behavior/UI-affecting changes, also do:
- `cargo run`

## High-Level Architecture Map

- `src/main.rs`: bootstrap, config load, dependency wiring.
- `src/config.rs`: provider/model/env parsing and key loading.
- `src/secrets.rs`: keychain persistence helpers.
- `src/agent.rs`: orchestration loop, command/model switching, tool loop.
- `src/providers/`: provider adapters + SSE consumption.
- `src/tools/mod.rs`: local tools (`readFile`, `grep`, `runCommand`, etc.).
- `src/ui/mod.rs`: TUI event loop, input handling, rendering, commands.
- `src/ui/logo.rs`: home-screen logo rendering.
- `src/models.rs`: shared enums/structs/events.

## Rust Style and Formatting

- Always format with `cargo fmt` (no manual formatting preferences).
- Keep functions focused and composable.
- Prefer early returns over deeply nested branching.
- Prefer `match` for enum-driven control flow.
- Keep rendering functions pure where practical.
- Keep side effects in orchestration layers (agent/ui handlers).

## Imports and Module Conventions

- Group imports in this order:
- std
- third-party crates
- `crate::...`
- Avoid wildcard imports (`use x::*`) unless strongly justified.
- Keep module filenames snake_case.
- Keep exported types and traits in PascalCase.
- Keep constants in UPPER_SNAKE_CASE.

## Naming Conventions

- Types/traits: `PascalCase` (e.g., `ProviderKind`, `SecretStore`).
- Functions/methods/variables: `snake_case`.
- Enum variants: `PascalCase`.
- Acronyms follow normal Rust style (`OpenAi`, not `OpenAI` type names).
- Keep command strings lowercase slash commands (`/model`, `/help`).

## Types and API Design

- Use concrete types where clarity matters (avoid opaque aliases).
- Use enums for closed sets (providers, roles, events).
- Use `Option<T>` for absent values, not sentinel strings.
- Use `Result<T, E>` for fallible operations.
- At application boundary, use `anyhow::Result` for ergonomics.

## Error Handling Guidelines

- Never panic in normal runtime paths.
- Avoid `unwrap()`/`expect()` outside tests.
- Return actionable error messages including operation context.
- For user-triggered command errors, convert to readable UI messages.
- For background task errors, emit `AgentEvent::Error` instead of crashing.
- Preserve safety guarantees (workspace boundaries, command timeouts).

## Async and Concurrency Guidelines

- Use `tokio` primitives already present in the codebase.
- Keep lock scope (`Mutex`) as short as possible.
- Avoid blocking operations on async paths unless unavoidable.
- Prefer spawning focused tasks for long-running model requests.
- Ensure UI remains responsive while streaming.

## UI/TUI Conventions

- Maintain existing command UX (`/help`, `/model`, etc.).
- Keep keybindings consistent with current behavior.
- Preserve auto-scroll semantics and status indicators.
- Keep transcript rendering readable and role-differentiated.
- Do not regress startup home/logo rendering behavior.

## Provider and Model Conventions

- Keep provider support parity across OpenAI/Anthropic/Google.
- Default models come from `ProviderKind::default_model()`.
- Suggested model lists live in `ProviderKind::suggested_models()`.
- When switching models/providers, rebuild provider client safely.
- Disconnected provider state must yield clear user guidance.

## Secrets and Security Conventions

- Never print full API keys in UI logs/messages.
- Persist keys via `SecretStore` APIs only.
- Respect workspace path boundaries in all file/tool commands.
- Do not weaken command timeout or sandbox behavior without reason.

## Testing Guidelines

- Add/adjust unit tests for parsing/state transitions/tool safety changes.
- Keep tests deterministic and independent.
- Prefer small focused tests over broad integration mocks.
- Update existing tests when behavior contracts intentionally change.

## YOLO Shortcut

- When the user says "YOLO":
- Generate the commit message using the `caveman-commit` skill.
- Create a release.
- Push to the remote.

## Change Management Expectations for Agents

- Make minimal, targeted edits.
- Preserve existing public behavior unless request requires change.
- If behavior changes, update README and command help text.
- Bump the app version by a small increment for each release.
- Run full validation pipeline before final response.
- Summarize touched files and rationale clearly in handoff.
