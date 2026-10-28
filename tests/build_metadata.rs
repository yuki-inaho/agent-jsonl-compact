//! Regression tests for build-time Git metadata (commit and dirty state).
//!
//! Every test builds an isolated synthetic repository under a temporary directory, so the
//! repository that contains the tests is never read or modified.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[path = "../build_metadata.rs"]
mod build_metadata;

use build_metadata::{dirty_env, probe};

const GIT: &str = "git";

fn git_available() -> bool {
    Command::new(GIT)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new(GIT)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("the test requires git on PATH")
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let output = git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}

fn commit_all(dir: &Path, message: &str) {
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-q", "-m", message]);
}

/// Temporary Git repository with one committed tracked file.
fn repository() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let dir = temp.path();
    git_ok(dir, &["init", "-q", "-b", "main"]);
    git_ok(dir, &["config", "user.name", "Build Metadata Test"]);
    git_ok(
        dir,
        &["config", "user.email", "build-metadata@example.invalid"],
    );
    fs::write(dir.join(".gitignore"), "target/\nCargo.lock\n").unwrap();
    fs::write(dir.join("tracked.txt"), "one\n").unwrap();
    commit_all(dir, "initial");
    temp
}

fn head_of(dir: &Path) -> String {
    git_ok(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn clean_repository_reports_the_current_commit_and_clean_state() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let metadata = probe(repo.path(), OsStr::new(GIT));

    assert_eq!(
        metadata.commit.as_deref(),
        Some(head_of(repo.path()).as_str())
    );
    assert_eq!(metadata.dirty, Some(false));
}

#[test]
fn modified_and_untracked_files_are_reported_dirty() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let dir = repo.path();

    fs::write(dir.join("tracked.txt"), "two\n").unwrap();
    assert_eq!(probe(dir, OsStr::new(GIT)).dirty, Some(true));

    git_ok(dir, &["checkout", "--", "tracked.txt"]);
    assert_eq!(probe(dir, OsStr::new(GIT)).dirty, Some(false));

    fs::write(dir.join("untracked.txt"), "new\n").unwrap();
    assert_eq!(probe(dir, OsStr::new(GIT)).dirty, Some(true));
}

#[test]
fn commit_advance_is_seen_even_though_dot_git_head_does_not_change() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let dir = repo.path();
    let head_before = fs::read(dir.join(".git/HEAD")).unwrap();
    let commit_before = probe(dir, OsStr::new(GIT)).commit.unwrap();

    fs::write(dir.join("tracked.txt"), "two\n").unwrap();
    commit_all(dir, "advance branch");

    assert_eq!(
        fs::read(dir.join(".git/HEAD")).unwrap(),
        head_before,
        "committing on a branch must leave .git/HEAD as a symbolic ref"
    );
    let metadata = probe(dir, OsStr::new(GIT));
    assert_ne!(metadata.commit.as_deref(), Some(commit_before.as_str()));
    assert_eq!(metadata.commit.as_deref(), Some(head_of(dir).as_str()));
    assert_eq!(metadata.dirty, Some(false));
}

#[test]
fn linked_worktree_with_dot_git_file_is_probed_through_git() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let base = tempfile::tempdir().unwrap();
    let worktree = base.path().join("linked");
    git_ok(
        repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked-branch",
            worktree.to_str().unwrap(),
        ],
    );

    assert!(
        worktree.join(".git").is_file(),
        "a linked worktree must store a .git file"
    );
    let metadata = probe(&worktree, OsStr::new(GIT));
    assert_eq!(
        metadata.commit.as_deref(),
        Some(head_of(&worktree).as_str())
    );
    assert_eq!(metadata.dirty, Some(false));

    fs::write(worktree.join("tracked.txt"), "dirty in worktree\n").unwrap();
    assert_eq!(probe(&worktree, OsStr::new(GIT)).dirty, Some(true));
    assert_eq!(
        probe(repo.path(), OsStr::new(GIT)).dirty,
        Some(false),
        "the main worktree must stay clean"
    );
}

#[test]
fn watched_paths_cover_head_refs_and_index_for_worktrees() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let paths = build_metadata::watch_paths(repo.path());
    for expected in [".git/HEAD", ".git/index", ".git/refs", ".git/packed-refs"] {
        assert!(
            paths.iter().any(|path| path.ends_with(expected)),
            "missing watch path {expected}: {paths:?}"
        );
    }

    let base = tempfile::tempdir().unwrap();
    let worktree = base.path().join("linked");
    git_ok(
        repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked-branch",
            worktree.to_str().unwrap(),
        ],
    );
    let worktree_paths = build_metadata::watch_paths(&worktree);
    assert!(
        worktree_paths
            .iter()
            .any(|path| path == &worktree.join(".git")),
        "a linked worktree's .git file must be watched: {worktree_paths:?}"
    );
}

