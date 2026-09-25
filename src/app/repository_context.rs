use std::{collections::BTreeMap, path::Path};

use crate::{
    domain::{
        error::{GitError, Result},
        repository_context::{
            BranchContext, BranchList, ChangeContextReport, ChangedFile, CommitDetails, CommitList,
            CommitReference, CompareReport, ConflictContext, ContextCommit, ContextPath,
            DiffReport, HeadContext, InspectScope, Patch, PathGroup, RepositoryContext,
            RepositoryIdentity, ShowReport, UpstreamContext, WorkingTreeContext,
        },
        BranchInfo, BranchKind,
    },
    git::{GitClient, GitProcess},
};

pub const DEFAULT_MAX_PATHS: usize = 100;
pub const DEFAULT_HISTORY_LIMIT: usize = 20;
pub const DEFAULT_MAX_BRANCHES: usize = 100;
pub const DEFAULT_MAX_COMMITS: usize = 20;
pub const DEFAULT_MAX_PATCH_BYTES: usize = 64 * 1024;
pub const MAX_PATHS: usize = 1000;
pub const MAX_HISTORY_LIMIT: usize = 100;
pub const MAX_COMMITS: usize = 100;
pub const MAX_PATCH_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLimits {
    pub max_paths: usize,
    pub history_limit: usize,
    pub max_branches: usize,
    pub max_commits: usize,
    pub max_patch_bytes: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        Self {
            max_paths: DEFAULT_MAX_PATHS,
            history_limit: DEFAULT_HISTORY_LIMIT,
            max_branches: DEFAULT_MAX_BRANCHES,
            max_commits: DEFAULT_MAX_COMMITS,
            max_patch_bytes: DEFAULT_MAX_PATCH_BYTES,
        }
    }
}

