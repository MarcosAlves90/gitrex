use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub fn dashboard(area: Rect) -> [Rect; 3] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(12),
            Constraint::Length(3),
        ])
        .split(area);
    [rows[0], rows[1], rows[2]]
}

pub fn graph_screen(area: Rect) -> [Rect; 3] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
    [rows[0], rows[1], rows[2]]
}

pub fn body(area: Rect) -> [Rect; 2] {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(16), Constraint::Length(72)])
        .split(area);
    [cols[0], cols[1]]
}

pub fn graph_workspace(area: Rect) -> [Rect; 2] {
    if area.width >= 104 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
            .split(area);
        [cols[0], cols[1]]
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        [rows[0], rows[1]]
    }
}

pub fn left_column(area: Rect) -> [Rect; 2] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(8)])
        .split(area);
    [rows[0], rows[1]]
}

pub fn branch_sections(area: Rect) -> [Rect; 3] {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Min(8),
        ])
        .split(area);
    [rows[0], rows[1], rows[2]]
}

pub fn loading_splash(area: Rect) -> [Rect; 2] {
    let area = centered_rect(84, 36, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    [rows[0], rows[2]]
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::{graph_screen, graph_workspace};
    use ratatui::layout::Rect;

    #[test]
    fn graph_screen_reclaims_header_space_for_history() {
        let [header, body, footer] = graph_screen(Rect::new(0, 0, 120, 30));
        assert_eq!(header.height, 3);
        assert_eq!(footer.height, 3);
        assert_eq!(body.height, 24);
    }

    #[test]
    fn graph_workspace_uses_side_details_on_wide_terminals() {
        let [graph, details] = graph_workspace(Rect::new(0, 0, 120, 30));
        assert_eq!(graph.y, details.y);
        assert!(details.x > graph.x);
        assert!(graph.width > details.width);
    }

    #[test]
    fn graph_workspace_stacks_details_on_narrow_terminals() {
        let [graph, details] = graph_workspace(Rect::new(0, 0, 80, 24));
        assert_eq!(graph.x, details.x);
        assert!(details.y > graph.y);
        assert!(graph.height > details.height);
    }
}
