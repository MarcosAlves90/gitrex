use crate::domain::{BranchInfo, CommitSummary, GraphLine, RepoStatus, StatusEntry};
use ratatui::prelude::{Line, Modifier, Span, Style};

pub fn print_status(status: &RepoStatus) {
    println!("branch: {}", status.branch_name);
    if let Some(upstream) = &status.upstream {
        println!("upstream: {upstream}");
    }
    if status.ahead > 0 || status.behind > 0 {
        println!("divergence: +{} -{}", status.ahead, status.behind);
    }
    if status.files.is_empty() {
        println!("working tree: clean");
        return;
    }
    println!("working tree:");
    for file in &status.files {
        println!("  {} {}", file.code, file.path);
    }
}

pub fn print_branches(branches: &[BranchInfo]) {
    for branch in branches {
        let marker = if branch.current { "*" } else { " " };
        match &branch.upstream {
            Some(upstream) => println!("{marker} {} -> {}", branch.name, upstream),
            None => println!("{marker} {}", branch.name),
        }
    }
}

pub fn print_log(entries: &[CommitSummary]) {
    for entry in entries {
        println!("{} {} {} {}", entry.hash, entry.author, entry.date, entry.subject);
    }
}

pub fn print_message(message: &str) {
    println!("{message}");
}

pub fn print_help_hint() {
    println!("Use `gitrex tui` or a command such as `gitrex status`.");
}

pub fn render_status_summary(status: &RepoStatus) -> String {
    let mut lines = vec![format!("branch: {}", status.branch_name)];
    if let Some(upstream) = &status.upstream {
        lines.push(format!("upstream: {upstream}"));
    }
    lines.push(if status.files.is_empty() {
        "working tree: clean".to_string()
    } else {
        format!("working tree: {} change(s)", status.files.len())
    });
    lines.join("\n")
}

pub fn render_branch_preview(branches: &[BranchInfo]) -> Vec<String> {
    branches
        .iter()
        .take(8)
        .map(|branch| {
            let marker = if branch.current { "*" } else { " " };
            match &branch.upstream {
                Some(upstream) => format!("{marker} {} -> {}", branch.name, upstream),
                None => format!("{marker} {}", branch.name),
            }
        })
        .collect()
}

pub fn render_log_preview(entries: &[CommitSummary]) -> Vec<String> {
    entries
        .iter()
        .take(8)
        .map(|entry| format!("{} {} {}", entry.hash, entry.author, entry.subject))
        .collect()
}

pub fn render_graph_rows(
    rows: &[GraphLine],
    selected: usize,
    offset: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(2) as usize;
    let mut commit_index = 0usize;
    rows
        .iter()
        .enumerate()
        .map(|(_, row)| {
            match row {
                GraphLine::Connector { graph } => render_graph_connector(graph),
                GraphLine::Commit { graph, summary } => {
                    let line = render_graph_line(
                        graph,
                        summary,
                        commit_index == selected,
                        offset,
                        content_width,
                    );
                    commit_index = commit_index.saturating_add(1);
                    line
                }
            }
        })
        .collect()
}

fn scroll_text(text: &str, offset: usize, width: usize) -> String {
    if text.is_empty() || width == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    let start = offset % chars.len().max(1);
    chars
        .iter()
        .cycle()
        .skip(start)
        .take(width)
        .collect()
}

fn clip_text(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn pad_text(text: &str, width: usize) -> String {
    let mut out = clip_text(text, width);
    let fill = width.saturating_sub(out.chars().count());
    if fill > 0 {
        out.push_str(&" ".repeat(fill));
    }
    out
}

fn render_graph_connector(graph: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("  ", Style::default().fg(ratatui::style::Color::Rgb(33, 38, 45))),
        Span::styled(graph.to_string(), Style::default().fg(ratatui::style::Color::Rgb(139, 148, 158))),
    ])
}

