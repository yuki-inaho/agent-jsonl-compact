//! `sessions` discovery: bounded recursive JSONL scan under declared roots.
//!
//! Only the leading/trailing bounded window of each file is parsed for cwd/title/timestamps;
//! whole session logs are never loaded. Symlinks are not followed, the number of scanned files
//! is capped (`--scan-limit`) and reaching the cap is reported as partial coverage.

use crate::query::model::Provider;
use crate::query::source::{discover_meta, file_source_id, SessionMeta};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};

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

fn walk(root: &Path, files: &mut Vec<PathBuf>, limit: usize) -> Result<bool> {
    let mut stack = vec![root.to_path_buf()];
    let mut truncated = false;
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut sorted: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        sorted.sort();
        for path in sorted {
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if files.len() >= limit {
                    truncated = true;
                    return Ok(truncated);
                }
                files.push(path);
            }
        }
    }
    Ok(truncated)
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

    for root in roots {
        if !root.exists() {
            continue;
        }
        let mut files = Vec::new();
        if walk(root, &mut files, scan_limit).unwrap_or(false) {
            report.scan_limit_reached = true;
            report.partial = true;
        }
        for path in files {
            report.files_scanned += 1;
            // Whole-file bounded scan so last activity after any preview window is found.
            let meta: SessionMeta = match discover_meta(
                &path,
                provider,
                crate::query::reader::DEFAULT_MAX_RECORD_BYTES,
            ) {
                Ok(meta) => meta,
                Err(_) => {
                    report.files_unreadable += 1;
                    continue;
                }
            };
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

    if let Some((provider, session_id)) = ambiguous {
        return Err(anyhow!(
            "ambiguous session id '{}' for provider {}: multiple files claim it",
            session_id,
            provider.as_str()
        ));
    }

    // Newest known event first; sessions without a parseable time keep a stable name order.
    candidates.sort_by(|a, b| {
        b.last_epoch_ms
            .cmp(&a.last_epoch_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Ok((candidates, report))
}
