use crate::domain::{
    build_branch_catalog,
    repository_context::{
        ChangeContextReport, CompareReport, ContextCommit, DiffReport, PathGroup,
        RepositoryContext, ShowReport, WorkingTreeContext,
    },
    BranchInfo, CommitSummary, GraphLine, OperationClass, OperationPlan, RepoStatus, StatusEntry,
};
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

pub fn print_operation_plan(plan: &OperationPlan) {
    println!("operation: {}", plan.operation);
    println!("risk class: {}", operation_class_name(plan.risk_class));
    println!(
        "effects: {}",
        plan.effects
            .iter()
            .map(|effect| operation_class_name(*effect))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("dry-run source: {:?}", plan.planning_source);
    println!(
        "network access required: {}",
        if plan.network_access_required {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "network access during planning: {}",
        if plan.network_access_during_planning {
            "yes"
        } else {
            "no"
        }
    );
    if !plan.expected_local_effects.is_empty() {
        println!("expected local effects:");
        for effect in &plan.expected_local_effects {
            print_expected_effect(
                effect.action.as_str(),
                effect.target.as_str(),
                effect.commit_id.as_deref(),
            );
        }
    }
    if !plan.expected_remote_effects.is_empty() {
        println!("expected remote effects:");
        for effect in &plan.expected_remote_effects {
            print_expected_effect(
                effect.action.as_str(),
                effect.target.as_str(),
                effect.commit_id.as_deref(),
            );
        }
    }
    if !plan.refs.is_empty() {
        println!("observed refs:");
        for reference in &plan.refs {
            match &reference.commit_id {
                Some(commit_id) => println!("  {} {commit_id}", reference.name),
                None => println!("  {} (absent)", reference.name),
            }
        }
    }
}

fn print_expected_effect(action: &str, target: &str, commit_id: Option<&str>) {
    match commit_id {
        Some(commit_id) => println!("  {action}: {target} at {commit_id}"),
        None => println!("  {action}: {target}"),
    }
}

fn operation_class_name(class: OperationClass) -> &'static str {
    match class {
        OperationClass::ReadOnly => "read_only",
        OperationClass::LocalMutation => "local_mutation",
        OperationClass::NetworkRead => "network_read",
        OperationClass::RemoteMutation => "remote_mutation",
        OperationClass::Destructive => "destructive",
    }
}

pub fn print_branches(branches: &[BranchInfo]) {
    let catalog = build_branch_catalog(branches);

    println!("remote branches:");
    for group in &catalog.remotes {
        println!("  {}", group.remote);
        for branch in &group.branches {
            println!("    {}", branch.branch_short_name());
        }
    }

    println!("local branches:");
    for entry in &catalog.locals {
        let marker = if entry.branch.current { "*" } else { " " };
        let status = match (
            entry.synced_remotes.is_empty(),
            entry.differing_remotes.is_empty(),
        ) {
            (true, true) => String::from("local-only"),
            (false, true) => format!("synced: {}", entry.synced_remotes.join(", ")),
            (true, false) => format!("differs: {}", entry.differing_remotes.join(", ")),
            (false, false) => format!(
                "synced: {}; differs: {}",
                entry.synced_remotes.join(", "),
                entry.differing_remotes.join(", ")
            ),
        };
        println!("{marker} {} [{}]", entry.branch.name, status);
    }
}

pub fn print_log(entries: &[CommitSummary]) {
    for entry in entries {
        println!(
            "{} {} {} {}",
            entry.hash, entry.author, entry.date, entry.subject
        );
    }
}

pub fn print_inspect(context: &RepositoryContext) {
    println!("repository: {}", context.repository.name);
    println!("root: {}", context.repository.root);
    match (
        &context.head.branch,
        &context.head.commit,
        context.head.detached,
    ) {
        (Some(branch), Some(commit), _) => println!("HEAD: {branch} ({commit})"),
        (Some(branch), None, _) => println!("HEAD: {branch} (unborn)"),
        (None, Some(commit), true) => println!("HEAD: detached ({commit})"),
        _ => println!("HEAD: unavailable"),
    }
    if let Some(upstream) = &context.upstream {
        println!(
            "upstream: {} (+{} -{})",
            upstream.reference, upstream.ahead, upstream.behind
        );
    }
    println!(
        "working tree: {}",
        if context.working_tree.clean {
            "clean"
        } else {
            "changes"
        }
    );
    print_working_tree(&context.working_tree);
    if context.conflicts.has_conflicts {
        println!("conflicts: {}", context.conflicts.count);
        if let Some(paths) = &context.conflicts.paths {
            for path in paths {
                println!("  {} {}", path.status, path.path);
            }
        }
        print_truncated(context.conflicts.truncated);
    }
    if let Some(branches) = &context.branches {
        println!("branches: {}", branches.count);
        for branch in &branches.branches {
            let marker = if branch.current { "*" } else { " " };
            println!(
                "  {marker} {} [{}] {}",
                branch.name, branch.kind, branch.commit
            );
        }
        print_truncated(branches.truncated);
    }
    if let Some(history) = &context.history {
        println!("history: {} commit(s)", history.count);
        for commit in &history.commits {
            print_context_commit(commit);
        }
        print_truncated(history.truncated);
    }
}

pub fn print_diff(diff: &DiffReport) {
    println!("diff: {}", diff.mode);
    if let Some(left) = &diff.left_commit {
        println!("left: {left}");
    }
    if let Some(right) = &diff.right_commit {
        println!("right: {right}");
    }
    println!(
        "changed files: {} (+{} -{})",
        diff.changed_file_count, diff.additions, diff.deletions
    );
    for file in &diff.changed_files {
        let stats = match (file.additions, file.deletions) {
            (Some(additions), Some(deletions)) => format!(" +{additions} -{deletions}"),
            _ => String::new(),
        };
        println!("  {} {}{stats}", file.status, file.path);
    }
    print_truncated(diff.changed_files_truncated);
    if let Some(patch) = &diff.patch {
        if !patch.text.is_empty() {
            print!("{}", patch.text);
            if !patch.text.ends_with('\n') {
                println!();
            }
        }
        if patch.truncated {
            println!(
                "[patch truncated: returned {} of {} bytes]",
                patch.returned_bytes, patch.total_bytes
            );
        }
    }
}

pub fn print_show(show: &ShowReport) {
    println!("commit: {}", show.commit.hash);
    println!("parents: {}", show.commit.parents.join(", "));
    println!("author: {}", show.commit.author);
    println!("date: {}", show.commit.timestamp);
    println!("subject: {}", show.commit.subject);
    if !show.commit.body.is_empty() {
        println!("\n{}", show.commit.body);
    }
    print_diff(&show.diff);
}

pub fn print_compare(compare: &CompareReport) {
    println!("left: {} ({})", compare.left_reference, compare.left_commit);
    println!(
        "right: {} ({})",
        compare.right_reference, compare.right_commit
    );
    println!("merge bases: {}", compare.merge_bases.join(", "));
    println!("ahead: {}; behind: {}", compare.ahead, compare.behind);
    println!("left-only commits:");
    for commit in &compare.left_only_commits {
        print_context_commit(commit);
    }
    print_truncated(compare.left_only_commits_truncated);
    println!("right-only commits:");
    for commit in &compare.right_only_commits {
        print_context_commit(commit);
    }
    print_truncated(compare.right_only_commits_truncated);
    print_diff(&compare.diff);
}

pub fn print_change_context(context: &ChangeContextReport) {
    println!("base: {} ({})", context.base.reference, context.base.commit);
    println!("HEAD: {}", context.head.commit);
    println!("merge bases: {}", context.merge_bases.join(", "));
    println!("commits since base: {}", context.commits.count);
    for commit in &context.commits.commits {
        print_context_commit(commit);
    }
    print_truncated(context.commits.truncated);
    print_diff(&context.diff);
    print_working_tree(&context.working_tree);
    if context.conflicts.has_conflicts {
        println!("conflicts: {}", context.conflicts.count);
        if let Some(paths) = &context.conflicts.paths {
            for path in paths {
                println!("  {} {}", path.status, path.path);
            }
        }
        print_truncated(context.conflicts.truncated);
    }
}

fn print_working_tree(working_tree: &WorkingTreeContext) {
    print_path_group("staged", &working_tree.staged);
    print_path_group("unstaged", &working_tree.unstaged);
    println!("untracked: {}", working_tree.untracked.count);
    if let Some(paths) = &working_tree.untracked.paths {
        for path in paths {
            println!("  {}", path.path);
        }
    }
    print_truncated(working_tree.untracked.truncated);
}

fn print_path_group(name: &str, group: &PathGroup) {
    println!("{name}: {}", group.count);
    if let Some(paths) = &group.paths {
        for path in paths {
            println!("  {} {}", path.status, path.path);
        }
    }
    print_truncated(group.truncated);
}

fn print_context_commit(commit: &ContextCommit) {
    println!(
        "  {} {} {} {}",
        commit.hash, commit.author, commit.date, commit.subject
    );
}

fn print_truncated(truncated: bool) {
    if truncated {
        println!("  [output truncated; raise the corresponding limit to see more]");
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
    let catalog = build_branch_catalog(branches);
    let mut lines = Vec::new();

    lines.push(String::from("remote branches:"));
    for group in catalog.remotes.iter().take(4) {
        lines.push(format!("  {}", group.remote));
        for branch in group.branches.iter().take(4) {
            lines.push(format!("    {}", branch.branch_short_name()));
        }
    }

    lines.push(String::from("local branches:"));
    for entry in catalog.locals.iter().take(4) {
        let marker = if entry.branch.current { "*" } else { " " };
        let status = match (
            entry.synced_remotes.is_empty(),
            entry.differing_remotes.is_empty(),
        ) {
            (true, true) => String::from("local-only"),
            (false, true) => format!("synced: {}", entry.synced_remotes.join(", ")),
            (true, false) => format!("differs: {}", entry.differing_remotes.join(", ")),
            (false, false) => format!(
                "synced: {}; differs: {}",
                entry.synced_remotes.join(", "),
                entry.differing_remotes.join(", ")
            ),
        };
        lines.push(format!("{marker} {} [{status}]", entry.branch.name));
    }

    lines
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
    active: bool,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(2) as usize;
    let mut commit_index = 0usize;
    rows.iter()
        .map(|row| match row {
            GraphLine::Connector { graph } => render_graph_connector(graph, active),
            GraphLine::Commit { graph, summary } => {
                let line = render_graph_line(
                    graph,
                    summary,
                    commit_index == selected,
                    offset,
                    content_width,
                    active,
                );
                commit_index = commit_index.saturating_add(1);
                line
            }
        })
        .collect()
}

fn viewport_text(text: &str, offset: usize, width: usize) -> String {
    if text.is_empty() || width == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    let start = offset.min(chars.len().saturating_sub(1));
    chars.iter().skip(start).take(width).collect()
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

fn render_graph_connector(graph: &str, active: bool) -> Line<'static> {
    let graph_color = if active {
        ratatui::style::Color::Rgb(0, 200, 255)
    } else {
        ratatui::style::Color::Rgb(139, 148, 158)
    };
    Line::from(vec![
        Span::styled(
            "  ",
            Style::default().fg(ratatui::style::Color::Rgb(33, 38, 45)),
        ),
        Span::styled(graph.to_string(), Style::default().fg(graph_color)),
    ])
}

fn render_graph_line(
    graph: &str,
    entry: &CommitSummary,
    selected: bool,
    offset: usize,
    content_width: usize,
    active: bool,
) -> Line<'static> {
    let subject_style = Style::default()
        .fg(if active {
            ratatui::style::Color::Rgb(230, 237, 243)
        } else {
            ratatui::style::Color::Rgb(139, 148, 158)
        })
        .add_modifier(Modifier::BOLD);
    let date_style = Style::default().fg(if active {
        ratatui::style::Color::Rgb(210, 153, 34)
    } else {
        ratatui::style::Color::Rgb(139, 148, 158)
    });
    let hash_style = Style::default().fg(if active {
        ratatui::style::Color::Rgb(168, 85, 247)
    } else {
        ratatui::style::Color::Rgb(139, 148, 158)
    });
    let graph_style = Style::default().fg(if !active {
        ratatui::style::Color::Rgb(139, 148, 158)
    } else if selected {
        ratatui::style::Color::Rgb(186, 85, 255)
    } else {
        ratatui::style::Color::Rgb(0, 200, 255)
    });
    let separator_style = Style::default().fg(ratatui::style::Color::Rgb(33, 38, 45));

    let short_hash = entry.hash.chars().take(8).collect::<String>();
    let graph_width = graph.chars().count();
    let date_text = entry.date.chars().take(10).collect::<String>();
    let prefix_width = graph_width.saturating_add(3);
    let show_hash = content_width >= prefix_width.saturating_add(24);
    let show_date = content_width >= prefix_width.saturating_add(38);
    let metadata_width = (if show_date { 12 } else { 0 }) + (if show_hash { 10 } else { 0 });
    let subject_width = content_width.saturating_sub(prefix_width.saturating_add(metadata_width));
    let subject_raw = if selected {
        viewport_text(&entry.subject, offset, subject_width)
    } else {
        clip_text(&entry.subject, subject_width)
    };
    let subject = pad_text(&subject_raw, subject_width);

    let mut spans = vec![
        Span::styled("  ", separator_style),
        Span::styled(graph.to_string(), graph_style),
        Span::styled(" ", separator_style),
        Span::styled(subject, subject_style),
    ];
    if show_date {
        spans.push(Span::styled("  ", separator_style));
        spans.push(Span::styled(date_text, date_style));
    }
    if show_hash {
        spans.push(Span::styled("  ", separator_style));
        spans.push(Span::styled(short_hash, hash_style));
    }
    Line::from(spans)
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
    use super::{render_graph_rows, render_graph_title, viewport_text};
    use crate::domain::{CommitSummary, GraphLine};
    use ratatui::style::Color;

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

        let lines = render_graph_rows(&graph, 1, 0, 80, true);
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

        let first = render_graph_rows(&graph, 0, 0, 60, true);
        let second = render_graph_rows(&graph, 0, 5, 60, true);
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

        let first = render_graph_rows(&graph, 0, 0, 60, true);
        let second = render_graph_rows(&graph, 0, 5, 60, true);
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

        let short_line = render_graph_rows(&short, 0, 0, 80, true);
        let long_line = render_graph_rows(&long, 0, 0, 80, true);
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

    #[test]
    fn graph_active_selected_lane_is_visually_distinct() {
        let graph = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Selected commit".to_string(),
            },
        }];

        let lines = render_graph_rows(&graph, 0, 0, 80, true);
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Rgb(186, 85, 255)));
    }

    #[test]
    fn graph_inactive_panels_use_muted_text_colors() {
        let graph = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "A much longer commit name".to_string(),
            },
        }];

        let lines = render_graph_rows(&graph, 0, 0, 80, false);
        let commit_line = &lines[0];

        assert_eq!(
            commit_line.spans[3].style.fg,
            Some(Color::Rgb(139, 148, 158))
        );
        assert_eq!(
            commit_line.spans[5].style.fg,
            Some(Color::Rgb(139, 148, 158))
        );
        assert_eq!(
            commit_line.spans[7].style.fg,
            Some(Color::Rgb(139, 148, 158))
        );
    }

    #[test]
    fn graph_horizontal_viewport_is_stable_and_non_cyclic() {
        let text = "first word last word";
        assert_eq!(viewport_text(text, 0, 10), "first word");
        assert_eq!(viewport_text(text, 6, 9), "word last");
        assert!(!viewport_text(text, 6, 20).contains("|"));
    }

    #[test]
    fn graph_rows_hide_metadata_when_terminal_is_narrow() {
        let graph = vec![GraphLine::Commit {
            graph: "*".to_string(),
            summary: CommitSummary {
                hash: "abc123def456".to_string(),
                author: "Marcos".to_string(),
                date: "2026-05-24".to_string(),
                subject: "Responsive graph row".to_string(),
            },
        }];

        let line = render_graph_rows(&graph, 0, 0, 24, true);
        let text = line[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("Responsive"));
        assert!(!text.contains("2026-05-24"));
        assert!(!text.contains("abc123de"));
    }
}
