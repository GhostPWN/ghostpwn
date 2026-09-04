mod logo;

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::agent::Agent;
use crate::config::ProviderKind;
use crate::images::{
    ClipboardContent, MAX_IMAGE_BYTES_PER_MESSAGE, MAX_IMAGES_PER_MESSAGE, normalize_pasted_text,
    read_clipboard, read_clipboard_image,
};
use crate::models::{AgentEvent, ConversationMessage, ConversationPart, ImageAttachment};
use crate::providers::{codex, copilot};

pub(super) mod palette {
    use ratatui::style::Color;
    pub const PHOSPHOR: Color = Color::Rgb(140, 90, 210);
    pub const BONE: Color = Color::Reset;
    pub const ASH: Color = Color::Reset;
    pub const ION: Color = Color::Rgb(140, 90, 210);
    pub const PLASMA: Color = Color::Rgb(140, 90, 210);
    pub const EMBER: Color = Color::Rgb(140, 90, 210);
    pub const BLOOD: Color = Color::Red;
    pub const STEEL: Color = Color::Reset;
    pub const FOG: Color = Color::Reset;
}

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiRole {
    User,
    Assistant,
    Tool,
    Error,
}

#[derive(Debug, Clone, Copy)]
struct CommandSpec {
    name: &'static str,
    description: &'static str,
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "/help",
        description: "Show available commands",
    },
    CommandSpec {
        name: "/model",
        description: "Open model selector and provider auth",
    },
    CommandSpec {
        name: "/audit",
        description: "Audit workspace; add --fix to apply approved fixes",
    },
    CommandSpec {
        name: "/clear",
        description: "Clear current chat",
    },
    CommandSpec {
        name: "/paste-image",
        description: "Attach image from clipboard",
    },
    CommandSpec {
        name: "/clear-images",
        description: "Remove queued clipboard images",
    },
    CommandSpec {
        name: "/exit",
        description: "Exit the app",
    },
    CommandSpec {
        name: "/quit",
        description: "Exit the app",
    },
];

#[derive(Debug, Clone)]
struct UiMessage {
    role: UiRole,
    content: String,
}

struct ModelSelector {
    id: u64,
    providers: Vec<ProviderKind>,
    provider_index: usize,
    provider_states: HashMap<ProviderKind, ModelSelectorProviderState>,
    mode: ModelSelectorMode,
    status: Option<ModelSelectorStatus>,
    oauth_task: Option<tokio::task::JoinHandle<()>>,
}

impl ModelSelector {
    fn replace_oauth_task(&mut self, task: tokio::task::JoinHandle<()>) {
        if let Some(previous) = self.oauth_task.replace(task) {
            previous.abort();
        }
    }
}

impl Drop for ModelSelector {
    fn drop(&mut self) {
        if let Some(task) = self.oauth_task.take() {
            task.abort();
        }
    }
}

struct ModelSelectorProviderState {
    models: Vec<String>,
    model_index: usize,
    loading: bool,
    error: Option<String>,
}

enum ModelSelectorMode {
    Browse,
    ApiKeyInput {
        provider: ProviderKind,
        input: String,
    },
}

struct ModelSelectorStatus {
    provider: ProviderKind,
    message: String,
    error: bool,
}

struct PendingApproval {
    response: oneshot::Sender<bool>,
}

struct UiState {
    provider_name: String,
    input: String,
    pending_images: Vec<ImageAttachment>,
    messages: Vec<UiMessage>,
    streaming_content: String,
    tool_status: String,
    is_streaming: bool,
    should_quit: bool,
    scroll_offset: u16,
    auto_scroll: bool,
    completion_matches: Vec<&'static CommandSpec>,
    completion_index: usize,
    selector: Option<ModelSelector>,
    next_selector_id: u64,
    pending_approval: Option<PendingApproval>,
    tick: u64,
}

impl UiState {
    fn new(provider_name: String) -> Self {
        Self {
            provider_name,
            input: String::new(),
            pending_images: Vec::new(),
            messages: Vec::new(),
            streaming_content: String::new(),
            tool_status: String::new(),
            is_streaming: false,
            should_quit: false,
            scroll_offset: 0,
            auto_scroll: true,
            completion_matches: Vec::new(),
            completion_index: 0,
            selector: None,
            next_selector_id: 0,
            pending_approval: None,
            tick: 0,
        }
    }

    fn push_message(&mut self, role: UiRole, content: String) {
        self.messages.push(UiMessage { role, content });
        if self.auto_scroll {
            self.scroll_offset = u16::MAX;
        }
    }

    fn flush_streaming_to_messages(&mut self) {
        if self.streaming_content.trim().is_empty() {
            self.streaming_content.clear();
            return;
        }

        let content = std::mem::take(&mut self.streaming_content);
        self.push_message(UiRole::Assistant, content);
    }

    fn sync_scroll(&mut self, line_count: u16, visible_lines: u16) {
        let max_scroll = max_scroll(line_count, visible_lines);
        if self.auto_scroll {
            self.scroll_offset = max_scroll;
        } else {
            self.scroll_offset = self.scroll_offset.min(max_scroll);
        }
    }

    fn scroll_up(&mut self, amount: u16, line_count: u16, visible_lines: u16) {
        let max_scroll = max_scroll(line_count, visible_lines);
        let current = if self.auto_scroll {
            max_scroll
        } else {
            self.scroll_offset
        };
        self.scroll_offset = current.saturating_sub(amount);
        self.auto_scroll = false;
    }

    fn scroll_down(&mut self, amount: u16, line_count: u16, visible_lines: u16) {
        let max_scroll = max_scroll(line_count, visible_lines);
        let current = if self.auto_scroll {
            max_scroll
        } else {
            self.scroll_offset
        };
        let next = current.saturating_add(amount).min(max_scroll);
        self.scroll_offset = next;
        self.auto_scroll = next >= max_scroll;
    }

    fn scroll_home(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
    }

    fn scroll_end(&mut self, line_count: u16, visible_lines: u16) {
        self.scroll_offset = max_scroll(line_count, visible_lines);
        self.auto_scroll = true;
    }

    fn refresh_completions(&mut self) {
        if self.is_streaming {
            self.completion_matches.clear();
            self.completion_index = 0;
            return;
        }

        let query = self.input.trim();
        if !query.starts_with('/') || query.contains(' ') {
            self.completion_matches.clear();
            self.completion_index = 0;
            return;
        }

        self.completion_matches = COMMANDS
            .iter()
            .filter(|cmd| cmd.name.starts_with(query))
            .collect();

        if self.completion_matches.is_empty()
            || self.completion_index >= self.completion_matches.len()
        {
            self.completion_index = 0;
        }
    }

    fn apply_completion(&mut self) {
        self.refresh_completions();
        if self.completion_matches.is_empty() {
            return;
        }

        let query = self.input.trim();
        let len = self.completion_matches.len();

        if len == 1 {
            self.input = format!("{} ", self.completion_matches[0].name);
            self.completion_index = 0;
            self.refresh_completions();
            return;
        }

        let next_index = self
            .completion_matches
            .iter()
            .position(|cmd| cmd.name == query)
            .map(|idx| (idx + 1) % len)
            .unwrap_or(0);

        self.completion_index = next_index;
        self.input = self.completion_matches[next_index].name.to_string();
        self.refresh_completions();
    }

    fn active_completion(&self) -> Option<&'static CommandSpec> {
        if self.completion_matches.is_empty() {
            None
        } else {
            let index = self.completion_index.min(self.completion_matches.len() - 1);
            Some(self.completion_matches[index])
        }
    }
}

