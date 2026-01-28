use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapVaultError {
    #[error("Repository already exists: {0}")]
    RepoAlreadyExists(PathBuf),

    #[error("Repository not found: {0}")]
    RepoNotFound(PathBuf),

    #[error("Not a SnapVault repository: missing config at {0}")]
    InvalidRepo(PathBuf),

    #[error("Unsupported repository version: {version} (expected {expected})")]
    UnsupportedVersion { version: u32, expected: u32 },

    #[error("Invalid snapshot ID: {0}")]
    InvalidSnapshotId(String),

    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("Snapshot already exists: {0}")]
    SnapshotAlreadyExists(String),

    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Unsafe path: {0}")]
    UnsafePath(String),

    #[error("Source path does not exist: {0}")]
    SourceNotFound(PathBuf),

    #[error("Source path is not a directory: {0}")]
    SourceNotDirectory(PathBuf),

    #[error("Destination is not empty: {0}")]
    DestinationNotEmpty(PathBuf),

    #[error("File too large: {size} bytes (max: {max} bytes)")]
    FileTooLarge { size: u64, max: u64 },

    #[error("No snapshots found in repository")]
    NoSnapshots,

    #[error("Must specify either --snapshot or --all")]
    DeleteArgsRequired,

    #[error("Cannot specify both --snapshot and --all")]
    DeleteArgsConflict,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SnapVaultError>;
