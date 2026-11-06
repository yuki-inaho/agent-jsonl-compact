//! OpenCode v2 session-export reader (single JSON document).
//!
//! The official export is `{"info": {...}, "messages": [...]}`. It is a single JSON document,
//! not JSONL, so the physical-line reader cannot locate message elements. This module scans the
//! document structure once, records the exact byte range of each `messages[*]` element in the
//! original bytes, and yields those ranges as bounded records. The range is the provenance
//! contract used by `event_id`, `source.byte_start/byte_end` and page cursors.
//!
//! This is deliberately a *fallback* whole-document reader: `serde_json` cannot report value
//! spans, so the document is read once with a fixed size cap (`MAX_EXPORT_BYTES`) and a small
//! structural scanner locates element boundaries. It never builds a full `Value` tree for the
//! messages. Files above the cap are rejected instead of being partially inspected.

use crate::query::cursor::sha256_hex;
use crate::query::reader::{LineItem, RecordSpan, ScanSummary};
use serde_json::Value;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Fixed whole-document cap for the v2 export reader. A larger file is rejected with rc1; the
/// limit is documented and not silently skipped.
pub const MAX_EXPORT_BYTES: u64 = 128 * 1024 * 1024;
/// Detection reads at most this prefix. The official export starts with `info`, so a valid
/// export's `messages` key appears well inside this window in practice.
pub const PROBE_PREFIX_BYTES: u64 = 4 * 1024 * 1024;
/// Metadata (`info`) is parsed for the session id/title only; it is bounded separately.
pub const MAX_INFO_BYTES: u64 = 2 * 1024 * 1024;
/// Session identity copied into every event and into cursor payloads. Keep it consistent with
/// `MAX_CURSOR_SESSION_BYTES` rather than accepting an unbounded `info.id`.
pub const MAX_EXPORT_ID_BYTES: usize = 512;
const MAX_DEPTH: usize = 256;

#[derive(Debug)]
pub enum ExportError {
    Io(io::Error),
    TooLarge { len: u64, limit: u64 },
    Invalid(String),
    InfoTooLarge { len: u64, limit: u64 },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(err) => write!(f, "cannot read export: {err}"),
            ExportError::TooLarge { len, limit } => write!(
                f,
                "OpenCode v2 export is {len} bytes, above the {limit}-byte cap"
            ),
            ExportError::Invalid(reason) => write!(f, "invalid OpenCode v2 export: {reason}"),
            ExportError::InfoTooLarge { len, limit } => write!(
                f,
                "OpenCode v2 export info object is {len} bytes, above the {limit}-byte cap"
            ),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(value: io::Error) -> Self {
        ExportError::Io(value)
    }
}

/// Cheap content probe used before opening the whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Not a single JSON object with a top-level `messages` key (e.g. run JSONL).
    NotExport,
    /// Looks like a v2 export; the caller must open it to validate.
    Candidate,
    /// Looks like a v2 export but exceeds the fixed cap.
    TooLarge,
}

/// One `messages[*]` element in the original document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSpan {
    /// 1-based physical line of the first byte of the element.
    pub raw_line: u64,
    /// 0-based inclusive start offset in the original JSON.
    pub byte_start: u64,
    /// 0-based exclusive end offset in the original JSON (excludes comma/whitespace).
    pub byte_end: u64,
}

impl ExportSpan {
    fn record_span(&self) -> RecordSpan {
        RecordSpan {
            raw_line: self.raw_line,
            byte_start: self.byte_start,
            byte_end: self.byte_end,
        }
    }
}

/// Validated, bounded v2 export document.
#[derive(Debug)]
pub struct ExportDoc {
    bytes: Vec<u8>,
    pub sha256: String,
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub spans: Vec<ExportSpan>,
    read_bytes: u64,
}

impl ExportDoc {
    pub fn open(path: &Path) -> Result<Self, ExportError> {
        Self::open_with_limit(path, MAX_EXPORT_BYTES)
    }

