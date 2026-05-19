use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub fn dashboard(area: Rect) -> [Rect; 4] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(3),
        ])
        .split(area);
    [rows[0], rows[1], rows[2], rows[3]]
}