pub async fn run_ui(agent: Arc<Mutex<Agent>>) -> Result<()> {
    let provider_name = {
        let locked = agent.lock().await;
        locked.provider_name()
    };

    let mut session = TerminalSession::enter()?;
    let stdout = io::stdout();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (event_tx, mut event_rx) = unbounded_channel::<AgentEvent>();
    let mut state = UiState::new(provider_name);

    let run_result = ui_loop(&mut terminal, &agent, &event_tx, &mut event_rx, &mut state).await;

    let restore_result = session.restore();
    match (run_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            Err(error.context(format!("terminal cleanup also failed: {cleanup_error}")))
        }
    }
}

struct TerminalSession {
    active: bool,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        Ok(Self { active: true })
    }

    fn restore(&mut self) -> Result<()> {
        let raw_result = disable_raw_mode();
        let mut stdout = io::stdout();
        let screen_result = execute!(
            stdout,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
        if raw_result.is_ok() && screen_result.is_ok() {
            self.active = false;
        }
        raw_result?;
        screen_result?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Show
        );
    }
}

async fn ui_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    event_rx: &mut UnboundedReceiver<AgentEvent>,
    state: &mut UiState,
) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        while let Ok(ev) = event_rx.try_recv() {
            apply_agent_event(state, ev);
        }

        state.tick = state.tick.wrapping_add(1);
        state.refresh_completions();

        let size = terminal.size()?;
        let line_count = transcript_line_count(state, transcript_content_width(size.width));
        let visible_lines = message_visible_lines(size.height);
        state.sync_scroll(line_count, visible_lines);

        terminal.draw(|frame| render(frame, state))?;

        if state.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(25))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    if state.selector.is_some() {
                        handle_selector_key(key.code, state, agent, event_tx).await;
                        continue;
                    }

                    if state.pending_approval.is_some() {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                resolve_approval(state, true);
                            }
                            KeyCode::Char('n')
                            | KeyCode::Char('N')
                            | KeyCode::Enter
                            | KeyCode::Esc => {
                                resolve_approval(state, false);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    let size = terminal.size()?;
                    let line_count =
                        transcript_line_count(state, transcript_content_width(size.width));
                    let visible_lines = message_visible_lines(size.height);

                    match key.code {
                        KeyCode::Enter => {
                            let text = state.input.trim().to_string();
                            handle_submit(text, state, agent, event_tx).await;
                        }
                        KeyCode::Backspace => {
                            state.input.pop();
                            state.refresh_completions();
                        }
                        KeyCode::Up => {
                            state.scroll_up(1, line_count, visible_lines);
                        }
                        KeyCode::Down => {
                            state.scroll_down(1, line_count, visible_lines);
                        }
                        KeyCode::PageUp => {
                            state.scroll_up(10, line_count, visible_lines);
                        }
                        KeyCode::PageDown => {
                            state.scroll_down(10, line_count, visible_lines);
                        }
                        KeyCode::Home => {
                            state.scroll_home();
                        }
                        KeyCode::End => {
                            state.scroll_end(line_count, visible_lines);
                        }
                        KeyCode::Tab => {
                            let before = state.input.clone();
                            state.apply_completion();
                            if state.input == before {
                                state.input.push(' ');
                                state.input.push(' ');
                                state.refresh_completions();
                            }
                        }
                        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            paste_clipboard_content(state).await;
                        }
                        KeyCode::Char(ch)
                            if !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            state.input.push(ch);
                            state.refresh_completions();
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    let line_count =
                        transcript_line_count(state, transcript_content_width(size.width));
                    let visible_lines = message_visible_lines(size.height);

                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            state.scroll_up(3, line_count, visible_lines);
                        }
                        MouseEventKind::ScrollDown => {
                            state.scroll_down(3, line_count, visible_lines);
                        }
                        _ => {}
                    }
                }
                Event::Paste(text)
                    if state.selector.is_none() && state.pending_approval.is_none() =>
                {
                    state.input.push_str(&normalize_pasted_text(&text));
                    state.refresh_completions();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn handle_submit(
    text: String,
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
    if (text.is_empty() && state.pending_images.is_empty()) || state.is_streaming {
        return;
    }

    if text == "/exit" || text == "/quit" {
        state.should_quit = true;
        return;
    }

    if text == "/clear" {
        state.messages.clear();
        state.streaming_content.clear();
        state.tool_status.clear();
        state.scroll_offset = 0;
        state.auto_scroll = true;
        state.input.clear();
        state.pending_images.clear();

        let mut locked = agent.lock().await;
        locked.clear_history();
        return;
    }

    if text == "/model" {
        state.input.clear();
        open_model_selector(state, agent, event_tx).await;
        return;
    }

    if text == "/help" {
        let help = COMMANDS
            .iter()
            .map(|cmd| format!("{} - {}", cmd.name, cmd.description))
            .collect::<Vec<String>>()
            .join("\n");
        state.push_message(UiRole::Assistant, format!("Commands:\n{}", help));
        state.input.clear();
        state.refresh_completions();
        return;
    }

    if text == "/paste-image" {
        match read_clipboard_image().await {
            Ok(image) => match queue_image(state, image) {
                Ok(()) => {
                    state.input.clear();
                    state.refresh_completions();
                }
                Err(error) => state.push_message(UiRole::Error, error.to_string()),
            },
            Err(error) => state.push_message(UiRole::Error, error.to_string()),
        }
        return;
    }

    if text == "/clear-images" {
        state.pending_images.clear();
        state.input.clear();
        state.refresh_completions();
        return;
    }

    if let Some((apply_fixes, target)) = parse_audit_command(&text) {
        let (scope_root, scope) = {
            let locked = agent.lock().await;
            match locked.resolve_audit_scope(target) {
                Ok(scope) => scope,
                Err(err) => {
                    state.push_message(UiRole::Error, err.to_string());
                    return;
                }
            }
        };
        let prompt = build_audit_prompt(&scope, &scope_root, apply_fixes);

        let flag = if apply_fixes { " --fix" } else { "" };
        let label = format!("/audit{flag} {target}");
        state.push_message(UiRole::User, label.trim().to_string());
        state.input.clear();
        state.is_streaming = true;
        state.streaming_content.clear();
        state.tool_status.clear();

        let tx = event_tx.clone();
        let handle = Arc::clone(agent);
        tokio::spawn(async move {
            let mut locked = handle.lock().await;
            if let Err(err) = locked
                .handle_audit(prompt, scope_root, apply_fixes, tx.clone())
                .await
            {
                let _ = tx.send(AgentEvent::Error(err.to_string()));
                let _ = tx.send(AgentEvent::Done);
            }
        });
        return;
    }

    if text.starts_with('/') {
        state.push_message(
            UiRole::Error,
            format!("Unknown command: {} (try /help)", text),
        );
        return;
    }

    let prepare = {
        let locked = agent.lock().await;
        locked.prepare_user_input(text, state.pending_images.clone())
    };
    let message = match prepare.await {
        Ok(message) => message,
        Err(error) => {
            state.push_message(UiRole::Error, error.to_string());
            return;
        }
    };

    state.push_message(UiRole::User, display_user_message(&message));
    state.input.clear();
    state.pending_images.clear();
    state.refresh_completions();
    state.is_streaming = true;
    state.streaming_content.clear();
    state.tool_status.clear();

    let tx = event_tx.clone();
    let handle = Arc::clone(agent);

    tokio::spawn(async move {
        let mut locked = handle.lock().await;
        if let Err(err) = locked.handle_user_message(message, tx.clone()).await {
            let _ = tx.send(AgentEvent::Error(err.to_string()));
            let _ = tx.send(AgentEvent::Done);
        }
    });
}

async fn paste_clipboard_content(state: &mut UiState) {
    match read_clipboard().await {
        Ok(ClipboardContent::Image(image)) => {
            if let Err(error) = queue_image(state, image) {
                state.push_message(UiRole::Error, error.to_string());
            }
        }
        Ok(ClipboardContent::Text(text)) => {
            state.input.push_str(&text);
            state.refresh_completions();
        }
        Err(error) => state.push_message(UiRole::Error, error.to_string()),
    }
}

fn queue_image(state: &mut UiState, image: ImageAttachment) -> Result<()> {
    if state.pending_images.len() >= MAX_IMAGES_PER_MESSAGE {
        return Err(anyhow::anyhow!(
            "A message can contain at most {MAX_IMAGES_PER_MESSAGE} images"
        ));
    }
    let total = state
        .pending_images
        .iter()
        .try_fold(image.data.len(), |total, pending| {
            total.checked_add(pending.data.len())
        })
        .ok_or_else(|| anyhow::anyhow!("Image attachment size overflow"))?;
    if total > MAX_IMAGE_BYTES_PER_MESSAGE {
        return Err(anyhow::anyhow!(
            "Image attachments exceed the {} MiB per-message limit",
            MAX_IMAGE_BYTES_PER_MESSAGE / (1024 * 1024)
        ));
    }
    state.pending_images.push(image);
    Ok(())
}

fn display_user_message(message: &ConversationMessage) -> String {
    let mut display = String::new();
    for part in &message.content {
        match part {
            ConversationPart::Text(text) => display.push_str(text),
            ConversationPart::Image(image) => {
                display.push_str(&format!("[image: {}]", image.name));
            }
        }
    }
    display.trim().to_string()
}

fn parse_audit_command(text: &str) -> Option<(bool, &str)> {
    let rest = text.strip_prefix("/audit")?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }

    let rest = rest.trim();
    if rest == "--fix" {
        Some((true, ""))
    } else if let Some(target) = rest.strip_prefix("--fix ") {
        Some((true, target.trim()))
    } else {
        Some((false, rest))
    }
}

