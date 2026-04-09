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
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::agent::Agent;
use crate::config::ProviderKind;
use crate::models::AgentEvent;

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
        description: "Show current model",
    },
    CommandSpec {
        name: "/models",
        description: "List/switch provider models",
    },
    CommandSpec {
        name: "/connect",
        description: "Connect provider API key",
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
        name: "/quit",
        description: "Exit the app",
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

    if text == "/quit" || text == "/exit" {
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
        let provider_name = {
            let locked = agent.lock().await;
            locked.provider_name()
        };
        state.provider_name = provider_name.clone();
        state.push_message(UiRole::Assistant, format!("Provider: {}", provider_name));
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
                    "Invalid provider. Use: anthropic | openai | google".to_string(),
                );
                return;
            };

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
                "Invalid provider. Use: anthropic | openai | google".to_string(),
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

        if parts.len() < 3 {
            state.push_message(
                UiRole::Error,
                "Usage: /connect <provider> <api_key>".to_string(),
            );
            return;
        }

        let Some(provider) = ProviderKind::parse(parts[1]) else {
            state.push_message(
                UiRole::Error,
                "Invalid provider. Use: anthropic | openai | google".to_string(),
            );
            return;
        };

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
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let transcript = build_transcript_lines(state);
    let messages = if transcript.is_empty() {
        let home_block = Block::default().borders(Borders::ALL).title("GhostPWN");
        let home_inner = home_block.inner(root[0]);
        frame.render_widget(home_block, root[0]);
        logo::render(frame, home_inner);

        let footer = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(root[0]);
        let hint = Paragraph::new(Line::styled(
            "Type your prompt and press Enter | /help /models /connect /quit | Scroll: MouseWheel Up/Down PgUp/PgDn Home/End",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(hint, footer[1]);

        None
    } else {
        Some(
            Paragraph::new(transcript)
                .block(Block::default().borders(Borders::ALL).title("GhostPWN"))
                .scroll((state.scroll_offset, 0))
                .wrap(Wrap { trim: false }),
        )
    };

    if let Some(messages) = messages {
        frame.render_widget(messages, root[0]);
    }

    let status = if state.is_streaming {
        if state.tool_status.is_empty() {
            format!("{}  |  streaming...", state.provider_name)
        } else {
            format!("{}  |  {}", state.provider_name, state.tool_status)
        }
    } else {
        state.provider_name.clone()
    };

    let auto_label = if state.auto_scroll {
        " AUTO-SCROLL ON "
    } else {
        " AUTO-SCROLL OFF "
    };

    let auto_style = if state.auto_scroll {
        Style::default().fg(Color::Black).bg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Black).bg(Color::Yellow).bold()
    };

    let status_line = Line::from(vec![
        Span::styled(auto_label, auto_style),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(Color::Cyan)),
    ]);

    let status_widget =
        Paragraph::new(status_line).block(Block::default().borders(Borders::LEFT | Borders::RIGHT));
    frame.render_widget(status_widget, root[1]);

    let input_line = if state.is_streaming {
        Line::styled(
            "waiting for model response...",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        let mut spans = vec![
            Span::styled("> ", Style::default().fg(Color::Green).bold()),
            Span::styled(state.input.clone(), Style::default().fg(Color::White)),
        ];

        if let Some((suffix, description)) = completion_hint(state)
            && !suffix.is_empty()
        {
            spans.push(Span::styled(
                suffix.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("{} (Tab)", description),
                Style::default().fg(Color::DarkGray).italic(),
            ));
        }

        spans.push(Span::styled("_", Style::default().fg(Color::White)));
        Line::from(spans)
    };

    let input_widget = Paragraph::new(input_line)
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::ALL).title("Input"))
        .wrap(Wrap { trim: false });
    frame.render_widget(input_widget, root[2]);
}

fn build_transcript_lines(state: &UiState) -> Vec<Line<'static>> {
    let mut lines = Vec::<Line<'static>>::new();

    for msg in &state.messages {
        match msg.role {
            UiRole::User => {
                lines.push(Line::styled(
                    "You",
                    Style::default().fg(Color::Green).bold(),
                ));
                lines.extend(styled_content_lines(&msg.content, UiRole::User));
            }
            UiRole::Assistant => {
                lines.push(Line::styled(
                    "GhostPWN",
                    Style::default().fg(Color::Cyan).bold(),
                ));
                lines.extend(styled_assistant_lines(&msg.content));
            }
            UiRole::Tool => {
                lines.push(Line::styled(
                    "Tool",
                    Style::default().fg(Color::Magenta).bold(),
                ));
                lines.extend(styled_content_lines(&msg.content, UiRole::Tool));
            }
            UiRole::Error => {
                lines.push(Line::styled(
                    "Error",
                    Style::default().fg(Color::Red).bold(),
                ));
                lines.extend(styled_content_lines(&msg.content, UiRole::Error));
            }
        }

        lines.push(Line::from(""));
    }

    if !state.streaming_content.is_empty() {
        lines.push(Line::styled(
            "GhostPWN (streaming)",
            Style::default().fg(Color::Cyan).bold(),
        ));
        lines.extend(styled_assistant_lines(&state.streaming_content));
    }

    lines
}

fn styled_content_lines(content: &str, role: UiRole) -> Vec<Line<'static>> {
    let color = match role {
        UiRole::User => Color::White,
        UiRole::Assistant => Color::White,
        UiRole::Tool => Color::LightMagenta,
        UiRole::Error => Color::LightRed,
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
                Style::default().fg(Color::Yellow).bold(),
            ));
            continue;
        }

        let style = if in_code_block {
            Style::default().fg(Color::LightYellow)
        } else if trimmed.starts_with('#') {
            Style::default().fg(Color::LightCyan).bold()
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            Style::default().fg(Color::White)
        } else if trimmed.starts_with('>') {
            Style::default().fg(Color::DarkGray).italic()
        } else {
            Style::default().fg(Color::White)
        };

        out.push(Line::styled(line.to_string(), style));
    }

    out
}

fn transcript_line_count(state: &UiState) -> u16 {
    build_transcript_lines(state).len() as u16
}

fn message_visible_lines(total_height: u16) -> u16 {
    total_height.saturating_sub(1 + 3 + 2)
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
