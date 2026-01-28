use crate::error::{Result, SnapVaultError};
use crate::repository::snapshot::SnapshotManifest;
use crate::repository::Repository;
use crate::utils::MAX_MANIFEST_SIZE;
use log::{info, warn};
use std::fs;
use std::path::Path;

pub fn list(repo_path: &Path) -> Result<()> {
    info!("Listing snapshots in repository: {}", repo_path.display());

    let repo = Repository::open(repo_path)?;
    let snapshots_dir = repo.snapshots_dir();

    if !snapshots_dir.exists() {
        println!("No snapshots found in repository.");
        return Ok(());
    }

    let mut snapshots: Vec<SnapshotManifest> = Vec::new();
    for entry in fs::read_dir(&snapshots_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("json")) {
            // Security: Check manifest size before reading
            let metadata = fs::metadata(&path)?;
            if metadata.len() > MAX_MANIFEST_SIZE {
                warn!(
                    "Skipping oversized manifest: {} ({} bytes)",
                    path.display(),
                    metadata.len()
                );
                continue;
            }

            let raw = fs::read_to_string(&path).map_err(|e| {
                SnapVaultError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read manifest {}: {}", path.display(), e),
                ))
            })?;
            
            let manifest: SnapshotManifest = serde_json::from_str(&raw).map_err(|e| {
                SnapVaultError::Json(serde_json::Error::custom(format!(
                    "Failed to parse manifest {}: {}",
                    path.display(),
                    e
                )))
            })?;
            snapshots.push(manifest);
        }
    }

    if snapshots.is_empty() {
        println!("No snapshots found in repository.");
        return Ok(());
    }

    // Sort by created_at descending (latest first)
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("Snapshots in repository {}:", repo_path.display());
    println!(
        "{:<40} {:<25} {:<10} {:<10} {}",
        "Snapshot ID", "Created At", "Files", "Bytes", "Source Root"
    );
    println!("{}", "-".repeat(100));

    for snap in snapshots {
        println!(
            "{:<40} {:<25} {:<10} {:<10} {}",
            snap.snapshot_id,
            snap.created_at,
            snap.total_files,
            snap.total_bytes,
            snap.source_root
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::backup;
    use assert_fs::prelude::*;
    use tempfile::TempDir;

    #[test]
    fn test_list_empty_repository() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        list(&repo_path).unwrap(); // Should not error on empty repo
    }

    #[test]
    fn test_list_with_snapshots() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        source.child("file1.txt").write_str("content1").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        list(&repo_path).unwrap(); // Should list 1 snapshot
    }
}
