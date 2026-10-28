//! Regression tests for the F-01..F-09 security hardening workdoc.

mod viewer_support;

use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use viewer_support::*;

fn claude_long_record(timestamp: &str, blocks: usize) -> String {
    let content: Vec<Value> = (0..blocks)
        .map(|index| json!({"type": "text", "text": format!("block {index}")}))
        .collect();
    line(&json!({
        "type": "user",
        "sessionId": "long-session",
        "uuid": "long-uuid",
        "timestamp": timestamp,
        "message": {"role": "user", "content": content},
    }))
}

fn claude_meta_record(session: &str, uuid: &str, text: &str) -> Value {
    json!({
        "type": "user",
        "sessionId": session,
        "uuid": uuid,
        "timestamp": "2026-01-01T00:00:00Z",
        "isMeta": true,
        "message": {"role": "user", "content": text},
    })
}

fn claude_compact_record(session: &str, uuid: &str, text: &str) -> Value {
    json!({
        "type": "user",
        "sessionId": session,
        "uuid": uuid,
        "timestamp": "2026-01-01T00:00:01Z",
        "isCompactSummary": true,
        "message": {"role": "user", "content": text},
    })
}

fn decode_cursor(token: &str) -> Value {
    let body = token.split_once(':').expect("cursor prefix").1;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body)
        .expect("cursor base64");
    serde_json::from_slice(&bytes).expect("cursor json")
}

