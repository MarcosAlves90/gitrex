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

pub fn render_graph_preview(entries: &[CommitSummary], selected: usize) -> Vec<String> {
    entries
        .iter()
        .take(8)
        .enumerate()
        .map(|(index, entry)| {
            let marker = if index == selected { "▶" } else { " " };
            let short_hash = entry.hash.chars().take(8).collect::<String>();
            format!("{marker} {short_hash} {} {}", entry.date, entry.subject)
        })
        .collect()
}

pub fn render_status_entries(entries: &[StatusEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("{} {}", entry.code, entry.path))
        .collect()
}
