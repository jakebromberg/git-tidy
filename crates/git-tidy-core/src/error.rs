use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("git command failed: {command}: {message}")]
    GitCommand { command: String, message: String },

    #[error("could not determine default branch for {repo}")]
    NoDefaultBranch { repo: PathBuf },

    #[error("not a valid worktree .git file: {path}")]
    InvalidGitFile { path: PathBuf },

    #[error("directory not found: {path}")]
    DirectoryNotFound { path: PathBuf },

    #[error("worktree removal failed: {path}: {reason}")]
    RemovalFailed { path: PathBuf, reason: String },

    #[error("branch deletion failed: {branch} in {repo}: {reason}")]
    BranchDeletionFailed {
        repo: PathBuf,
        branch: String,
        reason: String,
    },

    #[error("dirty worktrees blocked removal (rerun with --force)")]
    DirtyBlocked,

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Exit code 1 for general errors, 2 for dirty-blocked.
impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::DirtyBlocked => 2,
            _ => 1,
        }
    }
}
