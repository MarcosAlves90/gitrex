use thiserror::Error;

pub type Result<T> = std::result::Result<T, GitError>;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Io(#[from] std::io::Error),
    #[error("git command failed with exit code {code:?}: {stderr}")]
    CommandFailed {
        code: Option<i32>,
        stderr: String,
    },
    #[error("repository not found")]
    NotRepository,
    #[error("failed to parse git output: {0}")]
    Parse(String),
    #[error("invalid utf-8 in git output")]
    Utf8,
}
