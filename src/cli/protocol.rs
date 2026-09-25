use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::domain::{
    BranchInfo, BranchKind as DomainBranchKind, CommitSummary, GitError, RepoStatus, StatusEntry,
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
    effects: &'static [Effect],
    output_formats: &'static [&'static str],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Effect {
    ReadOnly,
    LocalMutation,
    NetworkAccess,
    RemoteMutation,
}

#[derive(Debug, Serialize)]
struct Authorization {
    granted: bool,
    note: &'static str,
}

pub(crate) fn capabilities() -> CapabilitiesData {
    use Effect::{LocalMutation as Local, NetworkAccess as Network, ReadOnly, RemoteMutation};

    const TEXT_JSON: &[&str] = &["text", "json"];
    const TEXT: &[&str] = &["text"];
    const NO_FORMATS: &[&str] = &[];
    const READ: &[Effect] = &[ReadOnly];
    const LOCAL: &[Effect] = &[Local];
    const LOCAL_NETWORK: &[Effect] = &[Local, Network];
    const NETWORK_REMOTE: &[Effect] = &[Network, RemoteMutation];
    const TUI_EFFECTS: &[Effect] = &[ReadOnly, Local, Network, RemoteMutation];

    let operations = vec![
        Operation {
            name: "branch",
            effects: READ,
            output_formats: TEXT_JSON,
        },
        Operation {
            name: "capabilities",
            effects: READ,
            output_formats: TEXT_JSON,
        },
        Operation {
            name: "checkout",
            effects: LOCAL,
            output_formats: TEXT,
        },
        Operation {
            name: "cleanup",
            effects: LOCAL,
            output_formats: TEXT,
        },
        Operation {
            name: "clone",
            effects: LOCAL_NETWORK,
            output_formats: TEXT,
        },
        Operation {
            name: "create-branch",
            effects: LOCAL,
            output_formats: TEXT,
        },
        Operation {
            name: "fetch",
            effects: LOCAL_NETWORK,
            output_formats: TEXT,
        },
        Operation {
            name: "log",
            effects: READ,
            output_formats: TEXT_JSON,
        },
        Operation {
            name: "pull",
            effects: LOCAL_NETWORK,
            output_formats: TEXT,
        },
        Operation {
            name: "push",
            effects: NETWORK_REMOTE,
            output_formats: TEXT,
        },
        Operation {
            name: "status",
            effects: READ,
            output_formats: TEXT_JSON,
        },
        Operation {
            name: "switch",
            effects: LOCAL,
            output_formats: TEXT,
        },
        Operation {
            name: "tui",
            effects: TUI_EFFECTS,
            output_formats: NO_FORMATS,
        },
    ];

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
        let effects = operation
            .effects
            .iter()
            .map(|effect| match effect {
                Effect::ReadOnly => "read_only",
                Effect::LocalMutation => "local_mutation",
                Effect::NetworkAccess => "network_access",
                Effect::RemoteMutation => "remote_mutation",
            })
            .collect::<Vec<_>>()
            .join(", ");
        let formats = operation.output_formats.join(", ");
        println!("  {} [{effects}] formats: {formats}", operation.name);
    }
    println!("{}", capabilities.authorization.note);
}

#[cfg(test)]
mod tests {
    use super::{map_git_error, Envelope, ErrorCode, PROTOCOL_SCHEMA_VERSION};
    use crate::domain::GitError;
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
