//! Chunk storage module for content-addressed storage (CAS).
//!
//! This module handles the physical storage of chunks on disk using content addressing.
//! Chunks are stored in a two-level directory structure based on their hash prefix.

use crate::chunking::ChunkHash;
use crate::error::{Result, SnapVaultError};
use log::{debug, warn};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Chunk storage manager for content-addressed storage
pub struct ChunkStore {
    /// Root directory for chunk storage (typically repo/data/chunks/)
    root: PathBuf,
}

impl ChunkStore {
    /// Create a new chunk store at the given root directory
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Initialize the chunk storage directory structure
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        debug!("Initialized chunk storage at: {}", self.root.display());
        Ok(())
    }

    /// Get the path for a chunk based on its hash
    /// Uses a two-level directory structure: chunks/<prefix>/<hash>
    /// Example: chunks/ab/ab123456...
    pub fn chunk_path(&self, hash: &ChunkHash) -> PathBuf {
        let prefix = hash.prefix();
        self.root.join(&prefix).join(hash.to_hex())
    }

    /// Check if a chunk exists in storage
    pub fn contains(&self, hash: &ChunkHash) -> bool {
        self.chunk_path(hash).exists()
    }

    /// Store a chunk with the given data
    /// Returns true if the chunk was newly stored, false if it already existed
    pub fn store(&self, hash: &ChunkHash, data: &[u8]) -> Result<bool> {
        let path = self.chunk_path(hash);

        // If chunk already exists, skip writing (deduplication!)
        if path.exists() {
            debug!("Chunk already exists: {}", hash);
            return Ok(false);
        }

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Verify the data matches the hash (security: prevent hash collision attacks)
        let actual_hash = crate::chunking::hash_bytes(data);
        if actual_hash != *hash {
            return Err(SnapVaultError::Other(format!(
                "Hash mismatch: expected {}, got {}",
                hash, actual_hash
            )));
        }

        // Write the chunk atomically
        // TODO: Consider using tempfile + rename for atomic writes
        let mut file = fs::File::create(&path)?;
        file.write_all(data)?;
        file.sync_all()?;

        debug!("Stored new chunk: {} ({} bytes)", hash, data.len());
        Ok(true)
    }

    /// Read a chunk from storage
    pub fn read(&self, hash: &ChunkHash) -> Result<Vec<u8>> {
        let path = self.chunk_path(hash);

        if !path.exists() {
            return Err(SnapVaultError::Other(format!(
                "Chunk not found: {}",
                hash
            )));
        }

        let mut file = fs::File::open(&path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        // Verify the hash (integrity check)
        let actual_hash = crate::chunking::hash_bytes(&data);
        if actual_hash != *hash {
            warn!(
                "Chunk integrity check failed for {}: expected {}, got {}",
                path.display(),
                hash,
                actual_hash
            );
            return Err(SnapVaultError::Other(format!(
                "Chunk corrupted: hash mismatch for {}",
                hash
            )));
        }

        debug!("Read chunk: {} ({} bytes)", hash, data.len());
        Ok(data)
    }

    /// Delete a chunk from storage
    /// This should only be called after verifying the chunk is no longer referenced
    pub fn delete(&self, hash: &ChunkHash) -> Result<()> {
        let path = self.chunk_path(hash);

        if !path.exists() {
            // Already deleted or never existed
            return Ok(());
        }

        fs::remove_file(&path)?;
        debug!("Deleted chunk: {}", hash);

        // Try to remove empty parent directory (cleanup)
        if let Some(parent) = path.parent() {
            if let Ok(mut entries) = fs::read_dir(parent) {
                if entries.next().is_none() {
                    // Directory is empty, remove it
                    let _ = fs::remove_dir(parent);
                }
            }
        }

        Ok(())
    }

    /// Get the size of a chunk in bytes
    pub fn chunk_size(&self, hash: &ChunkHash) -> Result<u64> {
        let path = self.chunk_path(hash);
        let metadata = fs::metadata(&path)?;
        Ok(metadata.len())
    }

    /// List all chunks in storage (for debugging/verification)
    /// Returns a vector of (hash, size) tuples
    pub fn list_chunks(&self) -> Result<Vec<(ChunkHash, u64)>> {
        let mut chunks = Vec::new();

        if !self.root.exists() {
            return Ok(chunks);
        }

        // Iterate through prefix directories
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Iterate through chunks in this prefix directory
            for chunk_entry in fs::read_dir(&path)? {
                let chunk_entry = chunk_entry?;
                let chunk_path = chunk_entry.path();

                if !chunk_path.is_file() {
                    continue;
                }

                // Parse the hash from the filename
                if let Some(filename) = chunk_path.file_name().and_then(|s| s.to_str()) {
                    if let Ok(hash) = ChunkHash::from_hex(filename) {
                        let size = chunk_entry.metadata()?.len();
                        chunks.push((hash, size));
                    }
                }
            }
        }

        Ok(chunks)
    }

    /// Get storage statistics
    pub fn stats(&self) -> Result<StorageStats> {
        let chunks = self.list_chunks()?;
        let total_chunks = chunks.len();
        let total_size: u64 = chunks.iter().map(|(_, size)| size).sum();

        Ok(StorageStats {
            total_chunks,
            total_size,
        })
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    /// Total number of unique chunks
    pub total_chunks: usize,
    /// Total size in bytes
    pub total_size: u64,
}

