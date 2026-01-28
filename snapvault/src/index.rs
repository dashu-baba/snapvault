//! Chunk index module for tracking chunk references across snapshots.
//!
//! This module maintains an index that tracks which snapshots reference which chunks,
//! enabling safe chunk deletion and providing deduplication statistics.

use crate::chunking::ChunkHash;
use crate::error::Result;
use crate::repository::snapshot::SnapshotManifest;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Chunk reference index
/// Maps chunk hashes to the set of snapshot IDs that reference them
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ChunkIndex {
    /// Map from chunk hash (hex string) to set of snapshot IDs
    #[serde(with = "chunk_refs_serde")]
    chunk_refs: HashMap<ChunkHash, HashSet<String>>,
}

/// Custom serialization for HashMap<ChunkHash, HashSet<String>>
mod chunk_refs_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(
        map: &HashMap<ChunkHash, HashSet<String>>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Convert HashMap<ChunkHash, HashSet> to HashMap<String, Vec<String>>
        let string_map: HashMap<String, Vec<String>> = map
            .iter()
            .map(|(hash, refs)| {
                let mut sorted_refs: Vec<String> = refs.iter().cloned().collect();
                sorted_refs.sort(); // Sort for deterministic output
                (hash.to_hex(), sorted_refs)
            })
            .collect();
        string_map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<HashMap<ChunkHash, HashSet<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map: HashMap<String, Vec<String>> = HashMap::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for (hash_str, refs) in string_map {
            let hash = ChunkHash::from_hex(&hash_str).map_err(serde::de::Error::custom)?;
            let ref_set: HashSet<String> = refs.into_iter().collect();
            map.insert(hash, ref_set);
        }
        Ok(map)
    }
}

impl ChunkIndex {
    /// Create a new empty chunk index
    pub fn new() -> Self {
        Self {
            chunk_refs: HashMap::new(),
        }
    }

    /// Load chunk index from a file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            debug!("Index file not found, creating new index");
            return Ok(Self::new());
        }

        let content = fs::read_to_string(path)?;
        let index: Self = serde_json::from_str(&content)?;
        debug!("Loaded chunk index with {} chunks", index.chunk_refs.len());
        Ok(index)
    }

    /// Save chunk index to a file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&self)?;
        fs::write(path, content)?;
        debug!("Saved chunk index with {} chunks", self.chunk_refs.len());
        Ok(())
    }

    /// Add a snapshot's chunk references to the index
    pub fn add_snapshot(&mut self, manifest: &SnapshotManifest) {
        let snapshot_id = &manifest.snapshot_id;
        info!("Adding snapshot {} to chunk index", snapshot_id);

        for file in &manifest.files {
            for chunk in &file.chunks {
                self.chunk_refs
                    .entry(chunk.clone())
                    .or_insert_with(HashSet::new)
                    .insert(snapshot_id.clone());
            }
        }
    }

    /// Remove a snapshot's chunk references from the index
    /// Returns the set of chunks that are no longer referenced by any snapshot
    pub fn remove_snapshot(&mut self, manifest: &SnapshotManifest) -> HashSet<ChunkHash> {
        let snapshot_id = &manifest.snapshot_id;
        info!("Removing snapshot {} from chunk index", snapshot_id);

        let mut orphaned_chunks = HashSet::new();

        for file in &manifest.files {
            for chunk in &file.chunks {
                if let Some(refs) = self.chunk_refs.get_mut(chunk) {
                    refs.remove(snapshot_id);
                    
                    // If no more references, mark as orphaned
                    if refs.is_empty() {
                        self.chunk_refs.remove(chunk);
                        orphaned_chunks.insert(chunk.clone());
                    }
                }
            }
        }

        info!(
            "Found {} orphaned chunks after removing snapshot {}",
            orphaned_chunks.len(),
            snapshot_id
        );
        orphaned_chunks
    }

    /// Get all snapshots that reference a chunk
    pub fn get_snapshots(&self, chunk: &ChunkHash) -> Option<&HashSet<String>> {
        self.chunk_refs.get(chunk)
    }

    /// Check if a chunk is referenced by any snapshot
    pub fn is_referenced(&self, chunk: &ChunkHash) -> bool {
        self.chunk_refs.contains_key(chunk)
    }

    /// Get the total number of unique chunks in the index
    pub fn total_chunks(&self) -> usize {
        self.chunk_refs.len()
    }

    /// Get all chunks in the index
    pub fn all_chunks(&self) -> Vec<ChunkHash> {
        self.chunk_refs.keys().cloned().collect()
    }

    /// Find orphaned chunks (chunks in storage but not in index)
    /// This is useful for cleanup/verification
    pub fn find_orphans(
        &self,
        storage_chunks: &HashSet<ChunkHash>,
    ) -> HashSet<ChunkHash> {
        let indexed_chunks: HashSet<ChunkHash> = self.chunk_refs.keys().cloned().collect();
        storage_chunks.difference(&indexed_chunks).cloned().collect()
    }

    /// Rebuild index from scratch by scanning all manifests
    pub fn rebuild<P: AsRef<Path>>(snapshots_dir: P) -> Result<Self> {
        let mut index = Self::new();
        let snapshots_dir = snapshots_dir.as_ref();

        if !snapshots_dir.exists() {
            return Ok(index);
        }

        info!("Rebuilding chunk index from {}", snapshots_dir.display());

        for entry in fs::read_dir(snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // Load manifest
            let content = fs::read_to_string(&path)?;
            let manifest: SnapshotManifest = serde_json::from_str(&content)?;
            
            // Add to index
            index.add_snapshot(&manifest);
        }

        info!("Rebuilt index with {} unique chunks", index.total_chunks());
        Ok(index)
    }

    /// Get statistics about the index
    pub fn stats(&self) -> IndexStats {
        let total_chunks = self.chunk_refs.len();
        let total_references: usize = self.chunk_refs.values().map(|refs| refs.len()).sum();
        
        // Calculate average references per chunk
        let avg_refs = if total_chunks > 0 {
            total_references as f64 / total_chunks as f64
        } else {
            0.0
        };

        IndexStats {
            total_chunks,
            total_references,
            avg_refs_per_chunk: avg_refs,
        }
    }
}