fn render_graph_line(
    graph: &str,
    entry: &CommitSummary,
    selected: bool,
    offset: usize,
    content_width: usize,
) -> Line<'static> {
    let subject_style = Style::default()
        .fg(ratatui::style::Color::Rgb(230, 237, 243))
        .add_modifier(Modifier::BOLD);
    let date_style = Style::default().fg(ratatui::style::Color::Rgb(210, 153, 34));
    let hash_style = Style::default().fg(ratatui::style::Color::Rgb(168, 85, 247));
    let separator_style = Style::default().fg(ratatui::style::Color::Rgb(33, 38, 45));

    let short_hash = entry.hash.chars().take(8).collect::<String>();
    let graph_width = graph.chars().count();
    let date_text = entry.date.chars().take(10).collect::<String>();
    let name_fixed_width = content_width
        .saturating_sub(graph_width)
        .saturating_sub(date_text.chars().count())
        .saturating_sub(short_hash.chars().count())
        .saturating_sub(14)
        .max(18);
    let subject_width = name_fixed_width;
    let subject_raw = if subject_width == 0 {
        String::new()
    } else if selected && entry.subject.chars().count() > subject_width {
        scroll_text(&entry.subject, offset, subject_width)
    } else {
        clip_text(&entry.subject, subject_width)
    };
    let subject = pad_text(&subject_raw, name_fixed_width);

    Line::from(vec![
        Span::styled("  ", separator_style),
        Span::styled(graph.to_string(), separator_style),
        Span::styled(" ", separator_style),
        Span::styled(subject, subject_style),
        Span::styled("  ", separator_style),
        Span::styled(date_text, date_style),
        Span::styled("  ", separator_style),
        Span::styled(short_hash, hash_style),
    ])
}

pub fn render_graph_title(branch_name: Option<&str>) -> String {
    match branch_name {
        Some(branch_name) if !branch_name.trim().is_empty() => {
            format!("Git Graph • {branch_name}")
        }
        _ => String::from("Git Graph"),
    }
}

pub fn render_status_entries(entries: &[StatusEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("{} {}", entry.code, entry.path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{render_graph_rows, render_graph_title};
    use crate::domain::{CommitSummary, GraphLine};

    #[test]
    fn graph_title_shows_current_branch() {
        assert_eq!(render_graph_title(Some("main")), "Git Graph • main");
        assert_eq!(render_graph_title(None), "Git Graph");
    }

    #[test]
    fn graph_preview_keeps_all_commits() {
        let graph = vec![
            GraphLine::Commit {
                graph: "*".to_string(),
                summary: CommitSummary {
                    hash: "abc123".to_string(),
                    author: "Marcos".to_string(),
                    date: "2026-05-24".to_string(),
                    subject: "Initial commit".to_string(),
                },
            },
            GraphLine::Commit {
                graph: "|".to_string(),
                summary: CommitSummary {
                    hash: "def456".to_string(),
                    author: "Marcos".to_string(),
                    date: "2026-05-24".to_string(),
                    subject: "Add feature".to_string(),
                },
            },
            GraphLine::Commit {
                graph: "|".to_string(),
                summary: CommitSummary {
                    hash: "ghi789".to_string(),
                    author: "Marcos".to_string(),
                    date: "2026-05-24".to_string(),
                    subject: "Fix bug".to_string(),
                },
            },
        ];

        let lines = render_graph_rows(&graph, 1, 0, 80);
        assert_eq!(lines.len(), 3);
        let line_one = lines[1]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let line_two = lines[2]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(line_one.contains("|"));
        assert!(line_two.contains("Fix bug"));
    }

    #[test]
    fn graph_preview_scrolls_long_lines() {
        let graph = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "A very long commit subject that exceeds width".to_string(),
            },
        }];

        let first = render_graph_rows(&graph, 0, 0, 60);
        let second = render_graph_rows(&graph, 0, 5, 60);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0], second[0]);
    }

    #[test]
    fn graph_preview_only_scrolls_selected_line() {
        let graph = vec![
            GraphLine::Commit {
                graph: "*".to_string(),
                summary: CommitSummary {
                    hash: "abc123def456".to_string(),
                    author: "Marcos".to_string(),
                    date: "2026-05-24".to_string(),
                    subject: "A very long commit subject that exceeds width".to_string(),
                },
            },
            GraphLine::Commit {
                graph: "|".to_string(),
                summary: CommitSummary {
                    hash: "def456abc123".to_string(),
                    author: "Marcos".to_string(),
                    date: "2026-05-24".to_string(),
                    subject: "Another long commit subject that exceeds width".to_string(),
                },
            },
            GraphLine::Connector {
                graph: "|/".to_string(),
            },
        ];

        let first = render_graph_rows(&graph, 0, 0, 60);
        let second = render_graph_rows(&graph, 0, 5, 60);
        assert_ne!(first[0], second[0]);
        assert_eq!(first[1], second[1]);
    }

    #[test]
    fn graph_date_and_hash_keep_stable_columns() {
        let short = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Fix".to_string(),
            },
        }];
        let long = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "A much longer commit name".to_string(),
            },
        }];

        let short_line = render_graph_rows(&short, 0, 0, 80);
        let long_line = render_graph_rows(&long, 0, 0, 80);
        let short_text = short_line[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let long_text = long_line[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(short_text.find("2026-05-24"), long_text.find("2026-05-24"));
        assert_eq!(short_text.find("abc123de"), long_text.find("abc123de"));
    }
}
