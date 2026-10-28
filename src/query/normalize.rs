//! Provider normalization adapters for the query path.
//!
//! Reused idea: keep the legacy classifier's proven extraction of bodies (`text_from`,
//! `function_output_text`, Claude content arrays). Changed on purpose for the viewer:
//! * custom Codex tool calls/outputs are preserved (legacy drops them),
//! * reasoning/compaction/system-developer records are typed as private instead of dropped,
//! * every accepted/unknown record keeps a raw pointer, and one raw record may yield several
//!   subevents (Claude multi-block rows) with stable subevent indices,
//! * `isMeta`/`isCompactSummary`/developer records are marked `framework`, never `human`.

use crate::query::model::{
    parse_rfc3339_ms, Kind, Origin, Provider, QEvent, Role, SourceRef, ToolObserved,
};
use crate::query::reader::RecordSpan;
use crate::util::{function_output_text, json_text, text_from, value_as_string};
use serde_json::{Map, Value};
use std::collections::VecDeque;

/// Maximum number of raw-type coverage entries that keep their own key (bounded memory).
pub const COVERAGE_TYPE_CAP: usize = 64;

/// Per-record normalization limits. These bound the amplification from one physical line:
/// repeated metadata is copied once per subevent, so each field and the number of subevents is
/// capped before any event is built.
pub const MAX_TIMESTAMP_BYTES: usize = 128;
pub const MAX_ID_BYTES: usize = 512;
pub const MAX_PHASE_BYTES: usize = 64;
pub const MAX_SUBEVENTS_PER_RECORD: usize = 256;
pub const MAX_NORMALIZED_BYTES_PER_RECORD: usize = 2 * 1024 * 1024;

/// Bounded multiset for raw record types (exact counts, capped key cardinality).
#[derive(Debug, Clone, Default)]
pub struct TypeCoverage {
    pub counts: Vec<(String, u64)>,
    pub overflow: u64,
}

impl TypeCoverage {
    pub fn add(&mut self, key: &str) {
        if let Some(entry) = self.counts.iter_mut().find(|(k, _)| k == key) {
            entry.1 += 1;
            return;
        }
        if self.counts.len() < COVERAGE_TYPE_CAP {
            self.counts.push((key.to_string(), 1));
        } else {
            self.overflow += 1;
        }
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().map(|(_, c)| *c).sum::<u64>() + self.overflow
    }
}

/// Streaming normalizer: converts one raw record at a time into a bounded list of events.
pub struct Normalizer {
    pub provider: Provider,
    pub session_id: String,
    pub sequence: u64,
    pub coverage: TypeCoverage,
    /// Small window of mirror candidates: explicit item id + text hash is retained for at most
    /// this many records (constant memory, not a full-call index).
    /// Bounded window of (explicit origin id, body) used to merge provider mirrors.
    pub mirror_window: VecDeque<(String, String)>,
    pub mirror_candidates: u64,
    pub invalid_records: u64,
    last_public_text: Option<String>,
    /// Bounded pointers of mirrored records merged away (auditable, never a full log).
    pub merged_mirror_pointers: Vec<(u64, u64, u32)>,
}

const MIRROR_WINDOW: usize = 16;

impl Normalizer {
    pub fn new(provider: Provider, session_id: String) -> Self {
        Self {
            provider,
            session_id,
            sequence: 0,
            coverage: TypeCoverage::default(),
            mirror_window: VecDeque::new(),
            mirror_candidates: 0,
            invalid_records: 0,
            last_public_text: None,
            merged_mirror_pointers: Vec::new(),
        }
    }

