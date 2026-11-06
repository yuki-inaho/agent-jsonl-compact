//! Record source abstraction: physical JSONL lines or OpenCode v2 export elements.
//!
//! Both readers yield the same bounded [`LineItem`] stream so selection, provenance, cursor and
//! tool-pair logic can be shared. The export reader's spans are the original JSON element
//! ranges, never positions in a converted file.

use crate::query::export_v2::{ExportError, ExportReader};
use crate::query::model::RecordFlavor;
use crate::query::reader::{LineItem, ScanSummary, SourceReader};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub enum RecordError {
    Io(io::Error),
    Export(ExportError),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::Io(err) => write!(f, "{err}"),
            RecordError::Export(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RecordError {}

pub enum RecordReader {
    Jsonl(SourceReader),
    Export(ExportReader),
}

impl RecordReader {
    pub fn open(
        path: &Path,
        max_record_bytes: u64,
        flavor: RecordFlavor,
    ) -> Result<Self, RecordError> {
        match flavor {
            RecordFlavor::Jsonl => SourceReader::open(path, max_record_bytes)
                .map(RecordReader::Jsonl)
                .map_err(RecordError::Io),
            RecordFlavor::OpenCodeExport => ExportReader::open(path, max_record_bytes)
                .map(RecordReader::Export)
                .map_err(RecordError::Export),
        }
    }

    pub fn constrain_snapshot(&mut self, boundary: u64) {
        match self {
            RecordReader::Jsonl(reader) => reader.constrain_snapshot(boundary),
            RecordReader::Export(reader) => reader.constrain_snapshot(boundary),
        }
    }

    pub fn snapshot_end(&self) -> u64 {
        match self {
            RecordReader::Jsonl(reader) => reader.snapshot_end(),
            RecordReader::Export(reader) => reader.snapshot_end(),
        }
    }

    pub fn read_stream_bytes(&self) -> u64 {
        match self {
            RecordReader::Jsonl(reader) => reader.read_stream_bytes(),
            RecordReader::Export(reader) => reader.read_stream_bytes(),
        }
    }

    pub fn line_count(&self) -> u64 {
        match self {
            RecordReader::Jsonl(reader) => reader.line_count(),
            RecordReader::Export(reader) => reader.line_count(),
        }
    }

    pub fn complete_line_end(&self) -> u64 {
        match self {
            RecordReader::Jsonl(reader) => reader.complete_line_end(),
            RecordReader::Export(reader) => reader.complete_line_end(),
        }
    }

    /// Whole-document hash for fixed JSON exports; `None` for line-oriented sources.
    pub fn file_sha256(&self) -> Option<String> {
        match self {
            RecordReader::Jsonl(_) => None,
            RecordReader::Export(reader) => Some(reader.doc().sha256.clone()),
        }
    }

    pub fn scan_from<F>(&mut self, from: u64, callback: F) -> io::Result<ScanSummary>
    where
        F: FnMut(LineItem) -> io::Result<bool>,
    {
        match self {
            RecordReader::Jsonl(reader) => reader.scan_from(from, callback),
            RecordReader::Export(reader) => reader.scan_from(from, callback),
        }
    }
}