    /// Testable variant: `limit` is the whole-document cap.
    pub fn open_with_limit(path: &Path, limit: u64) -> Result<Self, ExportError> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len > limit {
            return Err(ExportError::TooLarge { len, limit });
        }
        // Bound the actual read as well: the file may grow between metadata and read, and an
        // unbounded `fs::read` would then allocate past the declared cap.
        let mut bytes = Vec::with_capacity(len as usize);
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(ExportError::TooLarge {
                len: bytes.len() as u64,
                limit,
            });
        }
        // Validate the outer document without building a full Value tree.
        serde_json::from_slice::<serde::de::IgnoredAny>(&bytes)
            .map_err(|err| ExportError::Invalid(format!("JSON parse failed: {err}")))?;
        let raw = scan_document(&bytes)?;
        let info_bytes = &bytes[raw.info_start..raw.info_end];
        if info_bytes.len() as u64 > MAX_INFO_BYTES {
            return Err(ExportError::InfoTooLarge {
                len: info_bytes.len() as u64,
                limit: MAX_INFO_BYTES,
            });
        }
        let info: Value = serde_json::from_slice(info_bytes)
            .map_err(|err| ExportError::Invalid(format!("info parse failed: {err}")))?;
        let info = info
            .as_object()
            .ok_or_else(|| ExportError::Invalid("info is not an object".to_string()))?;
        let session_id = info.get("id").and_then(Value::as_str).map(str::to_string);
        if session_id
            .as_deref()
            .is_some_and(|id| id.len() > MAX_EXPORT_ID_BYTES)
        {
            return Err(ExportError::Invalid(format!(
                "info.id exceeds {MAX_EXPORT_ID_BYTES} bytes"
            )));
        }
        let title = info
            .get("title")
            .and_then(Value::as_str)
            .map(|value| value.chars().take(200).collect());

        let mut spans: Vec<ExportSpan> = raw
            .messages
            .into_iter()
            .map(|(start, end)| ExportSpan {
                raw_line: 1,
                byte_start: start as u64,
                byte_end: end as u64,
            })
            .collect();
        assign_lines(&bytes, &mut spans);

        Ok(Self {
            sha256: sha256_hex(&bytes),
            session_id,
            title,
            spans,
            read_bytes: bytes.len() as u64,
            bytes,
        })
    }

    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn span_bytes(&self, span: &RecordSpan) -> Option<&[u8]> {
        let start = usize::try_from(span.byte_start).ok()?;
        let end = usize::try_from(span.byte_end).ok()?;
        self.bytes.get(start..end)
    }

    pub fn span_starting_at(&self, offset: u64) -> Option<&ExportSpan> {
        self.spans
            .binary_search_by_key(&offset, |span| span.byte_start)
            .ok()
            .map(|index| &self.spans[index])
    }
}

/// Probe a file's prefix without loading the full document.
pub fn probe(path: &Path) -> Result<Probe, ExportError> {
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let take = len.min(PROBE_PREFIX_BYTES);
    let mut bytes = Vec::with_capacity(take as usize);
    file.take(take).read_to_end(&mut bytes)?;
    let truncated = (bytes.len() as u64) < len;
    match probe_bytes(&bytes, truncated) {
        ProbeScan::NotExport => Ok(Probe::NotExport),
        ProbeScan::Candidate => {
            if len > MAX_EXPORT_BYTES {
                Ok(Probe::TooLarge)
            } else {
                Ok(Probe::Candidate)
            }
        }
    }
}

/// Bounded reader over the export's message elements.
pub struct ExportReader {
    path: PathBuf,
    doc: ExportDoc,
    snapshot_end: u64,
    complete_end: u64,
    max_record_bytes: u64,
    yielded: u64,
}

impl ExportReader {
    pub fn open(path: &Path, max_record_bytes: u64) -> Result<Self, ExportError> {
        let doc = ExportDoc::open(path)?;
        let len = doc.len();
        Ok(Self {
            path: path.to_path_buf(),
            snapshot_end: len,
            complete_end: 0,
            max_record_bytes: max_record_bytes.max(1),
            yielded: 0,
            doc,
        })
    }

