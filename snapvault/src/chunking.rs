//! File chunking module for content-addressed storage and deduplication.
//!
//! This module provides functionality to split files into fixed-size chunks,
//! compute their content hashes, and enable deduplication across snapshots.

use crate::error::{Result, SnapVaultError};
use blake3::Hasher;
use std::fs::File;
use std::io::{Read, BufReader};
use std::path::Path;

/// Default chunk size: 1 MiB
/// This is a good balance between deduplication granularity and metadata overhead
pub const DEFAULT_CHUNK_SIZE: usize = 1024 * 1024; // 1 MiB

/// Maximum chunk size to prevent memory issues
pub const MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

/// Minimum chunk size for practical deduplication
pub const MIN_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB

/// A chunk of file data with its content hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Blake3 hash of the chunk content (32 bytes)
    pub hash: ChunkHash,
    /// Size of the chunk in bytes
    pub size: usize,
    /// Offset of this chunk in the original file
    pub offset: u64,
}

/// Blake3 hash representation (32 bytes)
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ChunkHash(#[serde(with = "hex_serde")] [u8; 32]);

/// Serde helper for hex encoding/decoding
mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "Invalid hash length: expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(array)
    }
}

impl ChunkHash {
    /// Create a new ChunkHash from a 32-byte array
    pub fn new(bytes: [u8; 32]) -> Self {
        ChunkHash(bytes)
    }

