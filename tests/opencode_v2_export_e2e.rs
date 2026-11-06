//! OpenCode v2 session-export acceptance tests and boundary regressions.
//!
//! The official export is one JSON document (`info` + `messages`), not JSONL. All fixtures here
//! are synthetic and generated in the test; no real session, token, or personal path is read.
//! Expectations are derived from the fixture bytes and the declared contract, not from the
//! production normalizer.

mod viewer_support;

use serde_json::{json, Value};
use std::fs;
use viewer_support::*;

fn export_doc(messages: Vec<Value>, session: &str) -> Value {
    json!({
        "info": {
            "id": session,
            "projectID": "proj_synthetic",
            "title": "synthetic export",
            "time": {"created": 1_700_000_000_000u64}
        },
        "messages": messages
    })
}

fn export_text(messages: Vec<Value>, session: &str, pretty: bool) -> String {
    let doc = export_doc(messages, session);
    if pretty {
        format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
    } else {
        serde_json::to_string(&doc).unwrap()
    }
}

fn user(id: &str, text: &str, created: i64) -> Value {
    json!({"id": id, "type": "user", "time": {"created": created}, "text": text})
}

fn assistant(id: &str, content: Vec<Value>, created: i64) -> Value {
    json!({
        "id": id,
        "type": "assistant",
        "time": {"created": created},
        "content": content
    })
}

