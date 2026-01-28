# SnapVault

A command-line backup application that creates encrypted, incremental snapshots of your folders with intelligent content-addressed deduplication.

## Overview

SnapVault is a storage-efficient backup tool that creates point-in-time snapshots of your directories using content-addressed storage and chunk-level deduplication. Files are split into chunks, and identical chunks are stored only once across all snapshots, significantly reducing storage requirements.

### Key Features

- **Content-Addressed Storage**: Files are split into chunks identified by their Blake3 hash
- **Intelligent Deduplication**: Identical chunks stored only once across all snapshots
- **Chunked Storage**: 1 MiB fixed-size chunks for optimal balance of dedup and metadata
- **Snapshot Management**: List, restore, and delete individual or all snapshots
- **Reference Counting**: Safe chunk deletion - chunks are only removed when no snapshot references them
- **Fast Hashing**: Blake3 cryptographic hash for content integrity and addressing
- **Repository Structure**: Organized storage with config, snapshots, chunk index, and data directories
- **Security**: Path traversal protection, content verification, and repository validation
- **Error Handling**: Comprehensive error checking and logging
- **CLI Interface**: Declarative flag-based commands for clarity
- **Dedup Statistics**: See space savings and deduplication ratios for each snapshot

### How It Works

1. **Initialize** a repository with proper structure (config, snapshots/, data/chunks/)
2. **Backup** a source directory:
   - Files are split into 1 MiB chunks
   - Each chunk is hashed with Blake3
   - Chunks are stored once in content-addressed storage
   - Manifest tracks which chunks make up each file
   - Chunk index tracks which snapshots reference each chunk
3. **List** all available snapshots with deduplication statistics
4. **Restore** a snapshot by reassembling files from their constituent chunks
5. **Delete** snapshots safely:
   - Remove snapshot from chunk index
   - Identify chunks no longer referenced by any snapshot
   - Delete orphaned chunks from storage
   - Keep chunks still referenced by other snapshots

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/snapvault.git
cd snapvault

# Build and install
cargo build --release
cargo install --path .
```

### Using Cargo

```bash
cargo install snapvault
```

## Quick Start

```bash
# Initialize a repository
snapvault init --repo /path/to/backup-repo

# Create your first backup
snapvault backup --source /path/to/source --repo /path/to/backup-repo

# List all snapshots
snapvault list --repo /path/to/backup-repo

# Restore latest snapshot
snapvault restore --dest /path/to/restore --repo /path/to/backup-repo

# Restore specific snapshot
snapvault restore --dest /path/to/restore --snapshot <snapshot-id> --repo /path/to/backup-repo

# Delete a snapshot
snapvault delete --repo /path/to/backup-repo --snapshot <snapshot-id>

# Delete all snapshots
snapvault delete --repo /path/to/backup-repo --all
```

## Commands

### `init`
Initialize a new backup repository.

```bash
snapvault init --repo <repository-path>
```

Creates the repository structure:
- `config.json`: Repository configuration
- `snapshots/`: Directory for snapshot manifests
- `data/`: Directory for snapshot data

### `backup`
Create a backup snapshot of a source directory.

```bash
snapvault backup --source <source-directory> --repo <repository-path>
```

- Chunks files into 1 MiB blocks
- Hashes each chunk with Blake3
- Stores chunks with automatic deduplication
- Creates a snapshot manifest with file→chunk mappings
- Updates chunk index for reference counting
- Shows deduplication statistics (new chunks vs. reused chunks)

### `list`
List all snapshots in the repository.

```bash
snapvault list --repo <repository-path>
```

Displays a table with:
- Snapshot ID
- Creation timestamp
- File count
- Original size (total bytes)
- Stored size (deduplicated bytes)
- Deduplication percentage
- Source root path

### `restore`
Restore a snapshot to a destination directory.

```bash
# Restore latest snapshot
snapvault restore --dest <destination-directory> --repo <repository-path>

# Restore specific snapshot
snapvault restore --dest <destination-directory> --snapshot <snapshot-id> --repo <repository-path>
```

- Recreates the directory structure
- Reassembles files from their chunks
- Verifies chunk integrity during restoration
- Validates snapshot existence before restoration

### `delete`
Delete snapshots from the repository.

```bash
# Delete specific snapshot
snapvault delete --repo <repository-path> --snapshot <snapshot-id>

# Delete all snapshots
snapvault delete --repo <repository-path> --all
```

- Removes snapshot manifest
- Updates chunk index to remove references
- Deletes orphaned chunks (not referenced by other snapshots)
- Preserves chunks still used by other snapshots
- Provides confirmation and error handling
- Requires explicit `--all` flag to prevent accidental bulk deletion

## Repository Structure

A SnapVault repository has the following structure:

```
repository/
├── config.json          # Repository configuration and version info
├── index.json           # Chunk reference index (snapshot → chunks mapping)
├── snapshots/           # Snapshot manifests (JSON files)
│   └── <snapshot-id>.json  # File metadata + chunk references
└── data/
    └── chunks/          # Content-addressed chunk storage
        └── <prefix>/    # Two-char hash prefix for directory sharding
            └── <hash>   # Chunk file named by Blake3 hash
```

**Example:**
- Chunk with hash `ab123...` stored at `data/chunks/ab/ab123...`
- Manifest references chunks by hash
- Index tracks which snapshots use which chunks

## Current Limitations

- **Fixed-Size Chunking**: Uses 1 MiB fixed chunks (variable-size planned for better dedup)
- **No Encryption**: Data is stored unencrypted (encryption planned for Phase 2)
- **No Compression**: Files are stored uncompressed (compression planned for Phase 3)
- **Basic Security**: Path validation and content verification, but no encryption yet
- **Single Machine**: Local storage only (remote repository support planned)

## Deduplication in Action

### Example Scenario

**First Backup:**
```bash
$ snapvault backup --source ~/documents --repo ~/backup-repo
✓ Backup complete
  Files:            1000
  Total size:       5.00 GiB
  Unique chunks:    5120
  Stored size:      5.00 GiB
  Space saved:      0 B (100.0% dedup)  # First backup, nothing to dedup
  New chunks:       5120
  Reused chunks:    0
```

**Second Backup (80% identical files):**
```bash
$ snapvault backup --source ~/documents --repo ~/backup-repo
✓ Backup complete
  Files:            1000
  Total size:       5.00 GiB
  Unique chunks:    5120
  Stored size:      1.00 GiB           # Only 20% new data stored
  Space saved:      4.00 GiB (80.0% dedup)
  New chunks:       1024
  Reused chunks:    4096                # 80% of chunks already exist
```

## Future Plans (See NEXT_TASKS.md)

- ✅ **Phase 1: Deduplication** - COMPLETED
- **Phase 2: Encryption** - Password-based encryption for all data
- **Phase 3: Compression** - Compress chunks before storage
- **Phase 4: Incremental Backups** - Only process changed files
- **Phase 5: Verification** - Check and repair repository integrity
- **Phase 6: Remote Storage** - Support for S3, SFTP, etc.

## License

[Choose appropriate license: MIT, Apache-2.0, GPL-3.0, etc.]

## Acknowledgments

Inspired by excellent backup tools like:
- [restic](https://restic.net/)
- [borg](https://www.borgbackup.org/)
- [duplicacy](https://duplicacy.com/)

---

**Note**: This tool is under active development. Always test restores and maintain multiple backup copies of critical data. The current implementation is suitable for basic backup needs but lacks advanced features found in production backup tools.
