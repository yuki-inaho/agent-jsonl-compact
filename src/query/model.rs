//! Provider-agnostic normalized query event.
//!
//! This model is deliberately separate from the legacy `clean.jsonl` schema: the legacy
//! exporter keeps its fields and exit codes unchanged, while the viewer carries provenance
//! (raw line/byte/subevent), origin/phase and opaque tool observations.

use serde::{Deserialize, Serialize};

/// Query output schema version (separate from legacy clean schema).
pub const QUERY_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    ClaudeCode,
    OpenCode,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Codex => "codex",
            Provider::ClaudeCode => "claude_code",
            Provider::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    User,
    Assistant,
    ToolCall,
    ToolOutput,
    /// Internal reasoning/summary. Never rendered in the `messages` or `all` scope.
    Reasoning,
    /// Compaction summary / compacted record. Never public conversation.
    Compaction,
    /// Framework/system/developer injection that is excluded from `messages` by default.
    Injected,
    Unknown,
    ParseError,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::User => "user",
            Kind::Assistant => "assistant",
            Kind::ToolCall => "tool_call",
            Kind::ToolOutput => "tool_output",
            Kind::Reasoning => "reasoning",
            Kind::Compaction => "compaction",
            Kind::Injected => "injected",
            Kind::Unknown => "unknown",
            Kind::ParseError => "parse_error",
        }
    }

    pub fn is_tool(self) -> bool {
        matches!(self, Kind::ToolCall | Kind::ToolOutput)
    }

    pub fn is_message(self) -> bool {
        matches!(self, Kind::User | Kind::Assistant)
    }

    /// Hard scope: excluded from both `messages` and `all` even as context.
    pub fn is_private(self) -> bool {
        matches!(self, Kind::Reasoning | Kind::Compaction | Kind::Injected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Human,
    Framework,
    Compaction,
    Unknown,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Human => "human",
            Origin::Framework => "framework",
            Origin::Compaction => "compaction",
            Origin::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Developer,
    System,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Developer => "developer",
            Role::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub raw_line: u64,
    pub byte_start: u64,
    pub byte_end: u64,
    pub subevent_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolObserved {
    None,
    Returned {
        is_error: bool,
        status: Option<String>,
    },
}

/// One normalized query event (bounded: text is the full processable body, never the whole file).
#[derive(Debug, Clone)]
pub struct QEvent {
    pub provider: Provider,
    pub session_id: String,
    /// Stable source identity + raw start + subevent (set by the reader layer).
    pub event_id: String,
    /// Raw timestamp string as observed, if any.
    pub timestamp_raw: Option<String>,
    /// Parsed instant in milliseconds since the Unix epoch (UTC), if parseable.
    pub epoch_ms: Option<i64>,
    pub role: Option<Role>,
    pub phase: Option<String>,
    pub kind: Kind,
    pub source_kind: String,
    pub origin: Origin,
    pub text: String,
    /// Original tool input text (opaque; never executed).
    pub tool_name: Option<String>,
    pub call_id: Option<String>,
    pub tool_observed: ToolObserved,
    /// Provider's original stable id (message/item id) when present; used for mirror pairing.
    pub origin_id: Option<String>,
    pub source: SourceRef,
    /// True when the raw line number was produced by this scan; false when it was inherited
    /// from a cursor and could not be independently verified.
    pub raw_line_verified: bool,
    /// When this record is a provider mirror of another record, the shared explicit id.
    pub mirror_of: Option<String>,
    /// Provider/record relation for a mirror (kept, never text-deduped).
    pub mirror_relation: Option<String>,
    pub sequence: u64,
}

impl QEvent {
    pub fn is_private(&self) -> bool {
        self.kind.is_private()
    }

    pub fn subevent_index(&self) -> u32 {
        self.source.subevent_index
    }
}

/// Convert an RFC3339 timestamp string with an explicit offset into epoch milliseconds.
///
/// Uses the vetted `time` crate. Returns `None` for missing/ambiguous (no offset) or unparsable
/// values; those are counted as `time_unknown` and never filled in from mtime.
pub fn parse_rfc3339_ms(raw: &str) -> Option<i64> {
    use time::format_description::well_known::Rfc3339;
    let parsed = time::OffsetDateTime::parse(raw, &Rfc3339).ok()?;
    let millis: i128 = parsed.unix_timestamp_nanos() / 1_000_000;
    i64::try_from(millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_normalized_to_utc() {
        let utc = parse_rfc3339_ms("2026-10-01T09:00:00Z").unwrap();
        let jst = parse_rfc3339_ms("2026-10-01T18:00:00+09:00").unwrap();
        assert_eq!(utc, jst);
        let before = parse_rfc3339_ms("2026-10-01T08:59:59Z").unwrap();
        assert_eq!(utc - before, 1000);
        let frac = parse_rfc3339_ms("2026-10-01T09:00:00.250+00:00").unwrap();
        assert_eq!(frac - utc, 250);
    }

    #[test]
    fn missing_or_ambiguous_offsets_are_unknown() {
        assert!(parse_rfc3339_ms("2026-10-01T09:00:00").is_none());
        assert!(parse_rfc3339_ms("not a date").is_none());
        assert!(parse_rfc3339_ms("2026-13-01T00:00:00Z").is_none());
        assert!(parse_rfc3339_ms("2026-10-01T09:00:00+0900").is_none());
    }

    #[test]
    fn known_instant_matches_published_value() {
        // 2026-10-21T00:00:00Z == 1792540800 (independent check by hand arithmetic below).
        let epoch = parse_rfc3339_ms("2026-10-21T00:00:00Z").unwrap();
        assert_eq!(epoch, 1_792_540_800_000);
    }
}
