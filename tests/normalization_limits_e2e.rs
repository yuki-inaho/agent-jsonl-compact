//! S-1/S-6 regressions for record normalization limits.
//!
//! S-1: the pre-draft bound used to walk every `name`/`id` key of the raw record, including
//! arbitrary tool input, and dropped otherwise valid records. The bound is now context-aware:
//! it covers the metadata fields that are actually copied into a `QEvent` (session id, call id,
//! origin id, tool name, timestamp, phase), while tool input stays body text bounded by the
//! subevent/byte budget.
//!
//! S-6: an `isMeta:true` user record that carries a `tool_result` block must keep the block so
//! `--with-pair` does not assert a missing result. Framework provenance stays on the event.
//!
//! All fixtures are generated here; no real session log is read.

mod viewer_support;

use serde_json::{json, Value};
use std::path::Path;
use viewer_support::*;

/// Mirrors the product limits under test (asserted through observable behaviour, not internals).
const ID_LIMIT: usize = 512;
const TIMESTAMP_LIMIT: usize = 128;
const PHASE_LIMIT: usize = 64;

fn view_jsonl(sandbox: &Sandbox, path: &Path, extra: &[&str]) -> Output {
    let path = path.to_str().expect("utf-8 fixture path").to_string();
    let mut args = vec!["view", "-i", path.as_str(), "--output", "jsonl"];
    args.extend_from_slice(extra);
    run(sandbox, &args)
}

fn invalid_records(meta: &Value) -> i64 {
    count_field(meta, &["counts", "invalid"], "records")
}

fn pair_count(meta: &Value, key: &str) -> i64 {
    count_field(meta, &["counts", "pairs"], key)
}

fn event_of_kind(out: &Output, kind: &str) -> Value {
    let events = out.events();
    let matches: Vec<&Value> = events.iter().filter(|e| ev_kind(e) == kind).collect();
    assert_eq!(
        matches.len(),
        1,
        "exactly one {kind} event expected: {:?}",
        events
            .iter()
            .map(|e| (ev_kind(e), ev_text(e)))
            .collect::<Vec<_>>()
    );
    matches[0].clone()
}

/// One accepted value at `limit`, one rejected value at `limit + 1`.
///
/// The rejected record must surface as `partial` with exactly one invalid record, and the
/// accepted record must produce a complete `ok` view with no invalid records.
fn assert_metadata_boundary(
    sandbox: &Sandbox,
    label: &str,
    format: &str,
    at_limit: String,
    over_limit: String,
) {
    let path = sandbox.write(&format!("{label}-at-limit.jsonl"), &at_limit);
    let out = view_jsonl(sandbox, &path, &["--format", format, "--scope", "all"]);
    assert_eq!(
        out.code, 0,
        "{label} at the limit must be accepted: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok", "{label} at the limit: {meta:#?}");
    assert_eq!(invalid_records(&meta), 0, "{label} at the limit: {meta:#?}");

    let path = sandbox.write(&format!("{label}-over-limit.jsonl"), &over_limit);
    let out = view_jsonl(sandbox, &path, &["--format", format, "--scope", "all"]);
    assert_eq!(
        out.code, 1,
        "{label} over the limit must be rejected: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(
        status(&meta),
        "partial",
        "{label} over the limit: {meta:#?}"
    );
    assert_eq!(
        invalid_records(&meta),
        1,
        "{label} over the limit: {meta:#?}"
    );
}

// ---------------------------------------------------------------------------
// S-1 reproducer: tool input is not metadata
// ---------------------------------------------------------------------------

#[test]
fn s1_long_tool_input_name_keeps_the_record() {
    let sandbox = Sandbox::new();
    let long_name = "n".repeat(600);
    let body = format!(
        "{}{}",
        line(&claude_tool_use(
            "s1",
            "u1",
            "toolu_1",
            "mcp__big__lookup",
            json!({"name": long_name}),
            "2026-01-01T00:00:00Z",
        )),
        line(&claude_assistant_text(
            "s1",
            "u2",
            "plain answer",
            "2026-01-01T00:00:01Z",
        )),
    );
    assert!(
        body.len() < 4096,
        "reproducer must stay far below the 1 MiB record budget"
    );
    let input = sandbox.write("long-input-name.jsonl", &body);

    let out = view_jsonl(&sandbox, &input, &["--scope", "all"]);
    assert_eq!(
        out.code, 0,
        "a long key inside tool input must not reject the record: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok", "{meta:#?}");
    assert_eq!(invalid_records(&meta), 0, "{meta:#?}");

    let call = event_of_kind(&out, "tool_call");
    assert_eq!(call.get("call_id").and_then(Value::as_str), Some("toolu_1"));
    assert_eq!(
        call.get("tool_name").and_then(Value::as_str),
        Some("mcp__big__lookup")
    );
    assert!(
        ev_text(&call).contains(&long_name),
        "tool input must stay body text: {}",
        ev_text(&call)
    );
    let answer = event_of_kind(&out, "assistant");
    assert_eq!(ev_text(&answer), "plain answer");
}

