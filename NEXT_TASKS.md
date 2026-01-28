# Next Tasks - SnapVault Development Roadmap

## Current Status ✅

We have successfully completed:

- ✅ **Project Setup**: Rust project with proper dependencies
- ✅ **CLI Interface**: `clap`-based command structure
- ✅ **Init Command**: Repository initialization with structure
- ✅ **Backup Command**: Full copy backups with manifest generation
- ✅ **List Command**: Display all snapshots with metadata
- ✅ **Delete Command**: Remove individual or all snapshots
- ✅ **Restore Command**: Restore snapshots to destination
- ✅ **Modular Architecture**: Clean separation of concerns
- ✅ **Comprehensive Testing**: 90% code coverage (unit + integration tests)
- ✅ **Security**: Path traversal protection, validation, safe paths

## Current Limitations

Per `README.md`:
- **Full Copy Only**: No deduplication (each snapshot = complete copy)
- **No Encryption**: Data stored unencrypted
- **No Compression**: Files stored as-is
- **No Incremental Backups**: Each snapshot is independent

---

## Phase 1: Content-Addressed Storage & Deduplication 🎯 **NEXT PRIORITY**

**Goal**: Transform from full-copy to content-addressed storage with chunk-level deduplication.

### What This Means
Currently, if you backup the same 1GB file twice, it uses 2GB. With deduplication, it would use ~1GB (plus small metadata overhead).

### Technical Changes Required

#### 1.1 Implement Chunking Algorithm
- **Task**: Split files into fixed-size or variable-size chunks
- **Approach**: Start with fixed-size chunks (e.g., 1MB) for simplicity
- **Libraries**: Consider `blake3` for hashing (fast, secure)
- **Files to modify**: 
  - New module: `src/chunking.rs`
  - Types: `Chunk`, `ChunkHash`, `Chunker`

#### 1.2 Content-Addressed Storage (CAS)
- **Task**: Store chunks by their hash (content addressing)
- **Structure**:
  ```
  data/
  ├── chunks/
  │   ├── ab/
  │   │   └── ab123456...  (chunk file named by hash)
  │   └── cd/
  │       └── cd789012...
  └── snapshots/
      └── <snapshot-id>/
          └── manifest.json  (references chunk hashes)
  ```
- **Files to modify**:
  - `src/repository/mod.rs` - Add chunk storage paths
  - `src/repository/snapshot.rs` - Change `FileRecord` to reference chunks
  - New module: `src/storage/chunks.rs`

#### 1.3 Update Backup Command
- **Task**: Change backup to chunk files and store deduplicated
- **Logic**:
  1. For each file, split into chunks
  2. Hash each chunk
  3. If chunk exists (by hash), skip writing
  4. If new, write to `chunks/<prefix>/<hash>`
  5. Store file record as list of chunk hashes
- **Files to modify**:
  - `src/commands/backup.rs`
  - Update `SnapshotManifest` schema

#### 1.4 Update Restore Command
- **Task**: Reconstruct files from chunks
- **Logic**:
  1. Read manifest to get chunk list per file
  2. For each chunk hash, read from storage
  3. Concatenate chunks to recreate file
- **Files to modify**:
  - `src/commands/restore.rs`

#### 1.5 Update Delete Command
- **Task**: Track chunk references (ref counting)
- **Challenge**: Can't delete a chunk if other snapshots use it
- **Approach**:
  - Add chunk reference index: `index/chunks.json` (hash → [snapshot_ids])
  - On delete, only remove chunks unique to that snapshot
  - Implement "orphan cleanup" for unreferenced chunks
- **Files to modify**:
  - `src/commands/delete.rs`
  - New module: `src/index/chunk_index.rs`

#### 1.6 Add Stats Command (Optional)
- **Task**: Show repository statistics
- **Display**:
  - Total chunks stored
  - Total unique data size
  - Deduplication ratio
  - Compression ratio (for Phase 3)
- **Files**: New `src/commands/stats.rs`

#### 1.7 Testing
- **Unit tests**: Chunking algorithm, hash uniqueness
- **Integration tests**: 
  - Backup same file twice → verify single chunk storage
  - Backup files with shared content → verify deduplication
  - Delete snapshot → verify chunks remain if used elsewhere
- **Files**: 
  - `tests/deduplication_tests.rs`
  - Update existing integration tests

### Deliverables
- [ ] Chunking implementation with tests
- [ ] Content-addressed storage structure
- [ ] Updated backup command (chunk-based)
- [ ] Updated restore command (chunk reassembly)
- [ ] Updated delete command (with ref counting)
- [ ] Chunk index implementation
- [ ] Migration guide (how to upgrade existing repos)
- [ ] Updated README reflecting deduplication

### Estimated Complexity
**High** - This is a significant architectural change that touches most of the codebase.

---

## Phase 2: Encryption 🔐