fn build_audit_prompt(scope: &str, scope_root: &Path, apply_fixes: bool) -> String {
    let mode = if apply_fixes {
        "Fix mode permits generateDiff and the file mutation tools only inside the audit \
root. Apply the minimum fix for each confirmed issue; generate a diff before non-trivial \
changes. Every mutation requires user approval. Shell commands and general web access \
remain blocked."
    } else {
        "Read-only mode blocks shell commands, general web access, and all file mutations."
    };

    format!(
        "Perform a security audit of {scope}. The enforced audit root is \
{scope_root:?}; inspect nothing outside it. This is authorized review of local code. \
{mode}\n\
\n\
First call searchSkills with the query \"source code dependency vulnerability security \
audit\" and follow the best relevant skill without running its scripts. Then enumerate \
the relevant files with listDirectory, grep, and searchFiles, and read important files \
with readFile before drawing conclusions. Use numberedContent for exact line references. \
If a tool result is truncated, narrow or paginate the search; never imply full coverage \
of unseen code.\n\
\n\
If Cargo.lock exists in scope, call auditDependencies on {scope_root:?}. It performs \
an explicitly approved, read-only query against OSV.dev. If approval is denied or the \
result says complete=false, disclose that dependency coverage is incomplete.\n\
\n\
Check for these classes of issues:\n\
- Hardcoded secrets, credentials, API keys, or tokens\n\
- Injection risks: command, SQL, path traversal, and unsafe deserialization\n\
- Missing input validation and authorization/access-control gaps\n\
- Insecure cryptography or weak randomness\n\
- Dependency and configuration risks (manifests, lockfiles, permissions)\n\
- Unsafe code and error handling that could panic or crash in production\n\
\n\
Report a prioritized list of findings. For each finding give: severity \
(critical/high/medium/low), the file and line reference, a short explanation of \
why it is a risk, and a concrete recommended fix. If you find no issues in a \
category, say so briefly. End with a Coverage section listing inspected files, whether \
dependency scanning ran, and every truncation, denial, or tool error. Mark the audit \
incomplete if any relevant content was not inspected. If fix mode is enabled, also list \
every changed file and any validation that remains to be run."
    )
}

async fn open_model_selector(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
    state.next_selector_id = state.next_selector_id.wrapping_add(1);
    let selector_id = state.next_selector_id;
    let current_provider = {
        let locked = agent.lock().await;
        locked.current_provider()
    };
    let providers = ProviderKind::all().to_vec();
    let provider_index = providers
        .iter()
        .position(|provider| *provider == current_provider)
        .unwrap_or(0);
    let provider_states = providers
        .iter()
        .copied()
        .map(|provider| {
            (
                provider,
                ModelSelectorProviderState {
                    models: Vec::new(),
                    model_index: 0,
                    loading: false,
                    error: None,
                },
            )
        })
        .collect();

    state.selector = Some(ModelSelector {
        id: selector_id,
        providers,
        provider_index,
        provider_states,
        mode: ModelSelectorMode::Browse,
        status: None,
        oauth_task: None,
    });
    fetch_selector_models_if_needed(state, agent, event_tx, current_provider);
}

