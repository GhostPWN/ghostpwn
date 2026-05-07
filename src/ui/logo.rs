use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::palette;

const GHOST: &[&str] = &[
    "            ██████████████████            ",
    "          ██░░░░░░░░░░░░░░░░░░██          ",
    "        ██░░░░░░░░░░░░░░░░░░░░░░██        ",
    "      ██░░░░░░░░░░░░░░░░░░░░░░░░░░██      ",
    "      ██░░░░░░░░░░░░░░░░░░░░░░░░░░██      ",
    "      ██░░░░░░████░░░░░░████░░░░░░██      ",
    "      ██░░░░░░█▓▓█░░░░░░█▓▓█░░░░░░██      ",
    "      ██░░░░░░████░░░░░░████░░░░░░██      ",
    "      ██░░░░░░░░░░░░░░░░░░░░░░░░░░██      ",
    "      ██░░░░░░░░░░░░░░░░░░░░░░░░░░██      ",
    "      ██░░░░░░░░▒▒░░░░░░▒▒░░░░░░░░██      ",
    "      ██░░░░░░░░░░▒▒▒▒▒▒░░░░░░░░░░██      ",
    "      ██░░░░░░░░░░░░░░░░░░░░░░░░░░██      ",
    "      ██░░██  ██░░██  ██░░██  ██░░██      ",
    "      ██  ██  ██  ██  ██  ██  ██  ██      ",
];

const TITLE: &[&str] = &[
    "  ▄████  ██░ ██  ▒█████    ██████ ▄▄▄█████▓ ██▓██   █     █░ ███▄    █ ",
    " ██▒ ▀█▒▓██░ ██▒▒██▒  ██▒▒██    ▒ ▓  ██▒ ▓▒▓██▒▓█░ █ ░█░ ██ ▀█   █ ",
    "▒██░▄▄▄░▒██▀▀██░▒██░  ██▒░ ▓██▄   ▒ ▓██░ ▒░▒██▒▒█░ █ ░█ ▓██  ▀█ ██▒",
    "░▓█  ██▓░▓█ ░██ ▒██   ██░  ▒   ██▒░ ▓██▓ ░ ░██░░█░ █ ░█ ▓██▒  ▐▌██▒",
    "░▒▓███▀▒░▓█▒░██▓░ ████▓▒░▒██████▒▒  ▒██▒ ░ ░██░░░██▒██▓ ▒██░   ▓██░",
    " ░▒   ▒  ▒ ░░▒░▒░ ▒░▒░▒░ ▒ ▒▓▒ ▒ ░  ▒ ░░   ░▓  ░ ▓░▒ ▒  ░ ▒░   ▒ ▒ ",
    "  ░   ░  ▒ ░▒░ ░  ░ ▒ ▒░ ░ ░▒  ░ ░    ░     ▒ ░  ▒ ░ ░  ░ ░░   ░ ▒░",
    "░ ░   ░  ░  ░░ ░░ ░ ░ ▒  ░  ░  ░    ░       ▒ ░  ░   ░     ░   ░ ░ ",
    "      ░  ░  ░  ░    ░ ░        ░            ░      ░             ░ ",
];

pub fn render(frame: &mut Frame, area: Rect) {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(GHOST.len() as u16),
        Constraint::Length(1),
        Constraint::Length(TITLE.len() as u16),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Fill(2),
    ])
    .split(area);

    let ghost_lines: Vec<Line<'_>> = GHOST
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::new().fg(palette::PLASMA))))
        .collect();
    frame.render_widget(
        Paragraph::new(ghost_lines).alignment(Alignment::Center),
        vertical[1],
    );

    let title_lines: Vec<Line<'_>> = TITLE
        .iter()
        .map(|line| {
            Line::from(Span::styled(
                *line,
                Style::new()
                    .fg(palette::PHOSPHOR)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(title_lines).alignment(Alignment::Center),
        vertical[3],
    );

    let tagline = Line::from(vec![
        Span::styled("◂ ", Style::new().fg(palette::STEEL)),
        Span::styled(
            "autonomous",
            Style::new().fg(palette::ION).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::new().fg(palette::STEEL)),
        Span::styled(
            "web penetration",
            Style::new().fg(palette::ION).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::new().fg(palette::STEEL)),
        Span::styled(
            "research lab",
            Style::new().fg(palette::ION).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ▸", Style::new().fg(palette::STEEL)),
    ]);
    frame.render_widget(
        Paragraph::new(tagline).alignment(Alignment::Center),
        vertical[5],
    );

    render_chips(frame, vertical[7]);
}

fn render_chips(frame: &mut Frame, area: Rect) {
    let chips: [(&str, &str); 3] = [
        ("/copilot", "connect github copilot"),
        ("/model", "browse available models"),
        ("/help", "see every command"),
    ];

    let mut spans: Vec<Span<'_>> = Vec::new();
    for (i, (cmd, desc)) in chips.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", Style::new().fg(palette::STEEL)));
        }
        spans.push(Span::styled("[ ", Style::new().fg(palette::STEEL)));
        spans.push(Span::styled(
            *cmd,
            Style::new()
                .fg(palette::PHOSPHOR)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("  ", Style::new()));
        spans.push(Span::styled(*desc, Style::new().fg(palette::ASH)));
        spans.push(Span::styled(" ]", Style::new().fg(palette::STEEL)));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}
