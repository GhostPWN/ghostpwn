use image::DynamicImage;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};

use super::palette;

/// The real GhostPWN logo, embedded so it ships inside the binary.
const LOGO_SVG: &[u8] = include_bytes!("../../logo.svg");

/// Persistent image protocol state for the home-screen logo.
///
/// Created once at startup. Holds an encoded, terminal-graphics protocol
/// (Kitty / iTerm2 / Sixel, or a unicode halfblocks fallback) that knows how
/// to re-encode itself when the render area is resized.
pub struct LogoImage {
    protocol: StatefulProtocol,
}

/// Detect terminal graphics support and rasterize the SVG logo into an image
/// protocol. Returns `None` when detection or rasterization fails, in which
/// case the caller falls back to the ASCII ghost.
///
/// Must be called while the terminal is in raw mode, because
/// [`Picker::from_query_stdio`] probes the terminal over stdin/stdout.
pub fn init() -> Option<LogoImage> {
    let picker = Picker::from_query_stdio().ok()?;
    let image = rasterize(LOGO_SVG)?;
    Some(LogoImage {
        protocol: picker.new_resize_protocol(image),
    })
}

/// Supersampling factor for SVG rasterization. The logo is rendered at this
/// multiple of its native resolution so it stays crisp when the terminal
/// graphics protocol scales it down to the render cell area.
const RASTER_SCALE: f32 = 4.0;

fn rasterize(svg: &[u8]) -> Option<DynamicImage> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let width = (size.width() * RASTER_SCALE).ceil() as u32;
    let height = (size.height() * RASTER_SCALE).ceil() as u32;
    if width == 0 || height == 0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(RASTER_SCALE, RASTER_SCALE),
        &mut pixmap.as_mut(),
    );

    let rgba = image::RgbaImage::from_raw(width, height, pixmap.take())?;
    Some(DynamicImage::ImageRgba8(rgba))
}

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

/// On-screen height (in terminal rows) of the rendered logo. Larger than the
/// ASCII `GHOST` fallback so the rasterized SVG reads bigger on the home screen.
const LOGO_ROWS: u16 = 20;

pub fn render(frame: &mut Frame, area: Rect, logo: Option<&mut LogoImage>) {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(LOGO_ROWS),
        Constraint::Length(1),
        Constraint::Length(TITLE.len() as u16),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .split(area);

    render_ghost(frame, vertical[1], logo);

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

/// Draw the ghost mascot: the rasterized SVG via a terminal-graphics protocol
/// when available, otherwise the ASCII fallback.
fn render_ghost(frame: &mut Frame, area: Rect, logo: Option<&mut LogoImage>) {
    if let Some(logo) = logo {
        let image_area = centered_square(area);
        frame.render_stateful_widget(StatefulImage::default(), image_area, &mut logo.protocol);
        return;
    }

    let ghost_lines: Vec<Line<'_>> = GHOST
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::new().fg(palette::PLASMA))))
        .collect();
    frame.render_widget(
        Paragraph::new(ghost_lines).alignment(Alignment::Center),
        area,
    );
}

/// Center a roughly square region inside `area`, sized to the height so the
/// logo keeps its proportions (terminal cells are ~2:1, so width = 2 * height).
fn centered_square(area: Rect) -> Rect {
    let width = (area.height * 2).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect {
        x,
        y: area.y,
        width,
        height: area.height,
    }
}

fn render_chips(frame: &mut Frame, area: Rect) {
    let chips: [(&str, &str); 3] = [
        ("/model", "models and provider auth"),
        ("c/d", "connect or disconnect"),
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
