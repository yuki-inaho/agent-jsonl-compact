//! Git metadata probing shared by `build.rs` and its regression tests.
//!
//! This file is compiled into the build script (`#[path = "build_metadata.rs"] mod ...`) and
//! included directly by `tests/build_metadata.rs`, so it must stay free of `main` and of any
//! dependency outside the standard library.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Build-time Git state. `None` means "Git could not be queried", which is not the same as a
/// clean worktree: callers must not turn it into `clean`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
}

/// Probe `dir` for the current commit and worktree state.
///
/// `git` is the executable to run, so tests can pass a nonexistent path to simulate a host
/// without Git without mutating process-wide environment variables.
pub fn probe(dir: &Path, git: &OsStr) -> Metadata {
    Metadata {
        commit: commit(dir, git),
        dirty: dirty(dir, git),
    }
}

/// Full Git object ID of `HEAD`, or `None` when Git is unavailable or `dir` is not a repository.
pub fn commit(dir: &Path, git: &OsStr) -> Option<String> {
    let output = run(dir, git, &["rev-parse", "--verify", "HEAD"])?;
    let commit = std::str::from_utf8(&output.stdout).ok()?.trim().to_string();
    is_object_id(&commit).then_some(commit)
}

/// Whether `git status --porcelain` reports any change, or `None` when Git is unavailable.
///
/// Staged, unstaged, and untracked changes all count. `--no-optional-locks` keeps the probe
/// read-only by preventing Git from opportunistically refreshing the index.
pub fn dirty(dir: &Path, git: &OsStr) -> Option<bool> {
    let output = run(dir, git, &["status", "--porcelain=v1"])?;
    Some(output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()))
}

/// Files and directories that identify the current commit and index.
///
/// Cargo invalidates the build script when any of these change. `HEAD` alone is not enough:
/// committing on a branch rewrites `refs/heads/<branch>` (or `packed-refs`) and leaves
/// `.git/HEAD` untouched, and a linked worktree has `.git` as a file that points at
/// `.git/worktrees/<name>`. Dirty worktree edits are not covered by any of these paths, so
/// `build.rs` additionally requests an unconditional rerun.
pub fn watch_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // A linked worktree stores `gitdir: ...` in `.git` instead of a directory.
    let dot_git = dir.join(".git");
    if dot_git.is_file() {
        paths.push(dot_git);
    }

    let Some(git_dir) = rev_parse_path(dir, "--absolute-git-dir") else {
        return paths;
    };
    let common_dir = rev_parse_path(dir, "--git-common-dir").unwrap_or_else(|| git_dir.clone());

    paths.push(git_dir.join("HEAD"));
    paths.push(git_dir.join("index"));
    paths.push(common_dir.join("HEAD"));
    paths.push(common_dir.join("packed-refs"));
    // Loose refs; a directory entry makes Cargo watch the whole tree.
    paths.push(common_dir.join("refs"));

    paths.sort();
    paths.dedup();
    paths
}

/// Render the build-script value for [`Metadata::dirty`].
pub fn dirty_env(dirty: Option<bool>) -> &'static str {
    match dirty {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

/// Git object IDs are SHA-1 (40 hex) today, but SHA-256 repositories use 64.
pub fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn rev_parse_path(dir: &Path, flag: &str) -> Option<PathBuf> {
    let output = run(dir, OsStr::new("git"), &["rev-parse", flag])?;
    let raw = std::str::from_utf8(&output.stdout).ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    })
}

fn run(dir: &Path, git: &OsStr, args: &[&str]) -> Option<Output> {
    let output = Command::new(git)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output)
}