fn encode_cursor(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap();
    format!(
        "c{}:{}",
        value["v"].as_u64().unwrap_or(1),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn message_files(root: &Path, count: usize, duplicate_last: bool) -> PathBuf {
    for index in 0..count {
        let id = if index == 0 || (duplicate_last && index + 1 == count) {
            "dup"
        } else {
            "unique"
        };
        let body = format!(
            "{}{}",
            line(&codex_session_meta(id, "/repo", "2026-01-01T00:00:00Z")),
            line(&codex_message(
                "user",
                &format!("message {index}"),
                "2026-01-01T00:00:01Z",
                None
            ))
        );
        fs::write(root.join(format!("{index:05}.jsonl")), body).unwrap();
    }
    root.to_path_buf()
}

#[test]
fn f03_long_metadata_and_many_subevents_are_rejected() {
    let sandbox = Sandbox::new();
    let timestamp = "T".repeat(64 * 1024);
    let long = sandbox.write(
        "long-timestamp.jsonl",
        &claude_long_record(&timestamp, 1000),
    );
    assert!(fs::metadata(&long).unwrap().len() < 1024 * 1024);

    let out = run(
        &sandbox,
        &["view", "-i", long.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(out.code, 1, "normalization limit must be partial: {out:#?}");
    assert_eq!(status(&out.metadata()), "partial");
    assert_eq!(
        count_field(&out.metadata(), &["counts", "invalid"], "records"),
        1
    );

    let many = sandbox.write(
        "many-subevents.jsonl",
        &claude_long_record("2026-01-01T00:00:00Z", 300),
    );
    assert!(fs::metadata(&many).unwrap().len() < 1024 * 1024);
    let out = run(
        &sandbox,
        &["view", "-i", many.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(out.code, 1, "subevent cap must be partial: {out:#?}");
    assert_eq!(
        count_field(&out.metadata(), &["counts", "invalid"], "records"),
        1
    );

    #[cfg(target_os = "linux")]
    {
        // The rejected record must not expand the 64 KiB timestamp once per subevent.
        let measured = Command::new("/usr/bin/time")
            .arg("-f")
            .arg("%M")
            .arg(env!("CARGO_BIN_EXE_agent-jsonl-compact"))
            .args(["view", "-i"])
            .arg(&long)
            .args(["--output", "jsonl"])
            .output()
            .expect("/usr/bin/time");
        let rss_kb: u64 = String::from_utf8_lossy(&measured.stderr)
            .lines()
            .filter_map(|line| line.trim().parse::<u64>().ok())
            .next_back()
            .expect("peak RSS line");
        assert!(
            rss_kb < 100 * 1024,
            "normalization rejection used too much memory: {rss_kb} KiB"
        );
    }
}

#[test]
fn f05_query_length_and_count_are_bounded() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("query.jsonl", &simple_codex_file("query", "/repo"));
    let too_long = "a".repeat(4_097);
    let out = run(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            &too_long,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 2, "oversized query must be usage error");
}

#[test]
fn f04_timestamp_control_characters_are_escaped_in_text_and_markdown() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}",
        line(&codex_session_meta("ctrl", "/repo", "2026-01-01T00:00:00Z")),
        line(&json!({
            "timestamp": "2026-01-01T00:00:01Z\u{1b}[31m",
            "type": "response_item",
            "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "hello"}]},
        }))
    );
    let input = sandbox.write("ctrl.jsonl", &body);
    let text = ok(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "text"],
    );
    assert!(
        !text.stdout.contains('\u{1b}'),
        "raw ESC leaked: {:?}",
        text.stdout
    );
    assert!(
        text.stdout.contains("\\x1b"),
        "escaped ESC missing: {:?}",
        text.stdout
    );

    let out_dir = sandbox.path().join("md");
    ok(
        &sandbox,
        &[
            "-i",
            input.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--format-out",
            "md",
        ],
    );
    let markdown = fs::read_to_string(out_dir.join("ctrl.transcript.md")).unwrap();
    assert!(!markdown.contains('\u{1b}'), "raw ESC leaked into Markdown");
}

#[test]
fn f05_search_ranges_are_bounded_and_never_loop_without_progress() {
    let sandbox = Sandbox::new();
    let text = "a".repeat(8192);
    let body = format!(
        "{}{}",
        line(&codex_session_meta(
            "search",
            "/repo",
            "2026-01-01T00:00:00Z"
        )),
        line(&codex_message("user", &text, "2026-01-01T00:00:01Z", None))
    );
    let input = sandbox.write("search.jsonl", &body);

    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "a",
            "--limit",
            "1",
            "--max-output-bytes",
            "32768",
            "--output",
            "jsonl",
        ],
    );
    let event = out
        .events()
        .into_iter()
        .next()
        .expect("matching event must render");
    assert_eq!(event["matched_ranges_truncated"], json!(true));
    assert!(event["matched_ranges_total"].as_u64().unwrap_or(0) >= 256);
    assert!(
        out.metadata()
            .get("page_cursor")
            .and_then(Value::as_str)
            .is_none(),
        "a single rendered event must not emit a repeating page cursor"
    );

    let too_small = run(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "a",
            "--limit",
            "1",
            "--max-output-bytes",
            "8192",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        too_small.code, 2,
        "unrenderable primary must be rc2: {too_small:#?}"
    );
    assert_eq!(
        too_small.metadata()["error"]["type"],
        json!("budget_too_small")
    );
}

#[test]
fn f05_context_is_capped_and_overflow_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "context",
        "/repo",
        "2026-01-01T00:00:00Z",
    )));
    for index in 0..200 {
        body.push_str(&line(&codex_message(
            if index == 100 { "assistant" } else { "user" },
            &format!("message {index}"),
            "2026-01-01T00:00:01Z",
            None,
        )));
    }
    let input = sandbox.write("context.jsonl", &body);
    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "message 100",
            "--context",
            "10000",
            "--max-events",
            "64",
            "--output",
            "jsonl",
        ],
    );
    assert!(
        count_field(&out.metadata(), &["counts", "context_dropped"], "events") > 0,
        "context cap must be reported: {:#?}",
        out.metadata()
    );

    let too_large = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--context",
            "10001",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(too_large.code, 2);
}

#[cfg(unix)]
#[test]
fn f05_fifo_is_rejected_without_waiting_for_a_writer() {
    let sandbox = Sandbox::new();
    let fifo = sandbox.path().join("pipe.jsonl");
    let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success(), "mkfifo failed");
    let out = run(
        &sandbox,
        &["view", "-i", fifo.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.metadata()["error"]["type"], json!("source_unreadable"));
}

