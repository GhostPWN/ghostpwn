mod logo;

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
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
use unicode_width::UnicodeWidthChar;

use crate::agent::Agent;
use crate::config::ProviderKind;
use crate::models::AgentEvent;
use crate::providers::copilot;

pub(super) mod palette {
    use ratatui::style::Color;
    pub const PHOSPHOR: Color = Color::Rgb(190, 150, 255);
    pub const BONE: Color = Color::Rgb(240, 235, 250);
    pub const ASH: Color = Color::Rgb(140, 130, 165);
    pub const ION: Color = Color::Rgb(160, 115, 235);
    pub const PLASMA: Color = Color::Rgb(215, 140, 255);
    pub const EMBER: Color = Color::Rgb(225, 175, 255);
    pub const BLOOD: Color = Color::Rgb(255, 110, 180);
    pub const STEEL: Color = Color::Rgb(70, 55, 95);
    pub const FOG: Color = Color::Rgb(60, 58, 70);
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
        name: "/clear",
        description: "Clear current chat",
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
    providers: Vec<ProviderKind>,
    provider_index: usize,
    provider_states: HashMap<ProviderKind, ModelSelectorProviderState>,
    mode: ModelSelectorMode,
    status: Option<ModelSelectorStatus>,
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

struct UiState {
    provider_name: String,
    input: String,
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
    tick: u64,
}

impl UiState {
    fn new(provider_name: String) -> Self {
        Self {
            provider_name,
            input: String::new(),
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

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (event_tx, mut event_rx) = unbounded_channel::<AgentEvent>();
    let mut state = UiState::new(provider_name);

    let run_result = ui_loop(&mut terminal, &agent, &event_tx, &mut event_rx, &mut state).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    run_result
}

async fn ui_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    event_rx: &mut UnboundedReceiver<AgentEvent>,
    state: &mut UiState,
) -> Result<()> {
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

                    if state.selector.is_some() {
                        handle_selector_key(key.code, state, agent, event_tx).await;
                        continue;
                    }

                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    let size = terminal.size()?;
                    let line_count =
                        transcript_line_count(state, transcript_content_width(size.width));
                    let visible_lines = message_visible_lines(size.height);

                    match key.code {
                        KeyCode::Enter => {
                            let text = state.input.trim().to_string();
                            state.input.clear();
                            state.refresh_completions();
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
                        KeyCode::Char(ch) => {
                            if !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::ALT)
                            {
                                state.input.push(ch);
                                state.refresh_completions();
                            }
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
    if text.is_empty() || state.is_streaming {
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

        let mut locked = agent.lock().await;
        locked.clear_history();
        return;
    }

    if text == "/model" {
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
        return;
    }

    if text.starts_with('/') {
        state.push_message(
            UiRole::Error,
            format!("Unknown command: {} (try /help)", text),
        );
        return;
    }

    state.push_message(UiRole::User, text.clone());
    state.is_streaming = true;
    state.streaming_content.clear();
    state.tool_status.clear();

    let tx = event_tx.clone();
    let handle = Arc::clone(agent);

    tokio::spawn(async move {
        let mut locked = handle.lock().await;
        if let Err(err) = locked.handle_user_input(text, tx.clone()).await {
            let _ = tx.send(AgentEvent::Error(err.to_string()));
            let _ = tx.send(AgentEvent::Done);
        }
    });
}

async fn open_model_selector(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
) {
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
        providers,
        provider_index,
        provider_states,
        mode: ModelSelectorMode::Browse,
        status: None,
    });
    fetch_initial_selector_models(state, agent, event_tx, current_provider);
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

    let tx = event_tx.clone();
    let handle = Arc::clone(agent);
    tokio::spawn(async move {
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
}

fn fetch_initial_selector_models(
    state: &mut UiState,
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    current_provider: ProviderKind,
) {
    for provider in [
        ProviderKind::Anthropic,
        ProviderKind::OpenAi,
        ProviderKind::Google,
    ] {
        fetch_selector_models_if_needed(state, agent, event_tx, provider);
    }
    fetch_selector_models_if_needed(state, agent, event_tx, current_provider);
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
    fetch_selector_models(agent, event_tx, provider);
}

fn fetch_selector_models(
    agent: &Arc<Mutex<Agent>>,
    event_tx: &UnboundedSender<AgentEvent>,
    provider: ProviderKind,
) {
    let tx = event_tx.clone();
    let handle = Arc::clone(agent);
    tokio::spawn(async move {
        let provider_keys = {
            let locked = handle.lock().await;
            locked.provider_keys_snapshot()
        };
        match Agent::fetch_provider_models_with_keys(provider, &provider_keys).await {
            Ok(models) => {
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
        AgentEvent::AssistantDelta(delta) => {
            state.streaming_content.push_str(&delta);
            state.tool_status.clear();
        }
        AgentEvent::ToolCall { name, args_summary } => {
            state.flush_streaming_to_messages();
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
            state.flush_streaming_to_messages();
            state.tool_status.clear();
            state.is_streaming = false;
        }
    }
}

fn render(frame: &mut Frame, state: &UiState) {
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
            Span::styled(" ─", Style::default().fg(palette::STEEL)),
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
                Style::default().fg(palette::ASH)
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
                Span::styled("key  ", Style::default().fg(palette::ASH)),
                Span::styled(masked, Style::default().fg(palette::PHOSPHOR)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "enter saves · esc cancels",
                Style::default().fg(palette::ASH),
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
            ProviderKind::Copilot => "press c to connect with GitHub device OAuth",
            ProviderKind::Ollama => "local provider; no API key required",
            _ => "press c to paste an API key",
        };
        vec![
            Line::from(vec![Span::styled(
                error.clone(),
                Style::default().fg(palette::BLOOD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(hint, Style::default().fg(palette::ASH))]),
        ]
    } else if selected_state.is_none_or(|provider_state| provider_state.models.is_empty()) {
        vec![Line::from(vec![Span::styled(
            "no models returned",
            Style::default().fg(palette::ASH),
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
                            Style::default().fg(palette::ASH)
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
        Span::styled(" provider  ", Style::default().fg(palette::ASH)),
        Span::styled("↑/↓", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" model  ", Style::default().fg(palette::ASH)),
        Span::styled("pgup/pgdn", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" jump  ", Style::default().fg(palette::ASH)),
        Span::styled("c", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" connect  ", Style::default().fg(palette::ASH)),
        Span::styled("d", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" disconnect  ", Style::default().fg(palette::ASH)),
        Span::styled("enter", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" switch  ", Style::default().fg(palette::ASH)),
        Span::styled("esc", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" close", Style::default().fg(palette::ASH)),
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
    let mut lines = status
        .message
        .lines()
        .map(|line| {
            Line::from(vec![Span::styled(
                line.to_string(),
                Style::default().fg(color),
            )])
        })
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

fn render_transcript(frame: &mut Frame, area: Rect, state: &UiState) {
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
    let lines = build_transcript_lines(state, inner.width);
    let line_count = lines.len() as u16;
    let visible = inner.height;
    let max_scroll = line_count.saturating_sub(visible);
    let scroll = state.scroll_offset.min(max_scroll);

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
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
            .set_style(Style::default().fg(palette::STEEL));
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

    let left = Line::from(left_spans);

    let right = Line::from(vec![
        Span::styled("tab", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" complete ", Style::default().fg(palette::ASH)),
        Span::styled("·", Style::default().fg(palette::STEEL)),
        Span::styled(" ↵", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" send ", Style::default().fg(palette::ASH)),
        Span::styled("·", Style::default().fg(palette::STEEL)),
        Span::styled(" ^c", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" quit ", Style::default().fg(palette::ASH)),
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));

    let line = if state.is_streaming {
        let idx = (state.tick / 2) as usize % SPINNER.len();
        Line::from(vec![
            Span::styled(
                format!("{} ", SPINNER[idx]),
                Style::default().fg(palette::EMBER).bold(),
            ),
            Span::styled(
                "channel open · awaiting model",
                Style::default().fg(palette::ASH).italic(),
            ),
        ])
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
                Style::default().fg(palette::STEEL),
            ));
            spans.push(Span::raw("    "));
            spans.push(Span::styled(
                format!("↹ {description}"),
                Style::default().fg(palette::ASH).italic(),
            ));
        } else if state.input.is_empty() {
            spans.push(Span::styled("  try  ", Style::default().fg(palette::STEEL)));
            spans.push(Span::styled("/help", Style::default().fg(palette::ASH)));
            spans.push(Span::styled(
                "  or describe a target…",
                Style::default().fg(palette::STEEL).italic(),
            ));
        }

        Line::from(spans)
    };

    let para = Paragraph::new(line).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn build_transcript_lines(state: &UiState, width: u16) -> Vec<Line<'static>> {
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

    wrap_transcript_lines(lines, width)
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
        Style::default().fg(bullet_color)
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
            Span::styled("…", Style::default().fg(palette::ASH).italic()),
        ]));
    }
}

fn wrap_transcript_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    if width == 0 {
        return lines;
    }

    lines
        .into_iter()
        .flat_map(|line| wrap_line(line, width))
        .collect()
}

fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line];
    }

    let mut rows = Vec::<Line<'static>>::new();
    let mut current = Vec::<Span<'static>>::new();
    let mut pending = String::new();
    let mut pending_style = Style::default();
    let mut col = 0usize;

    let flush_pending =
        |current: &mut Vec<Span<'static>>, pending: &mut String, pending_style: Style| {
            if !pending.is_empty() {
                current.push(Span::styled(std::mem::take(pending), pending_style));
            }
        };

    let push_row = |rows: &mut Vec<Line<'static>>,
                    current: &mut Vec<Span<'static>>,
                    pending: &mut String,
                    pending_style: Style| {
        flush_pending(current, pending, pending_style);
        rows.push(Line::from(std::mem::take(current)));
    };

    for span in line.spans {
        let style = span.style;
        for ch in span.content.chars() {
            let char_width = ch.width().unwrap_or(0);
            if col > 0 && col.saturating_add(char_width) > width {
                push_row(&mut rows, &mut current, &mut pending, pending_style);
                col = 0;
            }

            if pending.is_empty() {
                pending_style = style;
            } else if pending_style != style {
                flush_pending(&mut current, &mut pending, pending_style);
                pending_style = style;
            }

            pending.push(ch);
            col += char_width;
        }
    }

    flush_pending(&mut current, &mut pending, pending_style);
    if current.is_empty() && rows.is_empty() {
        rows.push(Line::from(""));
    } else if !current.is_empty() {
        rows.push(Line::from(current));
    }

    rows
}

fn styled_content_lines(content: &str, role: UiRole) -> Vec<Line<'static>> {
    let color = match role {
        UiRole::User => palette::BONE,
        UiRole::Assistant => palette::BONE,
        UiRole::Tool => palette::FOG,
        UiRole::Error => palette::BLOOD,
    };

    let style = Style::default().fg(color);

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
                Span::styled("│ ", Style::default().fg(palette::STEEL)),
            ];
            spans.extend(render_inline(
                rest,
                Style::default().fg(palette::ASH).italic(),
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
    build_transcript_lines(state, width).len() as u16
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

async fn copilot_device_flow(
    events: UnboundedSender<AgentEvent>,
    agent: Arc<Mutex<Agent>>,
) -> Result<()> {
    let auth = copilot::authorize().await?;

    let _ = events.send(AgentEvent::ProviderStatus {
        provider: ProviderKind::Copilot,
        message: format!(
            "Open {} and enter code: {}. Waiting for authorization...",
            auth.verification_uri, auth.user_code
        ),
        error: false,
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(auth.expires_in);
    let interval = std::time::Duration::from_secs(auth.interval);

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
                let mut locked = agent.lock().await;
                let msg = locked.connect_key(ProviderKind::Copilot, refresh_token);
                let provider_name = locked.provider_name();
                let model_msg = match locked.fetch_provider_models(ProviderKind::Copilot).await {
                    Ok(models) if !models.is_empty() => {
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
                        format!(
                            "Fetched {} Copilot models. Use /model to select one.\n{}",
                            models.len(),
                            preview
                        )
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