fn text_part(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn reasoning_part(text: &str) -> Value {
    json!({"type": "reasoning", "text": text})
}

fn tool_completed(call_id: &str, name: &str, input: Value, output: &str) -> Value {
    json!({
        "type": "tool",
        "id": call_id,
        "name": name,
        "state": {
            "status": "completed",
            "input": input,
            "content": [{"type": "text", "text": output}]
        }
    })
}

fn tool_error(call_id: &str, name: &str, input: Value, message: &str) -> Value {
    json!({
        "type": "tool",
        "id": call_id,
        "name": name,
        "state": {
            "status": "error",
            "input": input,
            "error": {"type": "permission", "message": message}
        }
    })
}

fn tool_running(call_id: &str, name: &str, input: Value) -> Value {
    json!({
        "type": "tool",
        "id": call_id,
        "name": name,
        "state": {"status": "running", "input": input, "metadata": {}}
    })
}

fn tool_streaming(call_id: &str, name: &str, partial: &str) -> Value {
    json!({
        "type": "tool",
        "id": call_id,
        "name": name,
        "state": {"status": "streaming", "input": partial}
    })
}

fn write(sandbox: &Sandbox, name: &str, body: &str) -> String {
    sandbox
        .write(name, body)
        .to_str()
        .expect("utf-8 path")
        .to_string()
}

fn cursor_of(out: &Output) -> String {
    out.metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .expect("page cursor")
        .to_string()
}

fn signature(out: &Output) -> Vec<(String, Value, Value, Value)> {
    out.events()
        .into_iter()
        .map(|event| {
            (
                event
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                event.get("role").cloned().unwrap_or(Value::Null),
                event.get("text").cloned().unwrap_or(Value::Null),
                event
                    .get("source")
                    .and_then(|source| source.get("subevent_index"))
                    .cloned()
                    .unwrap_or(Value::Null),
            )
        })
        .collect()
}

// T1 -----------------------------------------------------------------------

#[test]
fn t1_auto_and_explicit_detect_compact_pretty_and_no_final_newline() {
    let sandbox = Sandbox::new();
    let messages = vec![
        user("m1", "AJC_T1_USER", 1_700_000_000_000),
        assistant("m2", vec![text_part("AJC_T1_ASSISTANT")], 1_700_000_001_000),
    ];
    let compact = export_text(messages.clone(), "ses_t1_compact", false);
    let pretty = export_text(messages.clone(), "ses_t1_pretty", true);
    let no_final = pretty.trim_end().to_string();
    let cases = [
        ("compact", compact, "ses_t1_compact"),
        ("pretty", pretty, "ses_t1_pretty"),
        ("no-final", no_final, "ses_t1_pretty"),
    ];
    let mut expected: Option<Vec<(String, Value, Value, Value)>> = None;
    for (name, body, session) in cases {
        let path = write(&sandbox, &format!("{name}.json"), &body);
        let auto = ok(
            &sandbox,
            &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
        );
        let explicit = ok(
            &sandbox,
            &[
                "view", "-i", &path, "--format", "opencode", "--scope", "all", "--output", "jsonl",
            ],
        );
        let auto_sig = signature(&auto);
        assert_eq!(
            auto_sig,
            signature(&explicit),
            "auto != explicit for {name}"
        );
        assert_eq!(
            auto.metadata()["source"]["input_kind"].as_str(),
            Some("opencode_export"),
            "{name}"
        );
        assert_eq!(
            auto.metadata()["source"]["session_id"].as_str(),
            Some(session),
            "{name}"
        );
        if let Some(previous) = &expected {
            assert_eq!(previous, &auto_sig, "variant {name} changed the events");
        } else {
            expected = Some(auto_sig);
        }
    }

    // Existing unknown-only run JSONL diagnosis stays distinct from an export.
    let unknown = sandbox.write("unknown.jsonl", "{\"kind\":\"mystery\"}\n");
    let auto = run(
        &sandbox,
        &["view", "-i", unknown.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(auto.code, 1);
    assert_eq!(
        auto.metadata()["error"]["type"].as_str(),
        Some("format_uncertain")
    );
    let explicit = ok(
        &sandbox,
        &[
            "view",
            "-i",
            unknown.to_str().unwrap(),
            "--format",
            "opencode",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(explicit.metadata()["counts"]["unknown"]["events"], 1);
}

// T2 -----------------------------------------------------------------------

#[test]
fn t2_duplicate_text_keeps_order_role_and_each_message() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            user("m1", "same text", 1_700_000_000_000),
            assistant("m2", vec![text_part("same text")], 1_700_000_001_000),
            user("m3", "same text", 1_700_000_002_000),
            assistant("m4", vec![text_part("same text")], 1_700_000_003_000),
        ],
        "ses_t2",
        true,
    );
    let path = write(&sandbox, "t2.json", &body);
    let out = ok(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "messages", "--output", "jsonl",
        ],
    );
    let events = out.events();
    assert_eq!(events.len(), 4);
    assert_eq!(
        events
            .iter()
            .map(|event| event["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["user", "assistant", "user", "assistant"]
    );
    assert!(events.iter().all(|event| event["text"] == "same text"));
    let ids: Vec<&str> = events
        .iter()
        .map(|event| event["event_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        4
    );
}

// T3 -----------------------------------------------------------------------

#[test]
fn t3_tool_states_call_result_error_and_pending_are_distinct() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            assistant(
                "m1",
                vec![tool_completed(
                    "call_ok",
                    "read",
                    json!({"path": "a.txt"}),
                    "file body",
                )],
                1_700_000_000_000,
            ),
            assistant(
                "m2",
                vec![tool_error(
                    "call_err",
                    "read",
                    json!({"path": "b.txt"}),
                    "permission denied",
                )],
                1_700_000_001_000,
            ),
            assistant(
                "m3",
                vec![tool_running("call_run", "read", json!({"path": "c.txt"}))],
                1_700_000_002_000,
            ),
            assistant(
                "m4",
                vec![tool_streaming("call_stream", "read", "{\"path\":")],
                1_700_000_003_000,
            ),
        ],
        "ses_t3",
        true,
    );
    let path = write(&sandbox, "t3.json", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();
    let calls: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "tool_call")
        .collect();
    let outputs: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "tool_output")
        .collect();
    assert_eq!(calls.len(), 4, "{events:#?}");
    assert_eq!(outputs.len(), 2, "{events:#?}");
    let state = |id: &str| {
        events
            .iter()
            .find(|event| event["call_id"] == id && event["kind"] == "tool_call")
            .map(|event| event["pair_state"].as_str().unwrap_or(""))
            .unwrap()
    };
    assert_eq!(state("call_ok"), "paired");
    assert_eq!(state("call_err"), "paired");
    assert_eq!(state("call_run"), "pending_result");
    assert_eq!(state("call_stream"), "pending_result");
    let error = outputs
        .iter()
        .find(|event| event["call_id"] == "call_err")
        .unwrap();
    assert_eq!(error["tool_observed"]["is_error"], true);
    assert!(error["text"]
        .as_str()
        .unwrap()
        .contains("permission denied"));
    assert!(calls
        .iter()
        .all(|event| event["source"]["subevent_index"].as_u64().is_some()));
}

