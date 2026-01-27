# SnapVault

A command-line backup application that creates encrypted, incremental snapshots of your folders with intelligent deduplication.

## Overview

SnapVault is a storage-efficient backup tool that creates point-in-time snapshots of your directories. It uses content-addressable storage and deduplication to ensure that identical data is stored only once, even across multiple snapshots. All data is encrypted at rest, making it safe to store backups on untrusted media.

### How It Works

1. **Scan** a source directory (files + metadata)
2. **Chunk** file content into blocks
3. **Hash** each chunk for content addressing
4. **Deduplicate** by storing only new chunks
5. **Encrypt** all data before writing to the repository
6. **Create** a snapshot manifest that references chunks by hash
7. **Restore**, verify integrity, and manage snapshots over time

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
snapvault init /path/to/backup-repo

# Create your first backup
snapvault backup /path/to/source --repo /path/to/backup-repo

# List all snapshots
snapvault snapshots --repo /path/to/backup-repo

# Restore a snapshot
snapvault restore --repo /path/to/backup-repo <snapshot-id> --target /path/to/restore
```

## License

[Choose appropriate license: MIT, Apache-2.0, GPL-3.0, etc.]

## Acknowledgments

Inspired by excellent backup tools like:
- [restic](https://restic.net/)
- [borg](https://www.borgbackup.org/)
- [duplicacy](https://duplicacy.com/)

---

**Note**: This tool is under active development. Always test restores and maintain multiple backup copies of critical data.
