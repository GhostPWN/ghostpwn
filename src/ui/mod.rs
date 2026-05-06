mod logo;

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
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::agent::Agent;
use crate::config::ProviderKind;
use crate::models::AgentEvent;
use crate::providers::copilot;

pub(super) mod palette {
    use ratatui::style::Color;
    pub const PHOSPHOR: Color = Color::Rgb(120, 255, 170);
    pub const BONE: Color = Color::Rgb(232, 232, 220);
    pub const ASH: Color = Color::Rgb(118, 122, 140);
    pub const ION: Color = Color::Rgb(80, 230, 220);
    pub const PLASMA: Color = Color::Rgb(255, 80, 170);
    pub const EMBER: Color = Color::Rgb(255, 140, 70);
    pub const BLOOD: Color = Color::Rgb(255, 90, 100);
    pub const STEEL: Color = Color::Rgb(58, 62, 82);
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
        name: "/models",
        description: "List/switch provider models",
    },
    CommandSpec {
        name: "/connect",
        description: "Connect provider API key or /connect github",
    },
    CommandSpec {
        name: "/disconnect",
        description: "Disconnect provider API key",
    },
    CommandSpec {
        name: "/clear",
        description: "Clear current chat",
    },
    CommandSpec {
        name: "/exit",
        description: "Exit the app",
    },
];

#[derive(Debug, Clone)]
struct UiMessage {
    role: UiRole,
    content: String,
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

        let line_count = transcript_line_count(state);
        let visible_lines = message_visible_lines(terminal.size()?.height);
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

                    let line_count = transcript_line_count(state);
                    let visible_lines = message_visible_lines(terminal.size()?.height);

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
                    let line_count = transcript_line_count(state);
                    let visible_lines = message_visible_lines(terminal.size()?.height);

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

    if text == "/exit" {
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

    if text.starts_with("/models") {
        let parts = text.split_whitespace().collect::<Vec<&str>>();
        let response = if parts.len() == 1 {
            let locked = agent.lock().await;
            locked.list_models_overview()
        } else {
            let Some(provider) = ProviderKind::parse(parts[1]) else {
                state.push_message(
                    UiRole::Error,
                    "Invalid provider. Use: anthropic | openai | google | github | ollama"
                        .to_string(),
                );
                return;
            };

            if parts.len() == 2 {
                state.push_message(
                    UiRole::Assistant,
                    "Fetching available models...".to_string(),
                );
                let tx = event_tx.clone();
                let handle = Arc::clone(agent);
                tokio::spawn(async move {
                    let mut locked = handle.lock().await;
                    let response = locked.list_provider_models(provider).await;
                    let _ = tx.send(AgentEvent::AssistantDelta(response));
                    let _ = tx.send(AgentEvent::Done);
                });
                return;
            }

            let model = if parts.len() > 2 {
                Some(parts[2..].join(" "))
            } else {
                None
            };

            let mut locked = agent.lock().await;
            let response = locked.switch_model(provider, model);
            state.provider_name = locked.provider_name();
            response
        };

        state.push_message(UiRole::Assistant, response);
        return;
    }

    if text.starts_with("/disconnect") {
        let parts = text.split_whitespace().collect::<Vec<&str>>();
        if parts.len() != 2 {
            state.push_message(UiRole::Error, "Usage: /disconnect <provider>".to_string());
            return;
        }

        let Some(provider) = ProviderKind::parse(parts[1]) else {
            state.push_message(
                UiRole::Error,
                "Invalid provider. Use: anthropic | openai | google | github | ollama".to_string(),
            );
            return;
        };

        let mut locked = agent.lock().await;
        let response = locked.disconnect_key(provider);
        state.provider_name = locked.provider_name();
        state.push_message(UiRole::Assistant, response);
        return;
    }

    if text.starts_with("/connect") {
        let parts = text.split_whitespace().collect::<Vec<&str>>();
        if parts.len() == 1 {
            let locked = agent.lock().await;
            state.push_message(UiRole::Assistant, locked.connection_overview());
            return;
        }

        let Some(provider) = ProviderKind::parse(parts[1]) else {
            state.push_message(
                UiRole::Error,
                "Invalid provider. Use: anthropic | openai | google | github | ollama".to_string(),
            );
            return;
        };

        if provider == ProviderKind::Copilot
            && (parts.len() == 2
                || parts.get(2) == Some(&"oauth")
                || parts.get(2) == Some(&"device"))
        {
            state.push_message(
                UiRole::Assistant,
                "Starting GitHub Copilot OAuth...".to_string(),
            );
            state.is_streaming = true;
            state.streaming_content.clear();
            state.tool_status.clear();

            let tx = event_tx.clone();
            let handle = Arc::clone(agent);

            tokio::spawn(async move {
                match copilot_device_flow(tx.clone(), handle).await {
                    Ok(()) => {}
                    Err(err) => {
                        let _ = tx.send(AgentEvent::Error(format!(
                            "Copilot authorization failed: {}",
                            err
                        )));
                        let _ = tx.send(AgentEvent::Done);
                    }
                }
            });
            return;
        }

        if provider == ProviderKind::Ollama {
            let model = if parts.len() > 2 {
                parts[2..].join(" ")
            } else {
                "llama3".to_string()
            };
            let mut locked = agent.lock().await;
            let response = locked.switch_model(ProviderKind::Ollama, Some(model));
            state.provider_name = locked.provider_name();
            state.push_message(UiRole::Assistant, response);
            return;
        }

        if parts.len() < 3 {
            state.push_message(
                UiRole::Error,
                "Usage: /connect <provider> <api_key>\nFor GitHub, use: /connect github\nFor Ollama, use: /connect ollama [model]"
                    .to_string(),
            );
            return;
        }

        let api_key = parts[2..].join(" ");
        if api_key.len() < 8 {
            state.push_message(
                UiRole::Error,
                "API key looks too short. Usage: /connect <provider> <api_key>".to_string(),
            );
            return;
        }

        let mut locked = agent.lock().await;
        let response = locked.connect_key(provider, api_key);
        state.provider_name = locked.provider_name();
        state.push_message(UiRole::Assistant, response);
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
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_header(frame, root[0], state);
    render_transcript(frame, root[1], state);
    render_status(frame, root[2], state);
    render_input(frame, root[3], state);
}

fn render_header(frame: &mut Frame, area: Rect, state: &UiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::STEEL))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([
        Constraint::Length(16),
        Constraint::Min(1),
        Constraint::Length(20),
    ])
    .split(inner);