// T4 -----------------------------------------------------------------------

#[test]
fn t4_private_generated_content_is_not_public_conversation() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            assistant(
                "m1",
                vec![
                    reasoning_part("SECRET_REASONING"),
                    text_part("VISIBLE_ANSWER"),
                ],
                1_700_000_000_000,
            ),
            json!({"id": "m2", "type": "system", "time": {"created": 1_700_000_001_000i64}, "text": "SECRET_SYSTEM"}),
            json!({"id": "m3", "type": "synthetic", "time": {"created": 1_700_000_002_000i64}, "text": "SECRET_SYNTHETIC"}),
            json!({"id": "m4", "type": "skill", "time": {"created": 1_700_000_003_000i64}, "skill": "s1", "name": "n", "text": "SECRET_SKILL"}),
            json!({"id": "m5", "type": "compaction", "status": "completed", "reason": "auto", "time": {"created": 1_700_000_004_000i64}, "summary": "SECRET_COMPACTION"}),
        ],
        "ses_t4",
        true,
    );
    let path = write(&sandbox, "t4.json", &body);
    let out = ok(
        &sandbox,
        &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
    );
    let stdout = &out.stdout;
    for secret in [
        "SECRET_REASONING",
        "SECRET_SYSTEM",
        "SECRET_SYNTHETIC",
        "SECRET_SKILL",
        "SECRET_COMPACTION",
    ] {
        assert!(
            !stdout.contains(secret),
            "{secret} leaked into public output"
        );
    }
    assert!(stdout.contains("VISIBLE_ANSWER"));
    let meta = out.metadata();
    assert_eq!(meta["counts"]["framework_excluded"]["events"], 4);
    let coverage = &meta["scan"]["coverage"];
    assert_eq!(coverage["opencode_export/assistant"], 1);
    assert_eq!(coverage["opencode_export/system"], 1);
    assert_eq!(coverage["opencode_export/synthetic"], 1);
    assert_eq!(coverage["opencode_export/skill"], 1);
    assert_eq!(coverage["opencode_export/compaction"], 1);
}

// T5 -----------------------------------------------------------------------