    pub fn doc(&self) -> &ExportDoc {
        &self.doc
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn constrain_snapshot(&mut self, boundary: u64) {
        if boundary < self.snapshot_end {
            self.snapshot_end = boundary;
        }
    }

    pub fn snapshot_end(&self) -> u64 {
        self.snapshot_end
    }

    pub fn read_stream_bytes(&self) -> u64 {
        self.doc.read_bytes
    }

    pub fn line_count(&self) -> u64 {
        self.doc.spans.len() as u64
    }

    pub fn complete_line_end(&self) -> u64 {
        self.complete_end
    }

    pub fn scan_from<F>(&mut self, from: u64, mut callback: F) -> io::Result<ScanSummary>
    where
        F: FnMut(LineItem) -> io::Result<bool>,
    {
        self.complete_end = from;
        self.yielded = 0;
        let mut stopped = false;
        for span in &self.doc.spans {
            if span.byte_end > self.snapshot_end {
                break;
            }
            if span.byte_start < from {
                continue;
            }
            self.complete_end = span.byte_end;
            self.yielded += 1;
            let record = span.record_span();
            let size = span.byte_end.saturating_sub(span.byte_start);
            let item = if size > self.max_record_bytes {
                LineItem::Uninspected { span: record }
            } else {
                let bytes = self.doc.span_bytes(&record).unwrap_or_default().to_vec();
                LineItem::Line {
                    span: record,
                    bytes,
                }
            };
            if !callback(item)? {
                stopped = true;
                break;
            }
        }
        if !stopped {
            self.complete_end = self.snapshot_end;
        }
        Ok(ScanSummary {
            complete_line_end: self.complete_end,
            complete_line_count: self.yielded,
            pending_tail: false,
            read_stream_bytes: self.doc.read_bytes,
        })
    }
}

// ---------------------------------------------------------------------------
// Structural scanner
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RawDocument {
    info_start: usize,
    info_end: usize,
    messages: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
enum ScanError {
    Eof,
    Invalid,
    Depth,
}

fn scan_document(bytes: &[u8]) -> Result<RawDocument, ExportError> {
    let mut p = skip_ws(bytes, 0).ok_or_else(|| invalid("empty document"))?;
    if bytes.get(p) != Some(&b'{') {
        return Err(invalid("top-level value is not an object"));
    }
    p += 1;
    let mut info: Option<(usize, usize)> = None;
    let mut messages: Option<Vec<(usize, usize)>> = None;
    loop {
        p = skip_ws(bytes, p).ok_or_else(|| invalid("truncated top-level object"))?;
        if bytes.get(p) == Some(&b'}') {
            p += 1;
            break;
        }
        let (key, next) = parse_key(bytes, p).map_err(|_| invalid("invalid top-level key"))?;
        p = skip_ws(bytes, next).ok_or_else(|| invalid("truncated after key"))?;
        if bytes.get(p) != Some(&b':') {
            return Err(invalid("missing ':' after top-level key"));
        }
        p += 1;
        p = skip_ws(bytes, p).ok_or_else(|| invalid("truncated after ':'"))?;
        match key.as_str() {
            "info" => {
                if bytes.get(p) != Some(&b'{') {
                    return Err(invalid("info is not an object"));
                }
                let start = p;
                p = skip_value(bytes, p, 0).map_err(map_scan)?;
                info = Some((start, p));
            }
            "messages" => {
                if bytes.get(p) != Some(&b'[') {
                    return Err(invalid("messages is not an array"));
                }
                messages = Some(parse_messages(bytes, p).map_err(map_scan)?);
                p = skip_value(bytes, p, 0).map_err(map_scan)?;
            }
            _ => {
                p = skip_value(bytes, p, 0).map_err(map_scan)?;
            }
        }
        p = skip_ws(bytes, p).ok_or_else(|| invalid("truncated after value"))?;
        match bytes.get(p) {
            Some(b',') => p += 1,
            Some(b'}') => {
                p += 1;
                break;
            }
            _ => return Err(invalid("expected ',' or '}' at top level")),
        }
    }
    if skip_ws(bytes, p) != Some(bytes.len()) {
        return Err(invalid("trailing data after top-level object"));
    }
    let info = info.ok_or_else(|| invalid("missing top-level info object"))?;
    let messages = messages.ok_or_else(|| invalid("missing top-level messages array"))?;
    Ok(RawDocument {
        info_start: info.0,
        info_end: info.1,
        messages,
    })
}

fn parse_messages(bytes: &[u8], start: usize) -> Result<Vec<(usize, usize)>, ScanError> {
    let mut p = skip_ws(bytes, start + 1).ok_or(ScanError::Eof)?;
    let mut out = Vec::new();
    if bytes.get(p) == Some(&b']') {
        return Ok(out);
    }
    loop {
        let element_start = p;
        p = skip_value(bytes, p, 0)?;
        out.push((element_start, p));
        p = skip_ws(bytes, p).ok_or(ScanError::Eof)?;
        match bytes.get(p) {
            Some(b',') => p = skip_ws(bytes, p + 1).ok_or(ScanError::Eof)?,
            Some(b']') => return Ok(out),
            _ => return Err(ScanError::Invalid),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeScan {
    NotExport,
    Candidate,
}

fn probe_bytes(bytes: &[u8], truncated: bool) -> ProbeScan {
    let Some(mut p) = skip_ws(bytes, 0) else {
        return ProbeScan::NotExport;
    };
    if bytes.get(p) != Some(&b'{') {
        return ProbeScan::NotExport;
    }
    p += 1;
    let mut has_info = false;
    let mut has_messages = false;
    loop {
        let Some(next) = skip_ws(bytes, p) else {
            return if truncated && (has_info || has_messages) {
                ProbeScan::Candidate
            } else {
                ProbeScan::NotExport
            };
        };
        p = next;
        if bytes.get(p) == Some(&b'}') {
            p += 1;
            break;
        }
        let Ok((key, next)) = parse_key(bytes, p) else {
            return if truncated && (has_info || has_messages) {
                ProbeScan::Candidate
            } else {
                ProbeScan::NotExport
            };
        };
        p = next;
        let Some(next) = skip_ws(bytes, p) else {
            return ProbeScan::Candidate;
        };
        p = next;
        if bytes.get(p) != Some(&b':') {
            return ProbeScan::NotExport;
        }
        p += 1;
        let Some(next) = skip_ws(bytes, p) else {
            return ProbeScan::Candidate;
        };
        p = next;
        if key == "info" {
            has_info = true;
        }
        if key == "messages" {
            has_messages = true;
        }
        match skip_value(bytes, p, 0) {
            Ok(next) => p = next,
            Err(ScanError::Eof) if has_messages => return ProbeScan::Candidate,
            Err(_) => {
                return if has_messages {
                    ProbeScan::Candidate
                } else {
                    ProbeScan::NotExport
                }
            }
        }
        let Some(next) = skip_ws(bytes, p) else {
            return if truncated {
                ProbeScan::Candidate
            } else {
                ProbeScan::NotExport
            };
        };
        p = next;
        match bytes.get(p) {
            Some(b',') => p += 1,
            Some(b'}') => {
                p += 1;
                break;
            }
            _ => return ProbeScan::NotExport,
        }
    }
    let Some(rest) = skip_ws(bytes, p) else {
        return ProbeScan::NotExport;
    };
    if rest != bytes.len() {
        return ProbeScan::NotExport;
    }
    if has_messages {
        ProbeScan::Candidate
    } else {
        ProbeScan::NotExport
    }
}

fn assign_lines(bytes: &[u8], spans: &mut [ExportSpan]) {
    let mut line = 1u64;
    let mut position = 0usize;
    for span in spans {
        let start = span.byte_start.min(bytes.len() as u64) as usize;
        if start > position {
            line += bytes[position..start]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count() as u64;
            position = start;
        }
        span.raw_line = line;
    }
}

fn map_scan(_error: ScanError) -> ExportError {
    invalid("unexpected end or structure inside the export")
}

fn invalid(reason: &str) -> ExportError {
    ExportError::Invalid(reason.to_string())
}

fn skip_ws(bytes: &[u8], mut p: usize) -> Option<usize> {
    while p < bytes.len() && bytes[p].is_ascii_whitespace() {
        p += 1;
    }
    if p <= bytes.len() {
        Some(p)
    } else {
        None
    }
}

fn parse_key(bytes: &[u8], p: usize) -> Result<(String, usize), ScanError> {
    if bytes.get(p) != Some(&b'"') {
        return Err(ScanError::Invalid);
    }
    let end = skip_string(bytes, p)?;
    let key: String = serde_json::from_slice(&bytes[p..end]).map_err(|_| ScanError::Invalid)?;
    Ok((key, end))
}

fn skip_value(bytes: &[u8], p: usize, depth: usize) -> Result<usize, ScanError> {
    if depth > MAX_DEPTH {
        return Err(ScanError::Depth);
    }
    let p = skip_ws(bytes, p).ok_or(ScanError::Eof)?;
    match *bytes.get(p).ok_or(ScanError::Eof)? {
        b'"' => skip_string(bytes, p),
        b'{' => {
            let mut p = skip_ws(bytes, p + 1).ok_or(ScanError::Eof)?;
            if bytes.get(p) == Some(&b'}') {
                return Ok(p + 1);
            }
            loop {
                let (_, next) = parse_key(bytes, p)?;
                p = skip_ws(bytes, next).ok_or(ScanError::Eof)?;
                if bytes.get(p) != Some(&b':') {
                    return Err(ScanError::Invalid);
                }
                p = skip_value(bytes, p + 1, depth + 1)?;
                p = skip_ws(bytes, p).ok_or(ScanError::Eof)?;
                match bytes.get(p) {
                    Some(b',') => p = skip_ws(bytes, p + 1).ok_or(ScanError::Eof)?,
                    Some(b'}') => return Ok(p + 1),
                    _ => return Err(ScanError::Invalid),
                }
            }
        }
        b'[' => {
            let mut p = skip_ws(bytes, p + 1).ok_or(ScanError::Eof)?;
            if bytes.get(p) == Some(&b']') {
                return Ok(p + 1);
            }
            loop {
                p = skip_value(bytes, p, depth + 1)?;
                p = skip_ws(bytes, p).ok_or(ScanError::Eof)?;
                match bytes.get(p) {
                    Some(b',') => p = skip_ws(bytes, p + 1).ok_or(ScanError::Eof)?,
                    Some(b']') => return Ok(p + 1),
                    _ => return Err(ScanError::Invalid),
                }
            }
        }
        b't' => expect_literal(bytes, p, b"true"),
        b'f' => expect_literal(bytes, p, b"false"),
        b'n' => expect_literal(bytes, p, b"null"),
        b'-' | b'0'..=b'9' => skip_number(bytes, p),
        _ => Err(ScanError::Invalid),
    }
}

fn expect_literal(bytes: &[u8], p: usize, literal: &[u8]) -> Result<usize, ScanError> {
    let end = p + literal.len();
    if bytes.get(p..end) == Some(literal) {
        Ok(end)
    } else {
        Err(ScanError::Invalid)
    }
}

fn skip_number(bytes: &[u8], mut p: usize) -> Result<usize, ScanError> {
    while let Some(byte) = bytes.get(p) {
        if byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E') {
            p += 1;
        } else {
            return Ok(p);
        }
    }
    Ok(p)
}

fn skip_string(bytes: &[u8], mut p: usize) -> Result<usize, ScanError> {
    if bytes.get(p) != Some(&b'"') {
        return Err(ScanError::Invalid);
    }
    p += 1;
    while let Some(byte) = bytes.get(p) {
        match byte {
            b'\\' => p += 2,
            b'"' => return Ok(p + 1),
            _ => p += 1,
        }
    }
    Err(ScanError::Eof)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages_of(doc: &str) -> Vec<String> {
        let bytes = doc.as_bytes();
        let raw = scan_document(bytes).unwrap();
        raw.messages
            .iter()
            .map(|(start, end)| String::from_utf8(bytes[*start..*end].to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn scans_compact_and_pretty_messages() {
        let compact = r#"{"info":{"id":"s"},"messages":[{"type":"user","text":"a"},{"type":"assistant","content":[{"type":"text","text":"b"}]}]}"#;
        assert_eq!(messages_of(compact).len(), 2);
        let pretty = "{\n  \"info\": {\"id\": \"s\"},\n  \"messages\": [\n    {\n      \"type\": \"user\",\n      \"text\": \"a\"\n    },\n    {\"type\": \"assistant\"}\n  ]\n}\n";
        let messages = messages_of(pretty);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].starts_with("{\n"));
        assert!(messages[1].starts_with("{\"type\""));
    }

    #[test]
    fn byte_spans_exclude_comma_and_whitespace() {
        let doc = r#"{"info":{"id":"s"},"messages":[{"type":"user","text":"a"},{"type":"idle"}]}"#;
        let bytes = doc.as_bytes();
        let raw = scan_document(bytes).unwrap();
        let (start, end) = raw.messages[0];
        assert_eq!(&bytes[start..end], br#"{"type":"user","text":"a"}"#);
    }

    #[test]
    fn probe_distinguishes_jsonl_and_export() {
        assert_eq!(
            probe_bytes(br#"{"sessionID":"s","type":"text"}"#, false),
            ProbeScan::NotExport
        );
        assert_eq!(
            probe_bytes(br#"{"info":{"id":"s"},"messages":[]}"#, false),
            ProbeScan::Candidate
        );
        assert_eq!(
            probe_bytes(br#"{"info":{"id":"s"},"messages":[]}"#, true),
            ProbeScan::Candidate
        );
    }

    #[test]
    fn rejects_missing_or_wrong_shape() {
        assert!(scan_document(br#"{"messages":[]}"#).is_err());
        assert!(scan_document(br#"{"info":{},"messages":{}}"#).is_err());
        assert!(scan_document(br#"{"info":{},"messages":[]} trailing"#).is_err());
    }

    #[test]
    fn lines_are_computed_for_each_element() {
        let doc = "{\n\"info\":{},\n\"messages\":[\n{\"a\":1},\n{\n\"b\":2\n}\n]\n}\n";
        let bytes = doc.as_bytes();
        let raw = scan_document(bytes).unwrap();
        let mut spans: Vec<ExportSpan> = raw
            .messages
            .into_iter()
            .map(|(start, end)| ExportSpan {
                raw_line: 1,
                byte_start: start as u64,
                byte_end: end as u64,
            })
            .collect();
        assign_lines(bytes, &mut spans);
        assert_eq!(spans[0].raw_line, 4);
        assert_eq!(spans[1].raw_line, 5);
    }

    #[test]
    fn open_enforces_whole_document_and_session_id_limits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        let body = r#"{"info":{"id":"s"},"messages":[]}"#;
        std::fs::write(&path, body).unwrap();
        assert!(matches!(
            ExportDoc::open_with_limit(&path, 10),
            Err(ExportError::TooLarge { .. })
        ));
        assert!(ExportDoc::open_with_limit(&path, body.len() as u64).is_ok());

        let long_id = format!(
            "{{\"info\":{{\"id\":\"{}\"}},\"messages\":[]}}",
            "x".repeat(MAX_EXPORT_ID_BYTES + 1)
        );
        std::fs::write(&path, long_id).unwrap();
        assert!(matches!(
            ExportDoc::open(&path),
            Err(ExportError::Invalid(reason)) if reason.contains("info.id")
        ));
    }

    #[test]
    fn reader_yields_bounded_records_with_original_positions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        let body = "{\n\"info\":{\"id\":\"s\"},\n\"messages\":[\n{\"type\":\"user\",\"text\":\"a\"},\n{\"type\":\"assistant\"}\n]\n}\n";
        std::fs::write(&path, body).unwrap();
        let mut reader = ExportReader::open(&path, MAX_EXPORT_BYTES).unwrap();
        let mut spans = Vec::new();
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    spans.push((span, String::from_utf8(bytes).unwrap()));
                }
                Ok(true)
            })
            .unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0.raw_line, 4);
        assert!(spans[0].1.contains("\"text\":\"a\""));
        assert_eq!(
            &body.as_bytes()[spans[0].0.byte_start as usize..spans[0].0.byte_end as usize],
            spans[0].1.as_bytes()
        );
    }

    #[test]
    fn oversized_message_is_uninspected_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        let body = format!(
            "{{\"info\":{{\"id\":\"s\"}},\"messages\":[{{\"type\":\"user\",\"text\":\"{}\"}},{{\"type\":\"assistant\"}}]}}",
            "a".repeat(200)
        );
        std::fs::write(&path, body).unwrap();
        let mut reader = ExportReader::open(&path, 100).unwrap();
        let mut kinds = Vec::new();
        reader
            .scan_from(0, |item| {
                kinds.push(match item {
                    LineItem::Line { .. } => "line",
                    LineItem::Uninspected { .. } => "uninspected",
                    LineItem::PendingTail { .. } => "pending",
                });
                Ok(true)
            })
            .unwrap();
        assert_eq!(kinds, vec!["uninspected", "line"]);
    }
}
