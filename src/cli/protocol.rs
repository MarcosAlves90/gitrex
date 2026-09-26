use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::domain::{
    repository_context as domain_context, BranchInfo, BranchKind as DomainBranchKind,
    CommitSummary, GitError, OperationClass, OperationExecution, OperationFailureKind,
    OperationPlan, RepoStatus, StatusEntry, OPERATION_CLASSIFICATIONS,
};

pub(crate) const PROTOCOL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct Envelope<T> {
    schema_version: u32,
    operation: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ProtocolError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

impl<T> Envelope<T> {
    fn success(operation: &'static str, data: T) -> Self {
        Self::success_with_warnings(operation, data, None)
    }

    fn success_with_warnings(
        operation: &'static str,
        data: T,
        warnings: Option<Vec<String>>,
    ) -> Self {
        Self {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            operation,
            ok: true,
            data: Some(data),
            error: None,
            warnings,
        }
    }

    fn failure(operation: &'static str, error: ProtocolError) -> Self {
        Self {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            operation,
            ok: false,
            data: None,
            error: Some(error),
            warnings: None,
        }
    }

    fn outcome(operation: &'static str, data: T, error: Option<ProtocolError>) -> Self {
        Self {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            operation,
            ok: error.is_none(),
            data: Some(data),
            error,
            warnings: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProtocolError {
    code: ErrorCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ErrorCode {
    GitNotInstalled,
    NotARepository,
    ReferenceNotFound,
    CommandFailed,
    Diverged,
    PreconditionChanged,
    VerificationFailed,
    BackendError,
    ParseError,
    InvalidUtf8,
}

fn map_git_error(error: &GitError) -> ProtocolError {
    let (code, retryable, details) = match error {
        GitError::GitNotInstalled => (ErrorCode::GitNotInstalled, Some(false), None),
        GitError::CommandFailed {
            command, exit_code, ..
        } => {
            let mut details = BTreeMap::new();
            details.insert("command".to_string(), json!(command));
            if let Some(exit_code) = exit_code {
                details.insert("exit_code".to_string(), json!(exit_code));
            }
            (ErrorCode::CommandFailed, None, Some(details))
        }
        GitError::Io(_) => (ErrorCode::BackendError, None, None),
        GitError::NotRepository => (ErrorCode::NotARepository, Some(false), None),
        GitError::ReferenceNotFound(reference) => {
            let details = BTreeMap::from([("reference".to_string(), json!(reference))]);
            (ErrorCode::ReferenceNotFound, Some(false), Some(details))
        }
        GitError::Diverged { ahead, behind } => {
            let details = BTreeMap::from([
                ("ahead".to_string(), json!(ahead)),
                ("behind".to_string(), json!(behind)),
            ]);
            (ErrorCode::Diverged, Some(false), Some(details))
        }
        GitError::PreconditionChanged { details } => {
            let details = BTreeMap::from([
                ("expected_head".to_string(), json!(&details.expected_head)),
                (
                    "expected_branch".to_string(),
                    json!(&details.expected_branch),
                ),
                (
                    "expected_upstream".to_string(),
                    json!(&details.expected_upstream),
                ),
                ("observed_head".to_string(), json!(&details.observed_head)),
                (
                    "observed_branch".to_string(),
                    json!(&details.observed_branch),
                ),
                (
                    "observed_upstream".to_string(),
                    json!(&details.observed_upstream),
                ),
                (
                    "changed_reference".to_string(),
                    json!(&details.changed_reference),
                ),
                (
                    "expected_reference_commit_id".to_string(),
                    json!(&details.expected_reference_commit_id),
                ),
                (
                    "observed_reference_commit_id".to_string(),
                    json!(&details.observed_reference_commit_id),
                ),
            ]);
            (ErrorCode::PreconditionChanged, Some(false), Some(details))
        }
        GitError::VerificationFailed(detail) => {
            let details = BTreeMap::from([("verification".to_string(), json!(detail))]);
            (ErrorCode::VerificationFailed, Some(false), Some(details))
        }
        GitError::Backend(_) => (ErrorCode::BackendError, None, None),
        GitError::Parse(_) => (ErrorCode::ParseError, Some(false), None),
        GitError::Utf8 => (ErrorCode::InvalidUtf8, Some(false), None),
    };

    ProtocolError {
        code,
        message: error.to_string(),
        retryable,
        details,
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusData {
    branch_name: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    files: Vec<StatusFile>,
}

#[derive(Debug, Serialize)]
struct StatusFile {
    code: String,
    path: String,
}

impl From<RepoStatus> for StatusData {
    fn from(status: RepoStatus) -> Self {
        Self {
            branch_name: status.branch_name,
            upstream: status.upstream,
            ahead: status.ahead,
            behind: status.behind,
            files: status.files.into_iter().map(StatusFile::from).collect(),
        }
    }
}

impl From<StatusEntry> for StatusFile {
    fn from(file: StatusEntry) -> Self {
        Self {
            code: file.code,
            path: file.path,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct BranchData {
    branches: Vec<Branch>,
}

#[derive(Debug, Serialize)]
struct Branch {
    name: String,
    current: bool,
    upstream: Option<String>,
    commit: String,
    subject: String,
    kind: BranchKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum BranchKind {
    Local,
    Remote,
}

impl From<BranchInfo> for Branch {
    fn from(branch: BranchInfo) -> Self {
        Self {
            name: branch.name,
            current: branch.current,
            upstream: branch.upstream,
            commit: branch.commit,
            subject: branch.subject,
            kind: match branch.kind {
                DomainBranchKind::Local => BranchKind::Local,
                DomainBranchKind::Remote => BranchKind::Remote,
            },
        }
    }
}

impl From<Vec<BranchInfo>> for BranchData {
    fn from(branches: Vec<BranchInfo>) -> Self {
        Self {
            branches: branches.into_iter().map(Branch::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct LogData {
    commits: Vec<Commit>,
}

#[derive(Debug, Serialize)]
struct Commit {
    hash: String,
    author: String,
    date: String,
    subject: String,
}

impl From<CommitSummary> for Commit {
    fn from(commit: CommitSummary) -> Self {
        Self {
            hash: commit.hash,
            author: commit.author,
            date: commit.date,
            subject: commit.subject,
        }
    }
}

impl From<Vec<CommitSummary>> for LogData {
    fn from(commits: Vec<CommitSummary>) -> Self {
        Self {
            commits: commits.into_iter().map(Commit::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct InspectData {
    scope: String,
    repository: RepositoryIdentityData,
    head: HeadData,
    upstream: Option<UpstreamData>,
    working_tree: WorkingTreeData,
    conflicts: ConflictData,
    #[serde(skip_serializing_if = "Option::is_none")]
    branches: Option<BranchListData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history: Option<CommitListData>,
}

#[derive(Debug, Serialize)]
struct RepositoryIdentityData {
    root: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct HeadData {
    commit: Option<String>,
    branch: Option<String>,
    detached: bool,
}

#[derive(Debug, Serialize)]
struct UpstreamData {
    reference: String,
    commit: Option<String>,
    ahead: u64,
    behind: u64,
}

#[derive(Debug, Serialize)]
struct ContextPathData {
    path: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct PathGroupData {
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<ContextPathData>>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct UntrackedPathGroupData {
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<String>>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct WorkingTreeData {
    clean: bool,
    staged: PathGroupData,
    unstaged: PathGroupData,
    untracked: UntrackedPathGroupData,
}

#[derive(Debug, Serialize)]
struct ConflictData {
    has_conflicts: bool,
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<ContextPathData>>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct BranchListData {
    count: usize,
    branches: Vec<ContextBranchData>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct ContextBranchData {
    name: String,
    current: bool,
    upstream: Option<String>,
    commit: String,
    subject: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct CommitListData {
    count: usize,
    commits: Vec<Commit>,
    truncated: bool,
}

impl From<domain_context::RepositoryContext> for InspectData {
    fn from(context: domain_context::RepositoryContext) -> Self {
        Self {
            scope: context.scope,
            repository: RepositoryIdentityData {
                root: context.repository.root,
                name: context.repository.name,
            },
            head: HeadData {
                commit: context.head.commit,
                branch: context.head.branch,
                detached: context.head.detached,
            },
            upstream: context.upstream.map(|upstream| UpstreamData {
                reference: upstream.reference,
                commit: upstream.commit,
                ahead: upstream.ahead,
                behind: upstream.behind,
            }),
            working_tree: WorkingTreeData::from(context.working_tree),
            conflicts: ConflictData::from(context.conflicts),
            branches: context.branches.map(BranchListData::from),
            history: context.history.map(CommitListData::from),
        }
    }
}

impl From<domain_context::WorkingTreeContext> for WorkingTreeData {
    fn from(working_tree: domain_context::WorkingTreeContext) -> Self {
        Self {
            clean: working_tree.clean,
            staged: PathGroupData::from(working_tree.staged),
            unstaged: PathGroupData::from(working_tree.unstaged),
            untracked: UntrackedPathGroupData {
                count: working_tree.untracked.count,
                paths: working_tree
                    .untracked
                    .paths
                    .map(|paths| paths.into_iter().map(|path| path.path).collect()),
                truncated: working_tree.untracked.truncated,
            },
        }
    }
}

impl From<domain_context::PathGroup> for PathGroupData {
    fn from(group: domain_context::PathGroup) -> Self {
        Self {
            count: group.count,
            paths: group
                .paths
                .map(|paths| paths.into_iter().map(ContextPathData::from).collect()),
            truncated: group.truncated,
        }
    }
}

impl From<domain_context::ContextPath> for ContextPathData {
    fn from(path: domain_context::ContextPath) -> Self {
        Self {
            path: path.path,
            status: path.status,
        }
    }
}

impl From<domain_context::ConflictContext> for ConflictData {
    fn from(conflicts: domain_context::ConflictContext) -> Self {
        Self {
            has_conflicts: conflicts.has_conflicts,
            count: conflicts.count,
            paths: conflicts
                .paths
                .map(|paths| paths.into_iter().map(ContextPathData::from).collect()),
            truncated: conflicts.truncated,
        }
    }
}

impl From<domain_context::BranchList> for BranchListData {
    fn from(branches: domain_context::BranchList) -> Self {
        Self {
            count: branches.count,
            branches: branches
                .branches
                .into_iter()
                .map(|branch| ContextBranchData {
                    name: branch.name,
                    current: branch.current,
                    upstream: branch.upstream,
                    commit: branch.commit,
                    subject: branch.subject,
                    kind: branch.kind,
                })
                .collect(),
            truncated: branches.truncated,
        }
    }
}

impl From<domain_context::CommitList> for CommitListData {
    fn from(commits: domain_context::CommitList) -> Self {
        Self {
            count: commits.count,
            commits: commits.commits.into_iter().map(Commit::from).collect(),
            truncated: commits.truncated,
        }
    }
}

impl From<domain_context::ContextCommit> for Commit {
    fn from(commit: domain_context::ContextCommit) -> Self {
        Self {
            hash: commit.hash,
            author: commit.author,
            date: commit.date,
            subject: commit.subject,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct DiffData {
    mode: String,
    left_commit: Option<String>,
    right_commit: Option<String>,
    changed_files: Vec<ChangedFileData>,
    changed_file_count: usize,
    changed_files_truncated: bool,
    additions: u64,
    deletions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    patch: Option<PatchData>,
}

#[derive(Debug, Serialize)]
struct ChangedFileData {
    path: String,
    status: String,
    additions: Option<u64>,
    deletions: Option<u64>,
}

#[derive(Debug, Serialize)]
struct PatchData {
    text: String,
    returned_bytes: usize,
    total_bytes: u64,
    truncated: bool,
}

impl From<domain_context::DiffReport> for DiffData {
    fn from(diff: domain_context::DiffReport) -> Self {
        Self {
            mode: diff.mode,
            left_commit: diff.left_commit,
            right_commit: diff.right_commit,
            changed_files: diff
                .changed_files
                .into_iter()
                .map(|file| ChangedFileData {
                    path: file.path,
                    status: file.status,
                    additions: file.additions,
                    deletions: file.deletions,
                })
                .collect(),
            changed_file_count: diff.changed_file_count,
            changed_files_truncated: diff.changed_files_truncated,
            additions: diff.additions,
            deletions: diff.deletions,
            patch: diff.patch.map(|patch| PatchData {
                text: patch.text,
                returned_bytes: patch.returned_bytes,
                total_bytes: patch.total_bytes,
                truncated: patch.truncated,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ShowData {
    commit: CommitDetailsData,
    diff: DiffData,
}

#[derive(Debug, Serialize)]
struct CommitDetailsData {
    hash: String,
    parents: Vec<String>,
    author: String,
    timestamp: String,
    subject: String,
    body: String,
}

impl From<domain_context::ShowReport> for ShowData {
    fn from(show: domain_context::ShowReport) -> Self {
        Self {
            commit: CommitDetailsData {
                hash: show.commit.hash,
                parents: show.commit.parents,
                author: show.commit.author,
                timestamp: show.commit.timestamp,
                subject: show.commit.subject,
                body: show.commit.body,
            },
            diff: DiffData::from(show.diff),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CompareData {
    left_reference: String,
    left_commit: String,
    right_reference: String,
    right_commit: String,
    merge_bases: Vec<String>,
    ahead: u64,
    behind: u64,
    left_only_commits: Vec<Commit>,
    left_only_commit_count: u64,
    left_only_commits_truncated: bool,
    right_only_commits: Vec<Commit>,
    right_only_commit_count: u64,
    right_only_commits_truncated: bool,
    diff: DiffData,
}

impl From<domain_context::CompareReport> for CompareData {
    fn from(compare: domain_context::CompareReport) -> Self {
        Self {
            left_reference: compare.left_reference,
            left_commit: compare.left_commit,
            right_reference: compare.right_reference,
            right_commit: compare.right_commit,
            merge_bases: compare.merge_bases,
            ahead: compare.ahead,
            behind: compare.behind,
            left_only_commits: compare
                .left_only_commits
                .into_iter()
                .map(Commit::from)
                .collect(),
            left_only_commit_count: compare.left_only_commit_count,
            left_only_commits_truncated: compare.left_only_commits_truncated,
            right_only_commits: compare
                .right_only_commits
                .into_iter()
                .map(Commit::from)
                .collect(),
            right_only_commit_count: compare.right_only_commit_count,
            right_only_commits_truncated: compare.right_only_commits_truncated,
            diff: DiffData::from(compare.diff),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ChangeContextData {
    base: CommitReferenceData,
    head: CommitReferenceData,
    merge_bases: Vec<String>,
    commits: CommitListData,
    diff: DiffData,
    working_tree: WorkingTreeData,
    conflicts: ConflictData,
}

#[derive(Debug, Serialize)]
struct CommitReferenceData {
    reference: String,
    commit: String,
}

impl From<domain_context::ChangeContextReport> for ChangeContextData {
    fn from(context: domain_context::ChangeContextReport) -> Self {
        Self {
            base: CommitReferenceData {
                reference: context.base.reference,
                commit: context.base.commit,
            },
            head: CommitReferenceData {
                reference: context.head.reference,
                commit: context.head.commit,
            },
            merge_bases: context.merge_bases,
            commits: CommitListData::from(context.commits),
            diff: DiffData::from(context.diff),
            working_tree: WorkingTreeData::from(context.working_tree),
            conflicts: ConflictData::from(context.conflicts),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CapabilitiesData {
    gitrex_version: &'static str,
    protocol_schema_version: u32,
    supported_output_formats: &'static [&'static str],
    operations: Vec<Operation>,
    authorization: Authorization,
}

#[derive(Debug, Serialize)]
struct Operation {
    name: &'static str,
    effects: Vec<&'static str>,
    classification: OperationClassificationData,
    output_formats: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct OperationClassificationData {
    effects: &'static [OperationClass],
    risk_class: OperationClass,
}

#[derive(Debug, Serialize)]
struct Authorization {
    granted: bool,
    note: &'static str,
}

pub(crate) fn capabilities() -> CapabilitiesData {
    const TEXT_JSON: &[&str] = &["text", "json"];
    let operations = OPERATION_CLASSIFICATIONS
        .iter()
        .map(|classification| Operation {
            name: classification.name,
            effects: classification
                .effects
                .iter()
                .filter_map(legacy_effect_name)
                .collect(),
            classification: OperationClassificationData {
                effects: classification.effects,
                risk_class: classification.risk_class,
            },
            output_formats: match classification.name {
                "clone" => &["text"],
                "checkout-detached"
                | "cherry-pick"
                | "delete-local-branch"
                | "delete-remote-branch"
                | "reset"
                | "tui" => &[],
                _ => TEXT_JSON,
            },
        })
        .collect();

    CapabilitiesData {
        gitrex_version: env!("CARGO_PKG_VERSION"),
        protocol_schema_version: PROTOCOL_SCHEMA_VERSION,
        supported_output_formats: TEXT_JSON,
        operations,
        authorization: Authorization {
            granted: false,
            note: "Listing an operation does not authorize it; normal CLI and repository checks still apply.",
        },
    }
}

pub(crate) fn print_success<T: Serialize>(operation: &'static str, data: T) -> anyhow::Result<()> {
    write_json(&Envelope::success(operation, data))
}

pub(crate) fn print_failure(operation: &'static str, error: &GitError) -> anyhow::Result<()> {
    write_json(&Envelope::<Value>::failure(operation, map_git_error(error)))
}

pub(crate) fn print_operation_plan(
    operation: &'static str,
    plan: OperationPlan,
) -> anyhow::Result<()> {
    print_success(operation, plan)
}

pub(crate) fn print_operation_execution(
    operation: &'static str,
    execution: OperationExecution,
) -> anyhow::Result<()> {
    let error = execution
        .failure
        .as_ref()
        .map(|failure| match failure.kind {
            OperationFailureKind::Execution | OperationFailureKind::Verification => {
                map_git_error(&failure.error)
            }
        });
    write_json(&Envelope::outcome(operation, execution.receipt, error))
}

fn write_json<T: Serialize>(envelope: &Envelope<T>) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(envelope)?);
    Ok(())
}

pub(crate) fn print_capabilities_text(capabilities: &CapabilitiesData) {
    println!(
        "GitRex {} (protocol schema {})",
        capabilities.gitrex_version, capabilities.protocol_schema_version
    );
    println!(
        "output formats: {}",
        capabilities.supported_output_formats.join(", ")
    );
    println!("operations:");
    for operation in &capabilities.operations {
        let classes = operation
            .classification
            .effects
            .iter()
            .map(class_name)
            .collect::<Vec<_>>()
            .join(", ");
        let formats = operation.output_formats.join(", ");
        println!(
            "  {} [{classes}] risk: {} formats: {formats}",
            operation.name,
            class_name(&operation.classification.risk_class)
        );
    }
    println!("{}", capabilities.authorization.note);
}

fn class_name(class: &OperationClass) -> &'static str {
    match class {
        OperationClass::ReadOnly => "read_only",
        OperationClass::LocalMutation => "local_mutation",
        OperationClass::NetworkRead => "network_read",
        OperationClass::RemoteMutation => "remote_mutation",
        OperationClass::Destructive => "destructive",
    }
}

fn legacy_effect_name(class: &OperationClass) -> Option<&'static str> {
    match class {
        OperationClass::ReadOnly => Some("read_only"),
        OperationClass::LocalMutation => Some("local_mutation"),
        OperationClass::NetworkRead => Some("network_access"),
        OperationClass::RemoteMutation => Some("remote_mutation"),
        OperationClass::Destructive => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{map_git_error, Envelope, ErrorCode, PROTOCOL_SCHEMA_VERSION};
    use crate::domain::{GitError, PreconditionChange};
    use serde_json::json;

    #[test]
    fn maps_every_git_error_to_a_stable_code() {
        let cases = [
            (GitError::GitNotInstalled, ErrorCode::GitNotInstalled),
            (GitError::NotRepository, ErrorCode::NotARepository),
            (
                GitError::ReferenceNotFound("main".to_string()),
                ErrorCode::ReferenceNotFound,
            ),
            (
                GitError::CommandFailed {
                    command: "fetch".to_string(),
                    exit_code: Some(128),
                    stderr: "remote unavailable".to_string(),
                },
                ErrorCode::CommandFailed,
            ),
            (
                GitError::Diverged {
                    ahead: 2,
                    behind: 3,
                },
                ErrorCode::Diverged,
            ),
            (
                GitError::Backend("backend".to_string()),
                ErrorCode::BackendError,
            ),
            (
                GitError::Parse("malformed".to_string()),
                ErrorCode::ParseError,
            ),
            (GitError::Utf8, ErrorCode::InvalidUtf8),
            (
                GitError::Io(std::io::Error::other("io")),
                ErrorCode::BackendError,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(map_git_error(&error).code, expected);
        }
    }

    #[test]
    fn error_code_is_separate_from_message_and_divergence_has_structured_details() {
        let error = map_git_error(&GitError::Diverged {
            ahead: 2,
            behind: 3,
        });
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(serialized["code"], "DIVERGED");
        assert_eq!(
            serialized["message"],
            "pull cannot fast-forward because histories diverged (+2 -3)"
        );
        assert_eq!(serialized["retryable"], false);
        assert_eq!(serialized["details"]["ahead"], 2);
        assert_eq!(serialized["details"]["behind"], 3);
    }

    #[test]
    fn unknown_retryability_is_omitted_and_command_details_are_structured() {
        let error = map_git_error(&GitError::CommandFailed {
            command: "fetch".to_string(),
            exit_code: None,
            stderr: "temporary failure".to_string(),
        });
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(serialized["code"], "COMMAND_FAILED");
        assert_eq!(serialized["details"]["command"], "fetch");
        assert!(serialized["details"].get("exit_code").is_none());
        assert!(serialized.get("retryable").is_none());
    }

    #[test]
    fn precondition_change_preserves_expected_and_observed_state_details() {
        let error = map_git_error(&GitError::PreconditionChanged {
            details: Box::new(PreconditionChange {
                expected_head: Some("a".repeat(40)),
                expected_branch: Some("main".to_string()),
                expected_upstream: Some("origin/main".to_string()),
                observed_head: Some("b".repeat(40)),
                observed_branch: Some("feature".to_string()),
                observed_upstream: Some("origin/feature".to_string()),
                changed_reference: Some("refs/heads/main".to_string()),
                expected_reference_commit_id: Some("a".repeat(40)),
                observed_reference_commit_id: Some("b".repeat(40)),
            }),
        });
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(serialized["code"], "PRECONDITION_CHANGED");
        assert_eq!(serialized["retryable"], false);
        assert_eq!(serialized["details"]["expected_branch"], "main");
        assert_eq!(serialized["details"]["observed_branch"], "feature");
        assert_eq!(
            serialized["details"]["changed_reference"],
            "refs/heads/main"
        );
        assert_eq!(
            serialized["details"]["expected_reference_commit_id"],
            "a".repeat(40)
        );
        assert_eq!(
            serialized["details"]["observed_reference_commit_id"],
            "b".repeat(40)
        );
    }

    #[test]
    fn envelope_omits_absent_fields_and_supports_warnings() {
        let success = Envelope::success("status", json!({"branch_name": "main"}));
        let success_value = serde_json::to_value(success).unwrap();
        assert_eq!(success_value["schema_version"], PROTOCOL_SCHEMA_VERSION);
        assert_eq!(success_value["ok"], true);
        assert!(success_value.get("error").is_none());
        assert!(success_value.get("warnings").is_none());

        let warnings = Envelope::success_with_warnings(
            "status",
            json!({"branch_name": "main"}),
            Some(vec!["partial data".to_string()]),
        );
        assert_eq!(
            serde_json::to_value(warnings).unwrap()["warnings"],
            json!(["partial data"])
        );
    }

    #[test]
    fn divergence_error_details_use_deterministic_key_order() {
        let error = map_git_error(&GitError::Diverged {
            ahead: 2,
            behind: 3,
        });
        let details = error.details.unwrap();
        assert_eq!(
            details.keys().cloned().collect::<Vec<_>>(),
            vec!["ahead".to_string(), "behind".to_string()]
        );
    }
}