#[test]
fn t5_unicode_search_context_role_and_time_use_original_text() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            user("m1", "トマトの収穫量は7個", 1_700_000_000_000),
            assistant(
                "m2",
                vec![text_part("トマトを確認しました")],
                1_700_000_001_000,
            ),
            user("m3", "別の話題", 1_800_000_000_000),
        ],
        "ses_t5",
        true,
    );
    let path = write(&sandbox, "t5.json", &body);
    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "トマト",
            "--scope",
            "messages",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();
    assert!(events
        .iter()
        .any(|event| event["text"] == "トマトの収穫量は7個"));
    assert!(events
        .iter()
        .any(|event| event["text"] == "トマトを確認しました"));
    for event in &events {
        if let Some(ranges) = event.get("matched_ranges").and_then(Value::as_array) {
            assert!(!ranges.is_empty());
            for range in ranges {
                assert_eq!(range["unit"], "utf8_byte");
                let text = event["text"].as_str().unwrap();
                let start = range["start"].as_u64().unwrap() as usize;
                let end = range["end"].as_u64().unwrap() as usize;
                assert_eq!(&text[start..end], "トマト");
            }
        }
    }
    // Metadata (session id) is not body text.
    let metadata_hit = ok(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "ses_t5", "--scope", "messages", "--output", "jsonl",
        ],
    );
    assert_eq!(metadata_hit.metadata()["selected"], 0);

    let filtered = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "messages",
            "--role",
            "assistant",
            "--since",
            "2023-11-14T22:13:19Z",
            "--until",
            "2023-11-14T22:13:22Z",
            "--output",
            "jsonl",
        ],
    );
    let events = filtered.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["text"], "トマトを確認しました");
    assert_eq!(events[0]["epoch_ms"], 1_700_000_001_000i64);
    assert_eq!(filtered.metadata()["counts"]["time_unknown"]["events"], 0);
}

// T6 -----------------------------------------------------------------------

