use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::{
    app::{BranchPanel, View},
    theme,
};

pub fn loading_splash_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("⠀⠀⠀⣴⣀⣤⣦⣤⣤⣀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"),
        Line::from("⠀⢠⣴⠿⠛⣿⣿⠋⠻⣿⣟⠻⠿⠿⢿⣿⣿⣶⣶⡦⣤⣀⡀⠀"),
        Line::from("⢰⣿⣧⣴⣦⢿⣿⣷⡦⠘⣿⠀⠀⠀⠀⣹⠉⣿⣿⣿⣶⣬⣷⠀"),
        Line::from("⠘⠟⢻⣿⠋⠀⢿⣿⣷⣼⣿⣷⣤⣤⣴⣿⣿⣿⣿⣿⣿⣿⢿⠃"),
        Line::from("⠀⠀⢠⣿⣶⣶⣿⣿⣿⣿⠟⠉⠉⠙⠻⠟⡿⢻⢿⢻⡏⠏⠀⠀"),
        Line::from("⠀⣾⣿⣿⣿⣿⣿⣿⣿⣧⣤⣀⡀⠀⠀⠀⠁⠈⠘⠈⠀⠀⠀⠀"),
        Line::from("⠀⠈⠉⠳⣾⣿⣿⣿⣿⣿⣿⣿⣿⣦⣶⣄⢠⢰⣴⢠⠀⣄⠀⠀"),
        Line::from("⠀⠀⠀⠀⠈⠙⠿⣿⣝⣿⣿⣿⣿⣿⣿⣿⣿⣾⣿⣿⣷⡟⠀⠀"),
        Line::from("⠀⠀⠀⠀⠀⠀⠀⠈⠙⠛⠛⠛⠛⠛⠛⠛⠛⠻⠿⠟⠋⠀⠀⠀"),
    ]
}

pub fn loading_splash_text(frame: usize) -> String {
    let base = "Synchronizing repository";
    let tail = match frame % 4 {
        0 => "   ",
        1 => ".  ",
        2 => ".. ",
        _ => "...",
    };
    format!("{base}{tail}")
}

pub fn mode_label(view: View) -> &'static str {
    match view {
        View::Branches => "branches",
        View::Log => "graph",
        View::Help => "help",
    }
}

fn key_style(color: ratatui::style::Color) -> Style {
    Style::default()
        .fg(theme::BG)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

fn keycap(label: &str, color: ratatui::style::Color) -> Span<'static> {
    Span::styled(format!(" {label} "), key_style(color))
}

fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD),
    ))
}

fn bullet(key: &str, color: ratatui::style::Color, text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("- "),
        keycap(key, color),
        Span::raw(" "),
        Span::raw(text.to_string()),
    ])
}

fn example(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  example: "),
        Span::styled(
            text.to_string(),
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
    ])
}

