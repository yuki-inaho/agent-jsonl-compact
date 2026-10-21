//! Query command runner: `view` / `search` / `sessions`.
//!
//! Exit contract (new commands only; legacy stays as before):
//! * rc0 – the requested range was processed (a legitimate empty result included),
//! * rc1 – source/decode/format-uncertain/partial/cursor error,
//! * rc2 – argument or query syntax error (including an unusable output budget).
//!
//! stdout carries only JSONL (or text) for the result; diagnostics are stderr. The combined
//! stdout+stderr size is measured against `--max-output-bytes` before anything is written.

use crate::cli::{
    QueryArgs, QueryProvider, QueryScope, SearchArgs, SessionFormat, SessionsArgs, SharedQueryArgs,
    VersionArgs, VersionOutput,
};
use crate::query::cursor::{
    generation_of, sha256_hex, CursorKind, CursorPayload, CursorProblem, CURSOR_VERSION,
};
use crate::query::detect::{detect_with, Detection};
use crate::query::filter::{parse_roles, Filters, Scope};
use crate::query::model::{parse_rfc3339_ms, Kind, Provider, Role, QUERY_SCHEMA_VERSION};
use crate::query::normalize::Normalizer;
use crate::query::output::{
    attach_observed, cut_utf8, escape_controls, event_to_json, event_to_text, line,
};
use crate::query::reader::{
    LineItem, RecordSpan, SourceReader, ANCHOR_WINDOW_BYTES, DEFAULT_MAX_RECORD_BYTES,
};
use crate::query::select::{in_scope, MatchInfo, Mode, SelectOptions, Selector};
use crate::query::source::file_source_id;
use crate::query::toolpair::{correlate, PairState};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_VIEW_LIMIT: usize = 50;
pub const DEFAULT_SEARCH_LIMIT: usize = 20;
pub const DEFAULT_MAX_EVENTS: usize = 200;
pub const MAX_MAX_EVENTS: usize = 10_000;
pub const DEFAULT_OUTPUT_BYTES: usize = 32_768;
pub const MIN_OUTPUT_BYTES: usize = 8_192;
/// Space reserved for the final metadata record.
const METADATA_RESERVE: usize = 4_096;
/// Maximum encoded semantic filters (roles/phases/event) inside a cursor.
const MAX_FILTER_BYTES: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Ok = 0,
    Failure = 1,
    Usage = 2,
}

pub struct Prepared {
    pub exit: Exit,
    pub stdout: String,
    pub stderr: String,
}

impl Prepared {
    pub fn code(&self) -> i32 {
        self.exit as i32
    }
}

fn emit(prepared: Prepared) -> i32 {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(prepared.stdout.as_bytes());
    let _ = stdout.flush();
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(prepared.stderr.as_bytes());
    let _ = stderr.flush();
    prepared.code()
}

fn usage(message: &str) -> Prepared {
    let mut text = message.to_string();
    if text.len() > 1_024 {
        text.truncate(1_024);
    }
    Prepared {
        exit: Exit::Usage,
        stdout: String::new(),
        stderr: format!("ERROR: {text}\n"),
    }
}

fn error_meta(error_type: &str, reason: &str) -> String {
    line(&json!({
        "schema_version": QUERY_SCHEMA_VERSION,
        "record_type": "metadata",
        "status": "error",
        "error": {"type": error_type, "reason": reason},
        "exhausted": false,
    }))
}

fn failure(error_type: &str, reason: &str) -> Prepared {
    Prepared {
        exit: Exit::Failure,
        stdout: error_meta(error_type, reason),
        stderr: format!("error: {error_type}: {reason}\n"),
    }
}

pub fn run_version(args: &VersionArgs) -> i32 {
    match args.output {
        VersionOutput::Text => print!("{}", crate::query::version::text()),
        VersionOutput::Json => {
            println!(
                "{}",
                serde_json::to_string(&crate::query::version::json()).unwrap()
            );
        }
    }
    0
}

fn scope_of(filters: &Filters) -> Scope {
    filters.scope.unwrap_or(Scope::Messages)
}

fn command_name(kind: QueryKind) -> &'static str {
    match kind {
        QueryKind::View => "view",
        QueryKind::Search => "search",
    }
}

pub fn run_view(args: &QueryArgs) -> i32 {
    let mut matcher = Matcher::View;
    let prepared = match parse_shared(&args.shared, QueryKind::View, 0) {
        Ok(shared) => run_query(shared, QueryKind::View, &mut matcher, args.with_pair),
        Err(err) => usage(&err),
    };
    emit(prepared)
}

pub fn run_search(args: &SearchArgs) -> i32 {
    let prepared = (|| {
        let resuming = args.shared.cursor.is_some() || args.shared.after_cursor.is_some();
        if args.query.is_empty() && !resuming {
            return Err(
                "--query requires at least one value (or resume with --cursor)".to_string(),
            );
        }
        let shared = parse_shared(&args.shared, QueryKind::Search, args.context.unwrap_or(0))?;
        // On resume the cursor's own query is reused; compile only what was given.
        let mut matcher = Matcher::new_search(&args.query, args.regex.unwrap_or(false))?;
        let mut shared = shared;
        shared.queries = args.query.clone();
        shared.regex = args.regex.unwrap_or(false);
        shared.explicit_regex = args.regex.is_some();
        shared.explicit_context = args.context;
        Ok::<Prepared, String>(run_query(
            shared,
            QueryKind::Search,
            &mut matcher,
            args.with_pair,
        ))
    })();
    emit(prepared.unwrap_or_else(|err| usage(&err)))
}

// ---------------------------------------------------------------------------
// Matcher (literal OR / opt-in regex)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Matcher {
    View,
    Search {
        literals: Vec<String>,
        regexes: Vec<CompiledPattern>,
    },
}

impl Matcher {
    fn new_search(queries: &[String], regex: bool) -> Result<Self, String> {
        // An empty set is legal only for cursor resume; the caller enforces that.
        if queries.is_empty() {
            return Ok(Matcher::Search {
                literals: Vec::new(),
                regexes: Vec::new(),
            });
        }
        if regex {
            let regexes = queries
                .iter()
                .map(|q| compile_regex(q))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Matcher::Search {
                literals: Vec::new(),
                regexes,
            })
        } else {
            Ok(Matcher::Search {
                literals: queries.to_vec(),
                regexes: Vec::new(),
            })
        }
    }

    fn matches(&mut self, text: &str) -> Option<MatchInfo> {
        match self {
            Matcher::View => Some(MatchInfo::default()),
            Matcher::Search { literals, regexes } => {
                let mut info = MatchInfo::default();
                for literal in literals {
                    if literal.is_empty() {
                        continue;
                    }
                    let mut from = 0;
                    while from <= text.len() {
                        let Some(pos) = text[from..].find(literal.as_str()) else {
                            break;
                        };
                        let start = from + pos;
                        info.ranges.push((start, start + literal.len()));
                        if !info.queries.contains(literal) {
                            info.queries.push(literal.clone());
                        }
                        from = start + literal.len().max(1);
                    }
                }
                for pattern in regexes {
                    let found = pattern.find_all(text);
                    if !found.is_empty() && !info.queries.iter().any(|q| q == &pattern.source) {
                        info.queries.push(pattern.source.clone());
                    }
                    for (start, end) in found {
                        info.ranges.push((start, end));
                    }
                }
                info.queries.sort();
                if info.queries.is_empty() && info.ranges.is_empty() {
                    None
                } else {
                    Some(info)
                }
            }
        }
    }
}

/// Compiled regex using the vetted `regex` crate. Regex is opt-in via `--regex`; a compile
/// failure is a query syntax error (rc2). Matches are reported as UTF-8 byte ranges of the
/// original (pre-preview) text.
#[derive(Debug, Clone)]
struct CompiledPattern {
    source: String,
    compiled: regex::Regex,
}

fn compile_regex(pattern: &str) -> Result<CompiledPattern, String> {
    let compiled =
        regex::Regex::new(pattern).map_err(|err| format!("invalid --regex '{pattern}': {err}"))?;
    Ok(CompiledPattern {
        source: pattern.to_string(),
        compiled,
    })
}

