# SnapVault

A command-line backup application that creates point-in-time snapshots of your directories using full copy backups with snapshot management.

## Overview

SnapVault is a robust backup tool designed to create reliable snapshots of your directories. The current implementation uses full copy backups (no deduplication yet), storing each snapshot as a complete copy of the source directory. It provides comprehensive snapshot management including listing, restoring, and deleting snapshots.

### Key Features

- **Full Copy Backups**: Creates complete copies of source directories for each snapshot
- **Snapshot Management**: List, restore, and delete individual or all snapshots
- **Repository Structure**: Organized storage with config, snapshots metadata, and data directories
- **Security**: Path traversal protection and repository validation
- **Error Handling**: Comprehensive error checking and logging
- **CLI Interface**: Declarative flag-based commands for clarity

### How It Works

1. **Initialize** a repository with proper structure and permissions
2. **Backup** a source directory by creating a full copy and snapshot manifest
3. **List** all available snapshots with metadata
4. **Restore** a snapshot to a target directory
5. **Delete** individual snapshots or all snapshots with confirmation

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

- Performs a full copy of the source directory
- Creates a snapshot manifest with file metadata
- Generates a unique snapshot ID based on timestamp

### `list`
List all snapshots in the repository.

```bash
snapvault list --repo <repository-path>
```

Displays a table with:
- Snapshot ID
- Creation timestamp
- File count
- Total bytes
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
- Copies all files from the snapshot
- Validates snapshot existence before restoration

### `delete`
Delete snapshots from the repository.

```bash
# Delete specific snapshot
snapvault delete --repo <repository-path> --snapshot <snapshot-id>

# Delete all snapshots
snapvault delete --repo <repository-path> --all
```

- Removes both manifest and data for each snapshot
- Provides confirmation and error handling
- Requires explicit `--all` flag to prevent accidental bulk deletion

## Repository Structure

A SnapVault repository has the following structure:

```
repository/
├── config.json          # Repository configuration and version info
├── snapshots/           # Snapshot manifests (JSON files)
│   └── <snapshot-id>.json
└── data/                # Snapshot data directories
    └── <snapshot-id>/
        └── <copied-files>
```

## Limitations

- **Full Copy Only**: Current implementation creates full copies without deduplication
- **No Encryption**: Data is stored unencrypted (encryption planned for future)
- **No Compression**: Files are stored uncompressed
- **Basic Security**: Repository permissions set, but no advanced security features
- **No Incremental Backups**: Each snapshot is independent

## Future Plans

- Implement deduplication for storage efficiency
- Add encryption for data security
- Compression support
- Incremental backup capabilities
- Remote repository support

## License

[Choose appropriate license: MIT, Apache-2.0, GPL-3.0, etc.]

## Acknowledgments

Inspired by excellent backup tools like:
- [restic](https://restic.net/)
- [borg](https://www.borgbackup.org/)
- [duplicacy](https://duplicacy.com/)

---

**Note**: This tool is under active development. Always test restores and maintain multiple backup copies of critical data. The current implementation is suitable for basic backup needs but lacks advanced features found in production backup tools.