    /// Parse and normalize one physical record. Returns an empty vec for records that carry no
    /// event (e.g. session metadata, which the caller extracts separately).
    pub fn normalize(&mut self, span: RecordSpan, bytes: &[u8], source_id: &str) -> Vec<QEvent> {
        self.coverage.add(&raw_type_key(self.provider, bytes));
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                self.invalid_records += 1;
                return vec![self.event(
                    span,
                    0,
                    source_id,
                    Kind::ParseError,
                    "invalid_utf8",
                    None,
                    None,
                    Origin::Unknown,
                    "invalid UTF-8 in raw record",
                    None,
                    None,
                    ToolObserved::None,
                    None,
                    None,
                )];
            }
        };
        let value: Value = match serde_json::from_str(text.trim_end_matches(['\n', '\r'])) {
            Ok(value) => value,
            Err(err) => {
                self.invalid_records += 1;
                return vec![self.event(
                    span,
                    0,
                    source_id,
                    Kind::ParseError,
                    "invalid_json",
                    None,
                    None,
                    Origin::Unknown,
                    &format!("invalid JSON: {err}"),
                    None,
                    None,
                    ToolObserved::None,
                    None,
                    None,
                )];
            }
        };
        let Some(object) = value.as_object() else {
            self.invalid_records += 1;
            return vec![self.event(
                span,
                0,
                source_id,
                Kind::Unknown,
                "non_object",
                None,
                None,
                Origin::Unknown,
                "non-object JSON record",
                None,
                None,
                ToolObserved::None,
                None,
                None,
            )];
        };

        if let Some(reason) = metadata_limit_reason(self.provider, object) {
            return self.rejected(span, source_id, reason);
        }
        if let Some(reason) = subevent_count_limit_reason(self.provider, object) {
            return self.rejected(span, source_id, reason);
        }

        // A file may contain records from more than one session (e.g. an appended export).
        // Trust the record's own metadata; never pair across sessions silently.
        if let Some(record_session) = record_session_id(self.provider, object) {
            if record_session.len() > MAX_ID_BYTES {
                return self.rejected(
                    span,
                    source_id,
                    "normalization limit exceeded: session id is too long",
                );
            }
            self.session_id = record_session;
        }

        let drafts = match self.provider {
            Provider::Codex => codex_drafts(object),
            Provider::ClaudeCode => claude_drafts(object),
            Provider::OpenCode => opencode_drafts(object),
        };
        if let Some(reason) = drafts_limit_reason(&drafts) {
            return self.rejected(span, source_id, reason);
        }

        let mut out = Vec::with_capacity(drafts.len().min(8));
        for (index, draft) in drafts.into_iter().enumerate() {
            let event = self.event(
                span,
                index as u32,
                source_id,
                draft.kind,
                &draft.source_kind,
                draft.timestamp_raw,
                draft.phase,
                draft.origin,
                &draft.text,
                draft.tool_name,
                draft.call_id,
                draft.observed,
                draft.origin_id,
                draft.role,
            );
            if event.kind.is_message() && self.is_explicit_mirror(&event) {
                // KEEP representation (root A02_PROVENANCE_GATE_1021): every physical row keeps
                // its own raw pointer, and the mirror relation is explicit. Bodies are never
                // deduplicated by text alone.
                self.mirror_candidates += 1;
                let mut event = event;
                event.mirror_of = event.origin_id.clone();
                event.mirror_relation = Some("mirror_record".to_string());
                self.note_mirror(&event);
                out.push(event);
                continue;
            }
            self.note_mirror(&event);
            out.push(event);
        }
        out
    }

    fn note_mirror(&mut self, event: &QEvent) {
        if let Some(origin_id) = &event.origin_id {
            self.mirror_window
                .push_back((origin_id.clone(), event.text.clone()));
            while self.mirror_window.len() > MIRROR_WINDOW {
                self.mirror_window.pop_front();
            }
        }
        if event.kind.is_message() && !event.text.is_empty() {
            if let Some(previous) = &self.last_public_text {
                if previous == &event.text && event.origin_id.is_none() {
                    // A mirror with no explicit id: kept (text equality alone is not identity),
                    // but recorded so metadata can explain the duplicate origin.
                    self.mirror_candidates += 1;
                }
            }
            self.last_public_text = Some(event.text.clone());
        }
    }

    fn rejected(&mut self, span: RecordSpan, source_id: &str, reason: &'static str) -> Vec<QEvent> {
        self.invalid_records += 1;
        vec![self.event(
            span,
            0,
            source_id,
            Kind::ParseError,
            "normalization_limit",
            None,
            None,
            Origin::Unknown,
            reason,
            None,
            None,
            ToolObserved::None,
            None,
            None,
        )]
    }

    /// True when this event is a provider mirror of an already-emitted message: same explicit
    /// source id and same body. Text equality without an id never merges.
    fn is_explicit_mirror(&self, event: &QEvent) -> bool {
        match &event.origin_id {
            Some(origin_id) => self
                .mirror_window
                .iter()
                .any(|(id, text)| id == origin_id && text == &event.text),
            None => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn event(
        &mut self,
        span: RecordSpan,
        subevent_index: u32,
        source_id: &str,
        kind: Kind,
        source_kind: &str,
        timestamp_raw: Option<String>,
        phase: Option<String>,
        origin: Origin,
        text: &str,
        tool_name: Option<String>,
        call_id: Option<String>,
        observed: ToolObserved,
        origin_id: Option<String>,
        role: Option<Role>,
    ) -> QEvent {
        self.sequence += 1;
        let epoch_ms = timestamp_raw.as_deref().and_then(parse_rfc3339_ms);
        QEvent {
            provider: self.provider,
            session_id: self.session_id.clone(),
            // The raw line is deliberately excluded: a cursor can carry an unverified line
            // number, and an event id must never turn that into evidence identity.
            event_id: format!("{}:{}:{}", source_id, span.byte_start, subevent_index),
            timestamp_raw,
            epoch_ms,
            role,
            phase,
            kind,
            source_kind: source_kind.to_string(),
            origin,
            text: text.to_string(),
            tool_name,
            call_id,
            tool_observed: observed,
            origin_id,
            source: SourceRef {
                raw_line: span.raw_line,
                byte_start: span.byte_start,
                byte_end: span.byte_end,
                subevent_index,
            },
            raw_line_verified: true,
            mirror_of: None,
            mirror_relation: None,
            sequence: self.sequence,
        }
    }
}

#[allow(dead_code)]
struct Draft {
    kind: Kind,
    source_kind: String,
    timestamp_raw: Option<String>,
    phase: Option<String>,
    origin: Origin,
    text: String,
    tool_name: Option<String>,
    call_id: Option<String>,
    observed: ToolObserved,
    origin_id: Option<String>,
    role: Option<Role>,
}

/// Bound the record metadata that the normalizer actually copies into a [`QEvent`].
///
/// The walk is context-aware: it visits the provider's known metadata paths instead of every
/// `id`/`name` key in the record. Tool input (`message.content[].input`, `payload.arguments`,
/// `part.state.input`, ...) is body text, not metadata, so a long `name` inside it no longer
/// discards the whole record. ADR 0007 §2.1 scopes the cap to the identity fields
/// (session id / call id / origin id / tool name) plus timestamp and phase.
///
/// Amplification is still bounded: metadata repeated across subevents is re-checked per draft in
/// [`drafts_limit_reason`] before any event is built, and that function also caps the total
/// normalized bytes of one record.
fn metadata_limit_reason(provider: Provider, object: &Map<String, Value>) -> Option<&'static str> {
    const IDENTIFIER_TOO_LONG: &str = "normalization limit exceeded: identifier is too long";
    const TIMESTAMP_TOO_LONG: &str = "normalization limit exceeded: timestamp is too long";
    const PHASE_TOO_LONG: &str = "normalization limit exceeded: phase is too long";

    fn bounded(value: Option<&Value>, limit: usize, reason: &'static str) -> Option<&'static str> {
        value
            .and_then(Value::as_str)
            .filter(|text| text.len() > limit)
            .map(|_| reason)
    }

    if let Some(reason) = bounded(
        object.get("timestamp"),
        MAX_TIMESTAMP_BYTES,
        TIMESTAMP_TOO_LONG,
    ) {
        return Some(reason);
    }

    match provider {
        Provider::Codex => {
            let payload = nested(object, "payload")?;
            if let Some(reason) = bounded(payload.get("phase"), MAX_PHASE_BYTES, PHASE_TOO_LONG) {
                return Some(reason);
            }
            for key in ["id", "item_id", "call_id", "name", "tool"] {
                if let Some(reason) = bounded(payload.get(key), MAX_ID_BYTES, IDENTIFIER_TOO_LONG) {
                    return Some(reason);
                }
            }
            let tool = nested(payload, "invocation").and_then(|invocation| invocation.get("tool"));
            bounded(tool, MAX_ID_BYTES, IDENTIFIER_TOO_LONG)
        }
        Provider::ClaudeCode => {
            if let Some(reason) =
                bounded(object.get("sessionId"), MAX_ID_BYTES, IDENTIFIER_TOO_LONG)
            {
                return Some(reason);
            }
            // Only the tool blocks carry identifiers that become event metadata. The record's
            // `uuid` is not copied to a QEvent, so it is deliberately not bounded here.
            let items = nested(object, "message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array);
            for item in items.into_iter().flatten() {
                for key in ["id", "tool_use_id", "name"] {
                    if let Some(reason) = bounded(item.get(key), MAX_ID_BYTES, IDENTIFIER_TOO_LONG)
                    {
                        return Some(reason);
                    }
                }
            }
            None
        }
        Provider::OpenCode => {
            if let Some(reason) =
                bounded(object.get("sessionID"), MAX_ID_BYTES, IDENTIFIER_TOO_LONG)
            {
                return Some(reason);
            }
            let part = nested(object, "part")?;
            for key in ["id", "callID", "tool"] {
                if let Some(reason) = bounded(part.get(key), MAX_ID_BYTES, IDENTIFIER_TOO_LONG) {
                    return Some(reason);
                }
            }
            None
        }
    }
}

fn subevent_count_limit_reason(
    provider: Provider,
    object: &Map<String, Value>,
) -> Option<&'static str> {
    let count = match provider {
        Provider::ClaudeCode => object
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        Provider::Codex => object
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("content"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        Provider::OpenCode => 0,
    };
    (count > MAX_SUBEVENTS_PER_RECORD).then_some("normalization limit exceeded: too many subevents")
}

fn drafts_limit_reason(drafts: &[Draft]) -> Option<&'static str> {
    if drafts.len() > MAX_SUBEVENTS_PER_RECORD {
        return Some("normalization limit exceeded: too many subevents");
    }
    let mut total = 0usize;
    for draft in drafts {
        if draft.timestamp_raw.as_deref().map_or(0, str::len) > MAX_TIMESTAMP_BYTES {
            return Some("normalization limit exceeded: timestamp is too long");
        }
        if draft.phase.as_deref().map_or(0, str::len) > MAX_PHASE_BYTES {
            return Some("normalization limit exceeded: phase is too long");
        }
        for (value, limit) in [
            (draft.call_id.as_deref(), MAX_ID_BYTES),
            (draft.origin_id.as_deref(), MAX_ID_BYTES),
            (draft.tool_name.as_deref(), MAX_ID_BYTES),
        ] {
            if value.map_or(0, str::len) > limit {
                return Some("normalization limit exceeded: identifier is too long");
            }
        }
        total = total
            .saturating_add(draft.text.len())
            .saturating_add(draft.source_kind.len())
            .saturating_add(draft.timestamp_raw.as_deref().map_or(0, str::len))
            .saturating_add(draft.phase.as_deref().map_or(0, str::len))
            .saturating_add(draft.call_id.as_deref().map_or(0, str::len))
            .saturating_add(draft.origin_id.as_deref().map_or(0, str::len))
            .saturating_add(draft.tool_name.as_deref().map_or(0, str::len));
    }
    (total > MAX_NORMALIZED_BYTES_PER_RECORD)
        .then_some("normalization limit exceeded: normalized bytes are too large")
}

/// Explicit provider metadata marking a message as internal reasoning. This is a metadata-only
/// rule (never a body keyword heuristic): a `role:assistant` message can still be private when its
/// channel/phase says so.
fn is_private_channel(payload: &Map<String, Value>) -> bool {
    let channel = payload.get("channel").and_then(Value::as_str);
    let phase = payload.get("phase").and_then(Value::as_str);
    matches!(channel, Some("analysis"))
        || matches!(phase, Some("analysis"))
        || payload.get("reasoning").and_then(Value::as_bool) == Some(true)
}

fn record_session_id(provider: Provider, object: &Map<String, Value>) -> Option<String> {
    match provider {
        Provider::Codex => {
            if object.get("type").and_then(Value::as_str) == Some("session_meta") {
                nested(object, "payload")
                    .and_then(|p| p.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        }
        Provider::ClaudeCode => object
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        Provider::OpenCode => object
            .get("sessionID")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn raw_type_key(provider: Provider, bytes: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return "invalid_utf8".to_string();
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text.trim()) else {
        return "invalid_json".to_string();
    };
    let outer = object.get("type").and_then(Value::as_str).unwrap_or("None");
    match provider {
        Provider::Codex => match object.get("payload").and_then(Value::as_object) {
            Some(payload) => format!(
                "{outer}/{}",
                payload
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("None")
            ),
            None => outer.to_string(),
        },
        _ => outer.to_string(),
    }
}

fn tmp() -> Draft {
    Draft {
        kind: Kind::Unknown,
        source_kind: String::new(),
        timestamp_raw: None,
        phase: None,
        origin: Origin::Unknown,
        text: String::new(),
        tool_name: None,
        call_id: None,
        observed: ToolObserved::None,
        origin_id: None,
        role: None,
    }
}

fn str_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Null) | None => None,
        Some(other) => Some(json_text(other)),
    }
}

fn nested<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    object.get(key).and_then(Value::as_object)
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

fn codex_drafts(object: &Map<String, Value>) -> Vec<Draft> {
    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = str_field(object, "timestamp");
    let Some(payload) = nested(object, "payload") else {
        let mut draft = tmp();
        draft.kind = Kind::Unknown;
        draft.source_kind = format!("codex/{record_type}");
        draft.timestamp_raw = ts;
        return vec![draft];
    };
    let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let source_kind = format!("codex/{record_type}/{payload_type}");
    let base = |kind: Kind| {
        let mut draft = tmp();
        draft.kind = kind;
        draft.source_kind = source_kind.clone();
        draft.timestamp_raw = ts.clone();
        draft
    };

    match (record_type, payload_type) {
        ("session_meta", _) => {
            let mut draft = base(Kind::Injected);
            draft.origin = Origin::Framework;
            draft.text = String::new();
            draft.origin_id = payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.role = Some(Role::System);
            vec![draft]
        }
        ("response_item", "message") => {
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            let mut draft = base(match role {
                "user" => Kind::User,
                "assistant" => Kind::Assistant,
                "developer" | "system" => Kind::Injected,
                _ => Kind::Unknown,
            });
            draft.role = match role {
                "user" => Some(Role::User),
                "assistant" => Some(Role::Assistant),
                "developer" => Some(Role::Developer),
                "system" => Some(Role::System),
                _ => None,
            };
            if draft.kind == Kind::Injected {
                draft.origin = Origin::Framework;
            }
            draft.phase = payload
                .get("phase")
                .and_then(Value::as_str)
                .map(str::to_string);
            if is_private_channel(payload) {
                // Hard-scope: never rendered in messages/all/context, even for role=assistant.
                draft.kind = Kind::Reasoning;
            }
            draft.text = text_from(payload.get("content"));
            draft.origin_id = payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);
            vec![draft]
        }
        ("event_msg", "user_message") | ("event_msg", "agent_message") => {
            let mut draft = base(if payload_type == "user_message" {
                Kind::User
            } else {
                Kind::Assistant
            });
            if is_private_channel(payload) {
                draft.kind = Kind::Reasoning;
            }
            draft.role = Some(if payload_type == "user_message" {
                Role::User
            } else {
                Role::Assistant
            });
            draft.phase = payload
                .get("phase")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.text = str_field(payload, "message").unwrap_or_default();
            draft.origin_id = payload
                .get("item_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            vec![draft]
        }
        ("response_item", "reasoning") => {
            let mut draft = base(Kind::Reasoning);
            draft.text = text_from(payload.get("summary"));
            vec![draft]
        }
        ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
            let mut draft = base(Kind::ToolCall);
            draft.tool_name = payload
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.call_id = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.text = match payload_type {
                "custom_tool_call" => str_field(payload, "input").unwrap_or_default(),
                _ => str_field(payload, "arguments").unwrap_or_default(),
            };
            vec![draft]
        }
        ("response_item", "function_call_output")
        | ("response_item", "custom_tool_call_output") => {
            let mut draft = base(Kind::ToolOutput);
            draft.call_id = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let raw_output = payload.get("output");
            draft.text = match payload_type {
                "custom_tool_call_output" => raw_output
                    .map(|v| function_output_text(Some(v)))
                    .unwrap_or_default(),
                _ => raw_output
                    .map(|v| function_output_text(Some(v)))
                    .unwrap_or_default(),
            };
            let exit_code = raw_output
                .and_then(Value::as_object)
                .and_then(|o| o.get("metadata"))
                .and_then(Value::as_object)
                .and_then(|m| m.get("exit_code"))
                .and_then(Value::as_i64);
            draft.observed = ToolObserved::Returned {
                is_error: exit_code.map(|c| c != 0).unwrap_or(false),
                status: exit_code.map(|c| format!("exit_code={c}")),
            };
            draft.tool_name = payload
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string);
            vec![draft]
        }
        ("compacted", _) | ("event_msg", "context_compacted") => {
            let mut draft = base(Kind::Compaction);
            draft.origin = Origin::Compaction;
            draft.text = str_field(payload, "message").unwrap_or_default();
            vec![draft]
        }
        ("event_msg", "mcp_tool_call_end") => {
            let invocation = nested(payload, "invocation");
            let mut draft = base(Kind::ToolCall);
            draft.tool_name = invocation
                .and_then(|o| o.get("tool"))
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.call_id = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.text = json_text(&Value::Object(payload.clone()));
            vec![draft]
        }
        ("event_msg", "patch_apply_end") => {
            let mut draft = base(Kind::ToolOutput);
            draft.call_id = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.tool_name = Some("apply_patch".to_string());
            draft.text = str_field(payload, "stdout").unwrap_or_default();
            draft.observed = ToolObserved::Returned {
                is_error: payload.get("success").and_then(Value::as_bool) == Some(false),
                status: payload
                    .get("success")
                    .and_then(Value::as_bool)
                    .map(|ok| format!("success={ok}")),
            };
            vec![draft]
        }
        _ => {
            let mut draft = base(Kind::Unknown);
            draft.text = String::new();
            draft.origin_id = None;
            vec![draft]
        }
    }
}

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