#[test]
fn unavailable_git_is_unknown_not_clean() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let repo = repository();
    let metadata = probe(
        repo.path(),
        OsStr::new("/nonexistent/definitely-not-a-git-executable"),
    );
    assert_eq!(metadata.commit, None);
    assert_eq!(metadata.dirty, None);
    assert_eq!(dirty_env(metadata.dirty), "unknown");
    assert_eq!(dirty_env(Some(false)), "false");
    assert_eq!(dirty_env(Some(true)), "true");
}

#[test]
fn cargo_incremental_build_refreshes_commit_and_dirty_state() {
    if !git_available() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::tempdir().unwrap();
    let crate_dir = temp.path().join("metadata-probe");
    let target_dir = temp.path().join("target");
    fs::create_dir_all(crate_dir.join("src")).unwrap();

    // Exercise the real build script and its shared probe, not a re-implementation.
    fs::copy(root.join("build.rs"), crate_dir.join("build.rs")).unwrap();
    fs::copy(
        root.join("build_metadata.rs"),
        crate_dir.join("build_metadata.rs"),
    )
    .unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"metadata-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )
    .unwrap();
    fs::write(
        crate_dir.join("src/main.rs"),
        "fn main() { println!(\"{} {}\", env!(\"AJC_SOURCE_COMMIT\"), env!(\"AJC_SOURCE_DIRTY\")); }\n",
    )
    .unwrap();
    fs::write(crate_dir.join(".gitignore"), "target/\nCargo.lock\n").unwrap();

    git_ok(&crate_dir, &["init", "-q", "-b", "main"]);
    git_ok(&crate_dir, &["config", "user.name", "Build Metadata Test"]);
    git_ok(
        &crate_dir,
        &["config", "user.email", "build-metadata@example.invalid"],
    );
    commit_all(&crate_dir, "initial");

    let (first_commit, first_dirty) = run_probe(&cargo, &crate_dir, &target_dir);
    assert_eq!(first_commit, head_of(&crate_dir));
    assert_eq!(first_dirty, "false");
    let head_file_before = fs::read(crate_dir.join(".git/HEAD")).unwrap();

    // Advance the branch without touching any source file. Only the always-rerun build-script
    // request can make the next incremental build notice the new commit.
    git_ok(
        &crate_dir,
        &["commit", "-q", "--allow-empty", "-m", "advance"],
    );
    assert_eq!(
        fs::read(crate_dir.join(".git/HEAD")).unwrap(),
        head_file_before
    );
    let (second_commit, second_dirty) = run_probe(&cargo, &crate_dir, &target_dir);
    assert_eq!(second_commit, head_of(&crate_dir));
    assert_ne!(second_commit, first_commit);
    assert_eq!(second_dirty, "false");

    // A dirty worktree changes no Git metadata file and no crate source: only the rerun request
    // keeps this truthful.
    fs::write(crate_dir.join("untracked.txt"), "dirty\n").unwrap();
    let (_, dirty_after_edit) = run_probe(&cargo, &crate_dir, &target_dir);
    assert_eq!(dirty_after_edit, "true");

    fs::remove_file(crate_dir.join("untracked.txt")).unwrap();
    let (_, clean_again) = run_probe(&cargo, &crate_dir, &target_dir);
    assert_eq!(clean_again, "false");
}

/// Build and run the synthetic crate, returning `(AJC_SOURCE_COMMIT, AJC_SOURCE_DIRTY)`.
fn run_probe(cargo: &OsStr, crate_dir: &Path, target_dir: &Path) -> (String, String) {
    let output = Command::new(cargo)
        .args(["run", "--quiet", "--offline"])
        .current_dir(crate_dir)
        .env_remove("AJC_SOURCE_COMMIT")
        .env_remove("AJC_SOURCE_DIRTY")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run cargo in the synthetic crate");
    assert!(
        output.status.success(),
        "cargo run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout
        .lines()
        .last()
        .expect("the probe crate prints one line")
        .trim();
    let (commit, dirty) = line
        .split_once(' ')
        .unwrap_or_else(|| panic!("unexpected probe output: {line:?}"));
    (commit.to_string(), dirty.to_string())
}