pub fn help_lines(
    selected_branch: Option<&str>,
    sync_target: Option<&str>,
    panel: BranchPanel,
) -> Vec<Line<'static>> {
    let branch = selected_branch.unwrap_or("no branch selected");
    let sync = sync_target.unwrap_or("no upstream");
    let branch_action = match panel {
        BranchPanel::Local => "Enter = open local branch actions",
        BranchPanel::Remote => "Enter = open remote branch actions",
    };
    vec![
        Line::from(vec![
            keycap("h", theme::WARNING),
            Span::raw(" toggles help, "),
            keycap("q", theme::WARNING),
            Span::raw(" quits, "),
            keycap("r", theme::ACCENT),
            Span::raw(" refreshes, "),
            keycap("1", theme::TEAL),
            Span::raw("/"),
            keycap("2", theme::TEAL),
            Span::raw(" switch Branches/Graph"),
        ]),
        Line::from(vec![
            Span::raw("Current branch: "),
            Span::styled(
                branch.to_string(),
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  Sync target: "),
            Span::styled(sync.to_string(), Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::raw("Active branch panel: "),
            Span::styled(
                panel.label().to_string(),
                Style::default()
                    .fg(theme::SUCCESS)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        section("Status panel"),
        bullet(
            "info",
            theme::STATUS,
            "informational only: branch, upstream, divergence, and working tree changes",
        ),
        example("main is ahead of origin/main, with 3 files changed."),
        Line::from(""),
        section("Branches panel"),
        bullet(
            "j/k",
            theme::ACCENT,
            "or arrows move; Tab / Shift+Tab switch local/remote; / searches",
        ),
        bullet("Enter", theme::SUCCESS, branch_action),
        example("use /feature to filter refs, then Enter on a branch to act on it."),
        Line::from(""),
        section("Local branch actions"),
        bullet(
            "Enter",
            theme::SUCCESS,
            &format!("checkout, switch, pull, push, create branch from source ({branch})"),
        ),
        bullet(
            "delete",
            theme::ERROR,
            "opens a warning dialog before deleting the local branch",
        ),
        example("press Enter on feature/login to switch or create a branch from it."),
        Line::from(""),
        section("Remote branch actions"),
        bullet(
            "Enter",
            theme::SUCCESS,
            "create local branch, checkout detached HEAD, or delete the remote branch",
        ),
        bullet(
            "delete",
            theme::ERROR,
            "opens a warning dialog before deleting the remote branch",
        ),
        example("press Enter on origin/main to create a local branch from the remote ref."),
        Line::from(""),
        section("Git Graph workspace"),
        bullet(
            "j/k",
            theme::ACCENT,
            "or up/down move one commit; PgUp/PgDn move one visible page",
        ),
        bullet(
            "Home/End",
            theme::PURPLE,
            "or g/G jump to the first or last commit",
        ),
        bullet(
            "left/right",
            theme::TEAL,
            "pan long selected commit subjects without automatic scrolling",
        ),
        bullet(
            "Enter",
            theme::SUCCESS,
            "checkout commit or create branch from commit",
        ),
        example("press 2 for the full graph workspace; details follow the selected commit."),
        Line::from(""),
        section("Cleanup modal"),
        bullet(
            "space",
            theme::SUCCESS,
            "toggle a merged branch; a selects all; n clears; Enter confirms",
        ),
        bullet(
            "Enter",
            theme::SUCCESS,
            "deletion is confirmed in a warning dialog",
        ),
        example("select merged branches with space, then Enter to delete them."),
    ]
}

pub fn wrapped_height(lines: &[Line<'_>], width: usize) -> usize {
    let width = width.max(1);
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        help_lines, loading_splash_lines, loading_splash_text, mode_label, wrapped_height,
    };
    use crate::tui::app::{BranchPanel, View};
    use ratatui::text::Line;

    fn flatten(lines: Vec<Line<'static>>) -> String {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn mode_label_matches_view_names() {
        assert_eq!(mode_label(View::Branches), "branches");
        assert_eq!(mode_label(View::Log), "graph");
        assert_eq!(mode_label(View::Help), "help");
    }

    #[test]
    fn help_lines_mentions_selected_branch() {
        let copy = flatten(help_lines(
            Some("feature/login"),
            Some("origin/feature/login"),
            BranchPanel::Local,
        ));
        assert!(copy.contains("feature/login"));
        assert!(copy.contains("toggles help"));
        assert!(copy.contains("switch Branches/Graph"));
        assert!(copy.contains("informational only"));
        assert!(copy.contains("Branches panel"));
        assert!(copy.contains("Git Graph workspace"));
        assert!(copy.contains("PgUp/PgDn"));
        assert!(copy.contains("left/right"));
        assert!(copy.contains("origin/feature/login"));
        assert!(copy.contains("example:"));
        assert!(!copy.contains("Close help with h or Esc."));
    }

    #[test]
    fn wrapped_height_grows_with_narrow_widths() {
        let lines = vec![Line::from("hello world"), Line::from("")];

        assert_eq!(wrapped_height(&lines, 20), 2);
        assert_eq!(wrapped_height(&lines, 5), 4);
    }

    #[test]
    fn loading_splash_contains_the_requested_ascii_art() {
        let flattened = flatten(loading_splash_lines());
        assert!(flattened.contains("⣴⣀⣤⣦⣤⣤⣀⡀"));
        assert!(!flattened.contains("Synchronizing repository"));
    }

    #[test]
    fn loading_splash_text_animates_with_dots() {
        assert_eq!(loading_splash_text(0), "Synchronizing repository   ");
        assert_eq!(loading_splash_text(1), "Synchronizing repository.  ");
        assert_eq!(loading_splash_text(2), "Synchronizing repository.. ");
        assert_eq!(loading_splash_text(3), "Synchronizing repository...");
    }
}
