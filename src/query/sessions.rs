//! `sessions` discovery: bounded recursive JSONL scan under declared roots.
//!
//! Only the leading/trailing bounded window of each file is parsed for cwd/title/timestamps;
//! whole session logs are never loaded. Symlinks are not followed, the number of scanned files
//! is capped (`--scan-limit`) and reaching the cap is reported as partial coverage.

use crate::query::model::Provider;
use crate::query::source::{discover_meta, file_source_id, SessionMeta};
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_SCAN_ENTRIES: u64 = 200_000;
pub const MAX_SCAN_DEPTH: usize = 32;
pub const MAX_SCAN_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_SCAN_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_SCAN_SECONDS: u64 = 30;

#[derive(Debug, Clone)]
pub struct SessionCandidate {
    pub provider: Provider,
    pub session_id: String,
    pub source_id: String,
    pub path: PathBuf,
    pub started_at: Option<String>,
    pub last_event_at: Option<String>,
    pub last_epoch_ms: Option<i64>,
    pub cwd: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub roots: Vec<PathBuf>,
    pub files_scanned: u64,
    pub files_unreadable: u64,
    pub files_skipped_no_id: u64,
    pub partial: bool,
    pub scan_limit_reached: bool,
    pub entries_visited: u64,
    pub scan_bytes: u64,
    pub limit_reason: Option<&'static str>,
    /// Roots, directories or entries that could not be inspected because of an I/O error.
    pub walk_errors: u64,
    /// Symlink entries skipped without being followed. Their targets are not claimed as scanned.
    pub symlinks_skipped: u64,
    /// A duplicate session id was found; only meaningful when the scan is complete.
    pub ambiguous: bool,
    pub ambiguous_session_id: Option<String>,
    /// Records that exceeded the per-record cap while reading the file's metadata window.
    pub records_uninspected: u64,
    /// Invalid JSON records seen while reading the file's metadata window.
    pub records_invalid: u64,
    /// Bounded samples of unread/invalid ranges for this file.
    pub coverage_samples: Vec<(u64, u64)>,
}

pub fn default_roots(provider: Provider, home: &Path) -> Vec<PathBuf> {
    match provider {
        Provider::Codex => vec![home.join(".codex").join("sessions")],
        Provider::ClaudeCode => vec![home.join(".claude").join("projects")],
        // OpenCode run logs are never guessed: an explicit --root is required.
        Provider::OpenCode => Vec::new(),
    }
}

fn path_line_hint(_path: &Path) -> u64 {
    0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkOutcome {
    Complete,
    Limit,
    TimeBudget,
}

fn mark_incomplete(report: &mut ScanReport, reason: &'static str) {
    report.partial = true;
    if report.limit_reason.is_none() {
        report.limit_reason = Some(reason);
    }
}

fn time_budget_exhausted(started: Instant) -> bool {
    started.elapsed() > Duration::from_secs(MAX_SCAN_SECONDS)
}

fn walk(
    root: &Path,
    files: &mut Vec<PathBuf>,
    limit: usize,
    report: &mut ScanReport,
    started: Instant,
) -> WalkOutcome {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if time_budget_exhausted(started) {
            mark_incomplete(report, "scan_time");
            return WalkOutcome::TimeBudget;
        }
        if depth > MAX_SCAN_DEPTH {
            mark_incomplete(report, "scan_depth");
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                report.walk_errors = report.walk_errors.saturating_add(1);
                mark_incomplete(report, "scan_io");
                continue;
            }
        };
        let remaining = MAX_SCAN_ENTRIES.saturating_sub(report.entries_visited) as usize;
        let mut sorted: Vec<PathBuf> = Vec::new();
        for entry in entries.take(remaining.saturating_add(1)) {
            if time_budget_exhausted(started) {
                mark_incomplete(report, "scan_time");
                return WalkOutcome::TimeBudget;
            }
            match entry {
                Ok(entry) => sorted.push(entry.path()),
                Err(_) => {
                    report.walk_errors = report.walk_errors.saturating_add(1);
                    mark_incomplete(report, "scan_io");
                }
            }
        }
        if sorted.len() > remaining {
            mark_incomplete(report, "scan_entries");
        }
        sorted.sort();
        for path in sorted {
            if time_budget_exhausted(started) {
                mark_incomplete(report, "scan_time");
                return WalkOutcome::TimeBudget;
            }
            report.entries_visited = report.entries_visited.saturating_add(1);
            if report.entries_visited > MAX_SCAN_ENTRIES {
                mark_incomplete(report, "scan_entries");
                return WalkOutcome::Limit;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    report.walk_errors = report.walk_errors.saturating_add(1);
                    mark_incomplete(report, "scan_io");
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                // Never follow arbitrary symlinks. Skipping one can hide a session file or a
                // whole subtree, so a complete scan cannot be claimed.
                report.symlinks_skipped = report.symlinks_skipped.saturating_add(1);
                mark_incomplete(report, "scan_symlink");
                continue;
            }
            if metadata.is_dir() {
                stack.push((path, depth + 1));
            } else if metadata.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
            {
                if files.len() >= limit {
                    mark_incomplete(report, "scan_limit");
                    return WalkOutcome::Limit;
                }
                files.push(path);
            }
        }
    }
    WalkOutcome::Complete
}

