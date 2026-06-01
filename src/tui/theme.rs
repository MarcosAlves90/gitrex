use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Rgb(13, 17, 23);
pub const SURFACE: Color = Color::Rgb(22, 27, 34);
pub const SURFACE_ALT: Color = Color::Rgb(33, 38, 45);
pub const TEXT: Color = Color::Rgb(230, 237, 243);
pub const MUTED: Color = Color::Rgb(139, 148, 158);
pub const ACCENT: Color = Color::Rgb(88, 166, 255);
pub const SUCCESS: Color = Color::Rgb(63, 185, 80);
pub const WARNING: Color = Color::Rgb(210, 153, 34);
pub const ERROR: Color = Color::Rgb(248, 81, 73);
pub const PURPLE: Color = Color::Rgb(168, 85, 247);

pub fn panel_title_style(active: bool, accent: Color) -> Style {
    let mut style = Style::default().fg(if active { accent } else { MUTED });
    if active {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

pub fn panel_border_style(active: bool, accent: Color) -> Style {
    Style::default().fg(if active { accent } else { MUTED })
}

pub fn panel_surface_style(active: bool, background: Color) -> Style {
    Style::default()
        .fg(if active { TEXT } else { MUTED })
        .bg(background)
}

pub fn panel_highlight_style(active: bool, accent: Color) -> Style {
    Style::default()
        .fg(if active { BG } else { TEXT })
        .bg(if active { accent } else { SURFACE_ALT })
        .add_modifier(Modifier::BOLD)
}

pub fn panel_item_style(active: bool) -> Style {
    Style::default().fg(if active { TEXT } else { MUTED })
}