async fn handle_selector_key(
    key: KeyCode,
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
    if selector_handles_api_key_input(key, state, agent, event_tx).await {
        return;
    }

    let mut provider_to_fetch = None;
    let Some(selector) = state.selector.as_mut() else {
        return;
    };

    match key {
        KeyCode::Esc => {
            state.selector = None;
        }
        KeyCode::Char('c') => {
            let provider = selector.providers[selector.provider_index];
            match provider {
                ProviderKind::Copilot => start_selector_copilot_auth(state, agent, event_tx),
                ProviderKind::Codex => start_selector_codex_auth(state, agent, event_tx),
                ProviderKind::Ollama => {
                    selector.status = Some(ModelSelectorStatus {
                        provider,
                        message: "Ollama is local and does not require an API key.".to_string(),
                        error: false,
                    });
                }
                _ => {
                    selector.mode = ModelSelectorMode::ApiKeyInput {
                        provider,
                        input: String::new(),
                    };
                    selector.status = Some(ModelSelectorStatus {
                        provider,
                        message: format!("Paste {} API key, then press enter.", provider.as_str()),
                        error: false,
                    });
                }
            }
        }
        KeyCode::Char('d') => {
            let provider = selector.providers[selector.provider_index];
            if provider == ProviderKind::Ollama {
                selector.status = Some(ModelSelectorStatus {
                    provider,
                    message: "Ollama is local and cannot be disconnected.".to_string(),
                    error: false,
                });
            } else {
                let mut locked = agent.lock().await;
                let message = locked.disconnect_key(provider);
                state.provider_name = locked.provider_name();
                if let Some(selector) = state.selector.as_mut() {
                    if let Some(provider_state) = selector.provider_states.get_mut(&provider) {
                        provider_state.models.clear();
                        provider_state.model_index = 0;
                        provider_state.loading = false;
                        provider_state.error = Some(format!(
                            "{} disconnected. Press c to connect.",
                            provider.as_str()
                        ));
                    }
                    selector.status = Some(ModelSelectorStatus {
                        provider,
                        message,
                        error: false,
                    });
                }
            }
        }
        KeyCode::Char('r') => {
            let provider = selector.providers[selector.provider_index];
            if let Some(provider_state) = selector.provider_states.get_mut(&provider) {
                provider_state.models.clear();
                provider_state.model_index = 0;
                provider_state.loading = false;
                provider_state.error = None;
            }
            provider_to_fetch = Some(provider);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            selector.provider_index = selector.provider_index.saturating_sub(1);
            let provider = selector.providers[selector.provider_index];
            provider_to_fetch = Some(provider);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            selector.provider_index =
                (selector.provider_index + 1).min(selector.providers.len().saturating_sub(1));
            let provider = selector.providers[selector.provider_index];
            provider_to_fetch = Some(provider);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let provider = selector.providers[selector.provider_index];
            if let Some(provider_state) = selector.provider_states.get_mut(&provider) {
                provider_state.model_index = provider_state.model_index.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let provider = selector.providers[selector.provider_index];
            if let Some(provider_state) = selector.provider_states.get_mut(&provider)
                && !provider_state.models.is_empty()
            {
                provider_state.model_index = (provider_state.model_index + 1)
                    .min(provider_state.models.len().saturating_sub(1));
            }
        }
        KeyCode::PageUp => {
            let provider = selector.providers[selector.provider_index];
            if let Some(provider_state) = selector.provider_states.get_mut(&provider) {
                provider_state.model_index = provider_state.model_index.saturating_sub(10);
            }
        }
        KeyCode::PageDown => {
            let provider = selector.providers[selector.provider_index];
            if let Some(provider_state) = selector.provider_states.get_mut(&provider)
                && !provider_state.models.is_empty()
            {
                provider_state.model_index = (provider_state.model_index + 10)
                    .min(provider_state.models.len().saturating_sub(1));
            }
        }
        KeyCode::Enter => {
            let provider = selector.providers[selector.provider_index];
            let model = selector
                .provider_states
                .get(&provider)
                .and_then(|provider_state| {
                    provider_state
                        .models
                        .get(provider_state.model_index)
                        .cloned()
                });
            if let Some(model) = model {
                let mut locked = agent.lock().await;
                let response = locked.switch_model(provider, Some(model));
                state.provider_name = locked.provider_name();
                state.selector = None;
                state.push_message(UiRole::Assistant, response);
            }
        }
        _ => {}
    }

    if let Some(provider) = provider_to_fetch {
        fetch_selector_models_if_needed(state, agent, event_tx, provider);
    }
}

async fn selector_handles_api_key_input(
    key: KeyCode,
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) -> bool {
    let Some(selector) = state.selector.as_mut() else {
        return false;
    };
    let ModelSelectorMode::ApiKeyInput { provider, input } = &mut selector.mode else {
        return false;
    };
    let provider = *provider;

    match key {
        KeyCode::Esc => {
            selector.mode = ModelSelectorMode::Browse;
            selector.status = Some(ModelSelectorStatus {
                provider,
                message: "API key entry cancelled.".to_string(),
                error: false,
            });
        }
        KeyCode::Enter => {
            let api_key = input.trim().to_string();
            if api_key.len() < 8 {
                selector.status = Some(ModelSelectorStatus {
                    provider,
                    message: "API key looks too short.".to_string(),
                    error: true,
                });
                return true;
            }

            let mut locked = agent.lock().await;
            let message = locked.connect_key(provider, api_key);
            state.provider_name = locked.provider_name();
            drop(locked);

            if let Some(selector) = state.selector.as_mut() {
                selector.mode = ModelSelectorMode::Browse;
                selector.status = Some(ModelSelectorStatus {
                    provider,
                    message,
                    error: false,
                });
                if let Some(provider_state) = selector.provider_states.get_mut(&provider) {
                    provider_state.models.clear();
                    provider_state.model_index = 0;
                    provider_state.loading = false;
                    provider_state.error = None;
                }
            }
            fetch_selector_models_if_needed(state, agent, event_tx, provider);
        }
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Char(ch) => {
            input.push(ch);
        }
        _ => {}
    }

    true
}

fn start_selector_copilot_auth(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
    let Some(selector_id) = state.selector.as_ref().map(|selector| selector.id) else {
        return;
    };
    if let Some(selector) = state.selector.as_mut() {
        selector.status = Some(ModelSelectorStatus {
            provider: ProviderKind::Copilot,
            message: "Starting GitHub Copilot OAuth...".to_string(),
            error: false,
        });
        if let Some(provider_state) = selector.provider_states.get_mut(&ProviderKind::Copilot) {
            provider_state.loading = true;
            provider_state.error = None;
        }
    }

    let tx = SelectorEventSender::new(event_tx.clone(), selector_id);
    let handle = Arc::clone(agent);
    let task = tokio::spawn(async move {
        if let Err(err) = copilot_device_flow(tx.clone(), handle).await {
            let _ = tx.send(AgentEvent::ProviderStatus {
                provider: ProviderKind::Copilot,
                message: format!("Copilot authorization failed: {}", err),
                error: true,
            });
            let _ = tx.send(AgentEvent::ModelList {
                provider: ProviderKind::Copilot,
                models: Vec::new(),
                error: Some("Copilot authorization failed.".to_string()),
            });
        }
    });
    if let Some(selector) = state.selector.as_mut() {
        selector.replace_oauth_task(task);
    } else {
        task.abort();
    }
}

fn start_selector_codex_auth(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
    let Some(selector_id) = state.selector.as_ref().map(|selector| selector.id) else {
        return;
    };
    if let Some(selector) = state.selector.as_mut() {
        selector.status = Some(ModelSelectorStatus {
            provider: ProviderKind::Codex,
            message: "Starting Codex OAuth...".to_string(),
            error: false,
        });
        if let Some(provider_state) = selector.provider_states.get_mut(&ProviderKind::Codex) {
            provider_state.loading = true;
            provider_state.error = None;
        }
    }

    let tx = SelectorEventSender::new(event_tx.clone(), selector_id);
    let handle = Arc::clone(agent);
    let task = tokio::spawn(async move {
        if let Err(err) = codex_oauth_flow(tx.clone(), handle).await {
            let _ = tx.send(AgentEvent::ProviderStatus {
                provider: ProviderKind::Codex,
                message: format!("Codex authorization failed: {}", err),
                error: true,
            });
            let _ = tx.send(AgentEvent::ModelList {
                provider: ProviderKind::Codex,
                models: Vec::new(),
                error: Some("Codex authorization failed.".to_string()),
            });
        }
    });
    if let Some(selector) = state.selector.as_mut() {
        selector.replace_oauth_task(task);
    } else {
        task.abort();
    }
}

fn fetch_selector_models_if_needed(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    provider: ProviderKind,
) {
    let Some(selector) = state.selector.as_mut() else {
        return;
    };
    let Some(provider_state) = selector.provider_states.get_mut(&provider) else {
        return;
    };
    if provider_state.loading || provider_state.error.is_some() || !provider_state.models.is_empty()
    {
        return;
    }

    provider_state.loading = true;
    fetch_selector_models(agent, event_tx, selector.id, provider);
}

fn fetch_selector_models(
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    selector_id: u64,
    provider: ProviderKind,
) {
    let tx = SelectorEventSender::new(event_tx.clone(), selector_id);
    let handle = Arc::clone(agent);
    tokio::spawn(async move {
        let provider_keys = {
            let locked = handle.lock().await;
            locked.provider_keys_snapshot()
        };
        match Agent::fetch_provider_models_with_keys(provider, &provider_keys).await {
            Ok(models) => {
                let reconciliation = {
                    let mut locked = handle.lock().await;
                    locked
                        .reconcile_current_model(provider, &models)
                        .map(|message| (message, locked.provider_name()))
                };
                if let Some((message, provider_name)) = reconciliation {
                    let _ = tx.send(AgentEvent::ProviderName(provider_name));
                    let _ = tx.send(AgentEvent::ProviderStatus {
                        provider,
                        message,
                        error: false,
                    });
                }
                let _ = tx.send(AgentEvent::ModelList {
                    provider,
                    models,
                    error: None,
                });
            }
            Err(err) => {
                let _ = tx.send(AgentEvent::ModelList {
                    provider,
                    models: Vec::new(),
                    error: Some(err.to_string()),
                });
            }
        }
    });
}

fn apply_agent_event(state: &mut UiState, event: AgentEvent) {
    match event {
        AgentEvent::Selector { id, event } => {
            if state
                .selector
                .as_ref()
                .is_some_and(|selector| selector.id == id)
            {
                apply_agent_event(state, *event);
            }
        }
        AgentEvent::AssistantDelta(delta) => {
            state.streaming_content.push_str(&delta);
            state.tool_status.clear();
        }
        AgentEvent::ApprovalRequired {
            name,
            args_summary,
            response,
        } => {
            let prompt = format!("Approve {name}({args_summary})? [y/N]");
            state.tool_status = prompt.clone();
            state.push_message(UiRole::Tool, prompt);
            state.pending_approval = Some(PendingApproval { response });
        }
        AgentEvent::ToolCall { name, args_summary } => {
            state.streaming_content.clear();
            let tool_line = format!("{}({})", name, args_summary);
            state.tool_status = tool_line.clone();
            state.push_message(UiRole::Tool, tool_line);
        }
        AgentEvent::ModelList {
            provider,
            models,
            error,
        } => {
            if let Some(selector) = state.selector.as_mut()
                && let Some(provider_state) = selector.provider_states.get_mut(&provider)
            {
                provider_state.models = models;
                provider_state.model_index = 0;
                provider_state.loading = false;
                provider_state.error = error;
            }
        }
        AgentEvent::ProviderStatus {
            provider,
            message,
            error,
        } => {
            if let Some(selector) = state.selector.as_mut() {
                selector.status = Some(ModelSelectorStatus {
                    provider,
                    message,
                    error,
                });
            } else if error {
                state.push_message(UiRole::Error, message);
            } else {
                state.push_message(UiRole::Assistant, message);
            }
        }
        AgentEvent::ProviderName(name) => {
            state.provider_name = name;
        }
        AgentEvent::Error(err) => {
            state.flush_streaming_to_messages();
            state.push_message(UiRole::Error, err);
        }
        AgentEvent::Done => {
            state.pending_approval = None;
            state.flush_streaming_to_messages();
            state.tool_status.clear();
            state.is_streaming = false;
        }
    }
}

fn resolve_approval(state: &mut UiState, approved: bool) {
    if let Some(pending) = state.pending_approval.take() {
        let _ = pending.response.send(approved);
        state.push_message(
            UiRole::Tool,
            if approved {
                "Approved.".to_string()
            } else {
                "Denied.".to_string()
            },
        );
        state.tool_status = if approved {
            "running approved tool".to_string()
        } else {
            "tool denied".to_string()
        };
    }
}

fn render(frame: &mut Frame, state: &mut UiState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_transcript(frame, root[0], state);
    render_status(frame, root[1], state);
    render_input(frame, root[2], state);
    render_selector(frame, state);
}

fn render_selector(frame: &mut Frame, state: &UiState) {
    let Some(selector) = state.selector.as_ref() else {
        return;
    };

    let area = centered_rect(72, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::PHOSPHOR))
        .title(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("▸ ", Style::default().fg(palette::PLASMA)),
            Span::styled("model selector", Style::default().fg(palette::BONE).bold()),
            Span::styled(" ─", Style::default().fg(palette::STEEL).dim()),
        ]))
        .padding(Padding::new(2, 2, 1, 1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let provider_spans = selector
        .providers
        .iter()
        .enumerate()
        .flat_map(|(idx, provider)| {
            let selected = idx == selector.provider_index;
            let style = if selected {
                Style::default().fg(palette::PHOSPHOR).bold()
            } else {
                Style::default().fg(palette::ASH).dim()
            };
            vec![
                Span::styled(
                    if selected {
                        format!("‹ {} ›", provider.as_str())
                    } else {
                        provider.as_str().to_string()
                    },
                    style,
                ),
                Span::raw("   "),
            ]
        })
        .collect::<Vec<Span>>();
    frame.render_widget(Paragraph::new(Line::from(provider_spans)), rows[0]);

    let selected_provider = selector.providers[selector.provider_index];
    let selected_state = selector.provider_states.get(&selected_provider);

    let list_lines = if let ModelSelectorMode::ApiKeyInput { provider, input } = &selector.mode {
        let masked = "•".repeat(input.chars().count());
        vec![
            Line::from(vec![Span::styled(
                format!("Paste {} API key", provider.as_str()),
                Style::default().fg(palette::BONE).bold(),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("key  ", Style::default().fg(palette::ASH).dim()),
                Span::styled(masked, Style::default().fg(palette::PHOSPHOR)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "enter saves · esc cancels",
                Style::default().fg(palette::ASH).dim(),
            )]),
        ]
    } else if selected_state.is_some_and(|provider_state| provider_state.loading) {
        vec![Line::from(vec![Span::styled(
            "fetching models...",
            Style::default().fg(palette::EMBER),
        )])]
    } else if let Some(error) =
        selected_state.and_then(|provider_state| provider_state.error.as_ref())
    {
        let hint = match selected_provider {
            ProviderKind::Copilot => "press r to retry or c to connect with GitHub device OAuth",
            ProviderKind::Codex => "press r to retry or c to connect with Codex OAuth",
            ProviderKind::Ollama => "press r to retry the configured local Ollama host",
            _ => "press r to retry or c to paste an API key",
        };
        vec![
            Line::from(vec![Span::styled(
                error.clone(),
                Style::default().fg(palette::BLOOD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                hint,
                Style::default().fg(palette::ASH).dim(),
            )]),
        ]
    } else if selected_state.is_none_or(|provider_state| provider_state.models.is_empty()) {
        vec![Line::from(vec![Span::styled(
            "no models returned",
            Style::default().fg(palette::ASH).dim(),
        )])]
    } else {
        let selected_state = selected_state.expect("selected provider state must exist");
        let visible = rows[1].height.max(1) as usize;
        let start = selected_state
            .model_index
            .saturating_add(1)
            .saturating_sub(visible);

        selected_state
            .models
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(idx, model)| {
                let selected = idx == selected_state.model_index;
                Line::from(vec![
                    Span::styled(
                        if selected { "❯ " } else { "  " },
                        Style::default().fg(palette::PHOSPHOR),
                    ),
                    Span::styled(
                        model.clone(),
                        if selected {
                            Style::default().fg(palette::BONE).bold()
                        } else {
                            Style::default().fg(palette::ASH).dim()
                        },
                    ),
                ])
            })
            .collect()
    };
    let list_lines = selector_status_lines(selector, selected_provider)
        .into_iter()
        .chain(list_lines)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(list_lines).wrap(Wrap { trim: false }),
        rows[1],
    );

    let footer = Line::from(vec![
        Span::styled("←/→", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" provider  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("↑/↓", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" model  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("pgup/pgdn", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" jump  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("c", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" connect  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("d", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" disconnect  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("r", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" refresh  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("enter", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" switch  ", Style::default().fg(palette::ASH).dim()),
        Span::styled("esc", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" close", Style::default().fg(palette::ASH).dim()),
    ]);
    frame.render_widget(Paragraph::new(footer), rows[2]);
}

fn selector_status_lines(
    selector: &ModelSelector,
    selected_provider: ProviderKind,
) -> Vec<Line<'static>> {
    let Some(status) = selector
        .status
        .as_ref()
        .filter(|status| status.provider == selected_provider)
    else {
        return Vec::new();
    };

    let color = if status.error {
        palette::BLOOD
    } else {
        palette::ASH
    };
    let style = if status.error {
        Style::default().fg(color).bold()
    } else {
        Style::default().fg(color).dim()
    };
    let mut lines = status
        .message
        .lines()
        .map(|line| Line::from(vec![Span::styled(line.to_string(), style)]))
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    lines
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn render_transcript(frame: &mut Frame, area: Rect, state: &mut UiState) {
    let is_home = state.messages.is_empty() && state.streaming_content.is_empty();

    let block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::new(2, 2, 0, 0));

    if is_home {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        logo::render(frame, inner);
        return;
    }

    let inner = block.inner(area);
    let lines = build_transcript_lines(state);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    let line_count = paragraph.line_count(inner.width).min(u16::MAX as usize) as u16;
    let visible = inner.height;
    let max_scroll = line_count.saturating_sub(visible);
    let scroll = state.scroll_offset.min(max_scroll);

    let paragraph = paragraph.scroll((scroll, 0));
    frame.render_widget(paragraph, area);

    if line_count > visible {
        render_scrollbar(frame, area, scroll, line_count, visible);
    }
}

fn render_scrollbar(frame: &mut Frame, area: Rect, offset: u16, total: u16, visible: u16) {
    if area.height < 4 || area.width < 3 {
        return;
    }
    let track_x = area.x + area.width - 2;
    let track_y = area.y + 1;
    let track_h = area.height.saturating_sub(2);
    if track_h == 0 {
        return;
    }

    let scrollable = total.saturating_sub(visible) as f32;
    let ratio = if scrollable > 0.0 {
        offset as f32 / scrollable
    } else {
        0.0
    };
    let thumb_h = (((visible as f32 / total as f32) * track_h as f32).round() as u16).max(1);
    let thumb_h = thumb_h.min(track_h);
    let thumb_y = track_y + (((track_h - thumb_h) as f32) * ratio).round() as u16;

    let buf = frame.buffer_mut();
    for y in track_y..track_y + track_h {
        let cell = &mut buf[(track_x, y)];
        cell.set_char('▏')
            .set_style(Style::default().fg(palette::STEEL).dim());
    }
    for y in thumb_y..thumb_y + thumb_h {
        let cell = &mut buf[(track_x, y)];
        cell.set_char('▐')
            .set_style(Style::default().fg(palette::PLASMA));
    }
}

fn render_status(frame: &mut Frame, area: Rect, state: &UiState) {
    let frame_idx = (state.tick / 2) as usize % SPINNER.len();
    let mut left_spans = vec![Span::raw(" ")];
    if state.is_streaming {
        let text = if state.tool_status.is_empty() {
            "streaming response".to_string()
        } else {
            state.tool_status.clone()
        };
        left_spans.push(Span::styled(
            format!("{} ", SPINNER[frame_idx]),
            Style::default().fg(palette::EMBER).bold(),
        ));
        left_spans.push(Span::styled(text, Style::default().fg(palette::BONE)));
        left_spans.push(Span::raw("   "));
    }
    left_spans.push(Span::styled(
        "◆ ",
        Style::default().fg(palette::PHOSPHOR).bold(),
    ));
    left_spans.push(Span::styled(
        state.provider_name.clone(),
        Style::default().fg(palette::ION).bold(),
    ));
    if !state.pending_images.is_empty() {
        let bytes = state
            .pending_images
            .iter()
            .map(|image| image.data.len())
            .sum::<usize>();
        left_spans.push(Span::styled(
            format!(
                " · {} image{} · {:.1} MiB",
                state.pending_images.len(),
                if state.pending_images.len() == 1 {
                    ""
                } else {
                    "s"
                },
                bytes as f64 / (1024.0 * 1024.0)
            ),
            Style::default().fg(palette::ASH).dim(),
        ));
    }

    let left = Line::from(left_spans);

    let right = Line::from(vec![
        Span::styled("tab", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" complete ", Style::default().fg(palette::ASH).dim()),
        Span::styled("·", Style::default().fg(palette::STEEL).dim()),
        Span::styled(" ^v", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" paste ", Style::default().fg(palette::ASH).dim()),
        Span::styled("·", Style::default().fg(palette::STEEL).dim()),
        Span::styled(" ↵", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" send ", Style::default().fg(palette::ASH).dim()),
        Span::styled("·", Style::default().fg(palette::STEEL).dim()),
        Span::styled(" ^c", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" quit ", Style::default().fg(palette::ASH).dim()),
    ]);

    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    frame.render_widget(Paragraph::new(left), cols[0]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), cols[1]);
}

fn render_input(frame: &mut Frame, area: Rect, state: &UiState) {
    let border_color = if state.is_streaming {
        palette::STEEL
    } else {
        palette::PHOSPHOR
    };

    let border_style = if state.is_streaming {
        Style::default().fg(border_color).dim()
    } else {
        Style::default().fg(border_color)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::horizontal(1));

    let line = if state.is_streaming {
        if state.pending_approval.is_some() {
            Line::from(vec![
                Span::styled("Approve tool? ", Style::default().fg(palette::EMBER).bold()),
                Span::styled("y", Style::default().fg(palette::PHOSPHOR).bold()),
                Span::raw(" / "),
                Span::styled("N", Style::default().fg(palette::BLOOD).bold()),
            ])
        } else {
            let idx = (state.tick / 2) as usize % SPINNER.len();
            Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER[idx]),
                    Style::default().fg(palette::EMBER).bold(),
                ),
                Span::styled(
                    "channel open · awaiting model",
                    Style::default().fg(palette::ASH).dim().italic(),
                ),
            ])
        }
    } else {
        let caret = if (state.tick / 8).is_multiple_of(2) {
            "▌"
        } else {
            " "
        };
        let mut spans = vec![
            Span::styled("❯ ", Style::default().fg(palette::PHOSPHOR).bold()),
            Span::styled(state.input.clone(), Style::default().fg(palette::BONE)),
            Span::styled(caret.to_string(), Style::default().fg(palette::PHOSPHOR)),
        ];

        if let Some((suffix, description)) = completion_hint(state)
            && !suffix.is_empty()
        {
            spans.push(Span::styled(
                suffix.to_string(),
                Style::default().fg(palette::STEEL).dim(),
            ));
            spans.push(Span::raw("    "));
            spans.push(Span::styled(
                format!("↹ {description}"),
                Style::default().fg(palette::ASH).dim().italic(),
            ));
        } else if state.input.is_empty() {
            spans.push(Span::styled(
                "  try  ",
                Style::default().fg(palette::STEEL).dim(),
            ));
            spans.push(Span::styled(
                "/help",
                Style::default().fg(palette::ASH).dim(),
            ));
            spans.push(Span::styled(
                "  or describe a target…",
                Style::default().fg(palette::STEEL).dim().italic(),
            ));
        }

        Line::from(spans)
    };

    let para = Paragraph::new(line).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn build_transcript_lines(state: &UiState) -> Vec<Line<'static>> {
    let mut lines = Vec::<Line<'static>>::new();

    for msg in &state.messages {
        push_message_lines(&mut lines, msg.role, &msg.content, false);
        lines.push(Line::from(""));
    }

    if !state.streaming_content.is_empty() {
        push_message_lines(
            &mut lines,
            UiRole::Assistant,
            &state.streaming_content,
            true,
        );
    }

    lines
}