#[test]
fn t6_event_source_ranges_point_at_original_message_elements() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            user(
                "m1",
                "escaped \"quote\" and \\ backslash",
                1_700_000_000_000,
            ),
            assistant(
                "m2",
                vec![
                    text_part("AJC_T6_本文"),
                    tool_completed("call_1", "read", json!({"path": "x"}), "result"),
                ],
                1_700_000_001_000,
            ),
            user("m3", "最終", 1_700_000_002_000),
        ],
        "ses_t6",
        true,
    );
    let path = write(&sandbox, "t6.json", &body);
    let out = ok(
        &sandbox,
        &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
    );
    let bytes = body.as_bytes();
    for event in out.events() {
        let source = &event["source"];
        let start = source["byte_start"].as_u64().unwrap() as usize;
        let end = source["byte_end"].as_u64().unwrap() as usize;
        assert!(start < end && end <= bytes.len());
        let slice = std::str::from_utf8(&bytes[start..end]).unwrap();
        let value: Value = serde_json::from_str(slice).unwrap();
        assert!(
            value.is_object(),
            "slice is not one message element: {slice}"
        );
        let expected_line = bytes[..start].iter().filter(|byte| **byte == b'\n').count() as u64 + 1;
        assert_eq!(source["raw_line"], expected_line);
        assert_eq!(source["raw_line_verified"], true);
        assert!(value.get("type").is_some());
    }
    let again = ok(
        &sandbox,
        &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
    );
    let ids = |out: &Output| {
        out.events()
            .into_iter()
            .map(|event| event["event_id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&out), ids(&again));
}

// T7 -----------------------------------------------------------------------

#[test]
fn t7_single_element_pages_have_no_gaps_or_duplicates_and_no_append_cursor() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            user("m1", "first", 1_700_000_000_000),
            assistant(
                "m2",
                vec![
                    text_part("second"),
                    tool_completed("call_1", "read", json!({"path": "x"}), "third"),
                ],
                1_700_000_001_000,
            ),
            user("m3", "fourth", 1_700_000_002_000),
        ],
        "ses_t7",
        true,
    );
    let path = write(&sandbox, "t7.json", &body);
    let one_shot = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "all",
            "--limit",
            "1000",
            "--max-events",
            "1000",
            "--output",
            "jsonl",
        ],
    );
    let expected: Vec<(String, Value, Value)> = one_shot
        .events()
        .into_iter()
        .map(|event| {
            (
                event["kind"].as_str().unwrap().to_string(),
                event["source"]["byte_start"].clone(),
                event["source"]["subevent_index"].clone(),
            )
        })
        .collect();
    assert!(expected.len() >= 4);

    let mut seen: Vec<(String, Value, Value)> = Vec::new();
    let mut cursor: Option<String> = None;
    for page in 0..20 {
        let mut args = vec![
            "view",
            "-i",
            path.as_str(),
            "--scope",
            "all",
            "--limit",
            "1",
            "--output",
            "jsonl",
        ];
        if let Some(token) = &cursor {
            args.push("--cursor");
            args.push(token);
        }
        let out = ok(&sandbox, &args);
        let meta = out.metadata();
        assert!(meta["append_cursor"].is_null(), "page {page} issued append");
        for event in out.events() {
            seen.push((
                event["kind"].as_str().unwrap().to_string(),
                event["source"]["byte_start"].clone(),
                event["source"]["subevent_index"].clone(),
            ));
        }
        cursor = meta["page_cursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, expected, "paged scan differs from the one-shot scan");
    let unique: std::collections::BTreeSet<(String, u64, u64)> = seen
        .iter()
        .map(|(kind, start, subevent)| {
            (
                kind.clone(),
                start.as_u64().unwrap(),
                subevent.as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(unique.len(), seen.len(), "duplicate page event");
}

// T8 -----------------------------------------------------------------------

#[test]
fn t8_fixed_export_cursor_rejects_changes_and_query_reuse() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            user("m1", "AJC_T8_ALPHA", 1_700_000_000_000),
            assistant("m2", vec![text_part("AJC_T8_BETA")], 1_700_000_001_000),
        ],
        "ses_t8_original",
        true,
    );
    let path = write(&sandbox, "t8.json", &body);
    let first = ok(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "all", "--limit", "1", "--output", "jsonl",
        ],
    );
    let cursor = cursor_of(&first);

    // Same-length rewrite: anchors could match, so the whole-document hash must reject it.
    let mut changed = body.clone().into_bytes();
    let position = changed
        .windows("AJC_T8_ALPHA".len())
        .position(|window| window == b"AJC_T8_ALPHA")
        .unwrap();
    changed[position] = b'B';
    fs::write(&path, &changed).unwrap();
    let rejected = run(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "all", "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);
    assert_eq!(
        rejected.metadata()["error"]["reason"].as_str(),
        Some("anchor_mismatch")
    );

    // Restore, then truncate.
    fs::write(&path, &body).unwrap();
    let truncated_body = &body.as_bytes()[..body.len() - 5];
    fs::write(&path, truncated_body).unwrap();
    let rejected = run(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "all", "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);

    // Re-export with a different session id (same length as the original).
    fs::write(&path, &body).unwrap();
    let other_session = body.replace("ses_t8_original", "ses_t8_changed!");
    fs::write(&path, &other_session).unwrap();
    let rejected = run(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "all", "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);

    // A cursor is bound to its source path/generation.
    fs::write(&path, &body).unwrap();
    let other = write(&sandbox, "t8-other.json", &body);
    let rejected = run(
        &sandbox,
        &[
            "view", "-i", &other, "--scope", "all", "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);
    assert_eq!(
        rejected.metadata()["error"]["reason"].as_str(),
        Some("generation_mismatch")
    );

    // Search semantics are part of the cursor profile.
    let search_cursor = ok(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "AJC_T8", "--limit", "1", "--output", "jsonl",
        ],
    );
    let token = cursor_of(&search_cursor);
    let conflicted = run(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "AJC_T8_BETA",
            "--cursor",
            &token,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(conflicted.code, 1);
    assert_eq!(
        conflicted.metadata()["error"]["reason"].as_str(),
        Some("filter_mismatch")
    );
}

// T9 -----------------------------------------------------------------------

#[test]
fn t9_invalid_shapes_limits_and_legitimate_empty_are_distinct() {
    let sandbox = Sandbox::new();

    // Broken JSON / truncation.
    let broken = write(
        &sandbox,
        "broken.json",
        "{\"info\":{\"id\":\"s\"},\"messages\":[",
    );
    let out = run(
        &sandbox,
        &[
            "view", "-i", &broken, "--format", "opencode", "--output", "jsonl",
        ],
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.metadata()["status"], "error");
    assert_eq!(out.metadata()["error"]["type"], "format_invalid");

    // Wrong shape: messages is not an array.
    let wrong = write(
        &sandbox,
        "wrong.json",
        "{\"info\":{\"id\":\"s\"},\"messages\":{}}",
    );
    let out = run(
        &sandbox,
        &[
            "view", "-i", &wrong, "--format", "opencode", "--output", "jsonl",
        ],
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.metadata()["error"]["type"], "format_invalid");

    // Legitimate empty export: rc0 and no events.
    let empty = write(
        &sandbox,
        "empty.json",
        "{\"info\":{\"id\":\"s\"},\"messages\":[]}",
    );
    let out = ok(&sandbox, &["view", "-i", &empty, "--output", "jsonl"]);
    assert_eq!(out.metadata()["selected"], 0);
    assert_eq!(out.metadata()["status"], "ok");
    assert_eq!(out.metadata()["exhausted"], true);

    // A message above --max-record-bytes is uninspected, with a reusable retry cursor.
    let big = export_text(
        vec![
            user("m1", &"x".repeat(400), 1_700_000_000_000),
            assistant("m2", vec![text_part("after")], 1_700_000_001_000),
        ],
        "ses_t9_big",
        true,
    );
    let big_path = write(&sandbox, "big.json", &big);
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            &big_path,
            "--scope",
            "all",
            "--max-record-bytes",
            "100",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 1);
    let meta = out.metadata();
    assert_eq!(meta["status"], "partial");
    let samples = meta["counts"]["uninspected"]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0]["reason"], "max_record_bytes");
    let retry = samples[0]["retry_cursor"].as_str().unwrap();
    let resumed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &big_path,
            "--scope",
            "all",
            "--max-record-bytes",
            "100000",
            "--cursor",
            retry,
            "--output",
            "jsonl",
        ],
    );
    assert!(resumed
        .events()
        .iter()
        .any(|event| event["text"].as_str().unwrap().len() == 400));

    // Deep nesting is rejected, not silently treated as zero events.
    let nested = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    let deep = format!(
        "{{\"info\":{{\"id\":\"s\"}},\"messages\":[{{\"id\":\"m\",\"type\":\"user\",\"time\":{{\"created\":1}},\"text\":{nested}}}]}}"
    );
    let deep_path = write(&sandbox, "deep.json", &deep);
    let out = run(
        &sandbox,
        &[
            "view", "-i", &deep_path, "--format", "opencode", "--output", "jsonl",
        ],
    );
    assert_eq!(out.code, 1);
    let meta = out.metadata();
    assert!(
        meta["status"] == "partial" || meta["error"]["type"] == "format_invalid",
        "deep structure must be rc1 with an explicit reason: {meta:#?}"
    );
    assert!(
        meta["counts"]["invalid"]["records"].as_u64().unwrap_or(0) > 0
            || meta["error"]["type"] == "format_invalid"
    );
}