/// Statistics about the chunk index
#[derive(Debug, Clone)]
pub struct IndexStats {
    /// Total number of unique chunks
    pub total_chunks: usize,
    /// Total number of chunk references across all snapshots
    pub total_references: usize,
    /// Average number of references per chunk
    pub avg_refs_per_chunk: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunking::hash_bytes;
    use crate::repository::snapshot::FileRecord;

    fn create_test_manifest(snapshot_id: &str, chunks: Vec<ChunkHash>) -> SnapshotManifest {
        let mut manifest = SnapshotManifest::new(snapshot_id.to_string(), "/test".to_string());
        manifest.files.push(FileRecord::new(
            "test.txt".to_string(),
            100,
            None,
            chunks,
            None,
        ));
        manifest
    }

    #[test]
    fn test_add_snapshot() {
        let mut index = ChunkIndex::new();
        let chunk1 = hash_bytes(b"chunk1");
        let chunk2 = hash_bytes(b"chunk2");
        
        let manifest = create_test_manifest("snap1", vec![chunk1.clone(), chunk2.clone()]);
        index.add_snapshot(&manifest);

        assert_eq!(index.total_chunks(), 2);
        assert!(index.is_referenced(&chunk1));
        assert!(index.is_referenced(&chunk2));
    }

    #[test]
    fn test_remove_snapshot() {
        let mut index = ChunkIndex::new();
        let chunk1 = hash_bytes(b"chunk1");
        let chunk2 = hash_bytes(b"chunk2");
        
        let manifest = create_test_manifest("snap1", vec![chunk1.clone(), chunk2.clone()]);
        index.add_snapshot(&manifest);

        let orphaned = index.remove_snapshot(&manifest);
        
        assert_eq!(orphaned.len(), 2);
        assert!(orphaned.contains(&chunk1));
        assert!(orphaned.contains(&chunk2));
        assert_eq!(index.total_chunks(), 0);
    }

    #[test]
    fn test_shared_chunks() {
        let mut index = ChunkIndex::new();
        let shared_chunk = hash_bytes(b"shared");
        let chunk1 = hash_bytes(b"chunk1");
        let chunk2 = hash_bytes(b"chunk2");
        
        // Add two snapshots with a shared chunk
        let manifest1 = create_test_manifest("snap1", vec![shared_chunk.clone(), chunk1.clone()]);
        let manifest2 = create_test_manifest("snap2", vec![shared_chunk.clone(), chunk2.clone()]);
        
        index.add_snapshot(&manifest1);
        index.add_snapshot(&manifest2);

        assert_eq!(index.total_chunks(), 3);

        // Remove first snapshot - shared chunk should still be referenced
        let orphaned = index.remove_snapshot(&manifest1);
        
        assert_eq!(orphaned.len(), 1);
        assert!(orphaned.contains(&chunk1));
        assert!(!orphaned.contains(&shared_chunk));
        assert!(index.is_referenced(&shared_chunk));
    }

    #[test]
    fn test_save_and_load() -> Result<()> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let index_path = temp_dir.path().join("index.json");

        // Create and save index
        let mut index = ChunkIndex::new();
        let chunk = hash_bytes(b"test");
        let manifest = create_test_manifest("snap1", vec![chunk.clone()]);
        index.add_snapshot(&manifest);
        index.save(&index_path)?;

        // Load and verify
        let loaded = ChunkIndex::load(&index_path)?;
        assert_eq!(loaded.total_chunks(), 1);
        assert!(loaded.is_referenced(&chunk));

        Ok(())
    }

    #[test]
    fn test_index_stats() {
        let mut index = ChunkIndex::new();
        let chunk1 = hash_bytes(b"chunk1");
        let chunk2 = hash_bytes(b"chunk2");
        
        let manifest1 = create_test_manifest("snap1", vec![chunk1.clone(), chunk2.clone()]);
        let manifest2 = create_test_manifest("snap2", vec![chunk1.clone()]); // Only chunk1
        
        index.add_snapshot(&manifest1);
        index.add_snapshot(&manifest2);

        let stats = index.stats();
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.total_references, 3); // chunk1: 2 refs, chunk2: 1 ref
        assert_eq!(stats.avg_refs_per_chunk, 1.5);
    }

    #[test]
    fn test_find_orphans() {
        let mut index = ChunkIndex::new();
        let chunk1 = hash_bytes(b"chunk1");
        let chunk2 = hash_bytes(b"chunk2");
        let chunk3 = hash_bytes(b"chunk3");
        
        // Index only references chunk1 and chunk2
        let manifest = create_test_manifest("snap1", vec![chunk1.clone(), chunk2.clone()]);
        index.add_snapshot(&manifest);

        // Storage has chunk1, chunk2, and chunk3
        let mut storage_chunks = HashSet::new();
        storage_chunks.insert(chunk1.clone());
        storage_chunks.insert(chunk2.clone());
        storage_chunks.insert(chunk3.clone());

        let orphans = index.find_orphans(&storage_chunks);
        
        assert_eq!(orphans.len(), 1);
        assert!(orphans.contains(&chunk3));
    }
}
