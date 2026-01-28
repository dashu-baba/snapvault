use crate::error::{Result, SnapVaultError};
use crate::repository::snapshot::SnapshotManifest;
use crate::repository::Repository;
use crate::utils::validate_snapshot_id;
use log::{info, warn};
use std::fs;
use std::path::Path;

pub fn delete(repo_path: &Path, snapshot_id_opt: Option<&str>, all: bool) -> Result<()> {
    // Validate arguments
    match (snapshot_id_opt, all) {
        (Some(_), true) => return Err(SnapVaultError::DeleteArgsConflict),
        (None, false) => return Err(SnapVaultError::DeleteArgsRequired),
        _ => {}
    }

    let repo = Repository::open(repo_path)?;

    if let Some(snapshot_id) = snapshot_id_opt {
        info!(
            "Deleting snapshot {} from repository {}",
            snapshot_id,
            repo_path.display()
        );
        delete_single_snapshot(&repo, snapshot_id)?;
        println!("✓ Snapshot {} deleted successfully", snapshot_id);
    } else {
        // all is true
        info!(
            "Deleting all snapshots from repository {}",
            repo_path.display()
        );

        let snapshots_dir = repo.snapshots_dir();
        if !snapshots_dir.exists() {
            println!("No snapshots found in repository.");
            return Ok(());
        }

        let mut snapshot_ids: Vec<String> = Vec::new();
        for entry in fs::read_dir(&snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension() == Some(std::ffi::OsStr::new("json")) {
                if let Some(stem) = path.file_stem() {
                    snapshot_ids.push(stem.to_string_lossy().to_string());
                }
            }
        }

        if snapshot_ids.is_empty() {
            println!("No snapshots found in repository.");
            return Ok(());
        }

        let total_snapshots = snapshot_ids.len();
        let mut deleted_count = 0;
        for id in snapshot_ids {
            match delete_single_snapshot(&repo, &id) {
                Ok(()) => {
                    println!("✓ Snapshot {} deleted successfully", id);
                    deleted_count += 1;
                }
                Err(e) => {
                    warn!("Failed to delete snapshot {}: {}", id, e);
                }
            }
        }

        println!("✓ Deleted {} out of {} snapshots", deleted_count, total_snapshots);
    }

    Ok(())
}

fn delete_single_snapshot(repo: &Repository, snapshot_id: &str) -> Result<()> {
    // Security: Validate snapshot ID
    validate_snapshot_id(snapshot_id)?;

    let manifest_path = repo
        .snapshots_dir()
        .join(format!("{}.json", snapshot_id));
    let data_path = repo.data_dir().join(snapshot_id);

    // Check if both manifest and data exist
    if !manifest_path.exists() {
        return Err(SnapVaultError::SnapshotNotFound(snapshot_id.to_string()));
    }
    if !data_path.exists() {
        return Err(SnapVaultError::SnapshotNotFound(snapshot_id.to_string()));
    }

    // Load manifest to verify it's a valid snapshot
    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: SnapshotManifest = serde_json::from_str(&raw)?;
    if manifest.snapshot_id != snapshot_id {
        return Err(SnapVaultError::Other(
            "Manifest snapshot ID mismatch".to_string(),
        ));
    }

    // Delete data directory first
    info!("Removing snapshot data directory: {}", data_path.display());
    fs::remove_dir_all(&data_path)?;

    // Delete manifest file
    info!("Removing snapshot manifest: {}", manifest_path.display());
    fs::remove_file(&manifest_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::backup;
    use assert_fs::prelude::*;
    use tempfile::TempDir;

    fn get_first_snapshot_id(repo_path: &Path) -> String {
        let snapshots_dir = repo_path.join("snapshots");
        for entry in fs::read_dir(&snapshots_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension() == Some(std::ffi::OsStr::new("json")) {
                return path.file_stem().unwrap().to_string_lossy().to_string();
            }
        }
        panic!("No snapshots found");
    }

    #[test]
    fn test_delete_single_snapshot() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        source.child("file.txt").write_str("content").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        let snapshot_id = get_first_snapshot_id(&repo_path);
        delete(&repo_path, Some(&snapshot_id), false).unwrap();

        // Verify snapshot is deleted
        let count = fs::read_dir(repo_path.join("snapshots")).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_all_snapshots() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        source.child("file.txt").write_str("content").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        delete(&repo_path, None, true).unwrap();

        let count = fs::read_dir(repo_path.join("snapshots")).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_args_conflict() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        let result = delete(&repo_path, Some("snap123"), true);
        assert!(matches!(result, Err(SnapVaultError::DeleteArgsConflict)));
    }

    #[test]
    fn test_delete_args_required() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        let result = delete(&repo_path, None, false);
        assert!(matches!(result, Err(SnapVaultError::DeleteArgsRequired)));
    }

    #[test]
    fn test_delete_nonexistent_snapshot() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        let result = delete(&repo_path, Some("nonexistent"), false);
        assert!(matches!(result, Err(SnapVaultError::SnapshotNotFound(_))));
    }
}