fn push_message_lines(
    lines: &mut Vec<Line<'static>>,
    role: UiRole,
    content: &str,
    streaming: bool,
) {
    let (bullet, bullet_color) = match role {
        UiRole::User => ("›", palette::ASH),
        UiRole::Assistant => ("●", palette::PHOSPHOR),
        UiRole::Tool => ("●", palette::FOG),
        UiRole::Error => ("●", palette::BLOOD),
    };

    let body = match role {
        UiRole::Assistant => styled_assistant_lines(content),
        _ => styled_content_lines(content, role),
    };

    let bullet_style = if role == UiRole::Tool {
        Style::default().fg(bullet_color).dim()
    } else {
        Style::default().fg(bullet_color).bold()
    };
    let bullet_span = Span::styled(format!("{bullet} "), bullet_style);

    let mut iter = body.into_iter();
    if let Some(first) = iter.next() {
        let mut spans = vec![bullet_span];
        spans.extend(first.spans);
        lines.push(Line::from(spans));
    } else {
        lines.push(Line::from(vec![bullet_span]));
    }

    for cl in iter {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(cl.spans);
        lines.push(Line::from(spans));
    }

    if streaming {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("…", Style::default().fg(palette::ASH).dim().italic()),
        ]));
    }
}

fn styled_content_lines(content: &str, role: UiRole) -> Vec<Line<'static>> {
    let color = match role {
        UiRole::User => palette::BONE,
        UiRole::Assistant => palette::BONE,
        UiRole::Tool => palette::FOG,
        UiRole::Error => palette::BLOOD,
    };

    let style = match role {
        UiRole::Tool => Style::default().fg(color).dim(),
        _ => Style::default().fg(color),
    };

    if content.is_empty() {
        return vec![Line::from("")];
    }

    content
        .lines()
        .map(|line| Line::styled(line.to_string(), style))
        .collect()
}

