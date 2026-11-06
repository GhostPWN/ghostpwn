use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::palette;

/// Five rows tall so the home screen fits a 24-row terminal with the status line and input.
const GHOST: &[&str] = &[
    "  ▄▄▄▄▄▄▄  ",
    " █░░░░░░░█ ",
    " █░█▀░█▀░█ ",
    " █░░░░░░░█ ",
    " █▄▀▄▀▄▀▄█ ",
];

const GAP: &str = "    ";
const COMMAND_COLUMN: usize = 14;

/// What the home screen shows before the first message.
pub struct HomeInfo<'a> {
    pub model: &'a str,
    pub workspace: &'a str,
    /// Slash commands to suggest, as `(name, description)`.
    pub commands: &'a [(&'a str, &'a str)],
}

pub fn render(frame: &mut Frame, area: Rect, info: &HomeInfo<'_>) {
    let lines = home_lines(info);
    let block_width = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let block_height = lines.len() as u16;
    // Center the block as a whole and keep its lines left-aligned, so the columns stay straight.
    let x = area.x + area.width.saturating_sub(block_width) / 2;
    let y = area.y + area.height.saturating_sub(block_height) / 2;
    let target = Rect {
        x,
        y,
        width: area.width.saturating_sub(x - area.x),
        height: area.height.saturating_sub(y - area.y),
    };
    frame.render_widget(Paragraph::new(lines), target);
}

fn home_lines(info: &HomeInfo<'_>) -> Vec<Line<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let ghost = Style::new().fg(palette::ACCENT);
    let side: [Vec<Span<'static>>; 5] = [
        vec![
            Span::styled("GhostPWN", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", env!("CARGO_PKG_VERSION")), dim),
        ],
        vec![Span::styled("Autonomous penetration testing agent", dim)],
        Vec::new(),
        vec![
            Span::styled("model      ", dim),
            Span::raw(info.model.to_string()),
        ],
        vec![
            Span::styled("workspace  ", dim),
            Span::raw(info.workspace.to_string()),
        ],
    ];

    let mut lines: Vec<Line<'static>> = GHOST
        .iter()
        .zip(side)
        .enumerate()
        .map(|(row, (art, text))| {
            let mut spans = if row == 2 {
                // The eyes use the terminal text color so they stand out from the violet body.
                let eyes = Style::new().add_modifier(Modifier::BOLD);
                vec![
                    Span::styled(" █░", ghost),
                    Span::styled("█▀", eyes),
                    Span::styled("░", ghost),
                    Span::styled("█▀", eyes),
                    Span::styled("░█ ", ghost),
                ]
            } else {
                vec![Span::styled(*art, ghost)]
            };
            if !text.is_empty() {
                spans.push(Span::raw(GAP));
                spans.extend(text);
            }
            Line::from(spans)
        })
        .collect();

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Start",
        Style::new().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (name, description) in info.commands {
        let pad = COMMAND_COLUMN.saturating_sub(name.chars().count()).max(2);
        lines.push(Line::from(vec![
            Span::styled(name.to_string(), Style::new().fg(palette::ACCENT)),
            Span::raw(" ".repeat(pad)),
            Span::raw(description.to_string()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Or type a target and a goal, for example: ", dim),
        Span::styled(
            "test the login form on :8080",
            dim.add_modifier(Modifier::ITALIC),
        ),
    ]));
    lines
}

#[cfg(test)]
pub(super) fn home_text(info: &HomeInfo<'_>) -> Vec<String> {
    home_lines(info)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}