impl ContextLimits {
    fn normalized(self) -> Self {
        Self {
            max_paths: self.max_paths.clamp(1, MAX_PATHS),
            history_limit: self.history_limit.clamp(1, MAX_HISTORY_LIMIT),
            max_branches: self.max_branches.clamp(1, MAX_PATHS),
            max_commits: self.max_commits.clamp(1, MAX_COMMITS),
            max_patch_bytes: self.max_patch_bytes.clamp(1, MAX_PATCH_BYTES),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSelection {
    Worktree,
    Staged,
    Base(String),
    Refs { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOptions {
    pub selection: DiffSelection,
    pub limits: ContextLimits,
    pub include_patch: bool,
}

struct DiffSpec {
    mode: &'static str,
    revisions: Vec<String>,
    left_commit: Option<String>,
    right_commit: Option<String>,
    root_tree: bool,
}

#[derive(Debug)]
struct WorktreePaths {
    staged: Vec<ContextPath>,
    unstaged: Vec<ContextPath>,
    untracked: Vec<ContextPath>,
    conflicts: Vec<ContextPath>,
}

type FileLineStats = (Option<u64>, Option<u64>);

struct NumstatSummary {
    files: BTreeMap<String, FileLineStats>,
    additions: u64,
    deletions: u64,
}

pub fn inspect(
    client: &GitClient,
    scope: InspectScope,
    limits: ContextLimits,
) -> Result<RepositoryContext> {
    let (git, root) = repository_git(client)?;
    let head_oid = resolve_known_ref(&git, "HEAD")?;
    inspect_resolved(&git, root, scope, limits.normalized(), head_oid)
}

fn inspect_resolved(
    git: &GitProcess,
    root: String,
    scope: InspectScope,
    limits: ContextLimits,
    head_oid: Option<String>,
) -> Result<RepositoryContext> {
    let branch = probe_text(git, ["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let branch = branch.map(|branch| branch.trim().to_string());
    let paths = read_worktree_paths(git)?;
    let include_paths = scope.includes_paths();
    let staged = path_group(paths.staged, include_paths, limits.max_paths);
    let unstaged = path_group(paths.unstaged, include_paths, limits.max_paths);
    let untracked = path_group(paths.untracked, include_paths, limits.max_paths);
    let conflicts = conflict_context(paths.conflicts, include_paths, limits.max_paths);
    let working_tree = WorkingTreeContext {
        clean: staged.count == 0 && unstaged.count == 0 && untracked.count == 0,
        staged,
        unstaged,
        untracked,
    };

    let upstream = read_upstream(git, head_oid.as_deref())?;
    let branches = if scope.includes_branches() {
        Some(read_branches(git, limits.max_branches)?)
    } else {
        None
    };
    let history = if scope.includes_history() {
        Some(read_history(
            git,
            head_oid.as_deref(),
            limits.history_limit,
        )?)
    } else {
        None
    };
    let name = Path::new(&root)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| root.clone());

    Ok(RepositoryContext {
        scope: scope.as_str().to_string(),
        repository: RepositoryIdentity { root, name },
        head: HeadContext {
            commit: head_oid.clone(),
            branch: branch.clone(),
            detached: branch.is_none() && head_oid.is_some(),
        },
        upstream,
        working_tree,
        conflicts,
        branches,
        history,
    })
}

pub fn diff(client: &GitClient, options: DiffOptions) -> Result<DiffReport> {
    let limits = options.limits.normalized();
    let (git, _) = repository_git(client)?;
    let head_oid = resolve_known_ref(&git, "HEAD")?;
    let spec = match options.selection {
        DiffSelection::Worktree => DiffSpec {
            mode: "worktree",
            revisions: Vec::new(),
            left_commit: None,
            right_commit: None,
            root_tree: false,
        },
        DiffSelection::Staged => DiffSpec {
            mode: "staged",
            revisions: Vec::new(),
            left_commit: head_oid,
            right_commit: None,
            root_tree: false,
        },
        DiffSelection::Base(reference) => {
            let base_oid = resolve_required_ref(&git, &reference)?;
            DiffSpec {
                mode: "base",
                revisions: vec![base_oid.clone()],
                left_commit: Some(base_oid),
                right_commit: head_oid,
                root_tree: false,
            }
        }
        DiffSelection::Refs { from, to } => {
            let from_oid = resolve_required_ref(&git, &from)?;
            let to_oid = resolve_required_ref(&git, &to)?;
            DiffSpec {
                mode: "refs",
                revisions: vec![from_oid.clone(), to_oid.clone()],
                left_commit: Some(from_oid),
                right_commit: Some(to_oid),
                root_tree: false,
            }
        }
    };

    collect_diff(&git, spec, limits, options.include_patch)
}

pub fn show(
    client: &GitClient,
    reference: &str,
    limits: ContextLimits,
    include_patch: bool,
) -> Result<ShowReport> {
    let limits = limits.normalized();
    let (git, _) = repository_git(client)?;
    let oid = resolve_required_ref(&git, reference)?;
    let format = "%H%x00%P%x00%an%x00%aI%x00%s%x00%b";
    let output = git.run([
        "show",
        "-s",
        &format!("--format={format}"),
        "--end-of-options",
        &oid,
    ])?;
    let metadata = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
    let mut parts = metadata.splitn(6, '\0');
    let hash = required_field(parts.next(), "commit hash")?
        .trim()
        .to_string();
    let parents = required_field(parts.next(), "commit parents")?
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let author = required_field(parts.next(), "commit author")?
        .trim()
        .to_string();
    let timestamp = required_field(parts.next(), "commit timestamp")?
        .trim()
        .to_string();
    let subject = required_field(parts.next(), "commit subject")?
        .trim()
        .to_string();
    let body = parts
        .next()
        .unwrap_or_default()
        .trim_end_matches(['\r', '\n'])
        .to_string();

    let spec = match parents.first() {
        Some(parent) => DiffSpec {
            mode: "commit",
            revisions: vec![parent.clone(), oid.clone()],
            left_commit: Some(parent.clone()),
            right_commit: Some(oid.clone()),
            root_tree: false,
        },
        None => DiffSpec {
            mode: "root_commit",
            revisions: vec![oid.clone()],
            left_commit: None,
            right_commit: Some(oid.clone()),
            root_tree: true,
        },
    };
    let diff = collect_diff(&git, spec, limits, include_patch)?;

    Ok(ShowReport {
        commit: CommitDetails {
            hash,
            parents,
            author,
            timestamp,
            subject,
            body,
        },
        diff,
    })
}

pub fn compare(
    client: &GitClient,
    left_reference: &str,
    right_reference: &str,
    limits: ContextLimits,
) -> Result<CompareReport> {
    let (git, _) = repository_git(client)?;
    let left_oid = resolve_required_ref(&git, left_reference)?;
    let right_oid = resolve_required_ref(&git, right_reference)?;
    compare_resolved(
        &git,
        left_reference.to_string(),
        left_oid,
        right_reference.to_string(),
        right_oid,
        limits.normalized(),
    )
}

fn compare_resolved(
    git: &GitProcess,
    left_reference: String,
    left_oid: String,
    right_reference: String,
    right_oid: String,
    limits: ContextLimits,
) -> Result<CompareReport> {
    let merge_bases = read_merge_bases(git, &left_oid, &right_oid)?;
    let (ahead, behind) = read_divergence(git, &left_oid, &right_oid)?;
    let left_only_commits = read_unique_commits(git, &left_oid, &right_oid, limits.max_commits)?;
    let right_only_commits = read_unique_commits(git, &right_oid, &left_oid, limits.max_commits)?;
    let diff = collect_diff(
        git,
        DiffSpec {
            mode: "refs",
            revisions: vec![left_oid.clone(), right_oid.clone()],
            left_commit: Some(left_oid.clone()),
            right_commit: Some(right_oid.clone()),
            root_tree: false,
        },
        limits,
        false,
    )?;

    Ok(CompareReport {
        left_reference,
        left_commit: left_oid,
        right_reference,
        right_commit: right_oid,
        merge_bases,
        ahead,
        behind,
        left_only_commit_count: ahead,
        left_only_commits_truncated: ahead > left_only_commits.len() as u64,
        left_only_commits,
        right_only_commit_count: behind,
        right_only_commits_truncated: behind > right_only_commits.len() as u64,
        right_only_commits,
        diff,
    })
}

pub fn change_context(
    client: &GitClient,
    base_reference: &str,
    limits: ContextLimits,
) -> Result<ChangeContextReport> {
    let limits = limits.normalized();
    let (git, root) = repository_git(client)?;
    let base_oid = resolve_required_ref(&git, base_reference)?;
    let head_oid = resolve_known_ref(&git, "HEAD")?
        .ok_or_else(|| GitError::ReferenceNotFound("HEAD".to_string()))?;
    let inspect = inspect_resolved(
        &git,
        root,
        InspectScope::Change,
        limits,
        Some(head_oid.clone()),
    )?;
    let comparison = compare_resolved(
        &git,
        base_reference.to_string(),
        base_oid.clone(),
        "HEAD".to_string(),
        head_oid.clone(),
        limits,
    )?;

    let introduced = comparison.right_only_commits;
    let introduced_count = comparison.right_only_commit_count;
    let introduced_truncated = comparison.right_only_commits_truncated;
    Ok(ChangeContextReport {
        base: CommitReference {
            reference: base_reference.to_string(),
            commit: base_oid,
        },
        head: CommitReference {
            reference: "HEAD".to_string(),
            commit: head_oid,
        },
        merge_bases: comparison.merge_bases,
        commits: CommitList {
            count: introduced_count.min(usize::MAX as u64) as usize,
            commits: introduced,
            truncated: introduced_truncated,
        },
        diff: comparison.diff,
        working_tree: inspect.working_tree,
        conflicts: inspect.conflicts,
    })
}

fn repository_git(client: &GitClient) -> Result<(GitProcess, String)> {
    let discovery = client.git();
    discovery.ensure_repository()?;
    let root = discovery
        .run_text(["rev-parse", "--show-toplevel"])?
        .trim()
        .to_string();
    Ok((GitProcess::new_read_only(&root), root))
}

fn resolve_known_ref(git: &GitProcess, reference: &str) -> Result<Option<String>> {
    let candidate = format!("{reference}^{{commit}}");
    let output = git.probe([
        "rev-parse",
        "--verify",
        "--end-of-options",
        candidate.as_str(),
    ])?;
    if !output.success() {
        return Ok(None);
    }
    let oid = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
    let oid = oid.trim();
    if oid.is_empty() {
        return Err(GitError::Parse(format!("empty object id for {reference}")));
    }
    Ok(Some(oid.to_string()))
}

fn resolve_required_ref(git: &GitProcess, reference: &str) -> Result<String> {
    resolve_known_ref(git, reference)?
        .ok_or_else(|| GitError::ReferenceNotFound(reference.to_string()))
}

fn probe_text<I, S>(git: &GitProcess, args: I) -> Result<Option<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git.probe(args)?;
    if !output.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?,
    ))
}

fn read_upstream(git: &GitProcess, head_oid: Option<&str>) -> Result<Option<UpstreamContext>> {
    let Some(reference) = probe_text(
        git,
        [
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )?
    else {
        return Ok(None);
    };
    let reference = reference.trim().to_string();
    let upstream_oid = resolve_known_ref(git, &reference)?;
    let (ahead, behind) = match (head_oid, upstream_oid.as_deref()) {
        (Some(head), Some(upstream)) => read_divergence(git, head, upstream)?,
        _ => (0, 0),
    };
    Ok(Some(UpstreamContext {
        reference,
        commit: upstream_oid,
        ahead,
        behind,
    }))
}

fn read_worktree_paths(git: &GitProcess) -> Result<WorktreePaths> {
    git.ensure_no_worktree_filters()?;
    let output = git.run([
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--no-renames",
    ])?;
    parse_status(&output.stdout)
}

fn is_conflict(code: &str) -> bool {
    matches!(code, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
        || code.as_bytes().contains(&b'U')
}

fn path_group(paths: Vec<ContextPath>, include_paths: bool, limit: usize) -> PathGroup {
    let count = paths.len();
    PathGroup {
        count,
        paths: include_paths.then(|| paths.into_iter().take(limit).collect()),
        truncated: include_paths && count > limit,
    }
}

fn conflict_context(paths: Vec<ContextPath>, include_paths: bool, limit: usize) -> ConflictContext {
    let count = paths.len();
    ConflictContext {
        has_conflicts: count > 0,
        count,
        paths: include_paths.then(|| paths.into_iter().take(limit).collect()),
        truncated: include_paths && count > limit,
    }
}

fn read_branches(git: &GitProcess, limit: usize) -> Result<BranchList> {
    let output = git.run([
        "for-each-ref",
        "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(objectname)%00%(subject)%00%(refname)",
        "refs/heads",
        "refs/remotes",
    ])?;
    let mut branches = parse_branches(&output.stdout)?;
    branches.sort_by(|left, right| left.name.cmp(&right.name));
    let count = branches.len();
    let truncated = count > limit;
    let branches = branches
        .into_iter()
        .take(limit)
        .map(|branch| BranchContext {
            name: branch.name,
            current: branch.current,
            upstream: branch.upstream,
            commit: branch.commit,
            subject: branch.subject,
            kind: match branch.kind {
                BranchKind::Local => "local".to_string(),
                BranchKind::Remote => "remote".to_string(),
            },
        })
        .collect();
    Ok(BranchList {
        count,
        branches,
        truncated,
    })
}

fn parse_branches(output: &[u8]) -> Result<Vec<BranchInfo>> {
    let text = String::from_utf8(output.to_vec()).map_err(|_| GitError::Utf8)?;
    let mut branches = Vec::new();
    for record in text.lines().filter(|record| !record.is_empty()) {
        let fields = record.split('\0').collect::<Vec<_>>();
        if fields.len() != 6 {
            return Err(GitError::Parse(
                "malformed branch metadata entry".to_string(),
            ));
        }
        let kind = if fields[5].starts_with("refs/heads/") {
            BranchKind::Local
        } else if fields[5].starts_with("refs/remotes/") {
            if fields[5].ends_with("/HEAD") {
                continue;
            }
            BranchKind::Remote
        } else {
            continue;
        };
        branches.push(BranchInfo {
            name: fields[0].to_string(),
            current: fields[1] == "*",
            upstream: (!fields[2].is_empty()).then(|| fields[2].to_string()),
            commit: fields[3].to_string(),
            subject: fields[4].to_string(),
            kind,
        });
    }
    Ok(branches)
}

fn read_history(git: &GitProcess, head_oid: Option<&str>, limit: usize) -> Result<CommitList> {
    let Some(head_oid) = head_oid else {
        return Ok(CommitList {
            count: 0,
            commits: Vec::new(),
            truncated: false,
        });
    };
    let count = git
        .run_text(["rev-list", "--count", head_oid])?
        .trim()
        .parse::<usize>()
        .map_err(|_| GitError::Parse("invalid revision count".to_string()))?;
    let max_count = format!("--max-count={limit}");
    let output = git.run_text([
        "log",
        "--topo-order",
        max_count.as_str(),
        "--date=iso-strict",
        "--format=%H%x00%an%x00%aI%x00%s",
        head_oid,
    ])?;
    let commits = parse_commit_summaries(&output)?;
    Ok(CommitList {
        count,
        truncated: count > commits.len(),
        commits,
    })
}

fn parse_commit_summaries(output: &str) -> Result<Vec<ContextCommit>> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = line.splitn(4, '\0');
            Ok(ContextCommit {
                hash: required_field(fields.next(), "commit hash")?.to_string(),
                author: required_field(fields.next(), "commit author")?.to_string(),
                date: required_field(fields.next(), "commit date")?.to_string(),
                subject: required_field(fields.next(), "commit subject")?.to_string(),
            })
        })
        .collect()
}

fn read_merge_bases(git: &GitProcess, left: &str, right: &str) -> Result<Vec<String>> {
    let output = git.probe(["merge-base", "--all", left, right])?;
    if !output.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
    let mut bases = text.lines().map(str::to_string).collect::<Vec<_>>();
    bases.sort();
    Ok(bases)
}

fn read_divergence(git: &GitProcess, left: &str, right: &str) -> Result<(u64, u64)> {
    let range = format!("{left}...{right}");
    let output = git.run_text(["rev-list", "--left-right", "--count", range.as_str()])?;
    let mut counts = output.split_whitespace();
    let ahead = required_field(counts.next(), "left-only commit count")?
        .parse()
        .map_err(|_| GitError::Parse("invalid left-only commit count".to_string()))?;
    let behind = required_field(counts.next(), "right-only commit count")?
        .parse()
        .map_err(|_| GitError::Parse("invalid right-only commit count".to_string()))?;
    Ok((ahead, behind))
}

fn read_unique_commits(
    git: &GitProcess,
    side: &str,
    other: &str,
    limit: usize,
) -> Result<Vec<ContextCommit>> {
    let max_count = format!("--max-count={}", limit.saturating_add(1));
    let output = git.run_text([
        "log",
        "--topo-order",
        max_count.as_str(),
        "--date=iso-strict",
        "--format=%H%x00%an%x00%aI%x00%s",
        side,
        "--not",
        other,
    ])?;
    let mut commits = parse_commit_summaries(&output)?;
    commits.truncate(limit);
    Ok(commits)
}

fn collect_diff(
    git: &GitProcess,
    spec: DiffSpec,
    limits: ContextLimits,
    include_patch: bool,
) -> Result<DiffReport> {
    if matches!(spec.mode, "worktree" | "base") {
        git.ensure_no_worktree_filters()?;
    }
    let names_args = diff_command_args(spec.mode, &spec.revisions, "--name-status", spec.root_tree);
    let names_output = git.run(names_args.iter().map(String::as_str))?;
    let names = parse_name_status(&names_output.stdout)?;
    let numstat_args = diff_command_args(spec.mode, &spec.revisions, "--numstat", spec.root_tree);
    let numstat_output = git.run(numstat_args.iter().map(String::as_str))?;
    let stats = parse_numstat(&numstat_output.stdout)?;
    let changed_file_count = names.len();
    let mut changed_files = names
        .into_iter()
        .map(|(path, status)| {
            let (file_additions, file_deletions) =
                stats.files.get(&path).cloned().unwrap_or((None, None));
            ChangedFile {
                path,
                status,
                additions: file_additions,
                deletions: file_deletions,
            }
        })
        .collect::<Vec<_>>();
    changed_files.sort_by(|left, right| left.path.cmp(&right.path));
    let changed_files_truncated = changed_file_count > limits.max_paths;
    changed_files.truncate(limits.max_paths);

    let patch = if include_patch {
        let patch_args = diff_command_args(spec.mode, &spec.revisions, "-p", spec.root_tree);
        let output = git.run_bounded(
            patch_args.iter().map(String::as_str),
            limits.max_patch_bytes,
        )?;
        let text = match String::from_utf8(output.output.stdout) {
            Ok(text) => text,
            Err(error) if output.truncated => {
                let valid = error.utf8_error().valid_up_to();
                String::from_utf8(error.into_bytes()[..valid].to_vec())
                    .map_err(|_| GitError::Utf8)?
            }
            Err(_) => return Err(GitError::Utf8),
        };
        Some(Patch {
            returned_bytes: text.len(),
            text,
            total_bytes: output.total_bytes,
            truncated: output.truncated,
        })
    } else {
        None
    };

    Ok(DiffReport {
        mode: spec.mode.to_string(),
        left_commit: spec.left_commit,
        right_commit: spec.right_commit,
        changed_file_count,
        changed_files_truncated,
        changed_files,
        additions: stats.additions,
        deletions: stats.deletions,
        patch,
    })
}

fn diff_command_args(mode: &str, revisions: &[String], stat: &str, root_tree: bool) -> Vec<String> {
    let mut args = if root_tree {
        vec![
            "diff-tree".to_string(),
            "--root".to_string(),
            "-r".to_string(),
            "--no-commit-id".to_string(),
        ]
    } else {
        vec!["diff".to_string()]
    };
    args.extend([
        "--no-ext-diff".to_string(),
        "--no-textconv".to_string(),
        "--no-color".to_string(),
        "--no-renames".to_string(),
        stat.to_string(),
    ]);
    if stat == "--name-status" || stat == "--numstat" {
        args.push("-z".to_string());
    }
    if mode == "staged" {
        args.push("--cached".to_string());
    }
    args.extend(revisions.iter().cloned());
    args.push("--".to_string());
    args
}

fn parse_name_status(output: &[u8]) -> Result<Vec<(String, String)>> {
    let fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut parsed = Vec::new();
    let mut index = 0;
    while index + 1 < fields.len() {
        let status = String::from_utf8(fields[index].to_vec()).map_err(|_| GitError::Utf8)?;
        let path = String::from_utf8(fields[index + 1].to_vec()).map_err(|_| GitError::Utf8)?;
        parsed.push((path, status));
        index += 2;
    }
    if index != fields.len() {
        return Err(GitError::Parse("malformed name-status output".to_string()));
    }
    Ok(parsed)
}

fn parse_numstat(output: &[u8]) -> Result<NumstatSummary> {
    let mut stats = BTreeMap::new();
    let mut total_additions = 0u64;
    let mut total_deletions = 0u64;
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let additions = parse_numstat_count(fields.next())?;
        let deletions = parse_numstat_count(fields.next())?;
        let path = String::from_utf8(
            fields
                .next()
                .ok_or_else(|| GitError::Parse("missing numstat path".to_string()))?
                .to_vec(),
        )
        .map_err(|_| GitError::Utf8)?;
        total_additions = total_additions.saturating_add(additions.unwrap_or(0));
        total_deletions = total_deletions.saturating_add(deletions.unwrap_or(0));
        stats.insert(path, (additions, deletions));
    }
    Ok(NumstatSummary {
        files: stats,
        additions: total_additions,
        deletions: total_deletions,
    })
}

fn parse_numstat_count(field: Option<&[u8]>) -> Result<Option<u64>> {
    let field = field.ok_or_else(|| GitError::Parse("missing numstat count".to_string()))?;
    if field == b"-" {
        return Ok(None);
    }
    let value = std::str::from_utf8(field)
        .map_err(|_| GitError::Utf8)?
        .parse::<u64>()
        .map_err(|_| GitError::Parse("invalid numstat count".to_string()))?;
    Ok(Some(value))
}

fn required_field<'a>(field: Option<&'a str>, name: &str) -> Result<&'a str> {
    field.ok_or_else(|| GitError::Parse(format!("missing {name}")))
}

