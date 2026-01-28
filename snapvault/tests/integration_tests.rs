use assert_fs::prelude::*;
use assert_fs::TempDir;
use predicates::prelude::*;
use snapvault::{commands, Repository};
use std::fs;

/// Test complete workflow: init -> backup -> list -> restore
#[test]
fn test_complete_workflow() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();
    let dest = temp.child("restored");

    // Create test files
    source.child("file1.txt").write_str("content1").unwrap();
    source.child("dir/file2.txt").write_str("content2").unwrap();

    // Init repository
    commands::init(repo_path.path()).unwrap();
    repo_path.assert(predicate::path::is_dir());
    repo_path
        .child("config.json")
        .assert(predicate::path::is_file());

    // Backup
    commands::backup(source.path(), repo_path.path()).unwrap();

    // List snapshots
    commands::list(repo_path.path()).unwrap();

    // Get snapshot ID
    let snapshot_id = get_first_snapshot_id(repo_path.path());

    // Restore
    commands::restore(Some(&snapshot_id), dest.path(), repo_path.path()).unwrap();

    // Verify restored files
    dest.child("file1.txt").assert("content1");
    dest.child("dir/file2.txt").assert("content2");
}

/// Test multiple backups and list ordering
#[test]
fn test_multiple_backups_ordering() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    source.child("file.txt").write_str("v1").unwrap();

    commands::init(repo_path.path()).unwrap();

    // Create multiple backups
    commands::backup(source.path(), repo_path.path()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    source.child("file.txt").write_str("v2").unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    // List should show 2 snapshots
    let count = fs::read_dir(repo_path.child("snapshots").path())
        .unwrap()
        .count();
    assert_eq!(count, 2);
}

/// Test backup with empty directory
#[test]
fn test_backup_empty_directory() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    // Should create snapshot even with no files
    let count = fs::read_dir(repo_path.child("snapshots").path())
        .unwrap()
        .count();
    assert_eq!(count, 1);
}

/// Test restore to new directory
#[test]
fn test_restore_creates_destination() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();
    let dest = temp.child("new_dir/subdir/restored");

    source.child("file.txt").write_str("content").unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    let snapshot_id = get_first_snapshot_id(repo_path.path());
    commands::restore(Some(&snapshot_id), dest.path(), repo_path.path()).unwrap();

    dest.child("file.txt").assert("content");
}

/// Test delete all snapshots
#[test]
fn test_delete_all_workflow() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    source.child("file.txt").write_str("content").unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    // Verify 2 snapshots exist
    let count_before = fs::read_dir(repo_path.child("snapshots").path())
        .unwrap()
        .count();
    assert_eq!(count_before, 2);

    // Delete all
    commands::delete(repo_path.path(), None, true).unwrap();

    // Verify all deleted
    let count_after = fs::read_dir(repo_path.child("snapshots").path())
        .unwrap()
        .count();
    assert_eq!(count_after, 0);
}

/// Test backup with deeply nested directories
#[test]
fn test_deeply_nested_directories() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    source
        .child("a/b/c/d/e/f/file.txt")
        .write_str("deep")
        .unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    let dest = temp.child("restored");
    let snapshot_id = get_first_snapshot_id(repo_path.path());
    commands::restore(Some(&snapshot_id), dest.path(), repo_path.path()).unwrap();

    dest.child("a/b/c/d/e/f/file.txt").assert("deep");
}

/// Test backup with special characters in filenames
#[test]
fn test_special_characters_in_filenames() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    // Note: Some characters may not be valid on all filesystems
    source.child("file with spaces.txt").write_str("spaces").unwrap();
    source.child("file-with-dashes.txt").write_str("dashes").unwrap();
    source.child("file_with_underscores.txt").write_str("underscores").unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    let dest = temp.child("restored");
    let snapshot_id = get_first_snapshot_id(repo_path.path());
    commands::restore(Some(&snapshot_id), dest.path(), repo_path.path()).unwrap();

    dest.child("file with spaces.txt").assert("spaces");
    dest.child("file-with-dashes.txt").assert("dashes");
    dest.child("file_with_underscores.txt").assert("underscores");
}

