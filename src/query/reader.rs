//! Bounded physical-line reader with source provenance.
//!
//! The reader never loads the whole input and never keeps every event: it streams complete
//! physical lines against a fixed snapshot boundary and yields at most one line at a time.
//! Records larger than `max_record_bytes` are not parsed; their exact byte range is recorded
//! as `uninspected` so a later retry with a larger cap can re-read them from the original
//! position instead of silently skipping them.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Default processing limit for one physical raw record (1 MiB).
pub const DEFAULT_MAX_RECORD_BYTES: u64 = 1_048_576;

/// Number of bytes kept immediately before a checkpoint for the lightweight anchor hash.
pub const ANCHOR_WINDOW_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordSpan {
    /// 1-based physical line number.
    pub raw_line: u64,
    /// 0-based inclusive start offset of the record.
    pub byte_start: u64,
    /// 0-based exclusive end offset (includes the trailing newline when present).
    pub byte_end: u64,
}

#[derive(Debug, Clone)]
pub enum LineItem {
    /// A complete, within-limit physical line.
    Line { span: RecordSpan, bytes: Vec<u8> },
    /// A line that exceeded `max_record_bytes`. Only its range is known.
    Uninspected { span: RecordSpan },
    /// Trailing bytes without a final newline at the snapshot boundary (still being written).
    PendingTail {
        raw_line: u64,
        byte_start: u64,
        last_complete_end: u64,
    },
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub complete_line_end: u64,
    pub complete_line_count: u64,
    pub pending_tail: bool,
    pub read_stream_bytes: u64,
}

/// A fixed view of one source file used for one query invocation.
pub struct SourceReader {
    path: PathBuf,
    file: File,
    snapshot_end: u64,
    lineno: u64,
    /// Number of complete (newline-terminated) lines consumed; the pending tail is excluded.
    complete_lines: u64,
    /// Offset of the next unread byte; advances only over complete, consumed lines.
    complete_end: u64,
    pending_tail: bool,
    read_stream_bytes: u64,
    max_record_bytes: u64,
}

