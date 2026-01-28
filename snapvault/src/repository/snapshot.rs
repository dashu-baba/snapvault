use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnapshotManifest {
    pub snapshot_id: String,
    pub created_at: String,
    pub source_root: String,
    pub total_files: u64,
    pub total_bytes: u64,
    pub files: Vec<FileRecord>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileRecord {
    pub rel_path: String,
    pub size: u64,
    pub modified: Option<String>,
}

impl SnapshotManifest {
    pub fn new(snapshot_id: String, source_root: String) -> Self {
        Self {
            snapshot_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            source_root,
            total_files: 0,
            total_bytes: 0,
            files: Vec::new(),
        }
    }
}
