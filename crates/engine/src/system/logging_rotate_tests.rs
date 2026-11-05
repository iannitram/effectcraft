//! Locked tests for `rotate_if_full` (crates/engine/src/system/logging.rs).
//!
//! This file is a child module of `logging` so it can reach the private helper.
//! Add these two lines at the end of crates/engine/src/system/logging.rs:
//!
//! #[cfg(test)]
//! #[path = "logging_rotate_tests.rs"]
//! mod rotate_tests;
//!
//! and commit this file as crates/engine/src/system/logging_rotate_tests.rs.

use super::rotate_if_full;
use std::path::PathBuf;

const LOG: &str = "EffectCraft Log.txt";
const OLD: &str = "EffectCraft Log.old.txt";

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("effectcraft-rotate-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_file_over_the_cap_is_moved_to_old() {
    let dir = fresh_dir("over");
    let path = dir.join(LOG);
    std::fs::write(&path, vec![b'a'; 101]).unwrap();
    assert!(rotate_if_full(&path, 100));
    assert!(!path.exists());
    assert_eq!(std::fs::read(dir.join(OLD)).unwrap(), vec![b'a'; 101]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_exactly_at_the_cap_is_moved_too() {
    let dir = fresh_dir("at");
    let path = dir.join(LOG);
    std::fs::write(&path, vec![b'b'; 100]).unwrap();
    assert!(rotate_if_full(&path, 100));
    assert!(!path.exists());
    assert_eq!(std::fs::read(dir.join(OLD)).unwrap(), vec![b'b'; 100]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_under_the_cap_is_left_alone() {
    let dir = fresh_dir("under");
    let path = dir.join(LOG);
    std::fs::write(&path, vec![b'c'; 99]).unwrap();
    assert!(!rotate_if_full(&path, 100));
    assert_eq!(std::fs::read(&path).unwrap(), vec![b'c'; 99]);
    assert!(!dir.join(OLD).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_file_is_not_a_rotation() {
    let dir = fresh_dir("missing");
    assert!(!rotate_if_full(&dir.join(LOG), 100));
    assert!(!dir.join(OLD).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_existing_old_file_is_replaced() {
    let dir = fresh_dir("replace");
    let path = dir.join(LOG);
    std::fs::write(&path, vec![b'd'; 150]).unwrap();
    assert!(rotate_if_full(&path, 100));
    std::fs::write(&path, vec![b'e'; 120]).unwrap();
    assert!(rotate_if_full(&path, 100));
    assert_eq!(std::fs::read(dir.join(OLD)).unwrap(), vec![b'e'; 120]);
    assert!(!path.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