fn parse_status(output: &[u8]) -> Result<WorktreePaths> {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if record.len() < 4 || record[2] != b' ' {
            return Err(GitError::Parse(
                "malformed porcelain status entry".to_string(),
            ));
        }
        let code = String::from_utf8(record[..2].to_vec()).map_err(|_| GitError::Utf8)?;
        let path = String::from_utf8(record[3..].to_vec()).map_err(|_| GitError::Utf8)?;
        if code == "??" {
            untracked.push(ContextPath { path, status: code });
            continue;
        }
        let entry = ContextPath {
            path,
            status: code.clone(),
        };
        if code.as_bytes()[0] != b' ' {
            staged.push(entry.clone());
        }
        if code.as_bytes()[1] != b' ' {
            unstaged.push(entry.clone());
        }
        if is_conflict(&code) {
            conflicts.push(entry);
        }
    }
    staged.sort_by(|left, right| left.path.cmp(&right.path));
    unstaged.sort_by(|left, right| left.path.cmp(&right.path));
    untracked.sort_by(|left, right| left.path.cmp(&right.path));
    conflicts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(WorktreePaths {
        staged,
        unstaged,
        untracked,
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_conflict, parse_branches, parse_name_status, parse_numstat, parse_status};

    #[test]
    fn branch_parser_preserves_tabs_in_commit_subjects() {
        let branches =
            parse_branches(b"main\0*\0\0abc123\0Add\tbranch context\0refs/heads/main\n").unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].subject, "Add\tbranch context");
    }

    #[test]
    fn parsers_keep_paths_with_spaces_and_report_binary_stats() {
        let names = parse_name_status(b"M\0a file.txt\0A\0new.txt\0").unwrap();
        assert_eq!(names[0], ("a file.txt".to_string(), "M".to_string()));
        assert_eq!(names[1], ("new.txt".to_string(), "A".to_string()));

        let stats = parse_numstat(b"2\t1\ta file.txt\0-\t-\timage.bin\0").unwrap();
        assert_eq!(stats.files["image.bin"], (None, None));
        assert_eq!((stats.additions, stats.deletions), (2, 1));
    }

    #[test]
    fn porcelain_status_detects_staged_unstaged_untracked_and_conflicts() {
        let paths =
            parse_status(b"M  staged.txt\0 M unstaged.txt\0?? new.txt\0UU conflict.txt\0").unwrap();
        assert_eq!(paths.staged.len(), 2);
        assert_eq!(paths.unstaged.len(), 2);
        assert_eq!(paths.untracked[0].path, "new.txt");
        assert_eq!(paths.conflicts[0].path, "conflict.txt");
        assert!(is_conflict("AA"));
    }
}
