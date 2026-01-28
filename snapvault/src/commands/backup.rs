use crate::chunking::{hash_file, Chunker};
use crate::error::{Result, SnapVaultError};
use crate::index::ChunkIndex;
use crate::repository::snapshot::{FileRecord, SnapshotManifest};
use crate::repository::Repository;
use crate::storage::ChunkStore;
use crate::utils::SNAPSHOT_UUID_LEN;
use log::{info, warn};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
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

    // Initialize chunk storage
    let chunk_store = ChunkStore::new(repo.chunks_dir());
    chunk_store.init()?;

    // Load chunk index
    let mut index = ChunkIndex::load(repo.index_path())?;

    let snapshot_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        &Uuid::new_v4().to_string()[..SNAPSHOT_UUID_LEN]
    );

    info!(
        "Starting chunked backup: source={}, repo={}, snapshot_id={}",
        source_path.display(),
        repo_path.display(),
        snapshot_id
    );

    let backup_result = perform_chunked_backup(source_path, &chunk_store);

    let (mut manifest, stats) = match backup_result {
        Ok(result) => result,
        Err(e) => {
            warn!("Backup failed: {}", e);
            return Err(e);
        }
    };

    // Set snapshot metadata
    manifest.snapshot_id = snapshot_id.clone();
    manifest.created_at = chrono::Utc::now().to_rfc3339();
    manifest.source_root = source_path.to_string_lossy().to_string();

    // Update chunk index
    index.add_snapshot(&manifest);
    index.save(repo.index_path())?;

    // Save manifest
    let snapshot_manifest_path = repo
        .snapshots_dir()
        .join(format!("{}.json", snapshot_id));
    fs::write(
        &snapshot_manifest_path,
        serde_json::to_string_pretty(&manifest)?,
    )?;

    // Print summary
    println!("✓ Backup complete");
    println!("  Snapshot:         {}", snapshot_id);
    println!("  Files:            {}", manifest.total_files);
    println!("  Total size:       {} ({} bytes)", 
        format_size(manifest.total_bytes), manifest.total_bytes);
    println!("  Unique chunks:    {}", manifest.total_chunks);
    println!("  Stored size:      {} ({} bytes)", 
        format_size(manifest.deduplicated_bytes), manifest.deduplicated_bytes);
    if let Some(ratio) = manifest.dedup_ratio() {
        let saved = manifest.space_saved();
        println!("  Space saved:      {} ({:.1}% dedup)", 
            format_size(saved), 100.0 - ratio);
    }
    println!("  New chunks:       {}", stats.new_chunks);
    println!("  Reused chunks:    {}", stats.reused_chunks);
    println!("  Manifest:         {}", snapshot_manifest_path.display());

    Ok(())
}

/// Statistics about a backup operation
struct BackupStats {
    new_chunks: usize,
    reused_chunks: usize,
}

fn perform_chunked_backup(
    source_path: &Path,
    chunk_store: &ChunkStore,
) -> Result<(SnapshotManifest, BackupStats)> {
    let mut manifest = SnapshotManifest::new(String::new(), String::new());
    let mut stats = BackupStats {
        new_chunks: 0,
        reused_chunks: 0,
    };
    
    // Track unique chunks in this snapshot for dedup calculation
    let mut unique_chunks = HashSet::new();
    let chunker = Chunker::new();

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

        let file_size = md.len();
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

        // Chunk the file
        let chunks = match chunker.chunk_file(path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to chunk file {}: {}", path.display(), e);
                continue;
            }
        };

        // Store chunks with deduplication
        for chunk in &chunks {
            let newly_stored = chunk_store.store(&chunk.hash, &read_chunk(path, chunk.offset, chunk.size)?)?;
            
            if newly_stored {
                stats.new_chunks += 1;
            } else {
                stats.reused_chunks += 1;
            }

            unique_chunks.insert(chunk.hash.clone());
        }

        // Compute file content hash
        let content_hash = match hash_file(path) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!("Failed to hash file {}: {}", path.display(), e);
                None
            }
        };

        // Create file record
        let file_record = FileRecord::new(
            rel_str,
            file_size,
            modified,
            chunks.iter().map(|c| c.hash.clone()).collect(),
            content_hash,
        );

        manifest.files.push(file_record);
        manifest.total_files += 1;
        manifest.total_bytes += file_size;
    }

    // Calculate deduplicated size
    manifest.total_chunks = unique_chunks.len() as u64;
    for chunk_hash in &unique_chunks {
        if let Ok(size) = chunk_store.chunk_size(chunk_hash) {
            manifest.deduplicated_bytes += size;
        }
    }

    info!(
        "Backup scan complete: {} files, {} bytes, {} unique chunks ({} new, {} reused)",
        manifest.total_files,
        manifest.total_bytes,
        manifest.total_chunks,
        stats.new_chunks,
        stats.reused_chunks
    );

    Ok((manifest, stats))
}

/// Read a specific chunk from a file
fn read_chunk(path: &Path, offset: u64, size: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; size];
    
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(offset))?;
    file.read_exact(&mut buffer)?;
    
    Ok(buffer)
}

fn systemtime_to_rfc3339(t: SystemTime) -> Result<String> {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Ok(dt.to_rfc3339())
}

/// Format a size in bytes to human-readable format
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
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