fn styled_assistant_lines(content: &str) -> Vec<Line<'static>> {
    if content.is_empty() {
        return vec![Line::from("")];
    }

    let mut out = Vec::<Line<'static>>::new();
    let mut in_code_block = false;
    let mut in_diff_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();

        if trimmed.starts_with("```") {
            if in_code_block {
                in_code_block = false;
                in_diff_block = false;
            } else {
                in_code_block = true;
                in_diff_block = trimmed.starts_with("```diff");
            }
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::STEEL).dim(),
            ));
            continue;
        }

        if in_diff_block && trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::PHOSPHOR),
            ));
            continue;
        }
        if in_diff_block && trimmed.starts_with('-') && !trimmed.starts_with("---") {
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::BLOOD),
            ));
            continue;
        }
        if in_diff_block
            && (trimmed.starts_with("@@")
                || trimmed.starts_with("+++")
                || trimmed.starts_with("---"))
        {
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::EMBER).bold(),
            ));
            continue;
        }
        if in_code_block {
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::PHOSPHOR),
            ));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("# ") {
            let mut spans = vec![Span::raw(leading.clone())];
            spans.extend(render_inline(
                rest,
                Style::default().fg(palette::ION).bold(),
            ));
            out.push(Line::from(spans));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let mut spans = vec![Span::raw(leading.clone())];
            spans.extend(render_inline(
                rest,
                Style::default().fg(palette::ION).bold(),
            ));
            out.push(Line::from(spans));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            let mut spans = vec![Span::raw(leading.clone())];
            spans.extend(render_inline(
                rest,
                Style::default().fg(palette::PLASMA).bold(),
            ));
            out.push(Line::from(spans));
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let mut spans = vec![
                Span::raw(leading.clone()),
                Span::styled("• ", Style::default().fg(palette::PLASMA)),
            ];
            spans.extend(render_inline(rest, Style::default().fg(palette::BONE)));
            out.push(Line::from(spans));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("> ") {
            let mut spans = vec![
                Span::raw(leading.clone()),
                Span::styled("│ ", Style::default().fg(palette::STEEL).dim()),
            ];
            spans.extend(render_inline(
                rest,
                Style::default().fg(palette::ASH).dim().italic(),
            ));
            out.push(Line::from(spans));
            continue;
        }

        let mut spans = vec![Span::raw(leading.clone())];
        spans.extend(render_inline(trimmed, Style::default().fg(palette::BONE)));
        out.push(Line::from(spans));
    }

    out
}