**Goal**: Encrypt all data at rest (chunks, manifests, index).

### Technical Changes Required

#### 2.1 Encryption Setup
- **Task**: Repository password/key management
- **Libraries**: 
  - `chacha20poly1305` or `aes-gcm` for encryption
  - `argon2` for key derivation
- **Storage**: 
  - `config.json` stores salt, KDF params
  - Key never stored, derived from password at runtime

#### 2.2 Encrypted Chunks
- **Task**: Encrypt chunks before writing to disk
- **Approach**: Encrypt each chunk with derived key
- **Files**: `src/crypto/encryption.rs`

#### 2.3 Encrypted Manifests
- **Task**: Encrypt snapshot manifests
- **Files**: `src/repository/snapshot.rs`

#### 2.4 CLI Changes
- **Task**: Add password prompt to relevant commands
- **Commands affected**: `init`, `backup`, `list`, `restore`, `delete`
- **UX**: Use `rpassword` crate for secure password input

#### 2.5 Testing
- **Tests**: 
  - Encryption/decryption roundtrip
  - Wrong password detection
  - Performance impact measurement

### Deliverables
- [ ] Encryption module
- [ ] Password-based key derivation
- [ ] Encrypted chunk storage
- [ ] Encrypted manifest storage
- [ ] CLI password handling
- [ ] Updated README with security notes

---

## Phase 3: Compression 📦

**Goal**: Compress chunks before encryption for storage efficiency.

### Technical Changes Required

#### 3.1 Compression Implementation
- **Task**: Add compression layer
- **Libraries**: `zstd` (fast, good ratio) or `lz4` (faster, lower ratio)
- **Pipeline**: `File → Chunk → Compress → Encrypt → Store`

#### 3.2 Storage Format
- **Task**: Store compression metadata
- **Approach**: Add compression algorithm field to chunk metadata

#### 3.3 CLI Flag
- **Task**: Optional compression level flag
- **Example**: `snapvault backup --compression-level 3`

### Deliverables
- [ ] Compression module
- [ ] Integration with chunking
- [ ] Compression statistics in stats command
- [ ] Updated README

---

## Phase 4: Incremental Backups 📈

**Goal**: Only process changed files since last snapshot.

### Technical Changes Required

#### 4.1 File Change Detection
- **Task**: Compare file metadata (mtime, size, hash)
- **Approach**: Store content hash in manifest, compare on next backup

#### 4.2 Differential Backup
- **Task**: Skip unchanged files
- **Logic**: 
  - Read last snapshot manifest
  - For each file, check if metadata matches
  - Only chunk/store if changed

#### 4.3 CLI Enhancement
- **Task**: Show incremental stats
- **Display**: "X files changed, Y files unchanged, Z new files"

### Deliverables
- [ ] Change detection algorithm
- [ ] Incremental backup logic
- [ ] Updated backup command
- [ ] Performance benchmarks

---

## Phase 5: Verification & Integrity 🔍

**Goal**: Verify backup integrity and detect corruption.

### Technical Changes Required

#### 5.1 Check Command
- **Task**: New command to verify repository
- **Checks**:
  - All referenced chunks exist
  - Chunk hashes match content
  - Manifests are valid JSON
  - No orphaned chunks (optional cleanup)

#### 5.2 Verify Command
- **Task**: Verify specific snapshot
- **Usage**: `snapvault verify --snapshot <id> --repo <path>`

### Deliverables
- [ ] Check command implementation
- [ ] Verify command implementation
- [ ] Corruption detection and reporting
- [ ] Optional repair mode

---

## Phase 6: Performance & Optimization ⚡

### Potential Improvements
- Parallel chunking (rayon)
- Parallel chunk reading/writing
- Better chunk index (SQLite or custom format)
- Memory-mapped file I/O for large files
- Progress bars for long operations
- Chunk cache for frequently accessed data

---

## Phase 7: Advanced Features 🚀

### Future Enhancements
- Remote repository support (S3, SFTP)
- Prune command (remove old snapshots by policy)
- Mount command (FUSE filesystem to browse snapshots)
- Tagging and snapshot descriptions
- Exclude patterns (like `.gitignore`)
- Sparse file support
- Extended attributes preservation
- ACL preservation

---

## Recommended Next Steps

1. **Start with Phase 1 (Deduplication)** - This is the most impactful feature
2. Break Phase 1 into smaller tasks:
   - First: Implement chunking with tests
   - Second: Implement CAS storage
   - Third: Update backup command
   - Fourth: Update restore command
   - Fifth: Update delete command with ref counting
3. After Phase 1 is stable, move to Phase 2 (Encryption)
4. Phases 3-7 can be done incrementally based on user needs

---

## Notes

- Each phase should include comprehensive testing
- Update README incrementally as features are added
- Consider backward compatibility or provide migration tools
- Performance benchmarks before/after each major change
- Security audit after encryption implementation
