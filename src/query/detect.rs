//! Evidence-based format detection for the query path.
//!
//! The legacy detector samples the first 50 records and falls back to a path hint (or Codex)
//! when nothing is recognized. The viewer must fail closed instead: it scans a bounded prefix
//! and, if it finds no recognized evidence, reports `format_uncertain` rather than pretending a
//! provider produced zero events. Conflicting recognized providers are reported as such.

use crate::query::model::Provider;
use crate::query::reader::{LineItem, SourceReader};
use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    Recognized(Provider),
    Uncertain,
    Conflicting,
}

pub const EVIDENCE_SCAN_LIMIT: u64 = 256;

#[derive(Debug, Clone, Copy, Default)]
struct Evidence {
    codex: u64,
    claude: u64,
    opencode: u64,
}

impl Evidence {
    fn providers(self) -> Vec<Provider> {
        let mut found = Vec::new();
        if self.codex > 0 {
            found.push(Provider::Codex);
        }
        if self.claude > 0 {
            found.push(Provider::ClaudeCode);
        }
        if self.opencode > 0 {
            found.push(Provider::OpenCode);
        }
        found
    }
}

pub fn classify_record(value: &Value) -> Option<Provider> {
    let object = value.as_object()?;
    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let has_payload = object.get("payload").map(Value::is_object).unwrap_or(false);
    if has_payload
        && matches!(
            record_type,
            "session_meta" | "response_item" | "event_msg" | "turn_context" | "compacted"
        )
    {
        return Some(Provider::Codex);
    }
    if !has_payload
        && (object.contains_key("sessionId") || object.contains_key("parentUuid"))
        && matches!(
            record_type,
            "user"
                | "assistant"
                | "system"
                | "summary"
                | "ai-title"
                | "mode"
                | "permission-mode"
                | "last-prompt"
                | "attachment"
                | "file-history-snapshot"
                | "queue-operation"
        )
    {
        return Some(Provider::ClaudeCode);
    }
    if object.contains_key("sessionID")
        && matches!(
            record_type,
            "step_start" | "text" | "reasoning" | "tool_use" | "step_finish" | "error"
        )
        && (object.contains_key("part") || object.contains_key("error"))
    {
        return Some(Provider::OpenCode);
    }
    None
}

/// Scan up to `EVIDENCE_SCAN_LIMIT` within-limit records and decide.
pub fn detect(path: &std::path::Path) -> Result<Detection> {
    let mut reader = SourceReader::open(path, crate::query::reader::DEFAULT_MAX_RECORD_BYTES)?;
    detect_with(&mut reader)
}

/// Same detection over an already-open bounded reader (caller's record cap applies).
pub fn detect_with(reader: &mut SourceReader) -> Result<Detection> {
    let mut evidence = Evidence::default();
    let mut seen = 0u64;
    let out = reader.scan_from(0, |item| {
        if let LineItem::Line { bytes, .. } = item {
            seen += 1;
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                match classify_record(&value) {
                    Some(Provider::Codex) => evidence.codex += 1,
                    Some(Provider::ClaudeCode) => evidence.claude += 1,
                    Some(Provider::OpenCode) => evidence.opencode += 1,
                    None => {}
                }
            }
        }
        Ok(seen < EVIDENCE_SCAN_LIMIT)
    });
    out?;

    // No ratio/majority heuristic (DETECTION_CURSOR_NOTE_1021): a single recognized provider is
    // the answer only when it is the *only* provider evidence. Any conflict between two
    // recognized providers is explicit inconsistency. Records that are merely unknown are not
    // foreign-provider evidence, so unknown-only input is "uncertain", not a conflict.
    match evidence.providers().as_slice() {
        [] => Ok(Detection::Uncertain),
        [provider] => Ok(Detection::Recognized(*provider)),
        _ => Ok(Detection::Conflicting),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn unknown_only_is_uncertain() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("u.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        for i in 0..60 {
            writeln!(file, "{{\"kind\":\"mystery\",\"n\":{i}}}").unwrap();
        }
        assert_eq!(detect(&path).unwrap(), Detection::Uncertain);
    }

    #[test]
    fn claude_body_after_fifty_unknowns_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        for i in 0..50 {
            writeln!(file, "{{\"kind\":\"mystery\",\"n\":{i}}}").unwrap();
        }
        writeln!(file, "{{\"type\":\"user\",\"sessionId\":\"s\",\"uuid\":\"u\",\"timestamp\":\"2026-10-01T00:00:00Z\",\"message\":{{\"content\":\"hi\"}}}}").unwrap();
        assert_eq!(
            detect(&path).unwrap(),
            Detection::Recognized(Provider::ClaudeCode)
        );
    }

    #[test]
    fn balanced_conflict_is_conflicting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{}}\n{\"type\":\"user\",\"sessionId\":\"s\",\"message\":{}}\n",
        )
        .unwrap();
        assert_eq!(detect(&path).unwrap(), Detection::Conflicting);
    }

    #[test]
    fn any_recognized_conflict_is_inconsistent_without_majority() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.jsonl");
        // Two Codex records vs one Claude record: a ratio heuristic would pick Codex, but any
        // conflict between recognized providers must be explicit.
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\"}}\n{\"type\":\"user\",\"sessionId\":\"s\",\"message\":{}}\n",
        )
        .unwrap();
        assert_eq!(detect(&path).unwrap(), Detection::Conflicting);
    }

    #[test]
    fn unknown_records_plus_one_provider_is_recognized_not_conflicting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("u1.jsonl");
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("{{\"kind\":\"mystery\",\"n\":{i}}}\n"));
        }
        body.push_str("{\"type\":\"user\",\"sessionId\":\"s\",\"uuid\":\"u\",\"message\":{\"content\":\"hi\"}}\n");
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            detect(&path).unwrap(),
            Detection::Recognized(Provider::ClaudeCode)
        );
    }

    #[test]
    fn oversized_first_record_does_not_break_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jsonl");
        let big = format!("{{\"blob\":\"{}\"}}\n", "x".repeat(1_200_000));
        let mut body = big;
        body.push_str("{\"type\":\"session_meta\",\"payload\":{}}\n");
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            detect(&path).unwrap(),
            Detection::Recognized(Provider::Codex)
        );
    }
}