fn render_inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::<Span<'static>>::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut buf = String::new();

    let flush = |spans: &mut Vec<Span<'static>>, buf: &mut String| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), base));
        }
    };

    while i < bytes.len() {
        if bytes[i] == b'*'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'*'
            && let Some(end) = find_marker(text, i + 2, "**")
        {
            flush(&mut spans, &mut buf);
            spans.push(Span::styled(
                text[i + 2..end].to_string(),
                base.add_modifier(Modifier::BOLD),
            ));
            i = end + 2;
            continue;
        }
        if bytes[i] == b'`'
            && let Some(end) = find_marker(text, i + 1, "`")
        {
            flush(&mut spans, &mut buf);
            spans.push(Span::styled(
                text[i + 1..end].to_string(),
                Style::default().fg(palette::PLASMA),
            ));
            i = end + 1;
            continue;
        }
        if bytes[i] == b'*'
            && let Some(end) = find_marker(text, i + 1, "*")
        {
            flush(&mut spans, &mut buf);
            spans.push(Span::styled(
                text[i + 1..end].to_string(),
                base.add_modifier(Modifier::ITALIC),
            ));
            i = end + 1;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut spans, &mut buf);
    spans
}

fn find_marker(text: &str, start: usize, marker: &str) -> Option<usize> {
    text[start..].find(marker).map(|p| start + p)
}

fn transcript_line_count(state: &UiState, width: u16) -> u16 {
    Paragraph::new(build_transcript_lines(state))
        .wrap(Wrap { trim: false })
        .line_count(width)
        .min(u16::MAX as usize) as u16
}

fn message_visible_lines(total_height: u16) -> u16 {
    total_height.saturating_sub(1 + 3)
}

fn transcript_content_width(total_width: u16) -> u16 {
    total_width.saturating_sub(4).max(1)
}

fn max_scroll(line_count: u16, visible_lines: u16) -> u16 {
    line_count.saturating_sub(visible_lines)
}

fn completion_hint(state: &UiState) -> Option<(&'static str, &'static str)> {
    if state.is_streaming {
        return None;
    }

    let query = state.input.trim();
    if !query.starts_with('/') || query.contains(' ') {
        return None;
    }

    let active = state.active_completion()?;
    if !active.name.starts_with(query) {
        return None;
    }

    let suffix = &active.name[query.len()..];
    Some((suffix, active.description))
}

#[derive(Clone)]
struct SelectorEventSender {
    events: UnboundedSender<AgentEvent>,
    selector_id: u64,
}

impl SelectorEventSender {
    fn new(events: UnboundedSender<AgentEvent>, selector_id: u64) -> Self {
        Self {
            events,
            selector_id,
        }
    }