#[test]
fn s1_nested_tool_input_is_body_text_for_codex_and_opencode() {
    let sandbox = Sandbox::new();
    let long_name = "n".repeat(600);

    let codex = sandbox.write(
        "codex-mcp-input.jsonl",
        &line(&json!({
            "timestamp": "2026-01-01T00:00:00Z",
            "type": "event_msg",
            "payload": {
                "type": "mcp_tool_call_end",
                "call_id": "call_codex",
                "invocation": {
                    "server": "mcp",
                    "tool": "lookup",
                    "arguments": {"name": long_name}
                }
            }
        })),
    );
    let out = view_jsonl(&sandbox, &codex, &["--format", "codex", "--scope", "tools"]);
    assert_eq!(out.code, 0, "codex tool arguments are body text: {out:#?}");
    assert_eq!(invalid_records(&out.metadata()), 0);
    let call = event_of_kind(&out, "tool_call");
    assert_eq!(
        call.get("tool_name").and_then(Value::as_str),
        Some("lookup")
    );
    assert!(ev_text(&call).contains(&long_name));

    let opencode = sandbox.write(
        "opencode-input.jsonl",
        &line(&json!({
            "type": "tool_use",
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionID": "oc1",
            "part": {
                "type": "tool",
                "callID": "call_oc",
                "tool": "read",
                "state": {"input": {"name": long_name}, "status": "running"}
            }
        })),
    );
    let out = view_jsonl(
        &sandbox,
        &opencode,
        &["--format", "opencode", "--scope", "tools"],
    );
    assert_eq!(out.code, 0, "opencode tool input is body text: {out:#?}");
    assert_eq!(invalid_records(&out.metadata()), 0);
    let call = event_of_kind(&out, "tool_call");
    assert_eq!(call.get("tool_name").and_then(Value::as_str), Some("read"));
    assert!(ev_text(&call).contains(&long_name));
}

// ---------------------------------------------------------------------------
// Boundary tests for genuine metadata fields, per provider
// ---------------------------------------------------------------------------

#[test]
fn codex_metadata_fields_are_bounded_at_their_real_paths() {
    let sandbox = Sandbox::new();
    let message = |phase: Option<String>, ts: &str| {
        let mut payload = json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]
        });
        if let Some(phase) = phase {
            payload["phase"] = Value::String(phase);
        }
        line(&json!({"timestamp": ts, "type": "response_item", "payload": payload}))
    };
    let call = |name: &str, call_id: &str| {
        line(&json!({
            "timestamp": "2026-01-01T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": name,
                "call_id": call_id,
                "arguments": "{}"
            }
        }))
    };

    assert_metadata_boundary(
        &sandbox,
        "codex-timestamp",
        "codex",
        message(None, &"T".repeat(TIMESTAMP_LIMIT)),
        message(None, &"T".repeat(TIMESTAMP_LIMIT + 1)),
    );
    assert_metadata_boundary(
        &sandbox,
        "codex-phase",
        "codex",
        message(Some("p".repeat(PHASE_LIMIT)), "2026-01-01T00:00:00Z"),
        message(Some("p".repeat(PHASE_LIMIT + 1)), "2026-01-01T00:00:00Z"),
    );
    assert_metadata_boundary(
        &sandbox,
        "codex-call-id",
        "codex",
        call("lookup", &"c".repeat(ID_LIMIT)),
        call("lookup", &"c".repeat(ID_LIMIT + 1)),
    );
    assert_metadata_boundary(
        &sandbox,
        "codex-tool-name",
        "codex",
        call(&"n".repeat(ID_LIMIT), "c1"),
        call(&"n".repeat(ID_LIMIT + 1), "c1"),
    );
}

#[test]
fn claude_metadata_fields_are_bounded_at_their_real_paths() {
    let sandbox = Sandbox::new();
    let user = |session: &str, ts: &str| line(&claude_user(session, "u1", "hello", ts));
    let tool_call = |tool_id: &str, name: &str| {
        line(&claude_tool_use(
            "c",
            "u1",
            tool_id,
            name,
            json!({"command": "ls"}),
            "2026-01-01T00:00:00Z",
        ))
    };
    let tool_result = |tool_id: &str| {
        line(&json!({
            "type": "user",
            "sessionId": "c",
            "uuid": "u2",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": tool_id, "is_error": false,
                 "content": [{"type": "text", "text": "ok"}]}
            ]}
        }))
    };

    assert_metadata_boundary(
        &sandbox,
        "claude-timestamp",
        "claude_code",
        user("c", &"T".repeat(TIMESTAMP_LIMIT)),
        user("c", &"T".repeat(TIMESTAMP_LIMIT + 1)),
    );
    assert_metadata_boundary(
        &sandbox,
        "claude-session-id",
        "claude_code",
        user(&"s".repeat(ID_LIMIT), "2026-01-01T00:00:00Z"),
        user(&"s".repeat(ID_LIMIT + 1), "2026-01-01T00:00:00Z"),
    );
    assert_metadata_boundary(
        &sandbox,
        "claude-tool-name",
        "claude_code",
        tool_call("toolu_1", &"n".repeat(ID_LIMIT)),
        tool_call("toolu_1", &"n".repeat(ID_LIMIT + 1)),
    );
    assert_metadata_boundary(
        &sandbox,
        "claude-tool-id",
        "claude_code",
        tool_call(&"i".repeat(ID_LIMIT), "lookup"),
        tool_call(&"i".repeat(ID_LIMIT + 1), "lookup"),
    );
    assert_metadata_boundary(
        &sandbox,
        "claude-tool-result-id",
        "claude_code",
        tool_result(&"i".repeat(ID_LIMIT)),
        tool_result(&"i".repeat(ID_LIMIT + 1)),
    );
}