    /// Get the hash as a byte slice
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert the hash to a hexadecimal string
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Parse a hash from a hexadecimal string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s)
            .map_err(|e| SnapVaultError::Other(format!("Invalid hex hash: {}", e)))?;
        
        if bytes.len() != 32 {
            return Err(SnapVaultError::Other(format!(
                "Invalid hash length: expected 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        Ok(ChunkHash(hash))
    }

    /// Get a two-character prefix for directory sharding
    /// This helps avoid too many files in a single directory
    pub fn prefix(&self) -> String {
        format!("{:02x}", self.0[0])
    }
}

impl std::fmt::Display for ChunkHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Fixed-size chunker that splits files into equal-sized chunks
pub struct Chunker {
    chunk_size: usize,
}

impl Chunker {
    /// Create a new chunker with the default chunk size
    pub fn new() -> Self {
        Self::with_size(DEFAULT_CHUNK_SIZE)
    }

    /// Create a new chunker with a custom chunk size
    pub fn with_size(chunk_size: usize) -> Self {
        // Validate chunk size
        let chunk_size = chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
        Self { chunk_size }
    }

    /// Get the chunk size
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Chunk a file and return a list of chunks with their hashes
    pub fn chunk_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Chunk>> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut offset = 0u64;

        loop {
            // Read a chunk
            let mut total_read = 0;
            while total_read < self.chunk_size {
                match reader.read(&mut buffer[total_read..]) {
                    Ok(0) => break, // EOF
                    Ok(n) => total_read += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }

            if total_read == 0 {
                break; // EOF
            }

            // Hash the chunk
            let hash = hash_bytes(&buffer[..total_read]);
            
            chunks.push(Chunk {
                hash,
                size: total_read,
                offset,
            });

            offset += total_read as u64;
        }

        Ok(chunks)
    }

    /// Chunk data from a byte slice (useful for testing)
    pub fn chunk_bytes(&self, data: &[u8]) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut offset = 0u64;

        for chunk_data in data.chunks(self.chunk_size) {
            let hash = hash_bytes(chunk_data);
            chunks.push(Chunk {
                hash,
                size: chunk_data.len(),
                offset,
            });
            offset += chunk_data.len() as u64;
        }

        chunks
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash a byte slice using Blake3
pub fn hash_bytes(data: &[u8]) -> ChunkHash {
    let hash = blake3::hash(data);
    ChunkHash(hash.into())
}

/// Hash a file using Blake3 (for whole-file hashing)
pub fn hash_file<P: AsRef<Path>>(path: P) -> Result<ChunkHash> {
    let mut hasher = Hasher::new();
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; 65536]; // 64 KiB buffer

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(ChunkHash(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_hash_hex_roundtrip() {
        let original = ChunkHash::new([42u8; 32]);
        let hex = original.to_hex();
        let parsed = ChunkHash::from_hex(&hex).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_chunk_hash_prefix() {
        let hash = ChunkHash::new([0xab, 0xcd, 0xef, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(hash.prefix(), "ab");
    }

    #[test]
    fn test_hash_bytes_deterministic() {
        let data = b"hello world";
        let hash1 = hash_bytes(data);
        let hash2 = hash_bytes(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_bytes_different() {
        let data1 = b"hello world";
        let data2 = b"hello world!";
        let hash1 = hash_bytes(data1);
        let hash2 = hash_bytes(data2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_chunker_default_size() {
        let chunker = Chunker::new();
        assert_eq!(chunker.chunk_size(), DEFAULT_CHUNK_SIZE);
    }

    #[test]
    fn test_chunker_custom_size() {
        let chunker = Chunker::with_size(512 * 1024); // 512 KiB
        assert_eq!(chunker.chunk_size(), 512 * 1024);
    }

    #[test]
    fn test_chunker_size_clamping() {
        // Too small
        let chunker = Chunker::with_size(1024);
        assert_eq!(chunker.chunk_size(), MIN_CHUNK_SIZE);

        // Too large
        let chunker = Chunker::with_size(100 * 1024 * 1024);
        assert_eq!(chunker.chunk_size(), MAX_CHUNK_SIZE);
    }

    #[test]
    fn test_chunk_bytes_single_chunk() {
        let chunker = Chunker::with_size(1024);
        let data = vec![42u8; 512]; // Smaller than chunk size
        let chunks = chunker.chunk_bytes(&data);
        
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].size, 512);
        assert_eq!(chunks[0].offset, 0);
    }

    #[test]
    fn test_chunk_bytes_multiple_chunks() {
        let chunker = Chunker::with_size(1024);
        let data = vec![42u8; 2500]; // Will create 3 chunks: 1024 + 1024 + 452
        let chunks = chunker.chunk_bytes(&data);
        
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].size, 1024);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[1].size, 1024);
        assert_eq!(chunks[1].offset, 1024);
        assert_eq!(chunks[2].size, 452);
        assert_eq!(chunks[2].offset, 2048);
    }

    #[test]
    fn test_chunk_bytes_deduplication() {
        let chunker = Chunker::with_size(1024);
        
        // Create data with repeating pattern
        let mut data = Vec::new();
        data.extend_from_slice(&[1u8; 1024]); // First chunk
        data.extend_from_slice(&[1u8; 1024]); // Identical second chunk
        
        let chunks = chunker.chunk_bytes(&data);
        
        assert_eq!(chunks.len(), 2);
        // Same content should produce same hash
        assert_eq!(chunks[0].hash, chunks[1].hash);
    }

    #[test]
    fn test_chunk_bytes_different_content() {
        let chunker = Chunker::with_size(1024);
        
        let mut data = Vec::new();
        data.extend_from_slice(&[1u8; 1024]); // First chunk
        data.extend_from_slice(&[2u8; 1024]); // Different second chunk
        
        let chunks = chunker.chunk_bytes(&data);
        
        assert_eq!(chunks.len(), 2);
        // Different content should produce different hashes
        assert_ne!(chunks[0].hash, chunks[1].hash);
    }

    #[test]
    fn test_chunk_file() -> Result<()> {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary file
        let mut temp_file = NamedTempFile::new()?;
        let data = vec![42u8; 2500];
        temp_file.write_all(&data)?;
        temp_file.flush()?;

        // Chunk the file
        let chunker = Chunker::with_size(1024);
        let chunks = chunker.chunk_file(temp_file.path())?;

        // Verify chunks
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].size, 1024);
        assert_eq!(chunks[1].size, 1024);
        assert_eq!(chunks[2].size, 452);

        Ok(())
    }

    #[test]
    fn test_chunk_file_empty() -> Result<()> {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new()?;
        let chunker = Chunker::new();
        let chunks = chunker.chunk_file(temp_file.path())?;

        assert_eq!(chunks.len(), 0);
        Ok(())
    }

    #[test]
    fn test_hash_file() -> Result<()> {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"hello world")?;
        temp_file.flush()?;

        let hash = hash_file(temp_file.path())?;
        
        // Verify it matches direct hashing
        let expected = hash_bytes(b"hello world");
        assert_eq!(hash, expected);

        Ok(())
    }
}
