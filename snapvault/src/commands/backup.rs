use crate::error::{Result, SnapVaultError};
use crate::repository::snapshot::{FileRecord, SnapshotManifest};
use crate::repository::Repository;
use crate::utils::SNAPSHOT_UUID_LEN;
use log::{info, warn};
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;
use uuid::Uuid;
use walkdir::WalkDir;

pub fn backup(source_path: &Path, repo_path: &Path) -> Result<()> {
    // Validate source
    if !source_path.exists() {
        return Err(SnapVaultError::SourceNotFound(source_path.to_path_buf()));
    }
    if !source_path.is_dir() {
        return Err(SnapVaultError::SourceNotDirectory(
            source_path.to_path_buf(),
        ));
    }

    let repo = Repository::open(repo_path)?;

    let snapshot_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        &Uuid::new_v4().to_string()[..SNAPSHOT_UUID_LEN]
    );
    let data_root = repo.data_dir().join(&snapshot_id);
    let snapshot_manifest_path = repo
        .snapshots_dir()
        .join(format!("{snapshot_id}.json"));

    fs::create_dir_all(&data_root)?;

    info!(
        "Starting basic backup: source={}, repo={}, snapshot_id={}",
        source_path.display(),
        repo_path.display(),
        snapshot_id
    );

    let backup_result = perform_backup(source_path, &data_root);

    let (total_files, total_bytes, files) = match backup_result {
        Ok(result) => result,
        Err(e) => {
            warn!(
                "Backup failed, cleaning up partial data at {}",
                data_root.display()
            );
            if let Err(cleanup_err) = fs::remove_dir_all(&data_root) {
                warn!("Failed to cleanup partial backup: {}", cleanup_err);
            }
            return Err(e);
        }
    };

    let manifest = SnapshotManifest {
        snapshot_id: snapshot_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_root: source_path.to_string_lossy().to_string(),
        total_files,
        total_bytes,
        files,
    };

    fs::write(
        &snapshot_manifest_path,
        serde_json::to_string_pretty(&manifest)?,
    )?;

    println!("✓ Backup complete");
    println!("  Snapshot:   {}", snapshot_id);
    println!("  Files:      {}", total_files);
    println!("  Bytes:      {}", total_bytes);
    println!("  Manifest:   {}", snapshot_manifest_path.display());
    println!("  Data root:  {}", data_root.display());

    Ok(())
}

fn perform_backup(
    source_path: &Path,
    data_root: &Path,
) -> Result<(u64, u64, Vec<FileRecord>)> {
    let mut files: Vec<FileRecord> = Vec::new();
    let mut total_files: u64 = 0;
    let mut total_bytes: u64 = 0;

    for entry in WalkDir::new(source_path).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("Walk error: {}", e);
                continue;
            }
        };

        let path = entry.path();
        let ft = entry.file_type();

        if ft.is_dir() {
            continue;
        }
        if ft.is_symlink() {
            warn!("Skipping symlink: {}", path.display());
            continue;
        }
        if !ft.is_file() {
            continue;
        }

        let md = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!("Metadata error for {}: {}", path.display(), e);
                continue;
            }
        };

        let modified = md
            .modified()
            .ok()
            .and_then(|t| systemtime_to_rfc3339(t).ok());

        // Build relative path
        let rel = match path.strip_prefix(source_path) {
            Ok(r) => r,
            Err(_) => {
                warn!("Failed to compute relative path for {}", path.display());
                continue;
            }
        };

        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        let dest_path = data_root.join(rel);

        // Ensure parent directories exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let bytes_copied = copy_file(path, &dest_path)?;

        total_files += 1;
        total_bytes = total_bytes.saturating_add(bytes_copied);

        files.push(FileRecord {
            rel_path: rel_str,
            size: bytes_copied,
            modified,
        });
    }

    Ok((total_files, total_bytes, files))
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<u64> {
    fs::copy(src, dst)
}

fn systemtime_to_rfc3339(t: SystemTime) -> Result<String> {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Ok(dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use tempfile::TempDir;

    #[test]
    fn test_backup_basic() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();

        source.child("file1.txt").write_str("hello").unwrap();
        source.child("file2.txt").write_str("world").unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        // Check that snapshot was created
        let snapshots = fs::read_dir(repo_path.join("snapshots")).unwrap();
        assert_eq!(snapshots.count(), 1);
    }

    #[test]
    fn test_backup_nested_directories() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();

        source.child("dir1/file1.txt").write_str("content1").unwrap();
        source
            .child("dir1/dir2/file2.txt")
            .write_str("content2")
            .unwrap();

        Repository::init(&repo_path).unwrap();
        backup(source.path(), &repo_path).unwrap();

        let snapshots: Vec<_> = fs::read_dir(repo_path.join("snapshots"))
            .unwrap()
            .collect();
        assert_eq!(snapshots.len(), 1);
    }

    #[test]
    fn test_backup_nonexistent_source() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source_path = temp.path().join("nonexistent");

        Repository::init(&repo_path).unwrap();
        let result = backup(&source_path, &repo_path);
        assert!(matches!(result, Err(SnapVaultError::SourceNotFound(_))));
    }

    #[test]
    fn test_backup_file_as_source() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");
        let source = assert_fs::TempDir::new().unwrap();
        let file = source.child("file.txt");
        file.write_str("content").unwrap();

        Repository::init(&repo_path).unwrap();
        let result = backup(file.path(), &repo_path);
        assert!(matches!(
            result,
            Err(SnapVaultError::SourceNotDirectory(_))
        ));
    }
}