    let brand = Line::from(vec![
        Span::styled("◈ ", Style::default().fg(palette::PLASMA).bold()),
        Span::styled("GHOSTPWN", Style::default().fg(palette::PHOSPHOR).bold()),
        Span::styled(" v0.1", Style::default().fg(palette::STEEL)),
    ]);
    frame.render_widget(Paragraph::new(brand), cols[0]);

    let center = Line::from(vec![
        Span::styled(
            "autonomous web pentest",
            Style::default().fg(palette::ASH).italic(),
        ),
        Span::raw("   "),
        Span::styled("◂", Style::default().fg(palette::STEEL)),
        Span::raw(" "),
        Span::styled(
            state.provider_name.clone(),
            Style::default().fg(palette::ION).bold(),
        ),
        Span::raw(" "),
        Span::styled("▸", Style::default().fg(palette::STEEL)),
    ]);
    frame.render_widget(Paragraph::new(center).alignment(Alignment::Center), cols[1]);

    let (dot_color, label) = if state.is_streaming {
        (palette::EMBER, "TRANSMITTING")
    } else {
        (palette::PHOSPHOR, "READY")
    };
    let right = Line::from(vec![
        Span::styled("● ", Style::default().fg(dot_color).bold()),
        Span::styled(label, Style::default().fg(palette::BONE).bold()),
    ]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), cols[2]);
}

fn render_transcript(frame: &mut Frame, area: Rect, state: &UiState) {
    let is_home = state.messages.is_empty() && state.streaming_content.is_empty();

    let title = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("▸ ", Style::default().fg(palette::PLASMA)),
        Span::styled(
            if is_home { "home" } else { "transcript" },
            Style::default().fg(palette::BONE).bold(),
        ),
        Span::styled(" ─", Style::default().fg(palette::STEEL)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::STEEL))
        .title(title)
        .padding(Padding::new(2, 2, 1, 0));

    if is_home {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        logo::render(frame, inner);
        return;
    }

    let inner = block.inner(area);
    let lines = build_transcript_lines(state);
    let line_count = lines.len() as u16;
    let visible = inner.height;
    let max_scroll = line_count.saturating_sub(visible);
    let scroll = state.scroll_offset.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
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
    let (glyph, glyph_color, state_text) = if state.is_streaming {
        let text = if state.tool_status.is_empty() {
            "streaming response".to_string()
        } else {
            state.tool_status.clone()
        };
        (SPINNER[frame_idx], palette::EMBER, text)
    } else {
        ("◆", palette::PHOSPHOR, "ready".to_string())
    };

    let scroll_info = {
        let total = transcript_line_count(state);
        if state.auto_scroll || total == 0 {
            "◉ live".to_string()
        } else {
            let pct = ((state.scroll_offset as u32 + 1) * 100 / total as u32).min(100);
            format!("◯ {pct}%")
        }
    };

    let left = Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{glyph} "), Style::default().fg(glyph_color).bold()),
        Span::styled(state_text, Style::default().fg(palette::BONE)),
        Span::raw("   "),
        Span::styled("⌁ ", Style::default().fg(palette::STEEL)),
        Span::styled(scroll_info, Style::default().fg(palette::ASH)),
    ]);

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
    let (border_color, title_color) = if state.is_streaming {
        (palette::STEEL, palette::ASH)
    } else {
        (palette::PHOSPHOR, palette::PHOSPHOR)
    };

    let title = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("▸ ", Style::default().fg(palette::PLASMA)),
        Span::styled("prompt", Style::default().fg(title_color).bold()),
        Span::styled(" ─", Style::default().fg(palette::STEEL)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title)
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