/// Test restore latest snapshot without specifying ID
#[test]
fn test_restore_latest_snapshot() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();
    let dest = temp.child("restored");

    source.child("v1.txt").write_str("version1").unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    source.child("v2.txt").write_str("version2").unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    // Restore latest (should have v2.txt)
    commands::restore(None, dest.path(), repo_path.path()).unwrap();

    dest.child("v1.txt").assert("version1");
    dest.child("v2.txt").assert("version2");
}

/// Test repository validation
#[test]
fn test_repository_validation() {
    let temp = TempDir::new().unwrap();
    let invalid_repo = temp.child("invalid");
    fs::create_dir_all(&invalid_repo).unwrap();

    // Try to open invalid repo
    let result = Repository::open(invalid_repo.path());
    assert!(result.is_err());
}

/// Test backup with large number of files
#[test]
fn test_backup_many_files() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    // Create 100 small files
    for i in 0..100 {
        source
            .child(format!("file_{:03}.txt", i))
            .write_str(&format!("content_{}", i))
            .unwrap();
    }

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    let dest = temp.child("restored");
    let snapshot_id = get_first_snapshot_id(repo_path.path());
    commands::restore(Some(&snapshot_id), dest.path(), repo_path.path()).unwrap();

    // Verify a few files
    dest.child("file_000.txt").assert("content_0");
    dest.child("file_050.txt").assert("content_50");
    dest.child("file_099.txt").assert("content_99");
}

/// Test error handling for operations on non-existent repo
#[test]
fn test_operations_on_nonexistent_repo() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("nonexistent");
    let source = TempDir::new().unwrap();

    source.child("file.txt").write_str("content").unwrap();

    // Backup should fail
    let result = commands::backup(source.path(), repo_path.path());
    assert!(result.is_err());

    // List should fail
    let result = commands::list(repo_path.path());
    assert!(result.is_err());

    // Delete should fail
    let result = commands::delete(repo_path.path(), Some("snap"), false);
    assert!(result.is_err());
}

/// Test concurrent backups don't conflict (different snapshot IDs)
#[test]
fn test_concurrent_backups_different_ids() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    source.child("file.txt").write_str("content").unwrap();

    commands::init(repo_path.path()).unwrap();

    // Create multiple backups rapidly
    for _ in 0..5 {
        commands::backup(source.path(), repo_path.path()).unwrap();
        // Small delay to ensure different millisecond timestamps
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // All snapshots should have unique IDs
    let count = fs::read_dir(repo_path.child("snapshots").path())
        .unwrap()
        .count();
    assert_eq!(count, 5);
}

/// Test backup preserves file metadata (timestamps)
#[test]
fn test_metadata_preservation() {
    let temp = TempDir::new().unwrap();
    let repo_path = temp.child("repo");
    let source = TempDir::new().unwrap();

    source.child("file.txt").write_str("content").unwrap();

    commands::init(repo_path.path()).unwrap();
    commands::backup(source.path(), repo_path.path()).unwrap();

    // Read manifest and verify metadata exists
    let snapshot_id = get_first_snapshot_id(repo_path.path());
    let manifest_path = repo_path
        .child("snapshots")
        .child(format!("{}.json", snapshot_id));
    let manifest_content = fs::read_to_string(manifest_path.path()).unwrap();

    assert!(manifest_content.contains("modified"));
    assert!(manifest_content.contains("size"));
}

// Helper function
fn get_first_snapshot_id(repo_path: &std::path::Path) -> String {
    let snapshots_dir = repo_path.join("snapshots");
    for entry in fs::read_dir(&snapshots_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("json")) {
            return path.file_stem().unwrap().to_string_lossy().to_string();
        }
    }
    panic!("No snapshots found");
}
