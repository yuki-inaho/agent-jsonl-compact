//! Source identity helpers: canonical path hash, session-id discovery, generation.

use crate::query::cursor::sha256_hex;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

/// Namespaced file source id: readable prefix + canonical-path hash (never the full real path).
pub fn file_source_id(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Source identity includes a stable generation marker: Unix `dev`+`inode` is unchanged by an
    // append to the same file but changes when the path is replaced by a new file. Without it,
    // two generations at the same path would share `event_id`s and an old `--event` lookup could
    // return new content as if it were old evidence.
    let hash = sha256_hex(&source_identity_bytes(&canonical));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "input".to_string());
    format!("file:{}:{}", truncate(&sanitize(&name), 40), &hash[..16])
}

/// Bytes hashed for source identity: canonical path plus the platform generation marker.
/// Non-Unix platforms have no stable file identity here; they keep path-only identity and cursor
/// mode is conservatively rejected (`cursor_unsupported_platform`).
fn source_identity_bytes(canonical: &Path) -> Vec<u8> {
    let mut bytes = canonical.to_string_lossy().as_bytes().to_vec();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(metadata) = std::fs::metadata(canonical) {
            bytes.extend_from_slice(b"|gen:");
            bytes.extend_from_slice(&metadata.dev().to_le_bytes());
            bytes.extend_from_slice(b":");
            bytes.extend_from_slice(&metadata.ino().to_le_bytes());
        }
    }
    bytes
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}

/// Trustworthy provider session id from the leading records only; `None` means "unknown".
///
/// Uses the bounded reader so an oversized first record is skipped as `uninspected` instead of
/// bypassing the record cap (`BufReader::lines` previously read it whole).
pub fn discover_session_id(
    path: &Path,
    provider: crate::query::model::Provider,
    max_record_bytes: u64,
) -> Result<Option<String>> {
    discover_session_id_counted(path, provider, max_record_bytes, u64::MAX).map(|(id, _)| id)
}