#[test]
fn opencode_metadata_fields_are_bounded_at_their_real_paths() {
    let sandbox = Sandbox::new();
    let text = |session: &str| {
        line(&json!({
            "type": "text",
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionID": session,
            "part": {"type": "text", "text": "hello"}
        }))
    };
    let tool = |tool: &str, call_id: &str| {
        line(&json!({
            "type": "tool_use",
            "timestamp": "2026-01-01T00:00:00Z",
            "sessionID": "oc1",
            "part": {
                "type": "tool",
                "callID": call_id,
                "tool": tool,
                "state": {"input": {}, "status": "running"}
            }
        }))
    };

    assert_metadata_boundary(
        &sandbox,
        "opencode-session-id",
        "opencode",
        text(&"s".repeat(ID_LIMIT)),
        text(&"s".repeat(ID_LIMIT + 1)),
    );
    assert_metadata_boundary(
        &sandbox,
        "opencode-tool-name",
        "opencode",
        tool(&"t".repeat(ID_LIMIT), "call_1"),
        tool(&"t".repeat(ID_LIMIT + 1), "call_1"),
    );
    assert_metadata_boundary(
        &sandbox,
        "opencode-call-id",
        "opencode",
        tool("read", &"c".repeat(ID_LIMIT)),
        tool("read", &"c".repeat(ID_LIMIT + 1)),
    );
}

// ---------------------------------------------------------------------------
// S-6: framework-marked tool results stay evidence
// ---------------------------------------------------------------------------

#[test]
fn s6_framework_marked_tool_result_is_paired_with_provenance() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}",
        line(&claude_tool_use(
            "s6",
            "m1",
            "toolu_9",
            "Bash",
            json!({"command": "ls"}),
            "2026-01-01T00:00:00Z",
        )),
        line(&json!({
            "type": "user",
            "sessionId": "s6",
            "uuid": "m2",
            "timestamp": "2026-01-01T00:00:01Z",
            "isMeta": true,
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_9", "is_error": false,
                 "content": [{"type": "text", "text": "file-a"}]}
            ]}
        })),
        line(&claude_assistant_text(
            "s6",
            "m3",
            "done",
            "2026-01-01T00:00:02Z",
        )),
    );
    let input = sandbox.write("is-meta-tool-result.jsonl", &body);

    let out = view_jsonl(&sandbox, &input, &["--scope", "tools", "--with-pair"]);
    assert_eq!(
        out.code, 0,
        "the framework-marked result exists and must not be a parse failure: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok", "{meta:#?}");
    assert_eq!(invalid_records(&meta), 0, "{meta:#?}");
    assert_eq!(
        pair_count(&meta, "pending_result"),
        0,
        "a result that exists must not be reported missing: {meta:#?}"
    );
    assert_eq!(pair_count(&meta, "paired"), 2, "{meta:#?}");
    assert_eq!(
        count_field(&meta, &["counts", "framework_excluded"], "events"),
        1,
        "framework provenance must still be disclosed: {meta:#?}"
    );

    let result = event_of_kind(&out, "tool_output");
    assert_eq!(ev_text(&result), "file-a");
    assert_eq!(
        result.get("message_origin").and_then(Value::as_str),
        Some("framework"),
        "framework provenance is preserved, not rewritten to human/unknown"
    );
    assert_eq!(
        result.get("pair_state").and_then(Value::as_str),
        Some("paired")
    );
    assert_eq!(
        result.get("call_id").and_then(Value::as_str),
        Some("toolu_9")
    );

    let call = event_of_kind(&out, "tool_call");
    assert_eq!(
        call.get("pair_state").and_then(Value::as_str),
        Some("paired"),
        "the call side must see the result: {call:#?}"
    );
}

#[test]
fn s6_text_only_meta_stays_private_framework() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}",
        line(&claude_framework_meta(
            "s6b",
            "u-meta",
            "2026-01-01T00:00:00Z",
        )),
        line(&claude_user(
            "s6b",
            "u1",
            "visible prompt",
            "2026-01-01T00:00:01Z",
        )),
    );
    let input = sandbox.write("meta-text-only.jsonl", &body);

    let out = view_jsonl(&sandbox, &input, &[]);
    assert_eq!(out.code, 0, "{out:#?}");
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok", "{meta:#?}");
    assert_eq!(
        count_field(&meta, &["counts", "framework_excluded"], "events"),
        1,
        "text-only framework injection is still excluded: {meta:#?}"
    );
    let events = out.events();
    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(ev_kind(&events[0]), "user");
    assert_eq!(ev_text(&events[0]), "visible prompt");
}