#[test]
fn f06_pairing_is_structured_and_side_specific() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}{}{}{}",
        line(&codex_session_meta("S1", "/repo", "2026-01-01T00:00:00Z")),
        line(&codex_function_call(
            "shell",
            "same",
            "{}",
            "2026-01-01T00:00:01Z"
        )),
        line(&codex_session_meta("S2", "/repo", "2026-01-01T00:00:02Z")),
        line(&codex_function_output(
            "same",
            "wrong session",
            "2026-01-01T00:00:03Z"
        )),
        line(&codex_session_meta("S3", "/repo", "2026-01-01T00:00:04Z")),
        line(&codex_function_call(
            "shell",
            "same",
            "{}",
            "2026-01-01T00:00:05Z"
        )),
    );
    let input = sandbox.write("pair.jsonl", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();
    let s1_call = events
        .iter()
        .find(|event| event["session_id"] == json!("S1") && event["kind"] == json!("tool_call"))
        .expect("S1 call");
    assert_eq!(
        s1_call["pair_state"],
        json!("pending_result"),
        "{s1_call:#?}"
    );
    assert_eq!(s1_call["paired_event_ids"], json!([]));
    let s3_call = events
        .iter()
        .find(|event| event["session_id"] == json!("S3") && event["kind"] == json!("tool_call"))
        .expect("S3 call");
    assert_eq!(
        s3_call["pair_state"],
        json!("pending_result"),
        "{s3_call:#?}"
    );
}

#[test]
fn f06_delimiter_joined_ids_do_not_collide() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}{}",
        line(&codex_session_meta(
            "a\u{1f}b",
            "/repo",
            "2026-01-01T00:00:00Z"
        )),
        line(&codex_function_call(
            "shell",
            "c",
            "{}",
            "2026-01-01T00:00:01Z"
        )),
        line(&codex_session_meta("a", "/repo", "2026-01-01T00:00:02Z")),
        line(&codex_function_output(
            "b\u{1f}c",
            "not the same call",
            "2026-01-01T00:00:03Z"
        )),
    );
    let input = sandbox.write("delimiter.jsonl", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let call = out
        .events()
        .into_iter()
        .find(|event| event["session_id"] == json!("a\u{1f}b"))
        .expect("call event");
    assert_eq!(call["pair_state"], json!("pending_result"), "{call:#?}");
    assert_eq!(call["paired_event_ids"], json!([]));
}

#[test]
fn f06_duplicate_calls_and_result_only_are_not_paired() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}{}",
        line(&codex_session_meta("S", "/repo", "2026-01-01T00:00:00Z")),
        line(&codex_function_call(
            "shell",
            "dup",
            "{}",
            "2026-01-01T00:00:01Z"
        )),
        line(&codex_function_call(
            "shell",
            "dup",
            "{}",
            "2026-01-01T00:00:02Z"
        )),
        line(&codex_function_output(
            "only-result",
            "orphan",
            "2026-01-01T00:00:03Z"
        )),
    );
    let input = sandbox.write("pair2.jsonl", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let duplicate = out
        .events()
        .into_iter()
        .find(|event| event["call_id"] == json!("dup"))
        .expect("duplicate call");
    assert_eq!(
        duplicate["pair_state"],
        json!("ambiguous"),
        "{duplicate:#?}"
    );
    let orphan = out
        .events()
        .into_iter()
        .find(|event| event["call_id"] == json!("only-result"))
        .expect("result only");
    assert_eq!(orphan["pair_state"], json!("orphan"), "{orphan:#?}");
}

