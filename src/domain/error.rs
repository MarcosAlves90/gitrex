use thiserror::Error;

pub type Result<T> = std::result::Result<T, GitError>;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Io(#[from] std::io::Error),
    #[error("repository not found")]
    NotRepository,
    #[error("git backend error: {0}")]
    Backend(String),
    #[error("failed to parse git output: {0}")]
    Parse(String),
    #[error("invalid utf-8 in git output")]
    Utf8,
}