#[test]
fn t16_cursor_validation_respects_the_record_limit() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![assistant(
            "m1",
            vec![text_part(&"x".repeat(400)), text_part("second")],
            1_700_000_000_000,
        )],
        "ses_cursor_limit",
        true,
    );
    let path = write(&sandbox, "cursor-limit.json", &body);
    let first = ok(
        &sandbox,
        &["view", "-i", &path, "--limit", "1", "--output", "jsonl"],
    );
    let page = cursor_of(&first);
    let limited = run(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--max-record-bytes",
            "100",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(limited.code, 1);
    let meta = limited.metadata();
    let retry = meta["counts"]["uninspected"]["samples"][0]["retry_cursor"]
        .as_str()
        .unwrap();
    // Both a retry checkpoint and an intra-message checkpoint require enough budget
    // to validate their element. Validation must not normalize an oversized record.
    for token in [page.as_str(), retry] {
        let rejected = run(
            &sandbox,
            &[
                "view",
                "-i",
                &path,
                "--cursor",
                token,
                "--max-record-bytes",
                "100",
                "--output",
                "jsonl",
            ],
        );
        assert_eq!(rejected.code, 1);
        assert_eq!(rejected.metadata()["error"]["type"], "cursor_invalid");
        assert!(rejected.events().is_empty());
    }
    let resumed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            &page,
            "--max-record-bytes",
            "10000",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(resumed.events().len(), 1);
    assert_eq!(resumed.events()[0]["text"], "second");
    let retried = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            retry,
            "--max-record-bytes",
            "10000",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(retried.events().len(), 2);
}