#[test]
fn f07_cursor_line_forgery_does_not_change_event_identity() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}",
        line(&codex_session_meta(
            "cursor",
            "/repo",
            "2026-01-01T00:00:00Z"
        )),
        line(&codex_message(
            "user",
            "first",
            "2026-01-01T00:00:01Z",
            None
        )),
        line(&codex_message(
            "assistant",
            "second",
            "2026-01-01T00:00:02Z",
            None
        )),
    );
    let input = sandbox.write("cursor.jsonl", &body);
    let first = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let full = ok(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    let full_second = full
        .events()
        .into_iter()
        .find(|event| ev_text(event) == "second")
        .expect("second event");
    let expected_id = ev_id(&full_second).to_string();
    let cursor = first
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .expect("page cursor")
        .to_string();
    let mut payload = decode_cursor(&cursor);
    payload["raw_line"] = json!(1);
    let forged = encode_cursor(&payload);
    let resumed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &forged,
            "--output",
            "jsonl",
        ],
    );
    let second = resumed
        .events()
        .into_iter()
        .find(|event| ev_text(event) == "second")
        .expect("second event after resume");
    assert_eq!(ev_id(&second), expected_id);
    assert_eq!(second["source"]["raw_line_verified"], json!(false));

    let mut payload = decode_cursor(&cursor);
    payload["session_id"] = json!("forged-session");
    let forged_session = encode_cursor(&payload);
    let rejected = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &forged_session,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);
    assert_eq!(
        rejected.metadata()["error"]["type"],
        json!("cursor_invalid")
    );

    let mut payload = decode_cursor(&cursor);
    payload["context"] = json!(100_000);
    let forged_context = encode_cursor(&payload);
    let rejected = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &forged_context,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);
    assert_eq!(
        rejected.metadata()["error"]["type"],
        json!("cursor_invalid")
    );

    let mut payload = decode_cursor(&cursor);
    payload["complete_line_count"] = json!(u64::MAX);
    let overflow = encode_cursor(&payload);
    let rejected = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &overflow,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(rejected.code, 1);
    assert_eq!(
        rejected.metadata()["error"]["type"],
        json!("cursor_invalid")
    );
}

#[test]
fn f08_incomplete_scan_does_not_claim_unique_session() {
    let sandbox = Sandbox::new();
    let root = sandbox.path().join("many");
    fs::create_dir_all(&root).unwrap();
    message_files(&root, 10_001, true);

    let out = run(
        &sandbox,
        &[
            "view",
            "--session",
            "codex:dup",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 1, "incomplete scan must be rc1: {out:#?}");
    assert_eq!(
        out.metadata()["error"]["type"],
        json!("session_resolution_incomplete")
    );

    let listed = run(
        &sandbox,
        &[
            "sessions",
            "--provider",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(listed.code, 1, "partial sessions scan is a failure");
    assert_eq!(status(&listed.metadata()), "partial");
    assert_eq!(listed.metadata()["scan"]["scan_limit_reached"], json!(true));
}

#[test]
fn f08_invalid_records_are_counted_before_skipping_a_file() {
    let sandbox = Sandbox::new();
    let root = sandbox.path().join("invalid-mix");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("valid.jsonl"),
        format!(
            "{}{}",
            line(&codex_session_meta(
                "valid",
                "/repo",
                "2026-01-01T00:00:00Z"
            )),
            line(&codex_message(
                "user",
                "hello",
                "2026-01-01T00:00:01Z",
                None
            ))
        ),
    )
    .unwrap();
    fs::write(root.join("broken.jsonl"), "{not json}\n").unwrap();
    let out = run(
        &sandbox,
        &[
            "sessions",
            "--provider",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 1, "partial scan is a failure: {out:#?}");
    assert_eq!(status(&out.metadata()), "partial");
    assert_eq!(
        out.metadata()["scan"]["records_invalid"],
        json!(1),
        "{:#?}",
        out.metadata()
    );
}

#[test]
fn f09_legacy_extraction_preserves_framework_provenance_and_fences_markdown() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}{}{}",
        line(&claude_user(
            "prov",
            "u0",
            "# FAKE HEADING\n![x](http://example.invalid/x.png)",
            "2026-01-01T00:00:00Z"
        )),
        line(&claude_meta_record(
            "prov",
            "u1",
            "framework instruction that is not a human message"
        )),
        line(&claude_compact_record(
            "prov",
            "u2",
            "compacted summary that is not a human message"
        )),
        line(&claude_assistant_text(
            "prov",
            "u3",
            "answer",
            "2026-01-01T00:00:02Z"
        )),
    );
    let input = sandbox.write("prov.jsonl", &body);
    let out_dir = sandbox.path().join("prov-out");
    ok(
        &sandbox,
        &[
            "-i",
            input.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
            "--format-out",
            "both",
        ],
    );
    let clean = fs::read_to_string(out_dir.join("prov.clean.jsonl")).unwrap();
    assert!(clean.contains("\"kind\":\"injected\""), "{clean}");
    assert!(clean.contains("\"origin\":\"framework\""), "{clean}");
    assert!(clean.contains("\"kind\":\"compacted\""), "{clean}");
    assert!(clean.contains("\"origin\":\"compaction\""), "{clean}");
    let markdown = fs::read_to_string(out_dir.join("prov.transcript.md")).unwrap();
    assert!(markdown.contains("FRAMEWORK(meta)"), "{markdown}");
    assert!(
        markdown.contains("```text\n# FAKE HEADING\n![x]"),
        "body must be fenced, not parsed as headings/images: {markdown}"
    );
}