impl SourceReader {
    /// Open `path` and fix the snapshot boundary at its current length.
    pub fn open(path: &Path, max_record_bytes: u64) -> io::Result<Self> {
        let file = File::open(path)?;
        let snapshot_end = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            snapshot_end,
            lineno: 0,
            complete_lines: 0,
            complete_end: 0,
            pending_tail: false,
            read_stream_bytes: 0,
            max_record_bytes: max_record_bytes.max(1),
        })
    }

    /// Page cursors hold the snapshot that existed when they were issued: constrain reading to
    /// that older boundary even if the file has since grown (append-only assumption).
    pub fn constrain_snapshot(&mut self, boundary: u64) {
        if boundary < self.snapshot_end {
            self.snapshot_end = boundary;
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn snapshot_end(&self) -> u64 {
        self.snapshot_end
    }

    pub fn read_stream_bytes(&self) -> u64 {
        self.read_stream_bytes
    }

    pub fn line_count(&self) -> u64 {
        self.lineno
    }

    /// Offset just after the last complete (newline-terminated) line consumed so far.
    pub fn complete_line_end(&self) -> u64 {
        self.complete_end
    }

    /// Iterate over physical lines starting at `from` (inclusive). `callback` receives one item
    /// at a time and returns `Ok(false)` to stop early (bounded window is full).
    pub fn scan_from<F>(&mut self, from: u64, mut callback: F) -> io::Result<ScanSummary>
    where
        F: FnMut(LineItem) -> io::Result<bool>,
    {
        self.lineno = 0;
        self.complete_lines = 0;
        self.complete_end = from;
        self.pending_tail = false;
        self.file.seek(SeekFrom::Start(from))?;
        let mut reader = BufReader::with_capacity(64 * 1024, self.file.try_clone()?);
        let mut buf: Vec<u8> = Vec::new();

        while self.complete_end < self.snapshot_end {
            let start = self.complete_end;
            let next_line = self.lineno + 1;
            buf.clear();
            let mut terminated = false;
            // `None` = the snapshot boundary was reached without a newline.
            let mut terminated_at: Option<u64> = None;

            // Bounded forward read: never look past the frozen snapshot, never read past the
            // newline (so there is nothing to put back / re-count).
            while terminated_at.is_none() {
                let position = start + buf.len() as u64;
                if position >= self.snapshot_end {
                    break;
                }
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    break;
                }
                let want = (self.snapshot_end - position).min(available.len() as u64) as usize;
                let window = &available[..want];
                match window.iter().position(|b| *b == b'\n') {
                    Some(index) => {
                        let take = index + 1;
                        buf.extend_from_slice(&window[..take]);
                        reader.consume(take);
                        self.read_stream_bytes += take as u64;
                        terminated = true;
                        terminated_at = Some(start + buf.len() as u64);
                    }
                    None => {
                        buf.extend_from_slice(window);
                        reader.consume(want);
                        self.read_stream_bytes += want as u64;
                        // Oversize: stop retaining bytes, only locate the end of the record.
                        if buf.len() as u64 > self.max_record_bytes {
                            break;
                        }
                    }
                }
            }

            if buf.is_empty() && terminated_at.is_none() {
                break;
            }
            let oversize = buf.len() as u64 > self.max_record_bytes;

            if oversize {
                if !terminated {
                    // The retained prefix already ended at `start + buf.len()`; drain only the
                    // remainder of this record, bounded by the frozen snapshot.
                    let prefix_end = start + buf.len() as u64;
                    let (drained, found) =
                        drain_to_newline(&mut reader, prefix_end, self.snapshot_end)?;
                    self.read_stream_bytes += drained;
                    if !found {
                        // No newline inside the snapshot: still being written. The checkpoint
                        // stays at the last complete line.
                        self.pending_tail = true;
                        callback(LineItem::PendingTail {
                            raw_line: next_line,
                            byte_start: start,
                            last_complete_end: start,
                        })?;
                        break;
                    }
                    terminated_at = Some(prefix_end + drained);
                }
                let end = terminated_at
                    .unwrap_or(self.snapshot_end)
                    .min(self.snapshot_end);
                self.complete_end = end;
                self.lineno += 1;
                self.complete_lines += 1;
                let span = RecordSpan {
                    raw_line: next_line,
                    byte_start: start,
                    byte_end: end,
                };
                if !callback(LineItem::Uninspected { span })? {
                    break;
                }
                continue;
            }

            if !terminated {
                // Incomplete final line inside the snapshot boundary: pending tail.
                // `complete_end` deliberately stays at the previous complete line so the append
                // checkpoint never advances past unfinished bytes. The partial line still counts
                // as a physical line for numbering.
                self.pending_tail = true;
                self.lineno += 1;
                callback(LineItem::PendingTail {
                    raw_line: next_line,
                    byte_start: start,
                    last_complete_end: start,
                })?;
                break;
            }

            let len = buf.len() as u64;
            self.complete_end = start + len;
            self.lineno += 1;
            self.complete_lines += 1;
            let span = RecordSpan {
                raw_line: next_line,
                byte_start: start,
                byte_end: self.complete_end,
            };
            if !callback(LineItem::Line {
                span,
                bytes: std::mem::take(&mut buf),
            })? {
                break;
            }
        }

        Ok(ScanSummary {
            complete_line_end: self.complete_end,
            complete_line_count: self.complete_lines,
            pending_tail: self.pending_tail,
            read_stream_bytes: self.read_stream_bytes,
        })
    }
}

/// Consume bytes until (and including) the next newline, never reading past `snapshot_end`.
/// `start` is the absolute offset of the first byte to examine; returns `(consumed, found)`.
fn drain_to_newline<R: BufRead>(
    reader: &mut R,
    start: u64,
    snapshot_end: u64,
) -> io::Result<(u64, bool)> {
    let mut consumed = 0u64;
    loop {
        let position = start + consumed;
        if position >= snapshot_end {
            return Ok((consumed, false));
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok((consumed, false));
        }
        let want = (snapshot_end - position).min(available.len() as u64) as usize;
        let window = &available[..want];
        match window.iter().position(|b| *b == b'\n') {
            Some(index) => {
                let take = index + 1;
                reader.consume(take);
                return Ok((consumed + take as u64, true));
            }
            None => {
                reader.consume(want);
                consumed += want as u64;
            }
        }
    }
}

/// Accounted variant: adds the logical bytes read to `counter` (no OS-I/O claim).
pub fn anchor_window_counted(
    path: &Path,
    checkpoint: u64,
    counter: &mut u64,
) -> io::Result<Vec<u8>> {
    let window = anchor_window(path, checkpoint)?;
    *counter += window.len() as u64;
    Ok(window)
}

