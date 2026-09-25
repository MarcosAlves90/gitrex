use thiserror::Error;

pub type Result<T> = std::result::Result<T, GitError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreconditionChange {
    pub expected_head: Option<String>,
    pub expected_branch: Option<String>,
    pub expected_upstream: Option<String>,
    pub observed_head: Option<String>,
    pub observed_branch: Option<String>,
    pub observed_upstream: Option<String>,
    pub changed_reference: Option<String>,
    pub expected_reference_commit_id: Option<String>,
    pub observed_reference_commit_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git executable was not found")]
    GitNotInstalled,
    #[error("git command `{command}` failed (exit {exit_code:?}): {stderr}")]
    CommandFailed {
        command: String,
        exit_code: Option<i32>,
        stderr: String,
    },
    #[error("failed to run git: {0}")]
    Io(#[from] std::io::Error),
    #[error("repository not found")]
    NotRepository,
    #[error("git reference not found: {0}")]
    ReferenceNotFound(String),
    #[error("pull cannot fast-forward because histories diverged (+{ahead} -{behind})")]
    Diverged { ahead: u32, behind: u32 },
    #[error("PRECONDITION_CHANGED: repository state no longer matches the expected values")]
    PreconditionChanged { details: Box<PreconditionChange> },
    #[error("VERIFICATION_FAILED: {0}")]
    VerificationFailed(String),
    #[error("git backend error: {0}")]
    Backend(String),
    #[error("failed to parse git output: {0}")]
    Parse(String),
    #[error("invalid utf-8 in git output")]
    Utf8,
}