#[test]
fn f05_regex_match_total_is_a_bounded_lower_bound() {
    let sandbox = Sandbox::new();
    let text = "a".repeat(8_192);
    let body = format!(
        "{}{}",
        line(&codex_session_meta(
            "regex-total",
            "/repo",
            "2026-01-01T00:00:00Z"
        )),
        line(&codex_message("user", &text, "2026-01-01T00:00:01Z", None))
    );
    let input = sandbox.write("regex-total.jsonl", &body);

    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "a",
            "--regex",
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let event = out.events().into_iter().next().expect("matching event");
    assert_eq!(event["matched_ranges"].as_array().unwrap().len(), 256);
    assert_eq!(event["matched_ranges_truncated"], json!(true));
    assert_eq!(
        event["matched_ranges_total"].as_u64(),
        Some(257),
        "truncated totals must be a truthful, bounded lower bound, not the retained cap"
    );
}

#[test]
fn f04_long_multibyte_usage_error_does_not_panic_at_a_utf8_boundary() {
    let sandbox = Sandbox::new();
    let input = sandbox.write(
        "utf8-usage.jsonl",
        &line(&codex_session_meta(
            "utf8-usage",
            "/repo",
            "2026-01-01T00:00:00Z",
        )),
    );
    // "invalid --regex '" is 17 bytes, so the multibyte character starts at byte 1023 and
    // straddles the old unconditional truncate(1024) boundary.
    let query = format!("{}あ(", "a".repeat(1_006));

    let out = run(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            &query,
            "--regex",
            "--output",
            "jsonl",
        ],
    );

    assert_eq!(out.code, 2, "usage error expected: {out:#?}");
    assert!(out.stderr.starts_with("ERROR: invalid --regex"));
    assert!(
        out.stderr.len() <= "ERROR: ".len() + 1_024 + 1,
        "diagnostic must stay within the 1024-byte text cap: {} bytes",
        out.stderr.len()
    );
}