/// Same as [`discover_session_id`], also returning the logical bytes read (accounting).
pub fn discover_session_id_counted(
    path: &Path,
    provider: crate::query::model::Provider,
    max_record_bytes: u64,
    snapshot_end: u64,
) -> Result<(Option<String>, u64)> {
    let mut reader = crate::query::reader::SourceReader::open(path, max_record_bytes)
        .with_context(|| format!("failed to open {}", path.display()))?;
    reader.constrain_snapshot(snapshot_end);
    let mut found = None;
    let mut scanned = 0u64;
    reader.scan_from(0, |item| {
        if found.is_some() || scanned >= 32 {
            return Ok(false);
        }
        if let crate::query::reader::LineItem::Line { bytes, .. } = item {
            scanned += 1;
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                return Ok(true);
            };
            let Some(object) = value.as_object() else {
                return Ok(true);
            };
            found = match provider {
                crate::query::model::Provider::Codex => {
                    if object.get("type").and_then(Value::as_str) == Some("session_meta") {
                        object
                            .get("payload")
                            .and_then(Value::as_object)
                            .and_then(|p| p.get("id"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    } else {
                        None
                    }
                }
                crate::query::model::Provider::ClaudeCode => object
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                crate::query::model::Provider::OpenCode => object
                    .get("sessionID")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
        }
        Ok(true)
    })?;
    Ok((found, reader.read_stream_bytes()))
}

/// Best-effort session metadata used by `sessions`.
#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub last_event_at: Option<String>,
    pub first_epoch_ms: Option<i64>,
    pub last_epoch_ms: Option<i64>,
    pub lines: u64,
    /// Records that exceeded the per-record cap (their body was not inspected).
    pub uninspected: u64,
    /// Records that were not valid JSON.
    pub invalid: u64,
}

/// Stream the whole file with the bounded reader: the *last* timestamp is genuinely the last
/// activity even when it lies far beyond any fixed preview window, and no record is loaded
/// beyond `max_record_bytes`. Memory stays constant regardless of file size.
pub fn discover_meta(
    path: &Path,
    provider: crate::query::model::Provider,
    max_record_bytes: u64,
) -> Result<SessionMeta> {
    let mut meta = SessionMeta::default();
    let mut reader = crate::query::reader::SourceReader::open(path, max_record_bytes)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut pending_tail = false;
    reader.scan_from(0, |item| {
        match item {
            crate::query::reader::LineItem::Line { bytes, .. } => {
                meta.lines += 1;
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    meta.invalid += 1;
                    return Ok(true);
                };
                let Some(object) = value.as_object() else {
                    return Ok(true);
                };
                if meta.session_id.is_none() {
                    meta.session_id = match provider {
                        crate::query::model::Provider::Codex => {
                            if object.get("type").and_then(Value::as_str) == Some("session_meta") {
                                let payload = object.get("payload").and_then(Value::as_object);
                                if meta.cwd.is_none() {
                                    meta.cwd = payload
                                        .and_then(|p| p.get("cwd"))
                                        .and_then(Value::as_str)
                                        .map(str::to_string);
                                }
                                payload
                                    .and_then(|p| p.get("id"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            } else {
                                None
                            }
                        }
                        crate::query::model::Provider::ClaudeCode => {
                            if meta.cwd.is_none() {
                                meta.cwd = object
                                    .get("cwd")
                                    .and_then(Value::as_str)
                                    .map(str::to_string);
                            }
                            object
                                .get("sessionId")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        }
                        crate::query::model::Provider::OpenCode => object
                            .get("sessionID")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    };
                }
                if meta.title.is_none() {
                    let title = object
                        .get("title")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            object
                                .get("message")
                                .and_then(Value::as_object)
                                .and_then(|m| m.get("content"))
                                .and_then(Value::as_str)
                                .filter(|_| {
                                    object.get("type").and_then(Value::as_str) == Some("user")
                                })
                        })
                        .or_else(|| {
                            object
                                .get("payload")
                                .and_then(Value::as_object)
                                .filter(|p| {
                                    p.get("type").and_then(Value::as_str) == Some("user_message")
                                })
                                .and_then(|p| p.get("message"))
                                .and_then(Value::as_str)
                        });
                    if let Some(title) = title {
                        // Bound the retained title; the full value is never kept.
                        meta.title = Some(title.chars().take(200).collect());
                    }
                }
                if let Some(ts) = object.get("timestamp").and_then(Value::as_str) {
                    let epoch = crate::query::model::parse_rfc3339_ms(ts);
                    if meta.started_at.is_none() {
                        meta.started_at = Some(ts.to_string());
                        meta.first_epoch_ms = epoch;
                    }
                    // Last *parseable* timestamp wins; a missing time does not erase it.
                    if epoch.is_some() {
                        meta.last_event_at = Some(ts.to_string());
                        meta.last_epoch_ms = epoch;
                    } else if meta.last_event_at.is_none() {
                        meta.last_event_at = Some(ts.to_string());
                    }
                }
            }
            crate::query::reader::LineItem::Uninspected { .. } => {
                meta.lines += 1;
                meta.uninspected += 1;
            }
            crate::query::reader::LineItem::PendingTail { .. } => pending_tail = true,
        }
        Ok(true)
    })?;
    let _ = pending_tail;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_id_is_namespaced_and_hashed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        std::fs::write(&path, "").unwrap();
        let id = file_source_id(&path);
        assert!(id.starts_with("file:rollout-x.jsonl:"));
        assert_eq!(id.len(), "file:rollout-x.jsonl:".len() + 16);
        assert!(!id.contains(dir.path().to_str().unwrap()));
    }

    #[test]
    fn session_id_comes_from_trustworthy_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("c.jsonl");
        std::fs::write(
            &codex,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"real-id\",\"cwd\":\"/r\"}}\n",
        )
        .unwrap();
        assert_eq!(
            discover_session_id(
                &codex,
                crate::query::model::Provider::Codex,
                crate::query::reader::DEFAULT_MAX_RECORD_BYTES
            )
            .unwrap(),
            Some("real-id".to_string())
        );
        let unknown = dir.path().join("u.jsonl");
        std::fs::write(&unknown, "{\"type\":\"response_item\",\"payload\":{}}\n").unwrap();
        assert_eq!(
            discover_session_id(
                &unknown,
                crate::query::model::Provider::Codex,
                crate::query::reader::DEFAULT_MAX_RECORD_BYTES
            )
            .unwrap(),
            None
        );
    }
}
