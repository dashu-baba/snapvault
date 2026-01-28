use crate::chunking::ChunkHash;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub created_at: String,
    pub source_root: String,
    pub total_files: u64,
    pub total_bytes: u64,
    /// Total number of unique chunks referenced
    pub total_chunks: u64,
    /// Total deduplicated size (sum of unique chunk sizes)
    pub deduplicated_bytes: u64,
    pub files: Vec<FileRecord>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileRecord {
    /// Relative path of the file
    pub rel_path: String,
    /// Original file size in bytes
    pub size: u64,
    /// Modification time (RFC3339 format)
    pub modified: Option<String>,
    /// List of chunk hashes that make up this file
    /// Empty for empty files or files that couldn't be chunked
    pub chunks: Vec<ChunkHash>,
    /// Content hash of the entire file (for quick comparison)
    pub content_hash: Option<ChunkHash>,
}

impl SnapshotManifest {
    pub fn new(snapshot_id: String, source_root: String) -> Self {
        Self {
            snapshot_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            source_root,
            total_files: 0,
            total_bytes: 0,
            total_chunks: 0,
            deduplicated_bytes: 0,
            files: Vec::new(),
        }
    }

    /// Calculate deduplication ratio as a percentage
    /// Returns None if no data has been processed
    pub fn dedup_ratio(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            return None;
        }
        let ratio = (self.deduplicated_bytes as f64 / self.total_bytes as f64) * 100.0;
        Some(ratio)
    }

    /// Calculate space saved by deduplication
    pub fn space_saved(&self) -> u64 {
        self.total_bytes.saturating_sub(self.deduplicated_bytes)
    }
}

impl FileRecord {
    /// Create a new file record
    pub fn new(
        rel_path: String,
        size: u64,
        modified: Option<String>,
        chunks: Vec<ChunkHash>,
        content_hash: Option<ChunkHash>,
    ) -> Self {
        Self {
            rel_path,
            size,
            modified,
            chunks,
            content_hash,
        }
    }
}