/// One draft for one Claude `message.content[]` block.
///
/// `origin` is the provenance the record asserts (unknown for a plain record, framework for a
/// meta-marked one). Tool blocks keep their kind in both cases: dropping a `tool_result` block
/// because the row was framework-injected would make `--with-pair` report a result that exists
/// as missing.
fn claude_block_draft(
    record_type: &str,
    ts: Option<&str>,
    role: Role,
    index: usize,
    item: &Map<String, Value>,
    origin: Origin,
) -> Option<Draft> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
    let mut draft = tmp();
    draft.source_kind = format!("claude/{record_type}#{index}");
    draft.timestamp_raw = ts.map(str::to_string);
    draft.origin = origin;
    match item_type {
        "text" => {
            // Framework-injected text is private; a plain text block follows the record role.
            draft.kind = if origin == Origin::Framework {
                Kind::Injected
            } else if role == Role::User {
                Kind::User
            } else {
                Kind::Assistant
            };
            draft.role = Some(role);
            draft.text = str_field(item, "text").unwrap_or_default();
        }
        "tool_use" => {
            draft.kind = Kind::ToolCall;
            draft.tool_name = item.get("name").and_then(Value::as_str).map(str::to_string);
            draft.call_id = item.get("id").and_then(Value::as_str).map(str::to_string);
            draft.text = item
                .get("input")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => json_text(other),
                })
                .unwrap_or_default();
        }
        "tool_result" => {
            draft.kind = Kind::ToolOutput;
            // Tool results are provider-produced, not human-authored.
            draft.call_id = item
                .get("tool_use_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.text = item
                .get("content")
                .map(|v| text_from(Some(v)))
                .unwrap_or_default();
            let is_error = item.get("is_error").and_then(Value::as_bool) == Some(true);
            draft.observed = ToolObserved::Returned {
                is_error,
                status: None,
            };
        }
        "thinking" | "redacted_thinking" => {
            draft.kind = Kind::Reasoning;
            draft.text = str_field(item, "thinking").unwrap_or_default();
        }
        _ => return None,
    }
    Some(draft)
}

