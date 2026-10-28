use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

#[path = "build_metadata.rs"]
mod build_metadata;

/// Deliberately missing path: Cargo treats a watched path that does not exist as changed, so the
/// script runs on every build. A dirty worktree edit touches no Git metadata file at all, so no
/// finite watch list can keep `source_commit_dirty` truthful; rerunning two short Git commands is
/// the trade-off that keeps the embedded values accurate for normal incremental builds.
const ALWAYS_RERUN: &str = ".git/ajc-build-metadata-refresh";

fn main() {
    println!("cargo:rerun-if-env-changed=AJC_SOURCE_COMMIT");
    println!("cargo:rerun-if-changed={ALWAYS_RERUN}");

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );

    // Explicit in case a future Cargo stops treating a missing path as always-dirty, and to make
    // ref advances visible in `cargo build -v` output.
    for path in build_metadata::watch_paths(&manifest_dir) {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let probe = build_metadata::probe(&manifest_dir, OsStr::new("git"));
    let commit = env::var("AJC_SOURCE_COMMIT")
        .ok()
        .filter(|value| build_metadata::is_object_id(value))
        .or(probe.commit)
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=AJC_SOURCE_COMMIT={commit}");
    println!(
        "cargo:rustc-env=AJC_SOURCE_DIRTY={}",
        build_metadata::dirty_env(probe.dirty)
    );
}