// T10 ----------------------------------------------------------------------

#[test]
fn t10_unknown_message_and_part_types_are_counted_with_positions_not_guessed() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            json!({"id": "m1", "type": "mystery", "time": {"created": 1_700_000_000_000i64}, "text": "SECRET_UNKNOWN_MESSAGE"}),
            assistant(
                "m2",
                vec![
                    json!({"type": "mystery", "text": "SECRET_UNKNOWN_PART"}),
                    text_part("visible"),
                ],
                1_700_000_001_000,
            ),
        ],
        "ses_t10",
        true,
    );
    let path = write(&sandbox, "t10.json", &body);
    let out = ok(
        &sandbox,
        &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
    );
    let stdout = &out.stdout;
    assert!(!stdout.contains("SECRET_UNKNOWN_MESSAGE"));
    assert!(!stdout.contains("SECRET_UNKNOWN_PART"));
    assert!(stdout.contains("visible"));
    let meta = out.metadata();
    assert_eq!(meta["status"], "ok");
    assert_eq!(meta["counts"]["unknown"]["events"], 2);
    let samples = meta["counts"]["unknown"]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 2);
    assert!(samples.iter().all(
        |sample| sample["byte_start"].as_u64().unwrap() < sample["byte_end"].as_u64().unwrap()
    ));
    let coverage = &meta["scan"]["coverage"];
    assert_eq!(coverage["opencode_export/mystery"], 1);
    assert_eq!(coverage["opencode_export/assistant"], 1);
}

// T14 ----------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn t14_fifo_input_is_rejected_without_blocking() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let sandbox = Sandbox::new();
    let path = sandbox.path().join("input.fifo");
    let status = Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed: {status}");

    let mut child = Command::new(BIN)
        .env_clear()
        .env("HOME", sandbox.home())
        .env("PATH", "/usr/bin:/bin")
        .current_dir(sandbox.path())
        .args([
            "view",
            "-i",
            path.to_str().unwrap(),
            "--format",
            "opencode",
            "--output",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO input blocked instead of being rejected");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("source_unreadable") || stderr.contains("regular file"),
        "unexpected stderr: {stderr}"
    );
}

// T15 ----------------------------------------------------------------------