#[test]
fn f04_provider_name_with_multibyte_chars_does_not_panic_in_usage() {
    let sandbox = Sandbox::new();
    let session = format!("{}:id", "あ".repeat(400));

    let out = run(
        &sandbox,
        &[
            "view",
            "--session",
            &session,
            "--root",
            sandbox.path().to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );

    assert_eq!(out.code, 2, "usage error expected: {out:#?}");
    assert!(out.stderr.starts_with("ERROR: unknown provider"));
    assert!(
        out.stderr.len() <= "ERROR: ".len() + 1_024 + 1,
        "diagnostic must stay within the 1024-byte text cap: {} bytes",
        out.stderr.len()
    );
}

#[test]
fn f06_cursor_session_must_match_source_fallback_without_metadata() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}",
        line(&codex_message(
            "user",
            "first",
            "2026-01-01T00:00:00Z",
            None
        )),
        line(&codex_message(
            "assistant",
            "second",
            "2026-01-01T00:00:01Z",
            None
        ))
    );
    let input = sandbox.write("no-session-meta.jsonl", &body);

    let first = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let cursor = first.metadata()["page_cursor"]
        .as_str()
        .expect("page cursor for the second event")
        .to_string();
    let mut payload = decode_cursor(&cursor);
    payload["session_id"] = json!("forged-session");
    let forged = encode_cursor(&payload);

    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &forged,
            "--output",
            "jsonl",
        ],
    );

    assert_eq!(out.code, 1, "forged session id must be rejected: {out:#?}");
    assert_eq!(out.metadata()["error"]["type"], json!("cursor_invalid"));
    assert_eq!(out.metadata()["error"]["reason"], json!("filter_mismatch"));
}

#[test]
fn f05_oversized_query_set_is_rejected_instead_of_emitting_an_undecodable_cursor() {
    let sandbox = Sandbox::new();
    let body = format!(
        "{}{}",
        line(&codex_message("user", "a", "2026-01-01T00:00:00Z", None)),
        line(&codex_message(
            "assistant",
            "a",
            "2026-01-01T00:00:01Z",
            None
        ))
    );
    let input = sandbox.write("oversized-cursor.jsonl", &body);
    let query = "a".repeat(4_096);
    let mut args: Vec<&str> = vec![
        "search",
        "-i",
        input.to_str().unwrap(),
        "--limit",
        "1",
        "--output",
        "jsonl",
    ];
    for _ in 0..20 {
        args.push("--query");
        args.push(query.as_str());
    }

    let out = run(&sandbox, &args);

    assert_eq!(
        out.code, 2,
        "oversized cursor must be a usage error: {out:#?}"
    );
    assert_eq!(out.metadata()["error"]["type"], json!("cursor_too_large"));
    assert!(
        out.metadata().get("page_cursor").is_none(),
        "must not emit a cursor that its own decoder rejects: {}",
        out.metadata()
    );
}