fn build_transcript_lines(state: &UiState) -> Vec<Line<'static>> {
    let mut lines = Vec::<Line<'static>>::new();

    for msg in &state.messages {
        let (marker, label, accent) = role_chrome(msg.role);

        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(accent).bold()),
            Span::styled(label, Style::default().fg(accent).bold()),
        ]));

        let body = match msg.role {
            UiRole::Assistant => styled_assistant_lines(&msg.content),
            _ => styled_content_lines(&msg.content, msg.role),
        };

        for cl in body {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(accent))];
            spans.extend(cl.spans);
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
    }

    if !state.streaming_content.is_empty() {
        let idx = (state.tick / 2) as usize % SPINNER.len();
        lines.push(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(palette::ION).bold()),
            Span::styled("ghostpwn", Style::default().fg(palette::ION).bold()),
            Span::raw("  "),
            Span::styled("·", Style::default().fg(palette::STEEL)),
            Span::raw("  "),
            Span::styled(
                SPINNER[idx].to_string(),
                Style::default().fg(palette::EMBER).bold(),
            ),
            Span::raw(" "),
            Span::styled("streaming", Style::default().fg(palette::ASH).italic()),
        ]));
        for cl in styled_assistant_lines(&state.streaming_content) {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(palette::ION))];
            spans.extend(cl.spans);
            lines.push(Line::from(spans));
        }
    }

    lines
}

fn role_chrome(role: UiRole) -> (&'static str, &'static str, Color) {
    match role {
        UiRole::User => ("❯", "you", palette::PHOSPHOR),
        UiRole::Assistant => ("◆", "ghostpwn", palette::ION),
        UiRole::Tool => ("⚙", "tool", palette::PLASMA),
        UiRole::Error => ("✖", "error", palette::BLOOD),
    }
}

fn styled_content_lines(content: &str, role: UiRole) -> Vec<Line<'static>> {
    let color = match role {
        UiRole::User => palette::BONE,
        UiRole::Assistant => palette::BONE,
        UiRole::Tool => palette::BONE,
        UiRole::Error => palette::BLOOD,
    };

    if content.is_empty() {
        return vec![Line::from("")];
    }

    content
        .lines()
        .map(|line| Line::styled(line.to_string(), Style::default().fg(color)))
        .collect()
}

fn styled_assistant_lines(content: &str) -> Vec<Line<'static>> {
    if content.is_empty() {
        return vec![Line::from("")];
    }

    let mut out = Vec::<Line<'static>>::new();
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            out.push(Line::styled(
                line.to_string(),
                Style::default().fg(palette::EMBER).dim(),
            ));
            continue;
        }

        let style = if in_code_block {
            Style::default().fg(palette::PHOSPHOR)
        } else if trimmed.starts_with('#') {
            Style::default().fg(palette::ION).bold()
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            Style::default().fg(palette::BONE)
        } else if trimmed.starts_with('>') {
            Style::default().fg(palette::ASH).italic()
        } else {
            Style::default().fg(palette::BONE)
        };

        out.push(Line::styled(line.to_string(), style));
    }

    out
}

fn transcript_line_count(state: &UiState) -> u16 {
    build_transcript_lines(state).len() as u16
}

fn message_visible_lines(total_height: u16) -> u16 {
    total_height.saturating_sub(3 + 1 + 3 + 3)
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

    let _ = events.send(AgentEvent::AssistantDelta(format!(
        "Open {}  and enter code:  {}\nWaiting for authorization...",
        auth.verification_uri, auth.user_code
    )));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(auth.expires_in);
    let interval = std::time::Duration::from_secs(auth.interval);

    loop {
        tokio::time::sleep(interval).await;

        if std::time::Instant::now() > deadline {
            let _ = events.send(AgentEvent::Error(
                "Authorization timed out. Try /connect github again.".to_string(),
            ));
            let _ = events.send(AgentEvent::Done);
            return Ok(());
        }

        match copilot::poll_authorization(&auth.device_code).await? {
            copilot::PollResult::Success(refresh_token) => {
                let mut locked = agent.lock().await;
                let msg = locked.connect_key(ProviderKind::Copilot, refresh_token);
                let switch_msg = locked.switch_model(ProviderKind::Copilot, None);

                let _ = events.send(AgentEvent::AssistantDelta(format!(
                    "\n{}\n{}",
                    msg, switch_msg
                )));
                let _ = events.send(AgentEvent::Done);
                return Ok(());
            }
            copilot::PollResult::Pending => continue,
            copilot::PollResult::Failed => {
                let _ = events.send(AgentEvent::Error(
                    "Authorization denied or expired.".to_string(),
                ));
                let _ = events.send(AgentEvent::Done);
                return Ok(());
            }
        }
    }
}
