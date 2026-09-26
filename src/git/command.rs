use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::domain::error::{GitError, Result};
use crate::domain::{
    branch::{BranchCleanupOutcome, BranchCleanupState},
    BranchInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitComparison {
    pub left_oid: String,
    pub right_oid: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CherryPickStatus {
    Applied,
    Conflict,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CherryPickResult {
    pub source_oid: String,
    pub destination: String,
    pub status: CherryPickStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Soft => "Soft",
            Self::Mixed => "Mixed",
            Self::Hard => "Hard",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            Self::Soft => "Move the branch tip; keep the index and worktree unchanged.",
            Self::Mixed => "Move the branch tip and reset the index; keep worktree files.",
            Self::Hard => {
                "Move the branch tip; overwrite tracked index/worktree files and obstructing untracked paths."
            }
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Hard => "--hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetStatus {
    Applied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetResult {
    pub mode: ResetMode,
    pub status: ResetStatus,
    pub previous_head: String,
    pub resulting_head: Option<String>,
    pub target_oid: String,
    pub detail: String,
}

fn nul_paths(output: &[u8]) -> Vec<Vec<u8>> {
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| path.strip_suffix(b"/").unwrap_or(path).to_vec())
        .collect()
}

fn path_is_same_or_parent(parent: &[u8], child: &[u8]) -> bool {
    child == parent
        || child
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.first() == Some(&b'/'))
}

fn paths_overlap_on_filesystem(left: &[u8], right: &[u8]) -> bool {
    let left = left.strip_suffix(b"/").unwrap_or(left);
    let right = right.strip_suffix(b"/").unwrap_or(right);
    if path_is_same_or_parent(left, right) || path_is_same_or_parent(right, left) {
        return true;
    }

    if cfg!(any(target_os = "windows", target_os = "macos")) {
        let left = String::from_utf8_lossy(left).to_lowercase();
        let right = String::from_utf8_lossy(right).to_lowercase();
        return path_is_same_or_parent(left.as_bytes(), right.as_bytes())
            || path_is_same_or_parent(right.as_bytes(), left.as_bytes());
    }

    false
}

fn display_git_path(path: &[u8]) -> String {
    String::from_utf8_lossy(path)
        .chars()
        .flat_map(char::escape_default)
        .collect()
}

#[derive(Debug, Clone)]
pub struct GitClient {
    discovery_path: PathBuf,
    read_only: bool,
}

impl Default for GitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClient {
    pub fn new() -> Self {
        let discovery_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            discovery_path,
            read_only: false,
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let discovery_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };

        Self {
            discovery_path,
            read_only: false,
        }
    }

    pub fn discovery_path(&self) -> &Path {
        &self.discovery_path
    }

    pub(crate) fn read_only(&self) -> Self {
        Self {
            discovery_path: self.discovery_path.clone(),
            read_only: true,
        }
    }

    pub(crate) fn git(&self) -> super::GitProcess {
        if self.read_only {
            super::GitProcess::new_read_only(&self.discovery_path)
        } else {
            super::GitProcess::new(&self.discovery_path)
        }
    }

    pub(crate) fn resolve_commit(&self, reference: &str) -> Result<String> {
        let git = self.git();
        git.ensure_repository()?;
        let candidate = format!("{reference}^{{commit}}");
        let output = git.probe([
            "rev-parse",
            "--verify",
            "--end-of-options",
            candidate.as_str(),
        ])?;
        if !output.success() {
            return Err(GitError::ReferenceNotFound(reference.to_string()));
        }

        let oid = String::from_utf8(output.stdout).map_err(|_| GitError::Utf8)?;
        let oid = oid.trim();
        if oid.is_empty() {
            return Err(GitError::Parse(format!(
                "empty object id for reference {reference}"
            )));
        }
        Ok(oid.to_string())
    }

    pub fn compare_commits(
        &self,
        left_reference: &str,
        right_reference: &str,
    ) -> Result<CommitComparison> {
        let left_oid = self.resolve_commit(left_reference)?;
        let right_oid = self.resolve_commit(right_reference)?;
        let summary = if left_oid == right_oid {
            String::from("No changes between these commits.")
        } else {
            let git = self.git();
            git.run_text([
                "diff",
                // Keep comparison output finite while leaving enough room for
                // the TUI's scrollable review to expose long but ordinary diffs.
                "--stat=100,60,250",
                "--no-ext-diff",
                "--no-color",
                left_oid.as_str(),
                right_oid.as_str(),
                "--",
            ])?
            .trim()
            .to_string()
        };

        Ok(CommitComparison {
            left_oid,
            right_oid,
            summary: if summary.is_empty() {
                String::from("No changes between these commits.")
            } else {
                summary
            },
        })
    }

    pub fn cherry_pick_to_branch(
        &self,
        source_reference: &str,
        destination: &str,
    ) -> Result<CherryPickResult> {
        let source_oid = self.resolve_commit(source_reference)?;
        self.cherry_pick_resolved_to_branch(&source_oid, destination, true)
    }

    pub(crate) fn cherry_pick_planned_to_branch(
        &self,
        source_oid: &str,
        destination: &str,
    ) -> Result<CherryPickResult> {
        self.cherry_pick_resolved_to_branch(source_oid, destination, false)
    }

    fn cherry_pick_resolved_to_branch(
        &self,
        source_oid: &str,
        destination: &str,
        validate_initial_state: bool,
    ) -> Result<CherryPickResult> {
        let source_oid = source_oid.to_string();
        let git = self.git();
        if validate_initial_state {
            git.ensure_repository()?;

            let destination_ref = format!("refs/heads/{destination}");
            let branch =
                git.probe(["show-ref", "--verify", "--quiet", destination_ref.as_str()])?;
            if !branch.success() {
                return Err(GitError::ReferenceNotFound(destination.to_string()));
            }

            let dirty = git.run_text(["status", "--porcelain=v1", "--untracked-files=all"])?;
            if !dirty.trim().is_empty() {
                return Err(GitError::Backend(
                    "cherry-pick requires a clean index and worktree".to_string(),
                ));
            }
        }

        let source_collisions = self.untracked_paths_overlapping_commit(&source_oid)?;
        if !source_collisions.is_empty() {
            return Ok(CherryPickResult {
                source_oid,
                destination: destination.to_string(),
                status: CherryPickStatus::Failed,
                detail: format!(
                    "cherry-pick refused before switching because ignored/untracked paths would be overwritten: {}",
                    source_collisions.join(", ")
                ),
            });
        }

        let switch_result = if validate_initial_state {
            self.switch_without_overwriting_ignored(destination)
        } else {
            git.run(["switch", "--no-overwrite-ignore", "--", destination])
                .map(|_| ())
        };
        if let Err(error) = switch_result {
            return Ok(CherryPickResult {
                source_oid,
                destination: destination.to_string(),
                status: CherryPickStatus::Failed,
                detail: format!("could not switch to destination branch: {error}"),
            });
        }
        let output = match git.probe(["cherry-pick", source_oid.as_str()]) {
            Ok(output) => output,
            Err(error) => {
                return Ok(CherryPickResult {
                    source_oid,
                    destination: destination.to_string(),
                    status: CherryPickStatus::Failed,
                    detail: format!(
                        "could not start cherry-pick after switching branches: {error}"
                    ),
                });
            }
        };
        if output.success() {
            return Ok(CherryPickResult {
                source_oid,
                destination: destination.to_string(),
                status: CherryPickStatus::Applied,
                detail: String::from("Cherry-pick completed."),
            });
        }

        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let in_progress = match git.probe(["rev-parse", "--verify", "--quiet", "CHERRY_PICK_HEAD"])
        {
            Ok(result) => result.success(),
            Err(error) => {
                return Ok(CherryPickResult {
                    source_oid,
                    destination: destination.to_string(),
                    status: CherryPickStatus::Failed,
                    detail: format!(
                        "{detail}; unable to determine whether cherry-pick is still in progress: {error}"
                    ),
                });
            }
        };
        if !in_progress {
            return Ok(CherryPickResult {
                source_oid,
                destination: destination.to_string(),
                status: CherryPickStatus::Failed,
                detail: if detail.is_empty() {
                    format!("git cherry-pick failed with exit {:?}", output.exit_code)
                } else {
                    detail
                },
            });
        }

        let unresolved = match git.probe(["ls-files", "--unmerged"]) {
            Ok(result) => result,
            Err(error) => {
                return Ok(CherryPickResult {
                    source_oid,
                    destination: destination.to_string(),
                    status: CherryPickStatus::Failed,
                    detail: format!(
                        "{detail}; unable to determine whether the cherry-pick has unresolved paths: {error}"
                    ),
                });
            }
        };
        Ok(CherryPickResult {
            source_oid,
            destination: destination.to_string(),
            status: if !unresolved.stdout.is_empty() {
                CherryPickStatus::Conflict
            } else {
                CherryPickStatus::Stopped
            },
            detail: if detail.is_empty() {
                format!("git cherry-pick failed with exit {:?}", output.exit_code)
            } else {
                detail
            },
        })
    }

    pub fn hard_reset_overwrite_paths(&self, target_reference: &str) -> Result<Vec<String>> {
        let target_oid = self.resolve_commit(target_reference)?;
        self.untracked_paths_overlapping_tree(&target_oid)
    }

    fn untracked_worktree_paths(&self) -> Result<Vec<Vec<u8>>> {
        let git = self.git();
        let mut paths = Vec::new();
        for arguments in [
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--directory",
                "-z",
            ][..],
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ][..],
        ] {
            let output = git.probe(arguments)?;
            if !output.success() {
                return Err(GitError::Backend(format!(
                    "could not inspect ignored/untracked paths: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            paths.extend(nul_paths(&output.stdout));
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    fn untracked_paths_overlapping_tree(&self, target_oid: &str) -> Result<Vec<String>> {
        let git = self.git();
        let output = git.probe([
            "ls-tree",
            "--full-tree",
            "-r",
            "--name-only",
            "-z",
            target_oid,
        ])?;
        if !output.success() {
            return Err(GitError::Backend(format!(
                "could not inspect target tree: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let target_paths = nul_paths(&output.stdout);
        let worktree_paths = self.untracked_worktree_paths()?;
        let mut collisions = worktree_paths
            .into_iter()
            .filter(|worktree_path| {
                target_paths
                    .iter()
                    .any(|target_path| paths_overlap_on_filesystem(worktree_path, target_path))
            })
            .collect::<Vec<_>>();
        collisions.sort();
        collisions.dedup();
        Ok(collisions
            .iter()
            .map(|path| display_git_path(path))
            .collect())
    }

    fn untracked_paths_overlapping_commit(&self, commit_oid: &str) -> Result<Vec<String>> {
        let git = self.git();
        let output = git.probe([
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            commit_oid,
        ])?;
        if !output.success() {
            return Err(GitError::Backend(format!(
                "could not inspect cherry-pick paths: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let commit_paths = nul_paths(&output.stdout);
        let worktree_paths = self.untracked_worktree_paths()?;
        let mut collisions = worktree_paths
            .into_iter()
            .filter(|worktree_path| {
                commit_paths
                    .iter()
                    .any(|commit_path| paths_overlap_on_filesystem(worktree_path, commit_path))
            })
            .collect::<Vec<_>>();
        collisions.sort();
        collisions.dedup();
        Ok(collisions
            .iter()
            .map(|path| display_git_path(path))
            .collect())
    }

    pub fn reset_to_commit(
        &self,
        target_reference: &str,
        mode: ResetMode,
        expected_branch: &str,
        expected_head: &str,
    ) -> Result<ResetResult> {
        let git = self.git();
        git.ensure_repository()?;
        let target_oid = self.resolve_commit(target_reference)?;

        let unresolved = git.run_text(["ls-files", "--unmerged"])?;
        if !unresolved.trim().is_empty() {
            return Err(GitError::Backend(
                "reset is blocked while unresolved conflicts exist".to_string(),
            ));
        }

        let symbolic_head = git.probe(["symbolic-ref", "--quiet", "HEAD"])?;
        if !symbolic_head.success() {
            return Err(GitError::Backend(
                "reset requires a checked-out local branch".to_string(),
            ));
        }
        let symbolic_head = String::from_utf8(symbolic_head.stdout).map_err(|_| GitError::Utf8)?;
        let current_branch = symbolic_head
            .trim()
            .strip_prefix("refs/heads/")
            .ok_or_else(|| {
                GitError::Backend("reset requires a checked-out local branch".to_string())
            })?;
        if current_branch != expected_branch {
            return Err(GitError::Backend(format!(
                "current branch changed: expected {expected_branch}, found {current_branch}"
            )));
        }

        let previous_head = self.resolve_commit("HEAD")?;
        if previous_head != expected_head {
            return Err(GitError::Backend(format!(
                "HEAD changed: expected {expected_head}, found {previous_head}"
            )));
        }

        if mode == ResetMode::Hard {
            let collisions = self.untracked_paths_overlapping_tree(&target_oid)?;
            if !collisions.is_empty() {
                return Err(GitError::Backend(format!(
                    "hard reset blocked because untracked/ignored paths collide with the target tree: {}",
                    collisions.join(", ")
                )));
            }
        }

        let output = git.probe(["reset", mode.flag(), target_oid.as_str()])?;
        let resulting_head = self.resolve_commit("HEAD").ok();
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(ResetResult {
            mode,
            status: if output.success() {
                ResetStatus::Applied
            } else {
                ResetStatus::Failed
            },
            previous_head,
            resulting_head,
            target_oid,
            detail,
        })
    }

    pub fn status(&self) -> Result<crate::domain::RepoStatus> {
        crate::git::status::read_status(self)
    }

    pub fn refresh_remote_refs(&self) -> Result<()> {
        self.fetch(None)
    }

    pub fn fetch(&self, remote_name: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        match remote_name {
            Some(remote_name) => {
                git.run(["fetch", "--prune", "--", remote_name])?;
            }
            None => {
                git.run(["fetch", "--all", "--prune"])?;
            }
        }
        Ok(())
    }

    pub fn branches(&self) -> Result<Vec<crate::domain::BranchInfo>> {
        crate::git::branch::list_branches(self)
    }

    pub fn merged_local_branches(
        &self,
        base_reference: &str,
        exclusions: &[String],
        upstream_remotes: &[String],
    ) -> Result<Vec<BranchInfo>> {
        let base_oid = self.resolve_commit(base_reference)?;
        let git = self.git();

        let base_ref_output = git.probe([
            "rev-parse",
            "--symbolic-full-name",
            "--verify",
            "--end-of-options",
            base_reference,
        ])?;
        let base_branch = if base_ref_output.success() {
            let resolved = String::from_utf8(base_ref_output.stdout).map_err(|_| GitError::Utf8)?;
            resolved
                .trim()
                .strip_prefix("refs/heads/")
                .map(str::to_owned)
        } else {
            None
        };

        let merged_option = format!("--merged={base_oid}");
        let merged_output = git.run_text([
            "for-each-ref",
            merged_option.as_str(),
            "--format=%(refname:short)",
            "refs/heads/",
        ])?;
        let merged_branches = merged_output
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect::<HashSet<_>>();

        let configured_remotes = if !upstream_remotes.is_empty() {
            let remotes = git
                .run_text(["remote"])?
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for selected in upstream_remotes {
                if !remotes.iter().any(|configured| configured == selected) {
                    return Err(GitError::Backend(format!(
                        "remote '{selected}' is not configured"
                    )));
                }
            }
            Some(remotes)
        } else {
            None
        };

        let mut candidates = self
            .branches()?
            .into_iter()
            .filter(|branch| {
                !branch.is_remote()
                    && !branch.current
                    && base_branch.as_deref() != Some(branch.name.as_str())
                    && merged_branches.contains(branch.name.as_str())
                    && !exclusions.iter().any(|excluded| excluded == &branch.name)
                    && match configured_remotes.as_ref() {
                        Some(remotes) => branch
                            .upstream
                            .as_deref()
                            .and_then(|upstream| {
                                remotes
                                    .iter()
                                    .filter(|remote| {
                                        matches!(
                                            upstream.strip_prefix(remote.as_str()),
                                            Some(suffix) if suffix.starts_with('/')
                                        )
                                    })
                                    .max_by_key(|remote| remote.len())
                                    .map(String::as_str)
                            })
                            .is_some_and(|resolved| {
                                upstream_remotes.iter().any(|selected| selected == resolved)
                            }),
                        None => true,
                    }
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(candidates)
    }

    pub fn cleanup_local_branch(
        &self,
        branch: &str,
        base_reference: &str,
        exclusions: &[String],
        upstream_remotes: &[String],
    ) -> Result<BranchCleanupOutcome> {
        let still_eligible = self
            .merged_local_branches(base_reference, exclusions, upstream_remotes)?
            .iter()
            .any(|candidate| candidate.name == branch);
        if !still_eligible {
            return Ok(BranchCleanupOutcome {
                branch: branch.to_string(),
                state: BranchCleanupState::Skipped,
                detail: Some(String::from(
                    "branch is no longer eligible, is protected, or no longer matches the filters",
                )),
            });
        }

        match self.delete_local_branch(branch) {
            Ok(()) => Ok(BranchCleanupOutcome {
                branch: branch.to_string(),
                state: BranchCleanupState::Deleted,
                detail: None,
            }),
            Err(error) => Ok(BranchCleanupOutcome {
                branch: branch.to_string(),
                state: BranchCleanupState::Failed,
                detail: Some(error.to_string()),
            }),
        }
    }

    pub fn log(&self, limit: usize) -> Result<Vec<crate::domain::CommitSummary>> {
        crate::git::log::read_log(self, limit)
    }

    pub fn history_for_ref(&self, reference: &str) -> Result<crate::domain::BranchHistory> {
        crate::git::read_branch_history(self, reference)
    }

    pub fn snapshot(&self) -> Result<crate::domain::RepoSnapshot> {
        crate::git::read_snapshot(self)
    }

    pub fn checkout(&self, target: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let local_ref = format!("refs/heads/{target}");
        let local_branch = git.probe(["show-ref", "--verify", "--quiet", local_ref.as_str()])?;

        if local_branch.success() {
            git.run(["switch", "--", target])?;
            return Ok(());
        }

        self.resolve_commit(target)?;
        git.run(["switch", "--detach", "--", target])?;
        Ok(())
    }

    pub fn switch(&self, target: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let local_ref = format!("refs/heads/{target}");
        let local_branch = git.probe(["show-ref", "--verify", "--quiet", local_ref.as_str()])?;
        if !local_branch.success() {
            return Err(GitError::ReferenceNotFound(target.to_string()));
        }
        git.run(["switch", "--", target])?;
        Ok(())
    }

    fn switch_without_overwriting_ignored(&self, target: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let local_ref = format!("refs/heads/{target}");
        let local_branch = git.probe(["show-ref", "--verify", "--quiet", local_ref.as_str()])?;
        if !local_branch.success() {
            return Err(GitError::ReferenceNotFound(target.to_string()));
        }
        git.run(["switch", "--no-overwrite-ignore", "--", target])?;
        Ok(())
    }

    pub fn create_branch(&self, branch: &str, start_point: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let mut args = vec![
            OsString::from("switch"),
            OsString::from("-c"),
            OsString::from(branch),
        ];
        if let Some(start_point) = start_point {
            args.push(OsString::from("--"));
            args.push(OsString::from(start_point));
        }
        git.run(args)?;
        Ok(())
    }

    pub fn delete_local_branch(&self, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        git.run(["branch", "-d", "--", branch])?;
        Ok(())
    }

    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        git.run(["push", "--delete", "--", remote, branch])?;
        Ok(())
    }

    pub(crate) fn delete_remote_branch_to_remote(&self, remote: &str, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        git.run([
            "push",
            "--no-follow-tags",
            "--recurse-submodules=no",
            "--delete",
            "--",
            remote,
            branch,
        ])?;
        Ok(())
    }

    pub fn clone_repository(&self, repository: &str, directory: Option<&Path>) -> Result<()> {
        let git = self.git();
        let path = match directory {
            Some(path) => path.to_path_buf(),
            None => default_clone_path(repository),
        };
        let args = vec![
            OsString::from("clone"),
            OsString::from("--"),
            OsString::from(repository),
            path.as_os_str().to_os_string(),
        ];
        git.run(args)?;
        Ok(())
    }

    pub fn pull(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;

        match (remote, branch) {
            (None, None) => {
                git.run(["pull", "--ff-only"])?;
            }
            (Some(remote), None) => {
                git.run(["pull", "--ff-only", "--", remote])?;
            }
            (remote, Some(branch)) => {
                let remote = remote.unwrap_or("origin");
                git.run(["fetch", "--prune", "--", remote, branch])?;
                let (ahead, behind) = ahead_behind(&git, "HEAD", "FETCH_HEAD")?;
                if behind == 0 {
                    return Ok(());
                }
                if ahead > 0 {
                    return Err(GitError::Diverged { ahead, behind });
                }
                git.run(["merge", "--ff-only", "FETCH_HEAD"])?;
            }
        }
        Ok(())
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;

        match (remote, branch) {
            (None, None) => {
                git.run(["push"])?;
            }
            (Some(remote), None) => {
                git.run(["push", "--", remote])?;
            }
            (remote, Some(branch)) => {
                let remote = remote.unwrap_or("origin");
                let refspec = format!("HEAD:refs/heads/{branch}");
                git.run(["push", "--", remote, refspec.as_str()])?;
            }
        }
        Ok(())
    }

    pub(crate) fn push_branch_to_remote(&self, remote: &str, branch: &str) -> Result<()> {
        let git = self.git();
        git.ensure_repository()?;
        let refspec = format!("HEAD:refs/heads/{branch}");
        git.run([
            "push",
            "--no-follow-tags",
            "--recurse-submodules=no",
            "--",
            remote,
            refspec.as_str(),
        ])?;
        Ok(())
    }
}

fn ahead_behind(git: &super::GitProcess, left: &str, right: &str) -> Result<(u32, u32)> {
    let range = format!("{left}...{right}");
    let output = git.run_text(["rev-list", "--left-right", "--count", range.as_str()])?;
    let mut counts = output.split_whitespace();
    let ahead = counts
        .next()
        .ok_or_else(|| GitError::Parse(String::from("missing ahead count")))?
        .parse::<u32>()
        .map_err(|_| GitError::Parse(String::from("invalid ahead count")))?;
    let behind = counts
        .next()
        .ok_or_else(|| GitError::Parse(String::from("missing behind count")))?
        .parse::<u32>()
        .map_err(|_| GitError::Parse(String::from("invalid behind count")))?;
    Ok((ahead, behind))
}

fn default_clone_path(repository: &str) -> PathBuf {
    let trimmed = repository.trim_end_matches('/');
    let name = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repository")
        .trim_end_matches(".git");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::{default_clone_path, GitClient, ResetMode};
    use crate::test_support::{
        checkout_branch, clone_bare_repo, clone_repo, commit_all, configure_user, create_branch,
        current_dir_lock, init_repo, push_branch, set_remote_head, set_upstream, write_file,
        CurrentDirGuard,
    };
    use std::{path::Path, process::Command};

    fn git_output(repository: &Path, arguments: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap()
    }

    fn checked_git(repository: &Path, arguments: &[&str]) {
        let output = git_output(repository, arguments);
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn reset_fixture() -> (
        tempfile::TempDir,
        crate::test_support::TestRepo,
        GitClient,
        String,
        String,
    ) {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "tracked.txt", "base\n");
        let base = commit_all(&repo, "base");
        write_file(temp.path(), "tracked.txt", "target\n");
        let target = commit_all(&repo, "target");
        create_branch(&repo, "target", target.as_str());
        let client = GitClient::from_path(temp.path());
        (temp, repo, client, base, target)
    }

    #[test]
    fn default_clone_path_handles_https_and_scp_style_urls() {
        assert_eq!(
            default_clone_path("https://example.com/acme/project.git"),
            std::path::PathBuf::from("project")
        );
        assert_eq!(
            default_clone_path("git@example.com:acme/project.git"),
            std::path::PathBuf::from("project")
        );
    }

    #[test]
    fn delete_local_branch_removes_ref_from_repository() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "initial commit");
        create_branch(&repo, "feature/login", "HEAD");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        client.delete_local_branch("feature/login").unwrap();

        assert!(repo.find_branch("feature/login").is_err());
    }

    #[test]
    fn delete_local_branch_refuses_unmerged_commits() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        commit_all(&repo, "initial commit");
        create_branch(&repo, "feature/unique", "HEAD");
        checkout_branch(&repo, "feature/unique");
        write_file(temp.path(), "feature.txt", "unique\n");
        commit_all(&repo, "unique feature commit");
        checkout_branch(&repo, "main");
        let _restore = CurrentDirGuard::push(temp.path());

        let client = GitClient::new();
        assert!(client.delete_local_branch("feature/unique").is_err());

        assert!(repo.find_branch("feature/unique").is_ok());
    }

    #[test]
    fn delete_remote_branch_pushes_refspec_to_remote() {
        let _guard = current_dir_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::TempDir::new().unwrap();
        let seed = temp.path().join("seed");
        let origin = temp.path().join("origin.git");
        let worktree = temp.path().join("worktree");

        let seed_repo = init_repo(&seed, "main");
        configure_user(&seed_repo);
        write_file(&seed, "README.md", "base\n");
        commit_all(&seed_repo, "initial commit");

        let origin_repo = clone_bare_repo(&seed, &origin);
        set_remote_head(&origin_repo, "refs/heads/main");

        let worktree_repo = clone_repo(&origin, &worktree);
        configure_user(&worktree_repo);
        set_upstream(&worktree_repo, "main", "origin/main");

        write_file(&worktree, "feature.txt", "feature\n");
        commit_all(&worktree_repo, "feature work");
        create_branch(&worktree_repo, "feature/login", "HEAD");
        checkout_branch(&worktree_repo, "feature/login");
        push_branch(&worktree_repo, "origin", "feature/login");

        let _restore = CurrentDirGuard::push(&worktree);
        let client = GitClient::new();
        client
            .delete_remote_branch("origin", "feature/login")
            .unwrap();

        assert!(origin_repo
            .find_reference("refs/heads/feature/login")
            .is_err());
    }

    #[test]
    fn compare_commits_resolves_refs_and_does_not_mutate_repository() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        let base = commit_all(&repo, "base");
        write_file(temp.path(), "feature.txt", "feature\n");
        let target = commit_all(&repo, "feature");
        create_branch(&repo, "feature/compare", target.as_str());

        let client = GitClient::from_path(temp.path());
        let head_before = client.resolve_commit("HEAD").unwrap();
        let main_before = repo.find_reference("refs/heads/main").unwrap().target();
        let feature_before = repo
            .find_reference("refs/heads/feature/compare")
            .unwrap()
            .target();
        let status_before = client.status().unwrap();

        let comparison = client
            .compare_commits(base.as_str(), "feature/compare")
            .unwrap();

        assert_eq!(comparison.left_oid, base);
        assert_eq!(comparison.right_oid, target);
        assert!(comparison.summary.contains("feature.txt"));
        assert_eq!(client.resolve_commit("HEAD").unwrap(), head_before);
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            main_before
        );
        assert_eq!(
            repo.find_reference("refs/heads/feature/compare")
                .unwrap()
                .target(),
            feature_before
        );
        assert_eq!(client.status().unwrap(), status_before);

        let equal = client
            .compare_commits("feature/compare", target.as_str())
            .unwrap();
        assert!(equal.summary.contains("No changes"));
        assert!(client
            .compare_commits(base.as_str(), "missing/target")
            .is_err());
    }

    #[test]
    fn compare_commits_bounds_large_path_summaries() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        let base = commit_all(&repo, "base");

        for index in 0..270 {
            write_file(temp.path(), &format!("changed-{index:03}.txt"), "changed\n");
        }
        let target = commit_all(&repo, "many changed paths");
        let comparison = GitClient::from_path(temp.path())
            .compare_commits(base.as_str(), target.as_str())
            .unwrap();

        let shown_paths = comparison
            .summary
            .lines()
            .filter(|line| line.contains(".txt |"))
            .count();
        assert!(shown_paths <= 250, "showed {shown_paths} paths");
        assert!(comparison.summary.contains("270 files changed"));
    }

    #[test]
    fn cherry_pick_applies_to_explicit_local_branch_and_leaves_it_active() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        let base = commit_all(&repo, "base");

        create_branch(&repo, "source", base.as_str());
        checkout_branch(&repo, "source");
        write_file(temp.path(), "source.txt", "from source\n");
        let source_oid = commit_all(&repo, "source change");
        checkout_branch(&repo, "main");
        create_branch(&repo, "destination", base.as_str());

        let client = GitClient::from_path(temp.path());
        let result = client
            .cherry_pick_to_branch(source_oid.as_str(), "destination")
            .unwrap();

        assert_eq!(result.source_oid, source_oid);
        assert_eq!(result.destination, "destination");
        assert_eq!(result.status, super::CherryPickStatus::Applied);
        assert_eq!(client.status().unwrap().branch_name, "destination");
        assert_eq!(
            repo.find_reference("refs/heads/source").unwrap().target(),
            Some(source_oid)
        );
        let source_file = git_output(temp.path(), &["show", "HEAD:source.txt"]);
        assert!(source_file.status.success());
        assert_eq!(
            String::from_utf8_lossy(&source_file.stdout),
            "from source\n"
        );
    }

    #[test]
    fn cherry_pick_rejects_dirty_worktree_before_switching_destination() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "README.md", "base\n");
        let base = commit_all(&repo, "base");

        create_branch(&repo, "source", base.as_str());
        checkout_branch(&repo, "source");
        write_file(temp.path(), "source.txt", "from source\n");
        let source_oid = commit_all(&repo, "source change");
        checkout_branch(&repo, "main");
        create_branch(&repo, "destination", base.as_str());
        write_file(temp.path(), "untracked.txt", "keep me\n");
        let client = GitClient::from_path(temp.path());
        let head_before = client.resolve_commit("HEAD").unwrap();

        let error = client
            .cherry_pick_to_branch(source_oid.as_str(), "destination")
            .unwrap_err();

        assert!(error.to_string().contains("clean"));
        assert_eq!(client.status().unwrap().branch_name, "main");
        assert_eq!(client.resolve_commit("HEAD").unwrap(), head_before);
        assert!(temp.path().join("untracked.txt").exists());
        assert!(
            !git_output(temp.path(), &["rev-parse", "--verify", "CHERRY_PICK_HEAD"])
                .status
                .success()
        );
    }

    #[test]
    fn cherry_pick_conflict_keeps_in_progress_state_on_destination_branch() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(temp.path(), "main");
        configure_user(&repo);
        write_file(temp.path(), "conflict.txt", "base\n");
        let base = commit_all(&repo, "base");

        create_branch(&repo, "source", base.as_str());
        checkout_branch(&repo, "source");
        write_file(temp.path(), "conflict.txt", "source\n");
        let source_oid = commit_all(&repo, "source change");
        checkout_branch(&repo, "main");
        create_branch(&repo, "destination", base.as_str());
        checkout_branch(&repo, "destination");
        write_file(temp.path(), "conflict.txt", "destination\n");
        commit_all(&repo, "destination change");

        let client = GitClient::from_path(temp.path());
        let result = client
            .cherry_pick_to_branch(source_oid.as_str(), "destination")
            .unwrap();

        assert_eq!(result.status, super::CherryPickStatus::Conflict);
        assert_eq!(client.status().unwrap().branch_name, "destination");
        assert!(client
            .status()
            .unwrap()
            .files
            .iter()
            .any(|entry| entry.code.contains('U')));
        assert!(
            git_output(temp.path(), &["rev-parse", "--verify", "CHERRY_PICK_HEAD"])
                .status
                .success()
        );
    }

    #[test]
    fn reset_soft_moves_head_and_preserves_index_and_worktree() {
        let (temp, repo, client, base, target) = reset_fixture();
        write_file(temp.path(), "tracked.txt", "staged\n");
        checked_git(temp.path(), &["add", "--", "tracked.txt"]);
        write_file(temp.path(), "tracked.txt", "worktree\n");

        let result = client
            .reset_to_commit(base.as_str(), ResetMode::Soft, "main", target.as_str())
            .unwrap();

        assert_eq!(result.previous_head, target);
        assert_eq!(result.status, super::ResetStatus::Applied);
        assert_eq!(result.resulting_head, Some(base.clone()));
        assert_eq!(result.mode, ResetMode::Soft);
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            Some(base)
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("tracked.txt")).unwrap(),
            "worktree\n"
        );
        assert_eq!(client.status().unwrap().files[0].code, "MM");
    }

    #[test]
    fn reset_mixed_moves_head_resets_index_and_preserves_worktree() {
        let (temp, repo, client, base, target) = reset_fixture();
        write_file(temp.path(), "tracked.txt", "staged\n");
        checked_git(temp.path(), &["add", "--", "tracked.txt"]);

        let result = client
            .reset_to_commit(base.as_str(), ResetMode::Mixed, "main", target.as_str())
            .unwrap();

        assert_eq!(result.status, super::ResetStatus::Applied);
        assert_eq!(result.resulting_head, Some(base.clone()));
        assert_eq!(result.mode, ResetMode::Mixed);
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            Some(base)
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("tracked.txt")).unwrap(),
            "staged\n"
        );
        assert_eq!(client.status().unwrap().files[0].code, " M");
    }

    #[test]
    fn reset_hard_moves_head_and_resets_index_and_tracked_worktree() {
        let (temp, repo, client, base, target) = reset_fixture();
        write_file(temp.path(), "tracked.txt", "staged\n");
        checked_git(temp.path(), &["add", "--", "tracked.txt"]);
        write_file(temp.path(), "tracked.txt", "worktree\n");

        let result = client
            .reset_to_commit(base.as_str(), ResetMode::Hard, "main", target.as_str())
            .unwrap();

        assert_eq!(result.previous_head, target);
        assert_eq!(result.status, super::ResetStatus::Applied);
        assert_eq!(result.resulting_head, Some(base.clone()));
        assert_eq!(result.mode, ResetMode::Hard);
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            Some(base)
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("tracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "base\n"
        );
        assert!(client.status().unwrap().files.is_empty());
    }

    #[test]
    fn reset_revalidates_expected_branch_and_head_before_mutation() {
        let (_temp, repo, client, base, target) = reset_fixture();

        let wrong_branch = client
            .reset_to_commit(base.as_str(), ResetMode::Soft, "other", target.as_str())
            .unwrap_err();
        assert!(wrong_branch.to_string().contains("branch"));
        assert_eq!(client.resolve_commit("HEAD").unwrap(), target);

        let wrong_head = client
            .reset_to_commit(base.as_str(), ResetMode::Soft, "main", base.as_str())
            .unwrap_err();
        assert!(wrong_head.to_string().contains("HEAD"));
        assert_eq!(client.resolve_commit("HEAD").unwrap(), target);
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            Some(target)
        );
    }

    #[test]
    fn reset_rejects_detached_head_and_unresolved_conflicts() {
        let (temp, repo, client, base, target) = reset_fixture();
        checked_git(temp.path(), &["switch", "--detach", "--", target.as_str()]);

        let detached = client
            .reset_to_commit(base.as_str(), ResetMode::Soft, "main", target.as_str())
            .unwrap_err();
        assert!(detached.to_string().contains("local branch"));
        assert_eq!(client.resolve_commit("HEAD").unwrap(), target);

        checkout_branch(&repo, "main");
        write_file(temp.path(), "conflict.txt", "base\n");
        let base = commit_all(&repo, "conflict base");
        create_branch(&repo, "source", base.as_str());
        checkout_branch(&repo, "source");
        write_file(temp.path(), "conflict.txt", "source\n");
        let source = commit_all(&repo, "source conflict");
        checkout_branch(&repo, "main");
        create_branch(&repo, "destination", base.as_str());
        checkout_branch(&repo, "destination");
        write_file(temp.path(), "conflict.txt", "destination\n");
        commit_all(&repo, "destination conflict");
        let conflict_head = client.resolve_commit("HEAD").unwrap();
        let pick = client
            .cherry_pick_to_branch(source.as_str(), "destination")
            .unwrap();
        assert_eq!(pick.status, super::CherryPickStatus::Conflict);

        let blocked = client
            .reset_to_commit(
                "main",
                ResetMode::Soft,
                "destination",
                conflict_head.as_str(),
            )
            .unwrap_err();
        assert!(blocked.to_string().contains("unresolved"));
        assert_eq!(client.resolve_commit("HEAD").unwrap(), conflict_head);
    }
}
