use crate::error::{Result, SnapVaultError};
use std::path::Path;

// Constants for security limits
pub const MAX_CONFIG_SIZE: u64 = 1024 * 1024; // 1MB
pub const MAX_MANIFEST_SIZE: u64 = 100 * 1024 * 1024; // 100MB
pub const SNAPSHOT_UUID_LEN: usize = 8;

/// Validate snapshot ID to prevent path traversal
pub fn validate_snapshot_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(SnapVaultError::InvalidSnapshotId(
            "Snapshot ID cannot be empty".to_string(),
        ));
    }
    if id.contains('\0') {
        return Err(SnapVaultError::InvalidSnapshotId(
            "Snapshot ID contains null byte".to_string(),
        ));
    }
    if id.contains('/') || id.contains('\\') {
        return Err(SnapVaultError::InvalidSnapshotId(
            "Snapshot ID cannot contain path separators".to_string(),
        ));
    }
    if id.starts_with('.') {
        return Err(SnapVaultError::InvalidSnapshotId(
            "Snapshot ID cannot start with dot".to_string(),
        ));
    }
    Ok(())
}

/// Check if a path is safe (no traversal, no absolute paths, no null bytes)
pub fn is_safe_path(path_str: &str) -> bool {
    // Check for null bytes (security: null byte injection)
    if path_str.contains('\0') {
        return false;
    }

    let path = Path::new(path_str);
    if path.is_absolute() {
        return false;
    }

    for comp in path.components() {
        match comp {
            std::path::Component::Normal(_) => {}
            std::path::Component::ParentDir => return false,
            std::path::Component::CurDir => {} // allow .
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_snapshot_id_valid() {
        assert!(validate_snapshot_id("20240101T120000.000Z-abc123").is_ok());
        assert!(validate_snapshot_id("snapshot-123").is_ok());
    }

    #[test]
    fn test_validate_snapshot_id_empty() {
        assert!(validate_snapshot_id("").is_err());
    }

    #[test]
    fn test_validate_snapshot_id_null_byte() {
        assert!(validate_snapshot_id("snap\0shot").is_err());
    }

    #[test]
    fn test_validate_snapshot_id_path_separator() {
        assert!(validate_snapshot_id("snap/shot").is_err());
        assert!(validate_snapshot_id("snap\\shot").is_err());
    }

    #[test]
    fn test_validate_snapshot_id_dot_prefix() {
        assert!(validate_snapshot_id(".snapshot").is_err());
    }

    #[test]
    fn test_is_safe_path_valid() {
        assert!(is_safe_path("file.txt"));
        assert!(is_safe_path("dir/file.txt"));
        assert!(is_safe_path("./file.txt"));
    }

    #[test]
    fn test_is_safe_path_null_byte() {
        assert!(!is_safe_path("file\0.txt"));
    }

    #[test]
    fn test_is_safe_path_absolute() {
        assert!(!is_safe_path("/etc/passwd"));
        #[cfg(windows)]
        assert!(!is_safe_path("C:\\Windows\\System32"));
    }

    #[test]
    fn test_is_safe_path_parent_dir() {
        assert!(!is_safe_path("../etc/passwd"));
        assert!(!is_safe_path("dir/../../etc/passwd"));
    }
}