#[test]
fn f08_missing_declared_root_is_incomplete_but_absent_defaults_are_not() {
    let sandbox = Sandbox::new();
    let missing = sandbox.path().join("missing-root");

    let listed = run(
        &sandbox,
        &[
            "sessions",
            "--provider",
            "codex",
            "--root",
            missing.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        listed.code, 1,
        "declared missing root is partial: {listed:#?}"
    );
    assert_eq!(status(&listed.metadata()), "partial");
    assert_eq!(listed.metadata()["scan"]["limit_reason"], json!("scan_io"));
    assert!(
        listed.metadata()["scan"]["walk_errors"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );

    for (command, extra) in [
        ("view", Vec::<&str>::new()),
        ("search", vec!["--query", "x"]),
    ] {
        let mut args = vec![
            command,
            "--session",
            "codex:nope",
            "--root",
            missing.to_str().unwrap(),
            "--output",
            "jsonl",
        ];
        args.extend(extra);
        let out = run(&sandbox, &args);
        assert_eq!(
            out.code, 1,
            "{command} must not claim not-found from a missing root: {out:#?}"
        );
        assert_eq!(
            out.metadata()["error"]["type"],
            json!("session_resolution_incomplete")
        );
    }

    // An absent default root is intentional: Codex has no declared root, so the complete scan
    // of zero roots resolves to "not found", not "incomplete".
    let absent_default = run(
        &sandbox,
        &["view", "--session", "codex:nope", "--output", "jsonl"],
    );
    assert_eq!(absent_default.code, 1, "{absent_default:#?}");
    assert_eq!(
        absent_default.metadata()["error"]["type"],
        json!("session_not_found")
    );
}

#[cfg(unix)]
#[test]
fn f08_symlink_entries_are_partial_and_never_followed() {
    let sandbox = Sandbox::new();
    let root = sandbox.path().join("symlink-root");
    let outside = sandbox.path().join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("secret.jsonl"),
        simple_codex_file("secret-session", "/repo"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.join("secret.jsonl"), root.join("link.jsonl")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linkdir")).unwrap();

    let out = run(
        &sandbox,
        &[
            "sessions",
            "--provider",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );

    assert_eq!(out.code, 1, "skipped symlinks are partial: {out:#?}");
    assert_eq!(status(&out.metadata()), "partial");
    assert_eq!(
        out.metadata()["scan"]["limit_reason"],
        json!("scan_symlink")
    );
    assert_eq!(out.metadata()["scan"]["symlinks_skipped"], json!(2));
    assert!(
        !out.stdout.contains("secret-session"),
        "symlink targets must never be followed: {}",
        out.stdout
    );
}

#[test]
fn f08_unrelated_duplicate_does_not_block_a_unique_requested_session() {
    let sandbox = Sandbox::new();
    let root = sandbox.path().join("duplicates");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.jsonl"), simple_codex_file("dup", "/repo")).unwrap();
    fs::write(root.join("b.jsonl"), simple_codex_file("dup", "/repo")).unwrap();
    fs::write(
        root.join("unique.jsonl"),
        simple_codex_file("unique", "/repo"),
    )
    .unwrap();

    let unique = ok(
        &sandbox,
        &[
            "view",
            "--session",
            "codex:unique",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert!(
        unique.stdout.contains("unique"),
        "an unrelated duplicate must not block unique resolution: {unique:#?}"
    );

    let duplicate = run(
        &sandbox,
        &[
            "view",
            "--session",
            "codex:dup",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(duplicate.code, 1, "the requested duplicate is ambiguous");
    assert_eq!(
        duplicate.metadata()["error"]["type"],
        json!("session_ambiguous")
    );

    let listed = run(
        &sandbox,
        &[
            "sessions",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(listed.code, 1, "sessions listing keeps ambiguity semantics");
    assert_eq!(
        listed.metadata()["error"]["type"],
        json!("session_ambiguous")
    );
}

#[cfg(unix)]
#[test]
fn f08_walk_io_failures_are_reported_as_partial() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let root = sandbox.path().join("walk-errors");
    let blocked = root.join("blocked");
    let trapped = root.join("trapped");
    fs::create_dir_all(&blocked).unwrap();
    fs::create_dir_all(&trapped).unwrap();
    fs::write(
        root.join("ok.jsonl"),
        line(&codex_session_meta("ok", "/repo", "2026-01-01T00:00:00Z")),
    )
    .unwrap();
    fs::write(blocked.join("hidden.jsonl"), "{}").unwrap();
    fs::write(trapped.join("hidden.jsonl"), "{}").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    fs::set_permissions(&trapped, fs::Permissions::from_mode(0o400)).unwrap();

    // Root or a permissive container can read these regardless; then the failure path cannot
    // be exercised in this environment.
    let observable = fs::read_dir(&blocked).is_err()
        && fs::symlink_metadata(trapped.join("hidden.jsonl")).is_err();
    if !observable {
        let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions(&trapped, fs::Permissions::from_mode(0o755));
        return;
    }

    let out = run(
        &sandbox,
        &[
            "sessions",
            "--provider",
            "codex",
            "--root",
            root.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
    let _ = fs::set_permissions(&trapped, fs::Permissions::from_mode(0o755));

    assert_eq!(out.code, 1, "an incomplete walk must be rc1: {out:#?}");
    assert_eq!(status(&out.metadata()), "partial");
    assert_eq!(out.metadata()["scan"]["limit_reason"], json!("scan_io"));
    assert!(
        out.metadata()["scan"]["walk_errors"].as_u64().unwrap_or(0) >= 2,
        "read_dir and symlink_metadata failures must be counted: {}",
        out.metadata()["scan"]
    );
}
