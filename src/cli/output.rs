use crate::domain::{BranchInfo, CommitSummary, RepoStatus, StatusEntry};

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

pub fn render_graph_preview(
    entries: &[CommitSummary],
    selected: usize,
    offset: usize,
    width: u16,
) -> Vec<String> {
    let content_width = width.saturating_sub(2) as usize;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let short_hash = entry.hash.chars().take(8).collect::<String>();
            let line = format!("{short_hash} {} {}", entry.date, entry.subject);
            let body = if content_width == 0 {
                String::new()
            } else if index == selected && line.chars().count() > content_width {
                scroll_text(&line, offset, content_width)
            } else {
                line.chars().take(content_width).collect()
            };
            format!("  {body}")
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
    use super::{render_graph_preview, render_graph_title};
    use crate::domain::CommitSummary;

    #[test]
    fn graph_title_shows_current_branch() {
        assert_eq!(render_graph_title(Some("main")), "Git Graph • main");
        assert_eq!(render_graph_title(None), "Git Graph");
    }

    #[test]
    fn graph_preview_keeps_all_commits() {
        let entries = vec![
            CommitSummary {
                hash: "abc123".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Initial commit".to_string(),
            },
            CommitSummary {
                hash: "def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Add feature".to_string(),
            },
            CommitSummary {
                hash: "ghi789".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Fix bug".to_string(),
            },
        ];

        let lines = render_graph_preview(&entries, 1, 0, 80);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("  "));
        assert!(lines[2].contains("Fix bug"));
    }

    #[test]
    fn graph_preview_scrolls_long_lines() {
        let entries = vec![CommitSummary {
            hash: "abc123def456".to_string(),
            author: "Marcos".to_string(),
            date: "2026-05-24".to_string(),
            subject: "A very long commit subject that exceeds width".to_string(),
        }];

        let first = render_graph_preview(&entries, 0, 0, 20);
        let second = render_graph_preview(&entries, 0, 5, 20);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0], second[0]);
    }

    #[test]
    fn graph_preview_only_scrolls_selected_line() {
        let entries = vec![
            CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "A very long commit subject that exceeds width".to_string(),
            },
            CommitSummary {
                hash: "def456abc123".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Another long commit subject that exceeds width".to_string(),
            },
        ];

        let first = render_graph_preview(&entries, 0, 0, 24);
        let second = render_graph_preview(&entries, 0, 5, 24);
        assert_ne!(first[0], second[0]);
        assert_eq!(first[1], second[1]);
    }
}