#[test]
fn t15_error_and_unknown_tool_shapes_keep_evidence_without_guessing() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            assistant(
                "m1",
                vec![json!({
                    "type": "tool",
                    "id": "call_error",
                    "name": "read",
                    "state": {
                        "status": "error",
                        "input": {"path": "x"},
                        "content": [{"type": "text", "text": "partial body"}],
                        "error": {"type": "tool.execution", "message": "permission denied"}
                    }
                })],
                1_700_000_000_000,
            ),
            assistant(
                "m2",
                vec![json!({
                    "type": "tool",
                    "id": "call_unknown_content",
                    "name": "read",
                    "state": {
                        "status": "completed",
                        "input": {},
                        "content": [
                            {"type": "mystery", "text": "SECRET_TOOL_BODY"},
                            {"type": "text", "text": "known body"}
                        ]
                    }
                })],
                1_700_000_001_000,
            ),
            assistant(
                "m3",
                vec![json!({
                    "type": "tool",
                    "id": "call_unknown_status",
                    "name": "read",
                    "state": {"status": "queued", "input": {}}
                })],
                1_700_000_002_000,
            ),
        ],
        "ses_t15",
        true,
    );
    let path = write(&sandbox, "t15.json", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let error_output = out
        .events()
        .into_iter()
        .find(|event| event["call_id"] == "call_error" && event["kind"] == "tool_output")
        .unwrap();
    let text = error_output["text"].as_str().unwrap();
    assert!(text.contains("partial body"), "{text}");
    assert!(text.contains("permission denied"), "{text}");
    assert_eq!(error_output["tool_observed"]["is_error"], true);

    let unknown = ok(
        &sandbox,
        &["view", "-i", &path, "--scope", "all", "--output", "jsonl"],
    );
    assert!(!unknown.stdout.contains("SECRET_TOOL_BODY"));
    assert!(unknown.stdout.contains("known body"));
    assert!(
        unknown.metadata()["counts"]["unknown"]["events"]
            .as_u64()
            .unwrap_or(0)
            >= 2
    );
    assert!(
        unknown
            .events()
            .iter()
            .all(|event| event["call_id"] != "call_unknown_status" || event["kind"] != "tool_call"),
        "unknown status must not become a pending call"
    );

    let error_search = ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "permission denied",
            "--scope",
            "tools",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(error_search.metadata()["selected"], 1);

    let secret_search = ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "SECRET_TOOL_BODY",
            "--scope",
            "all",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(secret_search.metadata()["selected"], 0);
}

// T11 ----------------------------------------------------------------------

#[test]
fn t11_pair_budget_and_uninspected_ranges_are_not_reported_as_pending() {
    let sandbox = Sandbox::new();
    let body = export_text(
        vec![
            assistant(
                "m1",
                vec![tool_completed(
                    "call_1",
                    "read",
                    json!({"path": "a"}),
                    "result-a",
                )],
                1_700_000_000_000,
            ),
            assistant(
                "m2",
                vec![tool_completed(
                    "call_2",
                    "read",
                    json!({"path": "b"}),
                    "result-b",
                )],
                1_700_000_001_000,
            ),
        ],
        "ses_t11",
        true,
    );
    let path = write(&sandbox, "t11.json", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "tools",
            "--with-pair",
            "--limit",
            "1",
            "--max-events",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let meta = out.metadata();
    assert!(
        meta["counts"]["pairs"]["not_rendered_budget"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "pair budget must be explicit: {meta:#?}"
    );
    assert_eq!(meta["counts"]["pairs"]["pending_result"], 0);

    // An over-limit result makes pairing unprovable, not "pending".
    let big_output = "y".repeat(300);
    let pair_body = export_text(
        vec![
            assistant(
                "m1",
                vec![tool_running("call_big", "read", json!({"path": "a"}))],
                1_700_000_000_000,
            ),
            assistant(
                "m2",
                vec![tool_completed(
                    "call_big",
                    "read",
                    json!({"path": "a"}),
                    &big_output,
                )],
                1_700_000_001_000,
            ),
        ],
        "ses_t11_big",
        false,
    );
    let pair_path = write(&sandbox, "t11-big.json", &pair_body);
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            &pair_path,
            "--scope",
            "tools",
            "--with-pair",
            "--max-record-bytes",
            "300",
            "--output",
            "jsonl",
        ],
    );
    let meta = out.metadata();
    let states: Vec<String> = out
        .events()
        .iter()
        .filter_map(|event| event["pair_state"].as_str().map(str::to_string))
        .collect();
    assert!(
        states.iter().any(|state| state == "unknown_pair")
            || meta["counts"]["pairs"]["unknown_pair"]
                .as_u64()
                .unwrap_or(0)
                > 0,
        "over-limit result must be unknown_pair, not pending: {meta:#?}"
    );
}
