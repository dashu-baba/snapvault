use crate::error::{Result, SnapVaultError};
use crate::repository::snapshot::SnapshotManifest;
use crate::repository::Repository;
use crate::storage::ChunkStore;
use crate::utils::{is_safe_path, validate_snapshot_id, MAX_MANIFEST_SIZE};
use log::{info, warn};
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn restore(snapshot_id_opt: Option<&str>, dest_path: &Path, repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path)?;

    // Determine snapshot ID
    let snapshot_id = if let Some(id) = snapshot_id_opt {
        // Security: Validate snapshot ID
        validate_snapshot_id(id)?;

        if !repo
            .snapshots_dir()
            .join(format!("{}.json", id))
            .exists()
        {
            return Err(SnapVaultError::SnapshotNotFound(id.to_string()));
        }
        id.to_string()
    } else {
        // Find latest snapshot
        let snapshots_dir = repo.snapshots_dir();
        if !snapshots_dir.exists() {
            return Err(SnapVaultError::NoSnapshots);
        }
        let mut snapshots: Vec<String> = fs::read_dir(&snapshots_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension() == Some(std::ffi::OsStr::new("json")) {
                    Some(path.file_stem()?.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        if snapshots.is_empty() {
            return Err(SnapVaultError::NoSnapshots);
        }
        snapshots.sort_by(|a, b| b.cmp(a)); // descending order
        snapshots[0].clone()
    };

    info!(
        "Restoring snapshot {} to {}",
        snapshot_id,
        dest_path.display()
    );

    // Validate dest
    if dest_path.exists() {
        if !dest_path.is_dir() {
            return Err(SnapVaultError::Other(format!(
                "Destination path is not a directory: {}",
                dest_path.display()
            )));
        }
        if fs::read_dir(dest_path)?.next().is_some() {
            return Err(SnapVaultError::DestinationNotEmpty(
                dest_path.to_path_buf(),
            ));
        }
    } else {
        fs::create_dir_all(dest_path)?;
    }

    // Load manifest
    let manifest_path = repo
        .snapshots_dir()
        .join(format!("{}.json", snapshot_id));
    if !manifest_path.is_file() {
        return Err(SnapVaultError::SnapshotNotFound(snapshot_id.clone()));
    }

    // Security: Check manifest size before reading
    let metadata = fs::metadata(&manifest_path)?;
    if metadata.len() > MAX_MANIFEST_SIZE {
        return Err(SnapVaultError::FileTooLarge {
            size: metadata.len(),
            max: MAX_MANIFEST_SIZE,
        });
    }

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: SnapshotManifest = serde_json::from_str(&raw)?;
    if manifest.snapshot_id != snapshot_id {
        return Err(SnapVaultError::Other(
            "Manifest snapshot ID mismatch".to_string(),
        ));
    }

    // Initialize chunk storage
    let chunk_store = ChunkStore::new(repo.chunks_dir());

    // Restore files by reassembling chunks
    let mut restored_count = 0;
    let total_files = manifest.files.len();

    for file in manifest.files.iter() {
        // Security: Validate path safety
        if !is_safe_path(&file.rel_path) {
            warn!("Skipping unsafe path: {}", file.rel_path);
            continue;
        }

        let dst_path = dest_path.join(&file.rel_path);

        // Create parent directories
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Reassemble file from chunks
        let mut output_file = fs::File::create(&dst_path)?;
        
        for chunk_hash in &file.chunks {
            // Read chunk from storage
            let chunk_data = chunk_store.read(chunk_hash)?;
            
            // Write chunk to output file
            output_file.write_all(&chunk_data)?;
        }
        
        // Ensure all data is written to disk
        output_file.sync_all()?;

        restored_count += 1;

        // Log progress every 100 files
        if restored_count % 100 == 0 {
            info!("Restored {}/{} files", restored_count, total_files);
        }
    }

    println!("✓ Restore complete");
    println!("  Snapshot:     {}", snapshot_id);
    println!("  Files:        {}", restored_count);
    println!("  Destination:  {}", dest_path.display());

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
    fn test_restore_snapshot() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        let dest = temp.path().join("restored");

        source.child("file1.txt").write_str("content1").unwrap();
        source.child("file2.txt").write_str("content2").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        let snapshot_id = get_first_snapshot_id(&repo_path);
        restore(Some(&snapshot_id), &dest, &repo_path).unwrap();

        // Verify files were restored
        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("file2.txt").exists());
        assert_eq!(fs::read_to_string(dest.join("file1.txt")).unwrap(), "content1");
    }

    #[test]
    fn test_restore_latest_snapshot() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        let dest = temp.path().join("restored");

        source.child("file.txt").write_str("content").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        restore(None, &dest, &repo_path).unwrap();

        assert!(dest.join("file.txt").exists());
    }

    #[test]
    fn test_restore_to_nonempty_directory() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        let dest = assert_fs::TempDir::new().unwrap();

        source.child("file.txt").write_str("content").unwrap();
        dest.child("existing.txt").write_str("exists").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        let snapshot_id = get_first_snapshot_id(&repo_path);
        let result = restore(Some(&snapshot_id), dest.path(), &repo_path);
        assert!(matches!(
            result,
            Err(SnapVaultError::DestinationNotEmpty(_))
        ));
    }

    #[test]
    fn test_restore_nonexistent_snapshot() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let dest = temp.path().join("restored");

        Repository::init(&repo_path).unwrap();
        let result = restore(Some("nonexistent"), &dest, &repo_path);
        assert!(matches!(result, Err(SnapVaultError::SnapshotNotFound(_))));
    }

    #[test]
    fn test_restore_nested_directories() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        let dest = temp.path().join("restored");

        source.child("dir1/file1.txt").write_str("content1").unwrap();
        source
            .child("dir1/dir2/file2.txt")
            .write_str("content2")
            .unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        let snapshot_id = get_first_snapshot_id(&repo_path);
        restore(Some(&snapshot_id), &dest, &repo_path).unwrap();

        assert!(dest.join("dir1/file1.txt").exists());
        assert!(dest.join("dir1/dir2/file2.txt").exists());
    }
}