/// Read up to `ANCHOR_WINDOW_BYTES` immediately before `checkpoint` for the local anchor hash.
pub fn anchor_window(path: &Path, checkpoint: u64) -> io::Result<Vec<u8>> {
    if checkpoint == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let len = ANCHOR_WINDOW_BYTES as u64;
    let start = checkpoint.saturating_sub(len);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; (checkpoint - start) as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(body)
            .unwrap();
        path
    }

    #[test]
    fn spans_and_subevent_offsets_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let a = b"{\"a\":1}\n";
        let b = "{\"b\":\"три\"}\n".as_bytes();
        let mut body = Vec::new();
        body.extend_from_slice(a);
        body.extend_from_slice(b);
        let path = write_file(dir.path(), "x.jsonl", &body);

        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        assert_eq!(reader.snapshot_end(), (a.len() + b.len()) as u64);
        let mut spans = Vec::new();
        let mut bodies = Vec::new();
        let summary = reader
            .scan_from(0, |item| {
                match item {
                    LineItem::Line { span, bytes } => {
                        spans.push(span);
                        bodies.push(bytes);
                    }
                    other => panic!("unexpected {other:?}"),
                }
                Ok(true)
            })
            .unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].raw_line, 1);
        assert_eq!(spans[0].byte_start, 0);
        assert_eq!(spans[0].byte_end, a.len() as u64);
        assert_eq!(spans[1].byte_start, a.len() as u64);
        assert_eq!(spans[1].byte_end, (a.len() + b.len()) as u64);
        assert_eq!(bodies[1], b);
        assert_eq!(summary.complete_line_end, (a.len() + b.len()) as u64);
        assert_eq!(summary.complete_line_count, 2);
        assert!(!summary.pending_tail);
    }

    #[test]
    fn pending_tail_is_distinct_from_invalid_middle() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"{\"a\":1}\n{\"b\":2}";
        let path = write_file(dir.path(), "p.jsonl", body);
        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        let mut items = 0;
        let summary = reader
            .scan_from(0, |item| {
                items += 1;
                if let LineItem::PendingTail {
                    raw_line,
                    byte_start,
                    last_complete_end,
                } = item
                {
                    assert_eq!(raw_line, 2);
                    assert_eq!(byte_start, 8);
                    assert_eq!(last_complete_end, 8);
                }
                Ok(true)
            })
            .unwrap();
        assert_eq!(items, 2);
        assert!(summary.pending_tail);
        assert_eq!(
            summary.complete_line_end, 8,
            "checkpoint stays before the partial line"
        );
        assert_eq!(summary.complete_line_count, 1);
    }

    /// Regression for READER_AUDIT_FIX_1021 / F-1: the snapshot is frozen at `open`; an
    /// unterminated over-limit tail must stay `PendingTail` even if a newline is appended before
    /// the scan starts. Draining must never read past the old snapshot.
    #[test]
    fn appended_newline_after_open_does_not_complete_pending_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frozen.jsonl");
        let mut body = Vec::new();
        body.extend_from_slice(b"{\"a\":1}\n");
        body.extend_from_slice(&vec![b'x'; 20_000]); // no trailing newline
        std::fs::write(&path, &body).unwrap();

        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        let snapshot = reader.snapshot_end();
        assert_eq!(snapshot, 8 + 20_000);

        // Append a newline and a valid record AFTER the snapshot was frozen.
        let mut appended = body.clone();
        appended.extend_from_slice(b"\n{\"b\":2}\n");
        std::fs::write(&path, &appended).unwrap();

        let mut items: Vec<String> = Vec::new();
        let summary = reader
            .scan_from(0, |item| {
                match item {
                    LineItem::Line { span, .. } => items.push(format!("line:{}", span.raw_line)),
                    LineItem::Uninspected { span } => {
                        items.push(format!("uninspected:{}", span.raw_line))
                    }
                    LineItem::PendingTail {
                        raw_line,
                        byte_start,
                        last_complete_end,
                    } => items.push(format!(
                        "pending:{raw_line}:{byte_start}:{last_complete_end}"
                    )),
                }
                Ok(true)
            })
            .unwrap();

        assert_eq!(
            items,
            vec!["line:1", "pending:2:8:8"],
            "frozen snapshot stays pending"
        );
        assert!(summary.pending_tail);
        assert_eq!(
            summary.complete_line_end, 8,
            "checkpoint stays at the last complete line"
        );
        assert_eq!(summary.complete_line_count, 1);
        assert!(
            summary.read_stream_bytes <= snapshot,
            "read bytes {} must not exceed the frozen snapshot {snapshot}",
            summary.read_stream_bytes
        );
    }

    /// Regression for F-3: an unterminated over-limit tail followed by a large append must not
    /// read (or count) bytes beyond the snapshot while draining.
    #[test]
    fn oversize_pending_drain_is_bounded_by_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bigtail.jsonl");
        let mut body = Vec::new();
        body.extend_from_slice(b"{\"a\":1}\n");
        body.extend_from_slice(b"{\"big\":\"");
        body.extend_from_slice(&vec![b'z'; 5_000]);
        let snapshot = body.len() as u64;
        std::fs::write(&path, &body).unwrap();

        let mut reader = SourceReader::open(&path, 1_024).unwrap();
        assert_eq!(reader.snapshot_end(), snapshot);
        let mut appended = body.clone();
        appended.extend_from_slice(&vec![b'q'; 40_000]);
        std::fs::write(&path, &appended).unwrap();

        let mut kinds: Vec<String> = Vec::new();
        let summary = reader
            .scan_from(0, |item| {
                kinds.push(match item {
                    LineItem::Line { span, .. } => format!("line:{}", span.raw_line),
                    LineItem::Uninspected { span } => format!("uninspected:{}", span.raw_line),
                    LineItem::PendingTail { raw_line, .. } => format!("pending:{raw_line}"),
                });
                Ok(true)
            })
            .unwrap();
        assert_eq!(kinds, vec!["line:1", "pending:2"]);
        assert!(summary.pending_tail);
        assert_eq!(summary.complete_line_end, 8);
        assert!(
            summary.read_stream_bytes <= snapshot,
            "read bytes {} must not exceed the snapshot {snapshot}",
            summary.read_stream_bytes
        );
    }

    /// Regression for P6/F-2: the reported stream byte count never exceeds the snapshot.
    #[test]
    fn read_stream_bytes_never_exceeds_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n{\"d\":4}\n";
        let path = write_file(dir.path(), "small.jsonl", body);
        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        let summary = reader.scan_from(0, |_| Ok(true)).unwrap();
        assert_eq!(
            summary.read_stream_bytes,
            body.len() as u64,
            "each byte counted once"
        );
        assert!(summary.read_stream_bytes <= reader.snapshot_end());
    }

    #[test]
    fn oversize_record_reports_range_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let big = format!("{{\"big\":\"{}\"}}\n", "x".repeat(200));
        let mut body = Vec::new();
        body.extend_from_slice(b"{\"a\":1}\n");
        body.extend_from_slice(big.as_bytes());
        body.extend_from_slice(b"{\"c\":3}\n");
        let path = write_file(dir.path(), "big.jsonl", &body);
        let mut reader = SourceReader::open(&path, 64).unwrap();
        let mut uninspected = Vec::new();
        let mut lines = Vec::new();
        reader
            .scan_from(0, |item| {
                match item {
                    LineItem::Uninspected { span } => uninspected.push(span),
                    LineItem::Line { span, .. } => lines.push(span.raw_line),
                    LineItem::PendingTail { .. } => panic!("no pending tail"),
                }
                Ok(true)
            })
            .unwrap();
        assert_eq!(
            lines,
            vec![1, 3],
            "oversize row is not yielded as a parsed line"
        );
        assert_eq!(uninspected.len(), 1);
        assert_eq!(uninspected[0].raw_line, 2);
        assert_eq!(uninspected[0].byte_start, 8);
        assert_eq!(uninspected[0].byte_end, 8 + big.len() as u64);
    }

    #[test]
    fn scan_from_resumes_without_rereading_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n";
        let path = write_file(dir.path(), "r.jsonl", body);
        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        reader.scan_from(0, |_| Ok(false)).unwrap();
        let mut reader = SourceReader::open(&path, DEFAULT_MAX_RECORD_BYTES).unwrap();
        let summary = reader
            .scan_from(8, |item| {
                if let LineItem::Line { span, .. } = item {
                    assert_eq!(span.raw_line, 1);
                    assert_eq!(span.byte_start, 8);
                }
                Ok(false)
            })
            .unwrap();
        assert!(summary.read_stream_bytes < body.len() as u64);
    }

    #[test]
    fn anchor_window_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let body = vec![b'z'; 1024];
        let path = write_file(dir.path(), "a.bin", &body);
        assert_eq!(anchor_window(&path, 0).unwrap().len(), 0);
        assert_eq!(anchor_window(&path, 512).unwrap().len(), 128);
        assert_eq!(anchor_window(&path, 10).unwrap().len(), 10);
    }
}