impl StorageStats {
    /// Format the total size in human-readable form
    pub fn format_size(&self) -> String {
        format_size(self.total_size)
    }
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
    use crate::chunking::hash_bytes;
    use tempfile::TempDir;

    #[test]
    fn test_chunk_store_init() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        assert!(temp_dir.path().join("chunks").exists());
        Ok(())
    }

    #[test]
    fn test_chunk_path() {
        let store = ChunkStore::new("/tmp/chunks");
        let hash = ChunkHash::from_hex("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890").unwrap();
        let path = store.chunk_path(&hash);

        assert!(path.to_string_lossy().contains("/ab/"));
        assert!(path.to_string_lossy().ends_with("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"));
    }

    #[test]
    fn test_store_and_read_chunk() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let hash = hash_bytes(data);

        // Store the chunk
        let newly_stored = store.store(&hash, data)?;
        assert!(newly_stored);

        // Read it back
        let read_data = store.read(&hash)?;
        assert_eq!(data, &read_data[..]);

        Ok(())
    }

    #[test]
    fn test_store_duplicate_chunk() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let hash = hash_bytes(data);

        // Store the chunk twice
        let first = store.store(&hash, data)?;
        let second = store.store(&hash, data)?;

        assert!(first); // First store should be new
        assert!(!second); // Second store should detect duplicate

        Ok(())
    }

    #[test]
    fn test_contains() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let hash = hash_bytes(data);

        assert!(!store.contains(&hash));
        store.store(&hash, data)?;
        assert!(store.contains(&hash));

        Ok(())
    }

    #[test]
    fn test_delete_chunk() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let hash = hash_bytes(data);

        store.store(&hash, data)?;
        assert!(store.contains(&hash));

        store.delete(&hash)?;
        assert!(!store.contains(&hash));

        Ok(())
    }

    #[test]
    fn test_chunk_size() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let hash = hash_bytes(data);

        store.store(&hash, data)?;
        let size = store.chunk_size(&hash)?;
        assert_eq!(size, data.len() as u64);

        Ok(())
    }

    #[test]
    fn test_list_chunks() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data1 = b"hello";
        let data2 = b"world";
        let hash1 = hash_bytes(data1);
        let hash2 = hash_bytes(data2);

        store.store(&hash1, data1)?;
        store.store(&hash2, data2)?;

        let chunks = store.list_chunks()?;
        assert_eq!(chunks.len(), 2);

        Ok(())
    }

    #[test]
    fn test_storage_stats() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data1 = b"hello";
        let data2 = b"world";
        let hash1 = hash_bytes(data1);
        let hash2 = hash_bytes(data2);

        store.store(&hash1, data1)?;
        store.store(&hash2, data2)?;

        let stats = store.stats()?;
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.total_size, 10); // "hello" (5) + "world" (5)

        Ok(())
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
    }

    #[test]
    fn test_hash_mismatch_detection() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let data = b"hello world";
        let wrong_hash = hash_bytes(b"wrong data");

        // Try to store with mismatched hash
        let result = store.store(&wrong_hash, data);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_read_nonexistent_chunk() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = ChunkStore::new(temp_dir.path().join("chunks"));
        store.init()?;

        let hash = hash_bytes(b"nonexistent");
        let result = store.read(&hash);
        assert!(result.is_err());

        Ok(())
    }
}