pub fn scan(
    roots: &[PathBuf],
    provider: Provider,
    cwd: Option<&str>,
    cwd_children: bool,
    scan_limit: usize,
) -> Result<(Vec<SessionCandidate>, ScanReport)> {
    let mut report = ScanReport {
        roots: roots.to_vec(),
        ..Default::default()
    };
    let mut candidates: Vec<SessionCandidate> = Vec::new();
    let mut seen_ids: Vec<(Provider, String)> = Vec::new();
    let mut ambiguous: Option<(Provider, String)> = None;
    let started = Instant::now();

    for root in roots {
        match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                // An explicit root that is not a directory cannot be scanned. A missing
                // default root is intentionally skipped below instead of being a failure.
                report.walk_errors = report.walk_errors.saturating_add(1);
                mark_incomplete(&mut report, "scan_io");
                continue;
            }
            Err(_) => {
                // Every root passed here is a declared root. Default roots that are absent are
                // filtered by the caller, so a missing root here means the declared scope could
                // not be scanned and completion cannot be claimed.
                report.walk_errors = report.walk_errors.saturating_add(1);
                mark_incomplete(&mut report, "scan_io");
                continue;
            }
        }
        let mut files = Vec::new();
        match walk(root, &mut files, scan_limit, &mut report, started) {
            WalkOutcome::Limit => {
                report.scan_limit_reached = true;
                report.partial = true;
            }
            WalkOutcome::TimeBudget => {}
            WalkOutcome::Complete => {}
        }
        for path in files {
            if time_budget_exhausted(started) {
                mark_incomplete(&mut report, "scan_time");
                break;
            }
            let file_len = match fs::metadata(&path) {
                Ok(metadata) => metadata.len(),
                Err(_) => {
                    report.files_unreadable = report.files_unreadable.saturating_add(1);
                    mark_incomplete(&mut report, "scan_io");
                    continue;
                }
            };
            if file_len > MAX_SCAN_FILE_BYTES {
                mark_incomplete(&mut report, "scan_file_bytes");
                continue;
            }
            if report.scan_bytes.saturating_add(file_len) > MAX_SCAN_BYTES {
                mark_incomplete(&mut report, "scan_bytes");
                break;
            }
            report.scan_bytes = report.scan_bytes.saturating_add(file_len);
            report.files_scanned += 1;
            // Whole-file bounded scan so last activity after any preview window is found.
            let meta: SessionMeta = match discover_meta(
                &path,
                provider,
                crate::query::reader::DEFAULT_MAX_RECORD_BYTES,
            ) {
                Ok(meta) => meta,
                Err(_) => {
                    report.files_unreadable = report.files_unreadable.saturating_add(1);
                    mark_incomplete(&mut report, "scan_io");
                    continue;
                }
            };
            if meta.uninspected > 0 || meta.invalid > 0 {
                report.partial = true;
                report.records_uninspected += meta.uninspected;
                report.records_invalid += meta.invalid;
                if report.coverage_samples.len() < 8 {
                    report
                        .coverage_samples
                        .push((path_line_hint(&path), meta.uninspected));
                }
            }
            let Some(session_id) = meta.session_id.clone() else {
                report.files_skipped_no_id += 1;
                continue;
            };
            if let Some((_, existing)) = seen_ids
                .iter()
                .find(|(p, id)| *p == provider && *id == session_id)
            {
                let _ = existing;
                ambiguous = Some((provider, session_id.clone()));
            }
            seen_ids.push((provider, session_id.clone()));
            if let Some(expected) = cwd {
                let Some(actual) = &meta.cwd else {
                    continue;
                };
                let matches = if cwd_children {
                    actual == expected
                        || actual.starts_with(&format!("{}/", expected.trim_end_matches('/')))
                } else {
                    actual == expected
                };
                if !matches {
                    continue;
                }
            }
            candidates.push(SessionCandidate {
                provider,
                source_id: file_source_id(&path),
                session_id,
                path,
                started_at: meta.started_at,
                last_event_at: meta.last_event_at,
                last_epoch_ms: meta.last_epoch_ms,
                cwd: meta.cwd,
                title: meta.title,
            });
        }
    }

    if let Some((_provider, session_id)) = ambiguous {
        report.ambiguous = true;
        report.ambiguous_session_id = Some(session_id);
        // Ambiguity is reported, not raised here: a `sessions` listing treats any duplicate as a
        // failure, while `--session` resolution only cares about the requested id. A partial scan
        // cannot prove that any duplicate set is complete; the caller keeps that conservatism.
    }

    // Newest known event first; sessions without a parseable time keep a stable name order.
    candidates.sort_by(|a, b| {
        b.last_epoch_ms
            .cmp(&a.last_epoch_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Ok((candidates, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_stops_when_the_time_budget_is_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("session.jsonl"), b"{}").unwrap();
        let started = Instant::now()
            .checked_sub(Duration::from_secs(MAX_SCAN_SECONDS + 1))
            .unwrap();
        let mut files = Vec::new();
        let mut report = ScanReport::default();

        let outcome = walk(dir.path(), &mut files, 10, &mut report, started);

        assert_eq!(outcome, WalkOutcome::TimeBudget);
        assert!(report.partial);
        assert_eq!(report.limit_reason, Some("scan_time"));
        assert!(files.is_empty());
    }

    #[test]
    fn declared_roots_that_cannot_be_scanned_are_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let (_, report) = scan(&[missing], Provider::Codex, None, false, 10).unwrap();
        assert!(report.partial);
        assert_eq!(report.limit_reason, Some("scan_io"));
        assert_eq!(report.walk_errors, 1);

        let file_root = dir.path().join("root.jsonl");
        fs::write(&file_root, b"{}").unwrap();
        let (_, report) = scan(&[file_root], Provider::Codex, None, false, 10).unwrap();
        assert!(report.partial);
        assert_eq!(report.limit_reason, Some("scan_io"));
        assert_eq!(report.walk_errors, 1);
    }

    #[cfg(unix)]
    #[test]
    fn skipped_symlinks_make_the_scan_partial_without_following_them() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.jsonl");
        fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("link.jsonl")).unwrap();

        let (candidates, report) = scan(
            &[dir.path().to_path_buf()],
            Provider::Codex,
            None,
            false,
            10,
        )
        .unwrap();

        assert!(candidates.is_empty());
        assert!(report.partial);
        assert_eq!(report.limit_reason, Some("scan_symlink"));
        assert_eq!(report.symlinks_skipped, 1);
    }
}