fn claude_drafts(object: &Map<String, Value>) -> Vec<Draft> {
    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = str_field(object, "timestamp");
    let is_meta = object.get("isMeta").and_then(Value::as_bool) == Some(true);
    let is_compact = object.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
    let message = nested(object, "message");
    let mut out = Vec::new();

    let make = |index: usize, kind: Kind, text: String| {
        let mut draft = tmp();
        draft.kind = kind;
        draft.source_kind = format!("claude/{record_type}#{index}");
        draft.timestamp_raw = ts.clone();
        draft.text = text;
        draft
    };

    if is_compact || record_type == "summary" {
        let mut draft = make(0, Kind::Compaction, String::new());
        draft.origin = Origin::Compaction;
        draft.text = str_field(object, "summary").unwrap_or_default();
        if draft.text.is_empty() {
            draft.text = message
                .and_then(|m| m.get("content"))
                .map(|v| text_from(Some(v)))
                .unwrap_or_default();
        }
        return vec![draft];
    }
    if is_meta {
        // `isMeta` marks a framework-injected record, but the flag can also sit on a row that
        // carries tool traffic. Collapsing such a row into one private text draft erased the
        // tool block, and `--with-pair` then asserted "no result" for a result that exists.
        // Keep the framework provenance and the blocks the row actually carries.
        let content = message.and_then(|m| m.get("content"));
        let items = content.and_then(Value::as_array);
        if let Some(items) = items.filter(|items| {
            items.iter().any(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("tool_use") | Some("tool_result")
                )
            })
        }) {
            for (index, item) in items.iter().enumerate() {
                let Some(item_object) = item.as_object() else {
                    continue;
                };
                if let Some(draft) = claude_block_draft(
                    record_type,
                    ts.as_deref(),
                    Role::User,
                    index,
                    item_object,
                    Origin::Framework,
                ) {
                    out.push(draft);
                }
            }
            return out;
        }
        let mut draft = make(
            0,
            Kind::Injected,
            content.map(|v| text_from(Some(v))).unwrap_or_default(),
        );
        draft.origin = Origin::Framework;
        draft.role = Some(Role::User);
        return vec![draft];
    }

    match record_type {
        "user" | "assistant" => {
            let role = if record_type == "user" {
                Role::User
            } else {
                Role::Assistant
            };
            match message.and_then(|m| m.get("content")) {
                Some(Value::String(text)) => {
                    let mut draft = make(
                        0,
                        if role == Role::User {
                            Kind::User
                        } else {
                            Kind::Assistant
                        },
                        text.clone(),
                    );
                    draft.role = Some(role);
                    // A plain `type:user` record carries no positive human-provenance field:
                    // absence of injection flags is not evidence of a human author.
                    draft.origin = Origin::Unknown;
                    out.push(draft);
                }
                Some(Value::Array(items)) => {
                    for (index, item) in items.iter().enumerate() {
                        let Some(item_object) = item.as_object() else {
                            continue;
                        };
                        if let Some(draft) = claude_block_draft(
                            record_type,
                            ts.as_deref(),
                            role,
                            index,
                            item_object,
                            Origin::Unknown,
                        ) {
                            out.push(draft);
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {
            let mut draft = make(0, Kind::Unknown, String::new());
            draft.source_kind = format!("claude/{record_type}");
            out.push(draft);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// OpenCode run JSONL
// ---------------------------------------------------------------------------

fn opencode_drafts(object: &Map<String, Value>) -> Vec<Draft> {
    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = str_field(object, "timestamp");
    let part = nested(object, "part");
    let mut draft = tmp();
    draft.source_kind = format!("opencode/{record_type}");
    draft.timestamp_raw = ts;
    match record_type {
        "text" => {
            draft.kind = Kind::Assistant;
            draft.role = Some(Role::Assistant);
            draft.text = part
                .map(|p| value_as_string(p.get("text"), ""))
                .unwrap_or_default();
        }
        "reasoning" => {
            draft.kind = Kind::Reasoning;
            draft.text = part
                .map(|p| value_as_string(p.get("text"), ""))
                .unwrap_or_default();
        }
        "tool_use" => {
            let state = part.and_then(|p| p.get("state")).and_then(Value::as_object);
            let output = state.and_then(|s| s.get("output").or_else(|| s.get("error")));
            let kind = if output.is_some() {
                Kind::ToolOutput
            } else {
                Kind::ToolCall
            };
            draft.kind = kind;
            draft.tool_name = part
                .and_then(|p| p.get("tool"))
                .and_then(Value::as_str)
                .map(str::to_string);
            draft.call_id = part
                .and_then(|p| p.get("callID").or_else(|| p.get("id")))
                .and_then(Value::as_str)
                .map(str::to_string);
            let text = match output {
                Some(value) => value_as_string(Some(value), ""),
                None => state
                    .and_then(|s| s.get("input"))
                    .map(json_text)
                    .unwrap_or_default(),
            };
            draft.text = text;
            if let Some(state) = state {
                let status = state
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let is_error = status.as_deref() == Some("error");
                draft.observed = ToolObserved::Returned { is_error, status };
            }
        }
        "step_start" | "step_finish" | "error" => {
            draft.kind = Kind::Unknown;
            draft.text = String::new();
        }
        _ => {
            draft.kind = Kind::Unknown;
        }
    }
    vec![draft]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(line: u64) -> RecordSpan {
        RecordSpan {
            raw_line: line,
            byte_start: (line - 1) * 100,
            byte_end: line * 100,
        }
    }

    #[test]
    fn custom_codex_tool_call_and_output_are_preserved() {
        let mut normalizer = Normalizer::new(Provider::Codex, "s".to_string());
        let call = br#"{"timestamp":"2026-10-01T00:00:00Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c1","input":"*** Begin Patch"}}"#;
        let events = normalizer.normalize(span(1), call, "src");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::ToolCall);
        assert_eq!(events[0].tool_name.as_deref(), Some("apply_patch"));
        assert_eq!(events[0].call_id.as_deref(), Some("c1"));
        assert_eq!(events[0].text, "*** Begin Patch");

        let output = br#"{"timestamp":"2026-10-01T00:00:01Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c1","output":"Done!"}}"#;
        let events = normalizer.normalize(span(2), output, "src");
        assert_eq!(events[0].kind, Kind::ToolOutput);
        assert_eq!(events[0].call_id.as_deref(), Some("c1"));
        assert_eq!(events[0].text, "Done!");
    }

    #[test]
    fn claude_three_text_blocks_get_stable_subevent_indices() {
        let mut normalizer = Normalizer::new(Provider::ClaudeCode, "c".to_string());
        let row = br#"{"type":"assistant","uuid":"m","sessionId":"c","timestamp":"2026-10-01T00:00:00Z","message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"},{"type":"text","text":"c"}]}}"#;
        let events = normalizer.normalize(span(3), row, "src");
        assert_eq!(events.len(), 3);
        assert_eq!(
            events
                .iter()
                .map(|e| e.source.subevent_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(events.iter().all(|e| e.source.raw_line == 3));
        assert!(events.iter().all(|e| e.source.byte_start == 200));
        assert!(events.iter().all(|e| e.source.byte_end == 300));
        assert_eq!(events[1].text, "b");
    }

    #[test]
    fn reasoning_and_developer_are_private_but_kept() {
        let mut normalizer = Normalizer::new(Provider::Codex, "s".to_string());
        let reasoning = br#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"secret"}]}}"#;
        let events = normalizer.normalize(span(1), reasoning, "src");
        assert_eq!(events[0].kind, Kind::Reasoning);
        assert!(events[0].is_private());
        assert_eq!(events[0].text, "secret");

        let developer = br#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rules"}]}}"#;
        let events = normalizer.normalize(span(2), developer, "src");
        assert_eq!(events[0].kind, Kind::Injected);
        assert_eq!(events[0].origin, Origin::Framework);
        assert_eq!(events[0].role, Some(Role::Developer));
        assert!(events[0].is_private());
    }

    #[test]
    fn unmarked_claude_user_is_unknown_origin_not_human() {
        let mut normalizer = Normalizer::new(Provider::ClaudeCode, "c".to_string());
        let row = br#"{"type":"user","uuid":"u","sessionId":"c","timestamp":"2026-10-01T00:00:00Z","message":{"role":"user","content":"plain prompt"}}"#;
        let events = normalizer.normalize(span(1), row, "src");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::User);
        assert_eq!(events[0].role, Some(Role::User));
        // Role alone must not assert a human author.
        assert_eq!(events[0].origin, Origin::Unknown);
        assert!(
            !events[0].is_private(),
            "unknown origin stays visible in messages"
        );
    }

    #[test]
    fn claude_is_meta_is_framework_and_private() {
        let mut normalizer = Normalizer::new(Provider::ClaudeCode, "c".to_string());
        let row = br#"{"type":"user","uuid":"m","sessionId":"c","timestamp":"2026-10-01T00:00:00Z","isMeta":true,"message":{"role":"user","content":"framework reminder"}}"#;
        let events = normalizer.normalize(span(1), row, "src");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].origin, Origin::Framework);
        assert!(
            events[0].is_private(),
            "framework injection is excluded by default"
        );
    }

    #[test]
    fn invalid_records_are_counted_not_silently_dropped() {
        let mut normalizer = Normalizer::new(Provider::Codex, "s".to_string());
        let events = normalizer.normalize(span(1), b"{oops", "src");
        assert_eq!(events[0].kind, Kind::ParseError);
        assert_eq!(normalizer.invalid_records, 1);
        let events = normalizer.normalize(span(2), &[0xff, 0xfe], "src");
        assert_eq!(events[0].kind, Kind::ParseError);
        assert_eq!(normalizer.invalid_records, 2);
        assert_eq!(normalizer.coverage.total(), 2);
    }

    #[test]
    fn coverage_cap_bounds_distinct_keys_but_keeps_counts() {
        let mut coverage = TypeCoverage::default();
        for i in 0..200u32 {
            coverage.add(&format!("t{i}"));
        }
        assert_eq!(coverage.counts.len(), COVERAGE_TYPE_CAP);
        assert_eq!(coverage.overflow, 200 - COVERAGE_TYPE_CAP as u64);
        assert_eq!(coverage.total(), 200);
    }

    #[test]
    fn unknown_records_are_typed_unknown_with_source_pointer() {
        let mut normalizer = Normalizer::new(Provider::Codex, "s".to_string());
        let unknown = br#"{"type":"mystery","payload":{"x":1}}"#;
        let events = normalizer.normalize(span(7), unknown, "src");
        assert_eq!(events[0].kind, Kind::Unknown);
        assert_eq!(events[0].source.raw_line, 7);
        assert!(normalizer
            .coverage
            .counts
            .iter()
            .any(|(k, _)| k.starts_with("mystery")));
    }
}
