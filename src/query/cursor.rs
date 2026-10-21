//! Opaque, versioned cursor tokens for the viewer.
//!
//! The token is base64url(JSON) with a version prefix. It is *validated*, never trusted: kind,
//! source generation, snapshot boundary, offsets, subevent index, filters and anchor are all
//! re-checked against the live file. Any mismatch is a typed `cursor_invalid` error rather than
//! an implicit rewind.

use crate::query::filter::Filters;
use crate::query::model::Provider;
use serde::{Deserialize, Serialize};

pub const CURSOR_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorKind {
    Page,
    Append,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    pub dev: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPayload {
    pub v: u64,
    pub kind: CursorKind,
    pub provider: Provider,
    pub source_id: String,
    pub generation: Option<Generation>,
    pub snapshot_end: u64,
    /// Resume offset (page: raw start of the last rendered record; append: checkpoint).
    pub offset: u64,
    /// Resume raw line (1-based) for page cursors.
    pub raw_line: u64,
    /// Resume subevent index for page cursors.
    pub subevent_index: u32,
    /// Complete line count at checkpoint (append), used for line numbering without rescan.
    pub complete_line_count: u64,
    pub filters: Filters,
    pub event_filter: Option<String>,
    /// Command kind (`view`/`search`) so a token cannot be used by the wrong command.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub session_id: String,
    /// Search semantics reused on resume (empty for plain view).
    #[serde(default)]
    pub queries: Vec<String>,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub context: usize,
    /// SHA-256 of up to 128 bytes before the checkpoint/boundary.
    pub anchor_hex: String,
    /// SHA-256 of up to 128 bytes immediately before `offset` (local resume anchor).
    #[serde(default)]
    pub resume_anchor_hex: String,
    /// Number of bytes covered by the anchor window.
    pub anchor_len: u64,
    /// Modification time (ns since epoch) observed when the cursor was issued. Used only as a
    /// conservative concurrent-write signal, never as file identity.
    #[serde(default)]
    pub mtime_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorProblem {
    Malformed,
    UnsupportedVersion,
    WrongKind,
    GenerationMismatch,
    Truncated,
    AnchorMismatch,
    FilterMismatch,
    OffsetOutOfRange,
    UnsupportedPlatform,
    ConcurrentModification,
}

impl CursorProblem {
    pub fn reason(self) -> &'static str {
        match self {
            CursorProblem::Malformed => "malformed",
            CursorProblem::UnsupportedVersion => "unsupported_version",
            CursorProblem::WrongKind => "wrong_cursor_kind",
            CursorProblem::GenerationMismatch => "generation_mismatch",
            CursorProblem::Truncated => "truncated",
            CursorProblem::AnchorMismatch => "anchor_mismatch",
            CursorProblem::FilterMismatch => "filter_mismatch",
            CursorProblem::OffsetOutOfRange => "offset_out_of_range",
            CursorProblem::UnsupportedPlatform => "cursor_unsupported_platform",
            CursorProblem::ConcurrentModification => "concurrent_modification",
        }
    }
}

impl CursorPayload {
    pub fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        format!("c{}:{}", self.v, base64url_encode(json.as_bytes()))
    }

    pub fn decode(token: &str) -> Result<Self, CursorProblem> {
        let token = token.trim();
        let Some(rest) = token.strip_prefix("c") else {
            return Err(CursorProblem::Malformed);
        };
        let Some((version, body)) = rest.split_once(':') else {
            return Err(CursorProblem::Malformed);
        };
        let version: u64 = version.parse().map_err(|_| CursorProblem::Malformed)?;
        if version != CURSOR_VERSION {
            return Err(CursorProblem::UnsupportedVersion);
        }
        let bytes = base64url_decode(body).ok_or(CursorProblem::Malformed)?;
        let payload: CursorPayload =
            serde_json::from_slice(&bytes).map_err(|_| CursorProblem::Malformed)?;
        if payload.v != CURSOR_VERSION {
            return Err(CursorProblem::UnsupportedVersion);
        }
        Ok(payload)
    }

    pub fn encoded_len(&self) -> usize {
        self.encode().len()
    }
}

/// Unix generation identity. Cursor mode is only supported on Unix; other platforms must reject
/// cursor mode conservatively (`cursor_unsupported_platform`) rather than guess an identity.
#[cfg(unix)]
pub fn generation_of(metadata: &std::fs::Metadata) -> Option<Generation> {
    use std::os::unix::fs::MetadataExt;
    Some(Generation {
        dev: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub fn generation_of(_metadata: &std::fs::Metadata) -> Option<Generation> {
    None
}

pub fn base64url_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
}

pub fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .ok()
}

/// SHA-256 (hex) via the vetted `sha2` crate, used for source identity and anchors.
pub fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: CursorKind) -> CursorPayload {
        CursorPayload {
            v: CURSOR_VERSION,
            kind,
            provider: Provider::Codex,
            source_id: "src".into(),
            generation: Some(Generation { dev: 1, inode: 2 }),
            snapshot_end: 100,
            offset: 40,
            raw_line: 3,
            subevent_index: 1,
            complete_line_count: 2,
            filters: Filters::default(),
            event_filter: None,
            command: "view".to_string(),
            session_id: "src".to_string(),
            queries: Vec::new(),
            regex: false,
            context: 0,
            anchor_hex: sha256_hex(b"anchor"),
            resume_anchor_hex: sha256_hex(b"resume"),
            anchor_len: 6,
            mtime_ns: 1,
        }
    }

    #[test]
    fn round_trip_preserves_payload() {
        let payload = sample(CursorKind::Page);
        let token = payload.encode();
        assert_eq!(CursorPayload::decode(&token).unwrap(), payload);
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        assert_eq!(CursorPayload::decode("nope"), Err(CursorProblem::Malformed));
        assert_eq!(
            CursorPayload::decode("c9:AAAA"),
            Err(CursorProblem::UnsupportedVersion)
        );
        assert!(CursorPayload::decode("c1:@@@@").is_err());
    }

    #[test]
    fn base64url_round_trip() {
        for len in 0..8 {
            let data: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(37)).collect();
            let encoded = base64url_encode(&data);
            assert_eq!(base64url_decode(&encoded).unwrap(), data);
        }
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }
}