    fn send(
        &self,
        event: AgentEvent,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<AgentEvent>> {
        self.events.send(AgentEvent::Selector {
            id: self.selector_id,
            event: Box::new(event),
        })
    }
}

async fn copilot_device_flow(events: SelectorEventSender, agent: Arc<Mutex<Agent>>) -> Result<()> {
    let auth = copilot::authorize().await?;

    let _ = events.send(AgentEvent::ProviderStatus {
        provider: ProviderKind::Copilot,
        message: format!(
            "Open {} and enter code: {}. Waiting for authorization...",
            auth.verification_uri, auth.user_code
        ),
        error: false,
    });

    let deadline = oauth_deadline(auth.expires_in)?;
    let interval = std::time::Duration::from_secs(auth.interval.max(1));

    loop {
        tokio::time::sleep(interval).await;

        if std::time::Instant::now() > deadline {
            let _ = events.send(AgentEvent::ProviderStatus {
                provider: ProviderKind::Copilot,
                message: "Authorization timed out. Press c to try again.".to_string(),
                error: true,
            });
            let _ = events.send(AgentEvent::ModelList {
                provider: ProviderKind::Copilot,
                models: Vec::new(),
                error: Some("Authorization timed out. Press c to try again.".to_string()),
            });
            return Ok(());
        }

        match copilot::poll_authorization(&auth.device_code).await? {
            copilot::PollResult::Success(refresh_token) => {
                let (msg, mut provider_name, provider_keys) = {
                    let mut locked = agent.lock().await;
                    let msg = locked.connect_key(ProviderKind::Copilot, refresh_token);
                    let provider_name = locked.provider_name();
                    let provider_keys = locked.provider_keys_snapshot();
                    (msg, provider_name, provider_keys)
                };
                let model_msg = match Agent::fetch_provider_models_with_keys(
                    ProviderKind::Copilot,
                    &provider_keys,
                )
                .await
                {
                    Ok(models) if !models.is_empty() => {
                        let reconciliation = {
                            let mut locked = agent.lock().await;
                            let message =
                                locked.reconcile_current_model(ProviderKind::Copilot, &models);
                            provider_name = locked.provider_name();
                            message
                        };
                        let _ = events.send(AgentEvent::ModelList {
                            provider: ProviderKind::Copilot,
                            models: models.clone(),
                            error: None,
                        });
                        let preview = models
                            .iter()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let mut message = format!(
                            "Fetched {} Copilot models. Use /model to select one.\n{}",
                            models.len(),
                            preview
                        );
                        if let Some(reconciliation) = reconciliation {
                            message.push('\n');
                            message.push_str(&reconciliation);
                        }
                        message
                    }
                    Ok(_) => {
                        let _ = events.send(AgentEvent::ModelList {
                            provider: ProviderKind::Copilot,
                            models: Vec::new(),
                            error: None,
                        });
                        "Copilot connected, but no models were returned.".to_string()
                    }
                    Err(err) => {
                        let message = format!("Copilot connected, but model fetch failed: {}", err);
                        let _ = events.send(AgentEvent::ModelList {
                            provider: ProviderKind::Copilot,
                            models: Vec::new(),
                            error: Some(message.clone()),
                        });
                        message
                    }
                };

                let _ = events.send(AgentEvent::ProviderName(provider_name));
                let _ = events.send(AgentEvent::ProviderStatus {
                    provider: ProviderKind::Copilot,
                    message: format!("{}\n{}", msg, model_msg),
                    error: false,
                });
                return Ok(());
            }
            copilot::PollResult::Pending => continue,
            copilot::PollResult::Failed => {
                let _ = events.send(AgentEvent::ProviderStatus {
                    provider: ProviderKind::Copilot,
                    message: "Authorization denied or expired.".to_string(),
                    error: true,
                });
                let _ = events.send(AgentEvent::ModelList {
                    provider: ProviderKind::Copilot,
                    models: Vec::new(),
                    error: Some("Authorization denied or expired.".to_string()),
                });
                return Ok(());
            }
        }
    }
}

async fn codex_oauth_flow(events: SelectorEventSender, agent: Arc<Mutex<Agent>>) -> Result<()> {
    match codex_browser_flow(events.clone(), Arc::clone(&agent)).await {
        Ok(()) => Ok(()),
        Err(browser_err) => {
            let _ = events.send(AgentEvent::ProviderStatus {
                provider: ProviderKind::Codex,
                message: format!(
                    "Browser OAuth unavailable: {}. Falling back to device login...",
                    browser_err
                ),
                error: false,
            });
            codex_device_flow(events, agent).await
        }
    }
}

async fn codex_browser_flow(events: SelectorEventSender, agent: Arc<Mutex<Agent>>) -> Result<()> {
    let auth = codex::start_browser_auth()?;
    let state = auth.state.clone();
    let verifier = auth.verifier.clone();
    let redirect_uri = auth.redirect_uri.clone();

    let _ = events.send(AgentEvent::ProviderStatus {
        provider: ProviderKind::Codex,
        message: format!(
            "Opened browser for Codex OAuth. If it did not open, visit {}",
            auth.authorization_url
        ),
        error: false,
    });

    let code = codex::wait_for_browser_code(auth).await?;
    if code.state != state {
        return Err(anyhow::anyhow!("OAuth callback state mismatch"));
    }
    let credentials = codex::exchange_browser_code(&code.code, &verifier, &redirect_uri).await?;
    finish_codex_connection(events, agent, credentials).await
}

async fn codex_device_flow(events: SelectorEventSender, agent: Arc<Mutex<Agent>>) -> Result<()> {
    let auth = codex::authorize_device().await?;
    let device_url = auth
        .verification_uri_complete
        .as_ref()
        .unwrap_or(&auth.verification_uri);

    let _ = events.send(AgentEvent::ProviderStatus {
        provider: ProviderKind::Codex,
        message: format!(
            "Open {} and enter code: {}. Waiting for authorization...",
            device_url, auth.user_code
        ),
        error: false,
    });

    let deadline = oauth_deadline(auth.expires_in)?;
    let interval = std::time::Duration::from_secs(auth.interval.max(1));

    loop {
        tokio::time::sleep(interval).await;

        if std::time::Instant::now() > deadline {
            let message = "Authorization timed out. Press c to try again.".to_string();
            let _ = events.send(AgentEvent::ProviderStatus {
                provider: ProviderKind::Codex,
                message: message.clone(),
                error: true,
            });
            let _ = events.send(AgentEvent::ModelList {
                provider: ProviderKind::Codex,
                models: Vec::new(),
                error: Some(message),
            });
            return Ok(());
        }

        match codex::poll_device_authorization(&auth.device_code).await? {
            codex::DevicePollResult::Success(credentials) => {
                return finish_codex_connection(events, agent, credentials).await;
            }
            codex::DevicePollResult::Pending => continue,
            codex::DevicePollResult::Failed(reason) => {
                let message = format!("Authorization denied or expired: {}", reason);
                let _ = events.send(AgentEvent::ProviderStatus {
                    provider: ProviderKind::Codex,
                    message: message.clone(),
                    error: true,
                });
                let _ = events.send(AgentEvent::ModelList {
                    provider: ProviderKind::Codex,
                    models: Vec::new(),
                    error: Some(message),
                });
                return Ok(());
            }
        }
    }
}

async fn finish_codex_connection(
    events: SelectorEventSender,
    agent: Arc<Mutex<Agent>>,
    credentials: codex::CodexCredentials,
) -> Result<()> {
    let serialized = codex::serialize_credentials(&credentials)?;
    let (msg, mut provider_name, provider_keys) = {
        let mut locked = agent.lock().await;
        let msg = locked.connect_key(ProviderKind::Codex, serialized);
        let provider_name = locked.provider_name();
        let provider_keys = locked.provider_keys_snapshot();
        (msg, provider_name, provider_keys)
    };
    let model_msg =
        match Agent::fetch_provider_models_with_keys(ProviderKind::Codex, &provider_keys).await {
            Ok(models) if !models.is_empty() => {
                let reconciliation = {
                    let mut locked = agent.lock().await;
                    let message = locked.reconcile_current_model(ProviderKind::Codex, &models);
                    provider_name = locked.provider_name();
                    message
                };
                let _ = events.send(AgentEvent::ModelList {
                    provider: ProviderKind::Codex,
                    models: models.clone(),
                    error: None,
                });
                let preview = models
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut message = format!(
                    "Fetched {} Codex models. Use /model to select one.\n{}",
                    models.len(),
                    preview
                );
                if let Some(reconciliation) = reconciliation {
                    message.push('\n');
                    message.push_str(&reconciliation);
                }
                message
            }
            Ok(_) => {
                let _ = events.send(AgentEvent::ModelList {
                    provider: ProviderKind::Codex,
                    models: Vec::new(),
                    error: None,
                });
                "Codex connected, but no models were returned.".to_string()
            }
            Err(err) => {
                let message = format!("Codex connected, but model fetch failed: {}", err);
                let _ = events.send(AgentEvent::ModelList {
                    provider: ProviderKind::Codex,
                    models: Vec::new(),
                    error: Some(message.clone()),
                });
                message
            }
        };

    let _ = events.send(AgentEvent::ProviderName(provider_name));
    let _ = events.send(AgentEvent::ProviderStatus {
        provider: ProviderKind::Codex,
        message: format!("{}\n{}", msg, model_msg),
        error: false,
    });
    Ok(())
}

fn oauth_deadline(expires_in: u64) -> Result<std::time::Instant> {
    std::time::Instant::now()
        .checked_add(std::time::Duration::from_secs(expires_in))
        .ok_or_else(|| anyhow::anyhow!("OAuth expiry exceeds the supported clock range"))
}

#[cfg(test)]
#[path = "../tests/ui.rs"]
mod tests;