impl CompiledPattern {
    fn find_all(&mut self, haystack: &str) -> Vec<(usize, usize)> {
        self.compiled
            .find_iter(haystack)
            .map(|m| (m.start(), m.end()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Shared parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryKind {
    View,
    Search,
}

struct Shared {
    input: Option<PathBuf>,
    session: Option<String>,
    roots: Vec<PathBuf>,
    format: SessionFormat,
    scope: Scope,
    roles: Vec<String>,
    phases: Vec<String>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cursor: Option<String>,
    after_cursor: Option<String>,
    event_filter: Option<String>,
    limit: usize,
    tail: Option<usize>,
    max_events: usize,
    max_output_bytes: usize,
    max_record_bytes: u64,
    context: usize,
    output: crate::cli::QueryOutput,
    kind: QueryKind,
    queries: Vec<String>,
    regex: bool,
    /// Explicitly supplied (not merely the default) semantic selections.
    explicit_scope: Option<Scope>,
    explicit_context: Option<usize>,
    explicit_format: Option<SessionFormat>,
    explicit_regex: bool,
}

fn parse_shared(args: &SharedQueryArgs, kind: QueryKind, context: usize) -> Result<Shared, String> {
    if args.input.is_none() && args.session.is_none() {
        return Err("requires --input <FILE> or --session <provider:SESSION_ID>".to_string());
    }
    if args.input.is_some() && !args.root.is_empty() {
        return Err("--root applies to --session discovery, not to --input".to_string());
    }
    if args.tail.is_some()
        && (args.cursor.is_some() || args.after_cursor.is_some() || args.event.is_some())
    {
        return Err("--tail cannot be combined with --cursor/--after-cursor/--event".to_string());
    }
    if args.cursor.is_some() && args.after_cursor.is_some() {
        return Err("--cursor and --after-cursor are mutually exclusive".to_string());
    }
    if args.event.is_some() && (args.cursor.is_some() || args.after_cursor.is_some()) {
        return Err("--event cannot be combined with cursors".to_string());
    }
    let explicit_scope = args.scope.map(|scope| match scope {
        QueryScope::Messages => Scope::Messages,
        QueryScope::Tools => Scope::Tools,
        QueryScope::All => Scope::All,
    });
    let scope = explicit_scope.unwrap_or(Scope::Messages);
    let roles = match &args.role {
        Some(raw) => parse_roles(raw)?,
        None => Vec::new(),
    };
    let phases: Vec<String> = match &args.phase {
        Some(raw) => raw
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
        None => Vec::new(),
    };
    let since_ms = args
        .since
        .as_deref()
        .map(|raw| {
            parse_rfc3339_ms(raw)
                .ok_or_else(|| format!("invalid --since '{raw}' (offset RFC3339 required)"))
        })
        .transpose()?;
    let until_ms = args
        .until
        .as_deref()
        .map(|raw| {
            parse_rfc3339_ms(raw)
                .ok_or_else(|| format!("invalid --until '{raw}' (offset RFC3339 required)"))
        })
        .transpose()?;
    if let (Some(since), Some(until)) = (since_ms, until_ms) {
        if since >= until {
            return Err("--since must be earlier than --until".to_string());
        }
    }
    let max_output_bytes = args.max_output_bytes.unwrap_or(DEFAULT_OUTPUT_BYTES);
    if max_output_bytes < MIN_OUTPUT_BYTES {
        return Err(format!(
            "--max-output-bytes {max_output_bytes} is below the minimum {MIN_OUTPUT_BYTES}"
        ));
    }
    let limit = args.limit.unwrap_or(match kind {
        QueryKind::View => DEFAULT_VIEW_LIMIT,
        QueryKind::Search => DEFAULT_SEARCH_LIMIT,
    });
    if limit == 0 {
        return Err("--limit must be at least 1".to_string());
    }
    if let Some(tail) = args.tail {
        if tail == 0 {
            return Err("--tail must be at least 1".to_string());
        }
    }
    let max_events = args.max_events.unwrap_or(DEFAULT_MAX_EVENTS);
    if max_events == 0 || max_events > MAX_MAX_EVENTS {
        return Err(format!(
            "--max-events must be between 1 and {MAX_MAX_EVENTS}"
        ));
    }
    let filters = Filters {
        scope: Some(scope),
        roles: roles.clone(),
        phases: phases.clone(),
        since_ms,
        until_ms,
        provider: None,
    };
    if filters.fingerprint_bytes(args.event.as_deref()) > MAX_FILTER_BYTES {
        return Err(format!(
            "combined --role/--phase/--event length exceeds {MAX_FILTER_BYTES} bytes (cursor payload is bounded)"
        ));
    }
    Ok(Shared {
        input: args.input.clone(),
        session: args.session.clone(),
        roots: args.root.clone(),
        format: args.format.unwrap_or(SessionFormat::Auto),
        scope,
        roles,
        phases,
        since_ms,
        until_ms,
        cursor: args.cursor.clone(),
        after_cursor: args.after_cursor.clone(),
        event_filter: args.event.clone(),
        limit,
        tail: args.tail,
        max_events,
        max_output_bytes,
        max_record_bytes: args
            .max_record_bytes
            .unwrap_or(DEFAULT_MAX_RECORD_BYTES)
            .max(1),
        context,
        output: args.output,
        kind,
        queries: Vec::new(),
        regex: false,
        explicit_scope,
        explicit_context: None,
        explicit_format: args.format,
        explicit_regex: false,
    })
}

fn matches_soft(
    event: &crate::query::model::QEvent,
    roles: &[String],
    phases: &[String],
    since: Option<i64>,
    until: Option<i64>,
) -> bool {
    if !roles.is_empty() {
        let role = event.role.map(Role::as_str).unwrap_or("");
        if !roles.iter().any(|r| r == role) {
            return false;
        }
    }
    if !phases.is_empty() {
        let phase = event.phase.as_deref().unwrap_or("");
        if !phases.iter().any(|p| p == phase) {
            return false;
        }
    }
    if since.is_some() || until.is_some() {
        let Some(epoch) = event.epoch_ms else {
            return false;
        };
        if since.map(|s| epoch < s).unwrap_or(false) {
            return false;
        }
        if until.map(|u| epoch >= u).unwrap_or(false) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Main query
// ---------------------------------------------------------------------------

fn run_query(
    mut shared: Shared,
    kind: QueryKind,
    matcher: &mut Matcher,
    with_pair: bool,
) -> Prepared {
    let input = match resolve_input(&shared) {
        Ok(Some(input)) => input,
        Ok(None) => return usage("no input resolved"),
        Err(prepared) => return prepared,
    };
    if !input.exists() {
        return failure(
            "source_unreadable",
            &format!("input not found: {}", input.display()),
        );
    }
    let metadata = match std::fs::metadata(&input) {
        Ok(metadata) => metadata,
        Err(err) => return failure("source_unreadable", &err.to_string()),
    };
    let snapshot_end = metadata.len();
    let open_mtime_ns = file_mtime_ns(&input).unwrap_or(0);
    let generation = generation_of(&metadata);
    let source_id = file_source_id(&input);

    // ---- N02 fast path: validate the cursor BEFORE any format detection or header scan. ----
    // The token is untrusted; it is decoded and validated against identity, anchors, offsets and
    // explicit semantic conflicts. Only then are provider/session/profile reused from it, so an
    // append read costs O(anchor + appended bytes) instead of re-detecting the prefix.
    // Effective frozen boundary for every reader in this invocation: the old page snapshot when
    // resuming a page, otherwise the length captured at open. Assigned once, never re-read later.
    let mut effective_snapshot_end = snapshot_end;
    let mut resume: Option<CursorPayload> = None;
    let (mut expected_kind, token) = if let Some(t) = &shared.cursor {
        (Some(CursorKind::Page), Some(t.clone()))
    } else if let Some(t) = &shared.after_cursor {
        (Some(CursorKind::Append), Some(t.clone()))
    } else {
        (None, None)
    };

    let mut aux_detect_bytes = 0u64;
    let mut aux_metadata_bytes = 0u64;
    let mut aux_anchor_bytes: u64 = 0;
    let mut aux_validation_bytes: u64 = 0;

    let (provider, session_id, filters) = if let (Some(kind), Some(token)) = (expected_kind, token)
    {
        let payload = match CursorPayload::decode(&token) {
            Ok(payload) => payload,
            Err(problem) => return cursor_failure(problem),
        };
        if payload.kind != kind {
            return cursor_failure(CursorProblem::WrongKind);
        }
        // Reuse the stored semantic profile. Only an *explicitly supplied* argument can conflict;
        // absence means "reuse the cursor", never "compare against the default".
        if let Some(explicit_scope) = shared.explicit_scope {
            if explicit_scope != scope_of(&payload.filters) {
                return cursor_failure(CursorProblem::FilterMismatch);
            }
        }
        // An explicit --format must agree with the stored provider; auto/absent reuses it.
        if let Some(explicit_format) = shared.explicit_format {
            let requested = match explicit_format {
                SessionFormat::Codex => Some(Provider::Codex),
                SessionFormat::ClaudeCode => Some(Provider::ClaudeCode),
                SessionFormat::OpenCode => Some(Provider::OpenCode),
                SessionFormat::Auto => None,
            };
            if let Some(requested) = requested {
                if requested != payload.provider {
                    return cursor_failure(CursorProblem::FilterMismatch);
                }
            }
        }
        let explicit_roles = !shared.roles.is_empty();
        let explicit_phases = !shared.phases.is_empty();
        // Each time bound is tracked separately: repeating since must not discard stored until.
        if (explicit_roles && shared.roles != payload.filters.roles)
            || (explicit_phases && shared.phases != payload.filters.phases)
            || (shared.since_ms.is_some() && shared.since_ms != payload.filters.since_ms)
            || (shared.until_ms.is_some() && shared.until_ms != payload.filters.until_ms)
        {
            return cursor_failure(CursorProblem::FilterMismatch);
        }
        // An explicit --regex must not silently override the stored literal/regex mode.
        if shared.explicit_regex && shared.regex != payload.regex {
            return cursor_failure(CursorProblem::FilterMismatch);
        }
        // `context` is part of the semantic query profile: omitted reuses the cursor's value,
        // explicit must match, explicit conflict is a typed error.
        match shared.explicit_context {
            Some(context) if context != payload.context => {
                return cursor_failure(CursorProblem::FilterMismatch);
            }
            Some(context) => shared.context = context,
            None => shared.context = payload.context,
        }
        // Adopt the cursor's semantics (query set too) without rescanning the prefix.
        shared.scope = scope_of(&payload.filters);
        shared.roles = payload.filters.roles.clone();
        shared.phases = payload.filters.phases.clone();
        shared.since_ms = payload.filters.since_ms;
        shared.until_ms = payload.filters.until_ms;
        if let Some(event) = &shared.event_filter {
            if payload.event_filter.as_deref() != Some(event.as_str()) {
                return cursor_failure(CursorProblem::FilterMismatch);
            }
        } else {
            shared.event_filter = payload.event_filter.clone();
        }
        // Query set: absent reuses the stored set; an explicit but different set conflicts.
        if shared.queries.is_empty() && !payload.queries.is_empty() {
            shared.queries = payload.queries.clone();
        } else if !shared.queries.is_empty()
            && !payload.queries.is_empty()
            && shared.queries != payload.queries
        {
            return cursor_failure(CursorProblem::FilterMismatch);
        }
        // Regex mode is profile state independent of whether the query was repeated: an omitted
        // `--regex` inherits the stored mode (an explicit conflict was already rejected above),
        // and the matcher is rebuilt under the adopted mode for an explicitly repeated query too.
        if !shared.explicit_regex {
            shared.regex = payload.regex;
        }
        if !shared.queries.is_empty() {
            if let Ok(new_matcher) = Matcher::new_search(&shared.queries, shared.regex) {
                *matcher = new_matcher;
            }
        }
        let filters = payload.filters.clone();
        let session = if payload.session_id.is_empty() {
            {
                let (id, bytes) = crate::query::source::discover_session_id_counted(
                    &input,
                    payload.provider,
                    shared.max_record_bytes,
                    effective_snapshot_end,
                )
                .unwrap_or((None, 0));
                aux_metadata_bytes += bytes;
                id.unwrap_or_else(|| source_id.clone())
            }
        } else {
            payload.session_id.clone()
        };
        if let Err(problem) = validate_cursor(
            &token,
            kind,
            payload.provider,
            &filters,
            &shared.event_filter,
            command_name(shared.kind),
            &shared.queries,
            shared.regex,
            &input,
            &source_id,
            snapshot_end,
            generation,
            shared.max_record_bytes,
            &mut aux_anchor_bytes,
            &mut aux_validation_bytes,
        ) {
            return cursor_failure(problem);
        }
        if payload.kind == CursorKind::Page {
            effective_snapshot_end = payload.snapshot_end;
        }
        expected_kind = None;
        resume = Some(payload.clone());
        (payload.provider, session, filters)
    } else {
        let provider = match shared.format {
            SessionFormat::Codex => Provider::Codex,
            SessionFormat::ClaudeCode => Provider::ClaudeCode,
            SessionFormat::OpenCode => Provider::OpenCode,
            SessionFormat::Auto => {
                let mut reader = match SourceReader::open(&input, shared.max_record_bytes) {
                    Ok(reader) => reader,
                    Err(err) => return failure("source_unreadable", &err.to_string()),
                };
                reader.constrain_snapshot(effective_snapshot_end);
                let detection = detect_with(&mut reader);
                aux_detect_bytes = reader.read_stream_bytes();
                match detection {
                    Ok(Detection::Recognized(provider)) => provider,
                    Ok(Detection::Uncertain) => {
                        let mut unknown = 0u64;
                        let mut unknown_samples: Vec<Value> = Vec::new();
                        if let Ok(mut scan) = SourceReader::open(&input, shared.max_record_bytes) {
                            scan.constrain_snapshot(effective_snapshot_end);
                            let _ = scan.scan_from(0, |item| {
                                if let LineItem::Line { span, bytes } = item {
                                    let parsed = serde_json::from_slice::<Value>(&bytes);
                                    let recognized = parsed
                                        .as_ref()
                                        .ok()
                                        .and_then(crate::query::detect::classify_record)
                                        .is_some();
                                    if parsed.is_ok() && !recognized {
                                        unknown += 1;
                                        if unknown_samples.len() < 8 {
                                            unknown_samples.push(json!({
                                                "raw_line": span.raw_line,
                                                "byte_start": span.byte_start,
                                                "byte_end": span.byte_end,
                                            }));
                                        }
                                    }
                                }
                                Ok(true)
                            });
                        }
                        let stdout = line(&json!({
                            "schema_version": QUERY_SCHEMA_VERSION,
                            "record_type": "metadata",
                            "status": "error",
                            "error": {
                                "type": "format_uncertain",
                                "reason": "no recognized provider evidence; pass --format codex|claude_code|opencode"
                            },
                            "counts": {"unknown": {"events": unknown, "samples": unknown_samples}},
                            "exhausted": false,
                        }));
                        return Prepared {
                            exit: Exit::Failure,
                            stdout,
                            stderr: "error: format_uncertain: pass --format explicitly\n"
                                .to_string(),
                        };
                    }
                    Ok(Detection::Conflicting) => {
                        return failure(
                            "format_inconsistent",
                            "the file contains conflicting provider evidence; pass --format to state the intended provider",
                        );
                    }
                    Err(err) => return failure("source_unreadable", &err.to_string()),
                }
            }
        };
        aux_detect_bytes += 0;
        let (session_id, metadata_bytes) = crate::query::source::discover_session_id_counted(
            &input,
            provider,
            shared.max_record_bytes,
            effective_snapshot_end,
        )
        .unwrap_or((None, 0));
        aux_metadata_bytes += metadata_bytes;
        let session = session_id.unwrap_or_else(|| source_id.clone());
        let filters = Filters {
            scope: Some(shared.scope),
            roles: shared.roles.clone(),
            phases: shared.phases.clone(),
            since_ms: shared.since_ms,
            until_ms: shared.until_ms,
            provider: Some(provider),
        };
        (provider, session, filters)
    };
    let _ = expected_kind;

    let resume_boundary = resume
        .as_ref()
        .filter(|p| p.kind == CursorKind::Page)
        .cloned();
    let start_offset = resume.as_ref().map(|p| p.offset).unwrap_or(0);
    let start_line = resume
        .as_ref()
        .map(|p| {
            if p.kind == CursorKind::Append {
                p.complete_line_count
            } else {
                p.raw_line.saturating_sub(1)
            }
        })
        .unwrap_or(0);

    // Streaming selection.
    // Head mode selects one event beyond the page so the page cursor can point at the first
    // unrendered event; without that extra look the caller can never tell "more exist".
    let page = shared.limit.min(shared.max_events);
    // The look-ahead stays one beyond the page even when the display cap is smaller: the
    // display cap governs what is rendered, not what must be examined to know more exists.
    let window = match shared.tail {
        Some(tail) => tail.min(shared.max_events),
        None => page.saturating_add(1),
    };
    let mode = if shared.tail.is_some() {
        Mode::Tail
    } else {
        Mode::Head
    };
    let event_filter_id = shared.event_filter.clone();
    let mut selector = Selector::new(SelectOptions {
        window,
        mode,
        context: shared.context,
        max_retained: shared.max_events.max(64),
    });
    let mut normalizer = Normalizer::new(provider, session_id.clone());
    let mut reader = match SourceReader::open(&input, shared.max_record_bytes) {
        Ok(reader) => reader,
        Err(err) => return failure("source_unreadable", &err.to_string()),
    };
    reader.constrain_snapshot(effective_snapshot_end);
    let mut stats = ScanStats::default();
    let mut line_counter = start_line;
    let roles = shared.roles.clone();
    let phases = shared.phases.clone();
    let since = shared.since_ms;
    let until = shared.until_ms;
    let scope = shared.scope;
    let mut last_line = start_line;
    let scan = reader.scan_from(start_offset, |item| {
        match item {
            LineItem::Line { span, bytes } => {
                line_counter += 1;
                last_line = line_counter;
                let mut adjusted = span;
                adjusted.raw_line = line_counter;
                let events = normalizer.normalize(adjusted, &bytes, &source_id);
                if events.iter().any(|e| e.kind == Kind::ParseError) {
                    stats.invalid += 1;
                    if stats.invalid_samples.len() < 8 {
                        stats.invalid_samples.push(adjusted);
                    }
                }
                if events.is_empty() {
                    stats.empty_records += 1;
                }
                if events
                    .iter()
                    .any(|e| e.kind == Kind::Unknown || e.kind == Kind::ParseError)
                    && stats.unknown_samples.len() < 8
                {
                    stats.unknown_samples.push(adjusted);
                }
                stats.unknown_events += events
                    .iter()
                    .filter(|e| e.kind == Kind::Unknown || e.kind == Kind::ParseError)
                    .count() as u64;
                stats.framework_excluded += events
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.origin,
                            crate::query::model::Origin::Framework
                                | crate::query::model::Origin::Compaction
                        )
                    })
                    .count() as u64;
                let time_filter = since.is_some() || until.is_some();
                for event in &events {
                    if !in_scope(event.kind, scope) {
                        continue;
                    }
                    // Time exclusions are time-only; role/phase exclusions are counted separately
                    // so metadata never double-misclassifies a soft filter as a time exclusion.
                    if event.epoch_ms.is_none() {
                        stats.time_unknown += 1;
                    }
                    if time_filter {
                        match event.epoch_ms {
                            None => stats.excluded_time += 1,
                            Some(epoch) => {
                                let before = since.map(|s| epoch < s).unwrap_or(false);
                                let after = until.map(|u| epoch >= u).unwrap_or(false);
                                if before || after {
                                    stats.excluded_time += 1;
                                }
                            }
                        }
                    }
                    if !roles.is_empty() {
                        let role = event.role.map(Role::as_str).unwrap_or("");
                        if !roles.iter().any(|r| r == role) {
                            stats.excluded_role += 1;
                        }
                    }
                    if !phases.is_empty() {
                        let phase = event.phase.as_deref().unwrap_or("");
                        if !phases.iter().any(|p| p == phase) {
                            stats.excluded_phase += 1;
                        }
                    }
                }
                for event in events {
                    // Page resumes skip everything up to and including the cursor boundary
                    // *before* selection; otherwise the window fills with already-emitted events
                    // and the true next subevent is never scanned.
                    if let Some(boundary) = &resume_boundary {
                        if (event.source.byte_start, event.source.subevent_index)
                            < (boundary.offset, boundary.subevent_index)
                        {
                            continue;
                        }
                    }
                    // `--event` is a scope-respecting filter, not a privacy override: it only
                    // narrows eligibility, and the normal hard-scope/private rules still apply.
                    if let Some(wanted) = &event_filter_id {
                        if &event.event_id != wanted {
                            continue;
                        }
                    }
                    // Hard scope is the kind range only; soft filters only decide primacy.
                    let hard = in_scope(event.kind, scope);
                    let primary_eligible =
                        hard && matches_soft(&event, &roles, &phases, since, until);
                    selector.feed(event, hard, primary_eligible, &mut |e| {
                        matcher.matches(&e.text)
                    });
                    if selector.can_stop() {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            LineItem::Uninspected { span } => {
                line_counter += 1;
                last_line = line_counter;
                let mut adjusted = span;
                adjusted.raw_line = line_counter;
                // No explicit skip in 0.3: an over-limit record is always reported as
                // `uninspected` and makes the response partial (rc1) without an append cursor.
                stats.uninspected.push(adjusted);
                Ok(false)
            }
            LineItem::PendingTail {
                last_complete_end, ..
            } => {
                stats.pending_tail = true;
                stats.complete_end = Some(last_complete_end);
                Ok(false)
            }
        }
    });
    stats.read_bytes = reader.read_stream_bytes();
    stats.aux_detect_bytes = aux_detect_bytes;
    stats.aux_metadata_bytes = aux_metadata_bytes;
    // The snapshot is only a promise about the file observed at open. A concurrent write during
    // the scan (same length or growth) makes this response partial, never a silent complete.
    if let Ok(end_meta) = std::fs::metadata(&input) {
        let end_mtime = file_mtime_ns(&input).unwrap_or(0);
        let end_generation = generation_of(&end_meta);
        if observable_source_change(
            end_meta.len(),
            effective_snapshot_end,
            end_mtime,
            open_mtime_ns,
            end_generation,
            generation,
        ) {
            stats.source_changed = true;
        }
    }
    // The reader may hold an older boundary than the current file length (page cursor).
    stats.reached_snapshot_end = reader.complete_line_end() >= reader.snapshot_end();
    if let Err(err) = scan {
        return failure("source_unreadable", &err.to_string());
    }
    // A pending (still-being-written) tail is fine for a checkpoint: the append cursor sits at
    // the last complete line. Uninspected/invalid records still block it.
    let scan_complete = stats.uninspected.is_empty()
        && !stats.source_changed
        && (stats.pending_tail || stats.reached_snapshot_end);
    let selection = selector.finish();
    if let Some(wanted) = &event_filter_id {
        if selection.primary_ids.is_empty()
            && stats.uninspected.is_empty()
            && stats.invalid == 0
            && !stats.source_changed
        {
            // Absence is proven only over a fully inspected snapshot.
            return failure(
                "event_not_found",
                &format!("event {wanted} is not present in the declared scope/snapshot"),
            );
        }
        // Otherwise the range was not fully read: fall through to normal rendering, which emits
        // partial metadata, the bounded gap pointer and a real retry cursor (never a false
        // "not found" and never a private body).
    }
    stats.hard_scope_events = selection.hard_scope_events;
    stats.scanned_events = selection.scanned_events;
    stats.coverage = normalizer.coverage.clone();
    stats.mirror_candidates = normalizer.mirror_candidates;
    stats.merged_mirror_pointers = normalizer.merged_mirror_pointers.clone();
    stats.context_dropped = selection.dropped_context;
    stats.time_unknown_excluded = 0;

    // Resume filtering already happened before selection (see `resume_boundary`), so no
    // post-selection retain is needed here.
    let mut events: Vec<crate::query::model::QEvent> = selection.events.clone();

    let selected_primary = selection.primary_ids.clone();
    let mut context_ids = selection.context_ids.clone();
    let mut matches = selection.matches.clone();

    // Tool pairing (at most two extra passes).
    let mut pair_report = None;
    if with_pair {
        // Pair passes read the same frozen boundary as the main scan (old page boundary when
        // resuming, otherwise the snapshot captured at open).
        match correlate(
            &input,
            provider,
            &session_id,
            &source_id,
            &events,
            shared.max_record_bytes,
            0,
            shared.max_events,
            reader.snapshot_end(),
        ) {
            Ok(mut report) => {
                // Counterparts already in the main window need no extra display slot; only the
                // genuinely additional ones do.
                let displayed: std::collections::BTreeSet<String> =
                    events.iter().map(|e| e.event_id.clone()).collect();
                let extra: usize = report
                    .pairs
                    .values()
                    .map(|info| {
                        info.counterpart_event_ids
                            .iter()
                            .filter(|id| !displayed.contains(*id))
                            .count()
                    })
                    .sum();
                if events.len() + extra > shared.max_events {
                    for info in report.pairs.values_mut() {
                        if info.state == Some(PairState::Paired) {
                            info.state = Some(PairState::NotRenderedBudget);
                        }
                        info.counterparts.clear();
                    }
                }
                for info in report.pairs.values() {
                    for counterpart in &info.counterparts {
                        context_ids.insert(counterpart.event_id.clone());
                        matches.remove(&counterpart.event_id);
                        if events.iter().all(|e| e.event_id != counterpart.event_id) {
                            events.push(counterpart.clone());
                        }
                    }
                }
                // Absolute raw order: selected events and re-scanned counterparts come from
                // different scans, so their `sequence` counters are not comparable.
                events.sort_by_key(|e| {
                    (
                        e.source.byte_start,
                        e.source.subevent_index,
                        e.source.raw_line,
                    )
                });
                pair_report = Some(report);
            }
            Err(err) => return failure("source_unreadable", &err.to_string()),
        }
    }

    // Append-cursor eligibility: a complete scan from the start of this invocation's suffix.
    // A page that finishes the old snapshot may issue the append cursor; `render` still
    // withholds it whenever a page continuation remains.
    let append_allowed = scan_complete && stats.invalid == 0;
    // Use the reader's boundary: a page sequence holds the old snapshot even if the file grew,
    // so the append checkpoint must be that old complete-line end, not the current file length.
    let complete_end = stats
        .complete_end
        .unwrap_or_else(|| reader.complete_line_end());
    let complete_line_count = last_line;

    if let Some(report) = &pair_report {
        stats.pair_read_bytes = report.read_bytes;
    }
    // Re-check after the auxiliary passes: a concurrent change during pair scanning must not be
    // reported as a complete answer.
    if let Ok(end_meta) = std::fs::metadata(&input) {
        let end_mtime = file_mtime_ns(&input).unwrap_or(0);
        let end_generation = generation_of(&end_meta);
        if observable_source_change(
            end_meta.len(),
            effective_snapshot_end,
            end_mtime,
            open_mtime_ns,
            end_generation,
            generation,
        ) {
            stats.source_changed = true;
        }
    }
    // Capture before the stats value is moved into render.
    let append_ok_final = append_allowed && !stats.source_changed;
    render(
        &shared,
        kind,
        provider,
        &source_id,
        &session_id,
        snapshot_end,
        generation,
        &filters,
        matcher,
        with_pair,
        &input,
        events,
        selected_primary,
        context_ids.clone(),
        pair_report.clone(),
        matches,
        selection.primary_ids.len() + selection.unrendered_matches as usize,
        stats,
        // A source change observed during (or after) the auxiliary passes invalidates any
        // checkpoint, even though it was computed earlier.
        if append_ok_final {
            Some((&input, complete_end, complete_line_count))
        } else {
            None
        },
        &mut aux_anchor_bytes,
        aux_validation_bytes,
    )
}

#[derive(Default)]
struct ScanStats {
    invalid: u64,
    invalid_samples: Vec<RecordSpan>,
    uninspected: Vec<RecordSpan>,
    pending_tail: bool,
    complete_end: Option<u64>,
    read_bytes: u64,
    hard_scope_events: u64,
    scanned_events: u64,
    empty_records: u64,
    unknown_events: u64,
    unknown_samples: Vec<RecordSpan>,
    time_unknown: u64,
    excluded_time: u64,
    excluded_role: u64,
    excluded_phase: u64,
    framework_excluded: u64,
    merged_mirror_pointers: Vec<(u64, u64, u32)>,
    reached_snapshot_end: bool,
    source_changed: bool,
    aux_detect_bytes: u64,
    aux_metadata_bytes: u64,
    aux_anchor_bytes: u64,
    aux_validation_bytes: u64,
    context_dropped: u64,
    pair_read_bytes: u64,
    time_unknown_excluded: u64,
    coverage: crate::query::normalize::TypeCoverage,
    mirror_candidates: u64,
}

#[allow(clippy::too_many_arguments)]
fn render(
    shared: &Shared,
    _kind: QueryKind,
    provider: Provider,
    source_id: &str,
    session_id: &str,
    snapshot_end: u64,
    generation: Option<crate::query::cursor::Generation>,
    filters: &Filters,
    _matcher: &Matcher,
    _with_pair: bool,
    input: &Path,
    events: Vec<crate::query::model::QEvent>,
    selected_primary: Vec<String>,
    context_ids: BTreeSet<String>,
    pair_report: Option<crate::query::toolpair::PairReport>,
    matches: BTreeMap<String, MatchInfo>,
    selected_count: usize,
    mut stats: ScanStats,
    append_ok: Option<(&Path, u64, u64)>,
    aux_anchor_bytes: &mut u64,
    aux_validation_bytes: u64,
) -> Prepared {
    // No pre-truncation: membership is bounded already (window + context + pairs). Which members
    // fit the display budget is decided in the loop, so no selected hit is silently dropped before
    // the page cursor is computed.
    // Any pair whose counterparts were dropped by the display cap is explicitly budget-limited.
    let displayed_ids: BTreeSet<String> = events.iter().map(|e| e.event_id.clone()).collect();
    let mut pair_report = pair_report;
    if let Some(report) = pair_report.as_mut() {
        for info in report.pairs.values_mut() {
            if info.state == Some(PairState::Paired) && !info.counterpart_event_ids.is_empty() {
                let shown = info
                    .counterpart_event_ids
                    .iter()
                    .filter(|id| displayed_ids.contains(*id))
                    .count();
                if shown == 0 {
                    info.state = Some(PairState::NotRenderedBudget);
                }
            }
        }
    }
    let budget = shared.max_output_bytes;
    let mut rendered_lines: Vec<String> = Vec::new();
    let mut rendered_values: Vec<Value> = Vec::new();
    let mut used = 0usize;
    let mut rendered_primary: BTreeSet<String> = BTreeSet::new();
    let mut next_unrendered_index: Option<usize> = None;
    let page_limit = shared.limit.min(shared.max_events);
    // Reserve one slot per selected primary *before* choosing context/pair members, so leading
    // context or a preceding counterpart can never consume the slot of a later primary.
    let chosen_primaries: Vec<&String> = selected_primary.iter().take(page_limit).collect();
    let chosen_set: BTreeSet<&String> = chosen_primaries.iter().copied().collect();
    let extra_budget = shared.max_events.saturating_sub(chosen_set.len());
    let mut extras_used: usize = 0;
    let mut cut_context: u64 = 0;
    let mut rendered_total: usize = 0;
    for (index, event) in events.iter().enumerate() {
        let is_primary = selected_primary.contains(&event.event_id);
        if is_primary && !chosen_set.contains(&event.event_id) {
            // First unrendered primary => genuine continuation for the next page.
            next_unrendered_index = Some(index);
            break;
        }
        if !is_primary {
            // Context/pair members only fill the slots left after the reserved primaries.
            if extras_used >= extra_budget {
                cut_context += 1;
                continue;
            }
            extras_used += 1;
        }
        let remaining = budget.saturating_sub(METADATA_RESERVE).saturating_sub(used);
        // Share the residual budget across the events still to render: a single huge body must
        // not consume the whole window and starve later events (which would wrongly look absent).
        let remaining_events = events.len().saturating_sub(index).max(1);
        let per_event = remaining / remaining_events;
        let text_cap = per_event.saturating_sub(720).max(64);
        let (value, _complete) = event_to_json(event, text_cap);
        let mut value = value;
        if let Some(object) = value.as_object_mut() {
            if let Some(info) = matches.get(&event.event_id) {
                if !info.queries.is_empty() {
                    object.insert("matched_queries".into(), json!(info.queries));
                    object.insert(
                        "matched_ranges".into(),
                        json!(info.ranges
                            .iter()
                            .map(|(s, e)| json!({"start": s, "end": e, "unit": "utf8_byte", "reference": "original_text"}))
                            .collect::<Vec<_>>()),
                    );
                }
            }
        }
        attach_observed(&mut value, event);
        if let Some(report) = &pair_report {
            if let Some(info) = report.pairs.get(&event.event_id) {
                if let Some(object) = value.as_object_mut() {
                    object.insert(
                        "pair_state".into(),
                        match info.state {
                            Some(state) => json!(state.as_str()),
                            None => Value::Null,
                        },
                    );
                    object.insert("paired_event_ids".into(), json!(info.counterpart_event_ids));
                    object.insert("paired_total".into(), json!(info.total_counterparts));
                    object.insert("paired_ids_truncated".into(), json!(info.ids_truncated));
                    object.insert(
                        "pair_reentry".into(),
                        match info.reentry {
                            Some((line, start)) => json!({"raw_line": line, "byte_start": start}),
                            None => Value::Null,
                        },
                    );
                }
            }
        }
        if context_ids.contains(&event.event_id) {
            if let Some(object) = value.as_object_mut() {
                object.insert("is_context".into(), json!(true));
            }
        }
        let rendered = line(&value);
        if used + rendered.len() + METADATA_RESERVE > budget {
            next_unrendered_index = Some(index);
            break;
        }
        used += rendered.len();
        rendered_total += 1;
        if is_primary {
            rendered_primary.insert(event.event_id.clone());
        }
        debug_assert!(rendered_total <= shared.max_events);
        rendered_lines.push(rendered);
        rendered_values.push(value);
    }

    let mut stdout: String = match shared.output {
        crate::cli::QueryOutput::Jsonl => rendered_lines.concat(),
        crate::cli::QueryOutput::Text => {
            let mut text = String::new();
            for value in &rendered_values {
                text.push_str(&event_to_text(value));
            }
            text
        }
    };
    let page_boundary = next_unrendered_index.and_then(|i| events.get(i));
    let anchor_hex =
        crate::query::reader::anchor_window_counted(input, snapshot_end, aux_anchor_bytes)
            .map(|window| sha256_hex(&window))
            .unwrap_or_default();
    let page_cursor = page_boundary.map(|event| {
        CursorPayload {
            v: CURSOR_VERSION,
            kind: CursorKind::Page,
            provider,
            source_id: source_id.to_string(),
            generation,
            snapshot_end,
            offset: event.source.byte_start,
            raw_line: event.source.raw_line,
            subevent_index: event.source.subevent_index,
            complete_line_count: event.source.raw_line.saturating_sub(1),
            filters: filters.clone(),
            event_filter: shared.event_filter.clone(),
            command: command_name(shared.kind).to_string(),
            session_id: session_id.to_string(),
            queries: shared.queries.clone(),
            regex: shared.regex,
            context: shared.context,
            anchor_hex: anchor_hex.clone(),
            resume_anchor_hex: crate::query::reader::anchor_window_counted(
                input,
                event.source.byte_start,
                aux_anchor_bytes,
            )
            .map(|w| sha256_hex(&w))
            .unwrap_or_default(),
            anchor_len: ANCHOR_WINDOW_BYTES.min(snapshot_end as usize) as u64,
            mtime_ns: file_mtime_ns(input).unwrap_or(0),
        }
        .encode()
    });
    // A snapshot is only fully consumed when there is no page continuation left.
    let append_cursor = if page_cursor.is_some() {
        None
    } else {
        append_ok.map(|(path, complete_end, complete_line_count)| {
            let anchor_hex =
                crate::query::reader::anchor_window_counted(path, complete_end, aux_anchor_bytes)
                    .map(|window| sha256_hex(&window))
                    .unwrap_or_default();
            CursorPayload {
                v: CURSOR_VERSION,
                kind: CursorKind::Append,
                provider,
                source_id: source_id.to_string(),
                generation,
                snapshot_end: complete_end,
                offset: complete_end,
                raw_line: complete_line_count,
                subevent_index: 0,
                complete_line_count,
                filters: filters.clone(),
                event_filter: shared.event_filter.clone(),
                command: command_name(shared.kind).to_string(),
                session_id: session_id.to_string(),
                queries: shared.queries.clone(),
                regex: shared.regex,
                context: shared.context,
                anchor_hex,
                resume_anchor_hex: crate::query::reader::anchor_window_counted(
                    path,
                    complete_end,
                    aux_anchor_bytes,
                )
                .map(|w| sha256_hex(&w))
                .unwrap_or_default(),
                anchor_len: ANCHOR_WINDOW_BYTES.min(complete_end as usize) as u64,
                mtime_ns: file_mtime_ns(path).unwrap_or(0),
            }
            .encode()
        })
    };

    // Actual unknown/parse-error events, not a string heuristic over type keys.
    let unknown_events = stats.unknown_events;
    stats.aux_anchor_bytes = stats.aux_anchor_bytes.saturating_add(*aux_anchor_bytes);
    stats.aux_validation_bytes = aux_validation_bytes;
    let not_rendered_primary = selected_count.saturating_sub(rendered_primary.len());
    let pair_gap = pair_report
        .as_ref()
        .map(|r| r.incomplete || r.uninspected > 0 || r.invalid > 0)
        .unwrap_or(false);
    let status_str =
        if !stats.uninspected.is_empty() || stats.invalid > 0 || stats.source_changed || pair_gap {
            "partial"
        } else {
            "ok"
        };
    let metadata = json!({
        "schema_version": QUERY_SCHEMA_VERSION,
        "record_type": "metadata",
        "status": status_str,
        "provider": provider.as_str(),
        "source": {
            "id": source_id,
            "session_id": session_id,
            "file_name": input.file_name().map(|n| n.to_string_lossy().into_owned()),
            "snapshot_end": snapshot_end,
        },
        "filters": {
            "scope": shared.scope.as_str(),
            "roles": shared.roles,
            "phases": shared.phases,
            "since_ms": shared.since_ms,
            "until_ms": shared.until_ms,
            "event": shared.event_filter,
        },
        "windows": {
            "limit": shared.limit,
            "tail": shared.tail,
            "context": shared.context,
            "max_events": shared.max_events,
            "max_output_bytes": shared.max_output_bytes,
            "max_record_bytes": shared.max_record_bytes,
        },
        "counts": {
            "pairs": match &pair_report {
                Some(report) => json!({
                    "paired": report.pairs.values().filter(|i| i.state == Some(PairState::Paired)).count(),
                    "pending_result": report.pairs.values().filter(|i| i.state == Some(PairState::PendingResult)).count(),
                    "orphan": report.pairs.values().filter(|i| i.state == Some(PairState::Orphan)).count(),
                    "not_rendered_budget": report.pairs.values().filter(|i| i.state == Some(PairState::NotRenderedBudget)).count(),
                    "unknown_pair": report.pairs.values().filter(|i| i.state == Some(PairState::UnknownPair)).count(),
                }),
                None => json!({
                    "paired": 0,
                    "pending_result": 0,
                    "orphan": 0,
                    "not_rendered_budget": 0,
                }),
            },
            "unknown": {
                "events": unknown_events,
                "samples": stats.unknown_samples.iter().map(|s| json!({
                    "raw_line": s.raw_line,
                    "byte_start": s.byte_start,
                    "byte_end": s.byte_end,
                })).collect::<Vec<_>>(),
            },
            "invalid": {
                "records": stats.invalid,
                "samples": stats.invalid_samples.iter().map(|s| json!({"raw_line": s.raw_line, "byte_start": s.byte_start, "byte_end": s.byte_end})).collect::<Vec<_>>(),
            },
            "uninspected": {
                "records": stats.uninspected.len() as u64,
                "samples": stats.uninspected.iter().map(|s| json!({
                    "raw_line": s.raw_line,
                    "byte_start": s.byte_start,
                    "byte_end": s.byte_end,
                    "reason": "max_record_bytes",
                    // Real reusable token: resuming it with a larger --max-record-bytes reads from
                    // this rejected row without re-emitting the processed prefix.
                    "retry_cursor": retry_cursor_token(
                        s,
                        provider,
                        source_id,
                        session_id,
                        generation,
                        snapshot_end,
                        filters,
                        shared,
                        input,
                        aux_anchor_bytes,
                    ),
                    "retry_hint": format!("--max-record-bytes {}", shared.max_record_bytes.saturating_mul(2)),
                })).collect::<Vec<_>>(),
            },
            "skipped_records": 0,
            "framework_excluded": {"events": stats.framework_excluded},
            "time_unknown": {"events": stats.time_unknown},
            "excluded_time": {"events": stats.excluded_time},
            "excluded_role": {"events": stats.excluded_role},
            "excluded_phase": {"events": stats.excluded_phase},
            "mirror_candidates": {"events": stats.mirror_candidates},
            "context_dropped": {"events": stats.context_dropped},
            "merged_mirror_pointers": stats.merged_mirror_pointers.iter().map(|(line, start, sub)| json!({
                "raw_line": line,
                "byte_start": start,
                "subevent_index": sub,
            })).collect::<Vec<_>>(),
        },
        "selected": selected_count,
        "rendered": rendered_primary.len(),
        "not_rendered": {
            "primary_events": not_rendered_primary,
            "context_events": cut_context,
        },
        "scan": {
            "hard_scope_events": stats.hard_scope_events,
            "scanned_events": stats.scanned_events,
            "read_bytes": stats.read_bytes,
            "read_bytes_breakdown": {
                "main": stats.read_bytes,
                "detect": stats.aux_detect_bytes,
                "metadata": stats.aux_metadata_bytes,
                "anchor": stats.aux_anchor_bytes,
                "validation": stats.aux_validation_bytes,
                "pair": stats.pair_read_bytes,
            },
            "total_read_bytes": stats.read_bytes
                + stats.aux_detect_bytes
                + stats.aux_metadata_bytes
                + stats.aux_anchor_bytes
                + stats.aux_validation_bytes
                + stats.pair_read_bytes,
            "coverage": stats.coverage.counts.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<BTreeMap<_, _>>(),
            "coverage_overflow": stats.coverage.overflow,
            "with_pair": pair_report.as_ref().map(|r| json!({
                "passes": r.passes,
                "read_bytes": r.read_bytes,
                "not_rendered": r.not_rendered,
                "incomplete": r.incomplete,
                "uninspected": r.uninspected,
                "invalid": r.invalid,
                "pending_tail": r.pending_tail,
                "gaps": r.gaps.iter().map(|(line, start, end)| json!({
                    "raw_line": line,
                    "byte_start": start,
                    "byte_end": end,
                })).collect::<Vec<_>>(),
            })),
        },
        "pending_tail": stats.pending_tail,
        "source_changed": stats.source_changed,
        "exhausted": page_cursor.is_none() && stats.uninspected.is_empty(),
        "page_cursor": page_cursor,
        "append_cursor": append_cursor,
        "cursor_note": "display cut => text_complete=false; page cursor resumes the fixed snapshot; append cursor reads later appends only",
    });
    match shared.output {
        crate::cli::QueryOutput::Jsonl => stdout.push_str(&line(&metadata)),
        crate::cli::QueryOutput::Text => stdout.push_str(&format!(
            "status={} selected={} rendered={} page_cursor={} append_cursor={}\n",
            metadata.get("status").and_then(Value::as_str).unwrap_or(""),
            metadata
                .get("selected")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            metadata
                .get("rendered")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            page_cursor.as_deref().unwrap_or("-"),
            append_cursor.as_deref().unwrap_or("-"),
        )),
    }
    if stdout.len() > budget {
        return failure(
            "budget_too_small",
            "required metadata envelope exceeds --max-output-bytes",
        );
    }
    Prepared {
        exit: if status_str == "partial" {
            Exit::Failure
        } else {
            Exit::Ok
        },
        stdout,
        stderr: String::new(),
    }
}

/// Build a reusable page token that resumes exactly at a rejected (over-limit) record.
/// Same semantic profile as the current invocation; kind=Page so it is accepted by `--cursor`.
#[allow(clippy::too_many_arguments)]
fn retry_cursor_token(
    span: &RecordSpan,
    provider: Provider,
    source_id: &str,
    session_id: &str,
    generation: Option<crate::query::cursor::Generation>,
    snapshot_end: u64,
    filters: &Filters,
    shared: &Shared,
    input: &Path,
    anchor_bytes: &mut u64,
) -> String {
    let anchor_hex = crate::query::reader::anchor_window_counted(input, snapshot_end, anchor_bytes)
        .map(|w| sha256_hex(&w))
        .unwrap_or_default();
    let resume_anchor_hex =
        crate::query::reader::anchor_window_counted(input, span.byte_start, anchor_bytes)
            .map(|w| sha256_hex(&w))
            .unwrap_or_default();
    CursorPayload {
        v: CURSOR_VERSION,
        kind: CursorKind::Page,
        provider,
        source_id: source_id.to_string(),
        generation,
        snapshot_end,
        offset: span.byte_start,
        raw_line: span.raw_line,
        subevent_index: 0,
        complete_line_count: span.raw_line.saturating_sub(1),
        filters: filters.clone(),
        event_filter: shared.event_filter.clone(),
        command: command_name(shared.kind).to_string(),
        session_id: session_id.to_string(),
        queries: shared.queries.clone(),
        regex: shared.regex,
        context: shared.context,
        anchor_hex,
        resume_anchor_hex,
        anchor_len: ANCHOR_WINDOW_BYTES.min(snapshot_end as usize) as u64,
        mtime_ns: file_mtime_ns(input).unwrap_or(0),
    }
    .encode()
}

fn cursor_failure(problem: CursorProblem) -> Prepared {
    let payload = json!({
        "schema_version": QUERY_SCHEMA_VERSION,
        "record_type": "metadata",
        "status": "error",
        "error": {"type": "cursor_invalid", "reason": problem.reason()},
        "exhausted": false,
    });
    Prepared {
        exit: Exit::Failure,
        stdout: line(&payload),
        stderr: format!("error: cursor_invalid: {}\n", problem.reason()),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_cursor(
    token: &str,
    kind: CursorKind,
    provider: Provider,
    filters: &Filters,
    event_filter: &Option<String>,
    command: &str,
    queries: &[String],
    regex: bool,
    path: &Path,
    source_id: &str,
    snapshot_end: u64,
    generation: Option<crate::query::cursor::Generation>,
    max_record_bytes: u64,
    anchor_bytes: &mut u64,
    validation_bytes: &mut u64,
) -> Result<CursorPayload, CursorProblem> {
    let payload = CursorPayload::decode(token)?;
    if payload.kind != kind {
        return Err(CursorProblem::WrongKind);
    }
    if generation.is_none() || payload.generation.is_none() {
        return Err(CursorProblem::UnsupportedPlatform);
    }
    if payload.provider != provider {
        return Err(CursorProblem::FilterMismatch);
    }
    // A cursor is bound to one source namespace. A different input (or a replaced generation at
    // the same path) must not inherit the old token's identity.
    if payload.source_id != source_id {
        return Err(CursorProblem::GenerationMismatch);
    }
    let expected = Filters {
        scope: filters.scope,
        roles: filters.roles.clone(),
        phases: filters.phases.clone(),
        since_ms: filters.since_ms,
        until_ms: filters.until_ms,
        provider: filters.provider,
    };
    let mut stored = expected.clone();
    stored.provider = filters.provider;
    if payload.filters.normalized() != expected.normalized() {
        return Err(CursorProblem::FilterMismatch);
    }
    if payload.event_filter.as_deref().map(str::to_string) != *event_filter {
        return Err(CursorProblem::FilterMismatch);
    }
    if !payload.command.is_empty() && payload.command != command {
        return Err(CursorProblem::WrongKind);
    }
    // Search semantics are reused automatically; an explicit conflicting --query is rejected.
    // Reuse the stored query set when the caller does not repeat it; only an explicitly
    // repeated, different query is a conflict.
    if !queries.is_empty() && !payload.queries.is_empty() && payload.queries != queries {
        return Err(CursorProblem::FilterMismatch);
    }
    if payload.regex && !payload.queries.is_empty() && payload.regex != regex {
        return Err(CursorProblem::FilterMismatch);
    }
    if payload.generation != generation {
        return Err(CursorProblem::GenerationMismatch);
    }
    match payload.kind {
        CursorKind::Append => {
            if snapshot_end < payload.snapshot_end {
                return Err(CursorProblem::Truncated);
            }
        }
        CursorKind::Page => {
            // Page cursors hold the snapshot they were issued against. Later appends are allowed;
            // truncation is not. Reading is constrained to the old boundary in `run_query`.
            if snapshot_end < payload.snapshot_end {
                return Err(CursorProblem::Truncated);
            }
        }
    }
    if payload.offset > snapshot_end {
        return Err(CursorProblem::OffsetOutOfRange);
    }
    // The resume offset must be a real record boundary inside the stored snapshot. A forged
    // mid-record offset fails closed instead of silently re-reading from the wrong place.
    if payload.offset > 0 {
        if payload.offset > payload.snapshot_end {
            return Err(CursorProblem::OffsetOutOfRange);
        }
        let previous =
            read_byte_before(path, payload.offset).map_err(|_| CursorProblem::Malformed)?;
        *validation_bytes += 1;
        if previous != b'\n' {
            return Err(CursorProblem::OffsetOutOfRange);
        }
    }
    // The subevent index must exist within the raw record at that offset. An append checkpoint
    // may legitimately sit at end-of-snapshot where no record starts yet.
    let subevents = subevents_at(
        path,
        payload.offset,
        &payload.provider,
        max_record_bytes,
        validation_bytes,
    )
    .map_err(|_| CursorProblem::Malformed)?;
    if subevents == 0 {
        if payload.offset != payload.snapshot_end || payload.subevent_index != 0 {
            return Err(CursorProblem::OffsetOutOfRange);
        }
    } else if i64::from(payload.subevent_index) >= subevents {
        return Err(CursorProblem::OffsetOutOfRange);
    }
    // Boundary anchor (old snapshot tail).
    let window =
        crate::query::reader::anchor_window_counted(path, payload.snapshot_end, anchor_bytes)
            .map_err(|_| CursorProblem::Malformed)?;
    let len = (payload.anchor_len as usize).min(window.len());
    let actual = sha256_hex(&window[window.len() - len..]);
    if actual != payload.anchor_hex {
        return Err(CursorProblem::AnchorMismatch);
    }
    // Conservative concurrent-write detection (not identity): a changed mtime since the cursor
    // was issued means the file was rewritten, even if the byte length and anchors happen to
    // match. `mtime_ns == 0` means the platform/filesystem did not provide one.
    // Local resume anchor: catches a same-inode edit immediately before the checkpoint.
    if !payload.resume_anchor_hex.is_empty() {
        let local = crate::query::reader::anchor_window_counted(path, payload.offset, anchor_bytes)
            .map_err(|_| CursorProblem::Malformed)?;
        if sha256_hex(&local) != payload.resume_anchor_hex {
            return Err(CursorProblem::AnchorMismatch);
        }
    }
    // Finally, a same-length rewrite that changed no checked anchor: conservative concurrent
    // modification signal (growth remains the legitimate append case).
    if payload.mtime_ns != 0 && snapshot_end == payload.snapshot_end {
        let current = file_mtime_ns(path).unwrap_or(0);
        if current != 0 && current != payload.mtime_ns {
            // Anchors all match, so the bytes are identical, but the file was replaced:
            // identity cannot be proven, which is the generation_mismatch condition.
            return Err(CursorProblem::GenerationMismatch);
        }
    }
    Ok(payload)
}

fn resolve_input(shared: &Shared) -> Result<Option<PathBuf>, Prepared> {
    if let Some(input) = &shared.input {
        return Ok(Some(input.clone()));
    }
    let Some(session) = &shared.session else {
        return Ok(None);
    };
    let Some((provider_name, session_id)) = session.split_once(':') else {
        return Err(usage("--session must be provider:SESSION_ID"));
    };
    let provider = match provider_name {
        "codex" => Provider::Codex,
        "claude_code" => Provider::ClaudeCode,
        "opencode" => Provider::OpenCode,
        other => return Err(usage(&format!("unknown provider '{other}'"))),
    };
    let mut roots = shared.roots.clone();
    if roots.is_empty() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Err(usage("HOME is not set and no --root given"));
        };
        roots = crate::query::sessions::default_roots(provider, &home);
    }
    if roots.is_empty() {
        return Err(usage(
            "OpenCode run logs need an explicit --root (no guessed database path)",
        ));
    }
    let (candidates, _) = match crate::query::sessions::scan(&roots, provider, None, false, 10_000)
    {
        Ok(result) => result,
        Err(err) => return Err(failure("session_ambiguous", &err.to_string())),
    };
    let matches: Vec<&crate::query::sessions::SessionCandidate> = candidates
        .iter()
        .filter(|c| c.session_id == session_id)
        .collect();
    match matches.len() {
        0 => Err(failure(
            "session_not_found",
            &format!("session {session} not found under the declared roots"),
        )),
        1 => Ok(Some(matches[0].path.clone())),
        n => Err(failure(
            "session_ambiguous",
            &format!("session {session} matches {n} files"),
        )),
    }
}

/// A change is "observable inconsistency" only when it breaks the frozen snapshot: truncation,
/// a same-length rewrite (mtime moved), or a different file generation. Growth is the legitimate
/// append case and is intentionally ignored (the reader stays bounded to the old snapshot).
fn observable_source_change(
    end_len: u64,
    snapshot_end: u64,
    end_mtime: u64,
    open_mtime: u64,
    end_generation: Option<crate::query::cursor::Generation>,
    generation: Option<crate::query::cursor::Generation>,
) -> bool {
    end_len < snapshot_end
        || (end_len == snapshot_end && open_mtime != 0 && end_mtime != open_mtime)
        || (generation.is_some() && end_generation != generation)
}

fn file_mtime_ns(path: &Path) -> std::io::Result<u64> {
    let modified = std::fs::metadata(path)?.modified()?;
    Ok(modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0))
}

fn read_byte_before(path: &Path, offset: u64) -> std::io::Result<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset - 1))?;
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte)?;
    Ok(byte[0])
}

/// Number of normalized subevents produced by the raw record starting at `offset`. A forged
/// subevent index (>= this count) fails closed in cursor validation.
fn subevents_at(
    path: &Path,
    offset: u64,
    provider: &crate::query::model::Provider,
    max_record_bytes: u64,
    validation_bytes: &mut u64,
) -> anyhow::Result<i64> {
    use crate::query::reader::{LineItem, SourceReader};
    // Use the caller's cap so a retry with a raised cap can validate the rejected row.
    let mut reader = SourceReader::open(path, max_record_bytes)?;
    let mut count: i64 = 0;
    let source_id = crate::query::source::file_source_id(path);
    let mut normalizer = Normalizer::new(*provider, String::new());
    let _ = reader.scan_from(offset, |item| match item {
        LineItem::Line { span, bytes } => {
            count = normalizer.normalize(span, &bytes, &source_id).len() as i64;
            Ok(false)
        }
        LineItem::Uninspected { .. } => Ok(false),
        LineItem::PendingTail { .. } => Ok(false),
    })?;
    *validation_bytes += reader.read_stream_bytes();
    Ok(count)
}

// ---------------------------------------------------------------------------
// sessions
// ---------------------------------------------------------------------------

pub fn run_sessions(args: &SessionsArgs) -> i32 {
    emit(prepare_sessions(args))
}

fn prepare_sessions(args: &SessionsArgs) -> Prepared {
    let providers: Vec<Provider> = match args.provider {
        QueryProvider::Codex => vec![Provider::Codex],
        QueryProvider::ClaudeCode => vec![Provider::ClaudeCode],
        QueryProvider::OpenCode => vec![Provider::OpenCode],
        // `all` covers every supported run format. OpenCode has no guessed default root, so it
        // participates only when the caller declares explicit roots (otherwise it would silently
        // scan nothing while claiming "all").
        QueryProvider::All => {
            if args.root.is_empty() {
                vec![Provider::Codex, Provider::ClaudeCode]
            } else {
                vec![Provider::Codex, Provider::ClaudeCode, Provider::OpenCode]
            }
        }
    };
    let budget = args.max_output_bytes.unwrap_or(DEFAULT_OUTPUT_BYTES);
    if budget < MIN_OUTPUT_BYTES {
        return usage(&format!(
            "--max-output-bytes {budget} is below the minimum {MIN_OUTPUT_BYTES}"
        ));
    }
    let roots: Vec<PathBuf> = if args.root.is_empty() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return usage("HOME is not set and no --root given");
        };
        let mut roots = Vec::new();
        for provider in &providers {
            for root in crate::query::sessions::default_roots(*provider, &home) {
                if root.exists() {
                    roots.push(root);
                }
            }
        }
        roots
    } else {
        args.root.clone()
    };

    let mut candidates = Vec::new();
    let mut reports = Vec::new();
    for provider in &providers {
        match crate::query::sessions::scan(
            &roots,
            *provider,
            args.cwd.as_deref(),
            args.cwd_children,
            args.scan_limit,
        ) {
            Ok((found, report)) => {
                candidates.extend(found);
                reports.push(report);
            }
            Err(err) => return failure("session_ambiguous", &err.to_string()),
        }
    }
    candidates.sort_by(|a, b| {
        b.last_epoch_ms
            .cmp(&a.last_epoch_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    let total_candidates = candidates.len();
    candidates.truncate(args.limit);

    let partial = reports.iter().any(|r| r.partial);
    let mut stdout = String::new();
    let mut rendered = 0usize;
    for candidate in &candidates {
        let record = json!({
            "schema_version": QUERY_SCHEMA_VERSION,
            "record_type": "session",
            "provider": candidate.provider.as_str(),
            "session_id": candidate.session_id,
            "source_id": candidate.source_id,
            "started_at": candidate.started_at,
            "last_event_at": candidate.last_event_at,
            "cwd": candidate.cwd,
            "title": truncate_escaped(candidate.title.as_deref().unwrap_or(""), 200),
        });
        let rendered_line = line(&record);
        if stdout.len() + rendered_line.len() + METADATA_RESERVE > budget {
            break;
        }
        stdout.push_str(&rendered_line);
        rendered += 1;
    }
    let metadata = json!({
        "schema_version": QUERY_SCHEMA_VERSION,
        "record_type": "metadata",
        "status": if partial { "partial" } else { "ok" },
        "scan": {
            "roots": roots.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "files_scanned": reports.iter().map(|r| r.files_scanned).sum::<u64>(),
            "scan_limit": args.scan_limit,
            "scan_limit_reached": reports.iter().any(|r| r.scan_limit_reached),
            "files_unreadable": reports.iter().map(|r| r.files_unreadable).sum::<u64>(),
            "files_skipped_no_id": reports.iter().map(|r| r.files_skipped_no_id).sum::<u64>(),
            "records_uninspected": reports.iter().map(|r| r.records_uninspected).sum::<u64>(),
            "records_invalid": reports.iter().map(|r| r.records_invalid).sum::<u64>(),
            "coverage_note": "record counts are from each file's bounded metadata window; unread ranges are not claimed complete",
        },
        "cwd": args.cwd,
        "cwd_children": args.cwd_children,
        "limit": args.limit,
        "candidates": total_candidates,
        "rendered": rendered,
        "not_rendered": total_candidates.saturating_sub(rendered),
        "coverage_note": "declared roots only; mtime is not execution state",
    });
    stdout.push_str(&line(&metadata));
    if stdout.len() > budget {
        return failure("budget_too_small", "sessions metadata exceeds the budget");
    }
    Prepared {
        exit: if partial { Exit::Failure } else { Exit::Ok },
        stdout,
        stderr: String::new(),
    }
}

fn truncate_escaped(text: &str, limit: usize) -> String {
    let (cut, _) = cut_utf8(text, limit);
    escape_controls(cut)
}
