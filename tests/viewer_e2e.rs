//! A01–A03: session discovery, provider bodies/origin/phase, time/role/tail selection.

mod viewer_support;
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use viewer_support::*;

fn roots(sandbox: &Sandbox) -> (String, String) {
    let codex = sandbox.path().join("codex-root");
    let claude = sandbox.path().join("claude-root");
    fs::create_dir_all(&codex).unwrap();
    fs::create_dir_all(&claude).unwrap();
    (
        codex.to_string_lossy().into_owned(),
        claude.to_string_lossy().into_owned(),
    )
}

// A01 -----------------------------------------------------------------------

#[test]
fn a01_sessions_discovers_exact_cwd_started_old_updated_recent() {
    let sandbox = Sandbox::new();
    let (codex_root, _) = roots(&sandbox);
    let long_title = "T".repeat(41_070);

    // Starts long ago but was updated today: must still be found and its start shown separately.
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "old-start",
        "/repoA",
        "2026-09-02T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        &long_title,
        "2026-09-02T00:00:01Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "latest answer",
        "2026-10-21T09:00:00Z",
        Some("final"),
    )));
    fs::write(Path::new(&codex_root).join("rollout-old-start.jsonl"), body).unwrap();

    // Child cwd session.
    fs::write(
        Path::new(&codex_root).join("rollout-child.jsonl"),
        simple_codex_file("child-session", "/repoA/sub"),
    )
    .unwrap();
    // Same cwd, different session.
    fs::write(
        Path::new(&codex_root).join("rollout-other1.jsonl"),
        simple_codex_file("other-session", "/repoA"),
    )
    .unwrap();

    // Outside every declared root.
    let outside = sandbox.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("rollout-outside.jsonl"),
        simple_codex_file("outside-session", "/repoA"),
    )
    .unwrap();

    let out = ok(
        &sandbox,
        &[
            "sessions",
            "--root",
            &codex_root,
            "--provider",
            "codex",
            "--cwd",
            "/repoA",
            "--output",
            "jsonl",
        ],
    );
    let sessions: Vec<Value> = out
        .jsonl()
        .into_iter()
        .filter(|row| row.get("record_type").and_then(Value::as_str) == Some("session"))
        .collect();
    let ids: Vec<&str> = sessions
        .iter()
        .map(|row| row.get("session_id").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(
        sessions.len(),
        2,
        "exact cwd: old-start + other-session, got {ids:?}"
    );
    assert!(ids.contains(&"old-start") && ids.contains(&"other-session"));
    assert!(!ids.contains(&"child-session"));
    assert!(!ids.contains(&"outside-session"));

    let old = sessions
        .iter()
        .find(|row| row.get("session_id").and_then(Value::as_str) == Some("old-start"))
        .unwrap();
    let started = old
        .get("started_at")
        .and_then(Value::as_str)
        .expect("started_at");
    let latest = old
        .get("last_event_at")
        .and_then(Value::as_str)
        .expect("last_event_at");
    assert_ne!(
        started, latest,
        "start and last event must be distinguishable"
    );
    assert!(started.starts_with("2026-09-02"));
    assert!(latest.starts_with("2026-10-21"));

    // Title fits the budget: the whole response must stay <= 32768 bytes and be valid JSONL.
    assert!(
        out.total_bytes() <= 32768,
        "sessions output {} bytes",
        out.total_bytes()
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok");
    let roots_reported: Vec<&str> = meta["scan"]["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(roots_reported, vec![codex_root.as_str()]);
    assert!(count_field(&meta, &["scan"], "files_scanned") >= 3);
    assert_eq!(
        meta["scan"]["files_scanned"].as_i64().unwrap(),
        count_field(&meta, &["scan"], "files_scanned")
    );
    assert!(out.stdout.contains("old-start"));

    // Child cwd requires the explicit flag.
    let child = ok(
        &sandbox,
        &[
            "sessions",
            "--root",
            &codex_root,
            "--provider",
            "codex",
            "--cwd",
            "/repoA",
            "--cwd-children",
            "--output",
            "jsonl",
        ],
    );
    let child_ids: Vec<String> = child
        .jsonl()
        .into_iter()
        .filter(|row| row.get("record_type").and_then(Value::as_str) == Some("session"))
        .map(|row| {
            row.get("session_id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(child_ids.len(), 3, "{child_ids:?}");
    assert!(child_ids.contains(&"child-session".to_string()));
}

#[test]
fn a01_sessions_ambiguous_duplicate_id_fails_rc1() {
    let sandbox = Sandbox::new();
    let (codex_root, _) = roots(&sandbox);
    fs::write(
        Path::new(&codex_root).join("a/rollout-dup.jsonl"),
        simple_codex_file("dup-session", "/repoA"),
    )
    .ok();
    fs::create_dir_all(Path::new(&codex_root).join("a")).unwrap();
    fs::write(
        Path::new(&codex_root).join("a/rollout-dup.jsonl"),
        simple_codex_file("dup-session", "/repoA"),
    )
    .unwrap();
    fs::create_dir_all(Path::new(&codex_root).join("b")).unwrap();
    fs::write(
        Path::new(&codex_root).join("b/rollout-dup.jsonl"),
        simple_codex_file("dup-session", "/repoA"),
    )
    .unwrap();

    let out = run(
        &sandbox,
        &["sessions", "--root", &codex_root, "--output", "jsonl"],
    );
    assert_eq!(
        out.code, 1,
        "duplicate session id must be ambiguity rc1: {out:#?}"
    );
    assert_eq!(status(&out.metadata()), "error");
    assert!(out.stderr.contains("ambiguous") || out.stderr.contains("duplicate"));
}

#[test]
fn a01_sessions_default_roots_resolve_under_isolated_home() {
    let sandbox = Sandbox::new();
    let home = sandbox.home();
    let codex_dir = home.join(".codex/sessions/2026/10/01");
    fs::create_dir_all(&codex_dir).unwrap();
    fs::write(
        codex_dir.join("rollout-home.jsonl"),
        simple_codex_file("home-session", "/repoA"),
    )
    .unwrap();

    let out = ok(
        &sandbox,
        &["sessions", "--provider", "codex", "--output", "jsonl"],
    );
    let meta = out.metadata();
    assert_eq!(
        meta["scan"]["roots"].as_array().unwrap()[0]
            .as_str()
            .unwrap(),
        codex_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
    );
    assert_eq!(meta["scan"]["roots"].as_array().unwrap().len(), 1);
    assert!(out.stdout.contains("home-session"));
}

// A02 -----------------------------------------------------------------------

#[test]
fn a02_provider_bodies_and_phase_and_framework() {
    let sandbox = Sandbox::new();
    let (codex_root, claude_root) = roots(&sandbox);

    let mut codex = String::new();
    codex.push_str(&line(&codex_session_meta(
        "codex-new",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    // Same text twice with DIFFERENT stable IDs must stay two separate events (no text dedup).
    codex.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:01Z", "type": "response_item",
        "payload": {"type": "message", "role": "user", "id": "item-a",
            "content": [{"type": "input_text", "text": "same text"}]}
    })));
    codex.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:02Z", "type": "response_item",
        "payload": {"type": "message", "role": "user", "id": "item-b",
            "content": [{"type": "input_text", "text": "same text"}]}
    })));
    // Terminal mirror of item-a via explicit mirror id: may be merged, never counted as a third body.
    codex.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:03Z", "type": "event_msg",
        "payload": {"type": "user_message", "message": "same text", "item_id": "item-a"}
    })));
    codex.push_str(&line(&codex_message(
        "assistant",
        "commentary body",
        "2026-10-01T00:00:04Z",
        Some("commentary"),
    )));
    codex.push_str(&line(&codex_message(
        "assistant",
        "final body",
        "2026-10-01T00:00:05Z",
        Some("final"),
    )));
    fs::write(Path::new(&codex_root).join("rollout-a02.jsonl"), codex).unwrap();

    let mut claude = String::new();
    claude.push_str(&line(&claude_user(
        "claude-a02",
        "u1",
        "claude question",
        "2026-10-01T01:00:00Z",
    )));
    claude.push_str(&line(&claude_framework_meta(
        "claude-a02",
        "u-meta",
        "2026-10-01T01:00:01Z",
    )));
    claude.push_str(&line(&claude_compact_summary(
        "claude-a02",
        "u-compact",
        "2026-10-01T01:00:02Z",
    )));
    claude.push_str(&line(&claude_assistant_text(
        "claude-a02",
        "a1",
        "claude answer",
        "2026-10-01T01:00:03Z",
    )));
    fs::write(Path::new(&claude_root).join("claude-a02.jsonl"), claude).unwrap();

    // Legacy Codex shape.
    let legacy_dir = sandbox.path().join("legacy");
    fs::create_dir_all(&legacy_dir).unwrap();
    let legacy: String = legacy_codex(
        "codex-legacy",
        "/repoA",
        "2026-09-01T00:00:00Z",
        "legacy question",
    )
    .iter()
    .map(line)
    .collect();
    fs::write(legacy_dir.join("rollout-legacy.jsonl"), legacy).unwrap();

    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            Path::new(&codex_root)
                .join("rollout-a02.jsonl")
                .to_str()
                .unwrap(),
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();
    let same = events
        .iter()
        .filter(|e| ev_text(e) == "same text")
        .collect::<Vec<_>>();
    // KEEP representation is the accepted one (root A02_PROVENANCE_GATE_1021): all three physical
    // rows stay independently addressable, and the mirror is explicitly labelled. Merging away a
    // body is only acceptable if alias pointers cover every row; this product keeps the rows.
    assert_eq!(
        same.len(),
        3,
        "every physical row with this body must remain addressable: {:?}",
        events
            .iter()
            .map(|e| (ev_text(e), raw_line(e)))
            .collect::<Vec<_>>()
    );
    let ids: Vec<&str> = same.iter().map(|e| ev_id(e)).collect();
    let unique_ids: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique_ids.len(),
        3,
        "each row needs its own stable event id: {ids:?}"
    );

    // Raw-row coverage, not just a body count: rows 2, 3 and 4 must each appear exactly once.
    let raw_rows: Vec<u64> = same.iter().map(|e| raw_line(e)).collect();
    assert_eq!(
        raw_rows,
        vec![2, 3, 4],
        "the two API item rows and the terminal mirror row must all be covered: {raw_rows:?}"
    );

    // Exactly one of them is the terminal mirror, and it is identifiable rather than a third
    // independent human body.
    let mirrors: Vec<&&Value> = same
        .iter()
        .filter(|e| e.get("mirror_of").and_then(Value::as_str).is_some())
        .collect();
    assert_eq!(
        mirrors.len(),
        1,
        "exactly one mirror relation expected: {same:#?}"
    );
    assert_eq!(
        mirrors[0].get("mirror_of").and_then(Value::as_str),
        Some("item-a"),
        "mirror must name the explicit shared origin id: {:#?}",
        mirrors[0]
    );
    assert!(
        mirrors[0]
            .get("mirror_relation")
            .and_then(Value::as_str)
            .is_some(),
        "mirror must state its relation: {:#?}",
        mirrors[0]
    );
    let api_rows: Vec<u64> = same
        .iter()
        .filter(|e| e.get("mirror_of").is_none())
        .map(|e| raw_line(e))
        .collect();
    assert_eq!(
        api_rows,
        vec![2, 3],
        "two API items remain distinct: {api_rows:?}"
    );

    let meta = out.metadata();
    assert!(
        count_field(&meta, &["counts", "mirror_candidates"], "events") >= 1,
        "mirror candidate evidence must be retained: {meta:#?}"
    );
    for event in &events {
        assert!(event.get("message_origin").is_some(), "origin required");
        assert!(event.get("source_kind").is_some(), "source_kind required");
    }

    let claude_out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            Path::new(&claude_root)
                .join("claude-a02.jsonl")
                .to_str()
                .unwrap(),
            "--output",
            "jsonl",
        ],
    );
    let claude_events = claude_out.events();
    let claude_texts: Vec<&str> = claude_events.iter().map(ev_text).collect();
    assert!(claude_texts.contains(&"claude question"));
    assert!(claude_texts.contains(&"claude answer"));
    let meta = claude_out.metadata();
    assert_eq!(
        count_field(&meta, &["counts", "framework_excluded"], "events"),
        2
    );
    assert!(!claude_out.stdout.contains("framework injected reminder"));

    let legacy_out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            legacy_dir.join("rollout-legacy.jsonl").to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert!(legacy_out
        .events()
        .iter()
        .any(|e| ev_text(e) == "legacy question"));
}

// A03 -----------------------------------------------------------------------

fn a03_file() -> String {
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "a03",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    // Boundary pair: 08:59:59Z is just before the lower bound; 18:00:00+09:00 == 09:00:00Z is the
    // inclusive lower bound. (16:59:59+09:00 would be 08:59:59Z, a common off-by-one-hour slip.)
    body.push_str(&line(&codex_message(
        "user",
        "before boundary",
        "2026-10-01T08:59:59Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "user",
        "at boundary plus09",
        "2026-10-01T18:00:00+09:00",
        None,
    )));
    // Missing time.
    body.push_str(&line(&json!({"type": "response_item", "payload": {
        "type": "message", "role": "user", "content": [{"type": "input_text", "text": "no timestamp"}]}})));
    // Retrograde timestamp: earlier than the previous record but must stay in raw order.
    body.push_str(&line(&codex_message(
        "user",
        "retrograde early",
        "2026-10-01T07:00:00Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "assistant after retrograde",
        "2026-10-01T08:30:00Z",
        Some("final"),
    )));
    // until exclusive: 10:00:00Z excluded, 09:59:59Z included.
    body.push_str(&line(&codex_message(
        "assistant",
        "just inside until",
        "2026-10-01T09:59:59Z",
        Some("final"),
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "at until",
        "2026-10-01T10:00:00Z",
        Some("final"),
    )));
    body
}

#[test]
fn a03_time_boundaries_unknown_retrograde_and_tail() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("a03.jsonl", &a03_file());

    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--since",
            "2026-10-01T09:00:00Z",
            "--until",
            "2026-10-01T10:00:00Z",
            "--output",
            "jsonl",
        ],
    );
    let a03_events = out.events();
    let texts: Vec<&str> = a03_events.iter().map(ev_text).collect();
    assert_eq!(
        texts,
        vec!["at boundary plus09", "just inside until"],
        "{texts:?}"
    );
    let meta = out.metadata();
    assert_eq!(count_field(&meta, &["counts", "time_unknown"], "events"), 1);
    // `excluded_time` counts every event excluded by the window: 4 out-of-range (08:59:59,
    // 07:00, 08:30, 10:00) plus the 1 missing-timestamp record. This precise value replaces the
    // earlier "2", which was an invented step-4 expectation, not an acceptance criterion.
    assert_eq!(
        count_field(&meta, &["counts", "excluded_time"], "events"),
        5
    );

    // Role filter keeps raw order (retrograde not re-sorted).
    let role = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--role",
            "assistant",
            "--output",
            "jsonl",
        ],
    );
    let role_events = role.events();
    let role_texts: Vec<&str> = role_events.iter().map(ev_text).collect();
    assert_eq!(
        role_texts,
        vec![
            "assistant after retrograde",
            "just inside until",
            "at until"
        ]
    );
    let role_lines: Vec<u64> = role_events.iter().map(raw_line).collect();
    assert!(
        role_lines.windows(2).all(|w| w[0] < w[1]),
        "raw order preserved"
    );

    // tail applies after filters.
    let tail = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--role",
            "assistant",
            "--tail",
            "2",
            "--output",
            "jsonl",
        ],
    );
    let tail_events = tail.events();
    let tail_texts: Vec<&str> = tail_events.iter().map(ev_text).collect();
    assert_eq!(tail_texts, vec!["just inside until", "at until"]);

    // Invalid date syntax -> rc2.
    let bad = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--since",
            "2026-13-45T99:00:00Z",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(bad.code, 2, "{bad:#?}");
    assert!(bad.stderr.contains("invalid") || bad.stderr.contains("since"));
}

// A04 -----------------------------------------------------------------------

fn a04_file() -> String {
    let long_body = format!("{}{}", "x".repeat(20_000), "NEEDLE_AT_END");
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "a04",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "alpha first",
        "2026-10-01T00:00:01Z",
        None,
    )));
    // Internal reasoning must never appear even as context.
    body.push_str(&line(&json!({"timestamp": "2026-10-01T00:00:02Z", "type": "response_item",
        "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": "internal reasoning alpha"}]}})));
    body.push_str(&line(&codex_message(
        "assistant",
        "assistant beta",
        "2026-10-01T00:00:03Z",
        Some("final"),
    )));
    body.push_str(&line(&codex_message(
        "user",
        "user gamma",
        "2026-10-01T00:00:04Z",
        None,
    )));
    // developer record (system/developer excluded from messages hard scope).
    body.push_str(&line(
        &json!({"timestamp": "2026-10-01T00:00:05Z", "type": "response_item",
        "payload": {"type": "message", "role": "developer",
            "content": [{"type": "input_text", "text": "developer alpha instructions"}]}}),
    ));
    body.push_str(&line(&codex_message(
        "assistant",
        &long_body,
        "2026-10-01T00:00:06Z",
        Some("final"),
    )));
    body.push_str(&line(&codex_message(
        "user",
        "with\u{1b}[31mansi\u{1b}]0;osc\u{9b}31m tail",
        "2026-10-01T00:00:07Z",
        None,
    )));
    body
}

#[test]
fn a04_search_or_queries_context_and_hard_scope() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("a04.jsonl", &a04_file());

    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "alpha",
            "--query",
            "beta",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();

    // PRIMARY WINS CONTEXT: an event that matches a query is a primary hit even when it is also
    // the context window of another hit (alpha first). It must appear exactly once, as primary.
    let primary: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("is_context").and_then(Value::as_bool) != Some(true))
        .collect();
    let primary_texts: Vec<&str> = primary.iter().map(|e| ev_text(e)).collect::<Vec<_>>();
    assert_eq!(
        primary_texts,
        vec!["alpha first", "assistant beta"],
        "OR alpha+beta returns both primary, got {primary_texts:?}"
    );

    let alpha = primary[0];
    let matched: Vec<&str> = alpha["matched_queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(matched, vec!["alpha"]);
    let beta = primary[1];
    let matched: Vec<&str> = beta["matched_queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(matched, vec!["beta"]);
    // Same unique event beta must not also be emitted as context of alpha.
    let beta_is_context = beta
        .get("is_context")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        !beta_is_context,
        "a primary hit must not be re-labelled context"
    );
    // Overlap (assistant beta itself hits AND is in alpha's window) must appear once.
    let occurrences = events
        .iter()
        .filter(|e| ev_text(e) == "assistant beta")
        .count();
    assert_eq!(occurrences, 1, "overlapping context must appear once");

    // SEPARATE INVOCATION: the soft role filter must still be crossed by context.
    // `--role user` keeps only "alpha first" as a hit, and assistant beta must come back as
    // context (is_context=true) even though the role filter would skip it as a hit.
    let scoped = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "alpha",
            "--role",
            "user",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let scoped_events = scoped.events();
    let scoped_primary: Vec<&str> = scoped_events
        .iter()
        .filter(|e| e.get("is_context").and_then(Value::as_bool) != Some(true))
        .map(ev_text)
        .collect();
    assert_eq!(
        scoped_primary,
        vec!["alpha first"],
        "role=user primary must be only the user hit: {scoped_primary:?}"
    );
    let scoped_context: Vec<&Value> = scoped_events
        .iter()
        .filter(|e| e.get("is_context").and_then(Value::as_bool) == Some(true))
        .collect();
    let scoped_context_texts: Vec<&str> = scoped_context.iter().map(|e| ev_text(e)).collect();
    assert!(
        scoped_context_texts.contains(&"assistant beta"),
        "context must cross the soft role filter: {scoped_context_texts:?}"
    );
    // Hard scope still holds in the role-filtered invocation.
    for text in scoped_events.iter().map(ev_text) {
        assert!(
            !text.contains("internal reasoning"),
            "reasoning leaked under --role: {text}"
        );
        assert!(
            !text.contains("developer"),
            "developer record leaked under --role: {text}"
        );
    }

    // Hard scope: reasoning/developer never appear, even as context.
    for text in events.iter().map(ev_text) {
        assert!(
            !text.contains("internal reasoning"),
            "reasoning leaked: {text}"
        );
        assert!(
            !text.contains("developer"),
            "developer record leaked: {text}"
        );
    }

    // Whole-body search before preview cut: NEEDLE beyond the preview window is still found.
    let needle = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "NEEDLE_AT_END",
            "--output",
            "jsonl",
        ],
    );
    let hits = needle.events();
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    let complete = hit
        .get("text_complete")
        .and_then(Value::as_bool)
        .expect("text_complete");
    // A 20 KB body fits inside the 32 KiB window, so a complete preview is correct; the oracle's
    // earlier "must be cut" was an assumption about size, not a requirement. What matters is that
    // the late hit was found in the whole text and reported at its original offset.
    if !complete {
        assert!(
            ev_text(hit).len() < 20_000,
            "an incomplete preview must actually be shorter than the body"
        );
    }
    let ranges = hit["matched_ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 1);
    let start = ranges[0].get("start").and_then(Value::as_u64).unwrap();
    assert_eq!(
        start, 20_000,
        "matched_ranges points at the original text (UTF-8 byte offset)"
    );
    assert!(ranges[0].get("end").and_then(Value::as_u64).unwrap() > start);
    if !complete {
        assert!(
            !ev_text(hit).contains("NEEDLE_AT_END"),
            "a cut preview omits the far hit but reports its original range"
        );
    } else {
        assert!(
            ev_text(hit).contains("NEEDLE_AT_END"),
            "a complete preview must contain the whole body"
        );
    }

    // Invalid regex -> rc2.
    let bad = run(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "(",
            "--regex",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(bad.code, 2, "{bad:#?}");

    // Control characters must be escaped in text and JSONL output.
    let esc = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "ansi",
            "--output",
            "text",
        ],
    );
    assert!(
        !esc.stdout.contains('\u{1b}'),
        "raw ESC leaked into text output"
    );
    assert!(
        !esc.stdout.contains('\u{9b}'),
        "raw C1 leaked into text output"
    );
    let esc_json = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "ansi",
            "--output",
            "jsonl",
        ],
    );
    assert!(
        !esc_json.stdout.contains('\u{1b}'),
        "raw ESC leaked into jsonl output"
    );

    // Text output never mixes with JSONL prose on stdout.
    assert!(esc_json
        .stdout
        .lines()
        .all(|l| serde_json::from_str::<Value>(l).is_ok()));
}

// A05 -----------------------------------------------------------------------

#[test]
fn a05_tool_pairs_multiple_parallel_custom_claude_pending_and_budget() {
    let sandbox = Sandbox::new();
    let (codex_root, claude_root) = roots(&sandbox);

    let mut codex = String::new();
    codex.push_str(&line(&codex_session_meta(
        "a05",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    codex.push_str(&line(&codex_message(
        "user",
        "run two calls",
        "2026-10-01T00:00:01Z",
        None,
    )));
    // Parallel calls a and b.
    codex.push_str(&line(&codex_function_call(
        "shell",
        "call-a",
        "{\"cmd\":\"echo a\"}",
        "2026-10-01T00:00:02Z",
    )));
    codex.push_str(&line(&codex_function_call(
        "shell",
        "call-b",
        "{\"cmd\":\"echo b\"}",
        "2026-10-01T00:00:03Z",
    )));
    // b result arrives first (out of order).
    codex.push_str(&line(&codex_function_output(
        "call-b",
        "result-b",
        "2026-10-01T00:00:04Z",
    )));
    // a has TWO outputs.
    codex.push_str(&line(&codex_function_output(
        "call-a",
        "result-a1",
        "2026-10-01T00:00:05Z",
    )));
    codex.push_str(&line(&codex_function_output(
        "call-a",
        "result-a2",
        "2026-10-01T00:00:06Z",
    )));
    // custom call/output (legacy classifier drops these).
    codex.push_str(&line(&codex_custom_call(
        "apply_patch",
        "custom-1",
        "*** Begin Patch\n",
        "2026-10-01T00:00:07Z",
    )));
    codex.push_str(&line(&codex_custom_output(
        "custom-1",
        "Done!",
        "2026-10-01T00:00:08Z",
    )));
    // pending call c (no result until snapshot).
    codex.push_str(&line(&codex_function_call(
        "shell",
        "call-c",
        "{\"cmd\":\"sleep 9\"}",
        "2026-10-01T00:00:09Z",
    )));
    // orphan output (result without call).
    codex.push_str(&line(&codex_function_output(
        "call-orphan",
        "orphan result",
        "2026-10-01T00:00:10Z",
    )));
    // rc0 plus refusal body that must not be turned into task success.
    codex.push_str(&line(&codex_function_call(
        "shell",
        "call-refuse",
        "{\"cmd\":\"rm -rf /\"}",
        "2026-10-01T00:00:11Z",
    )));
    codex.push_str(&line(&json!({"timestamp": "2026-10-01T00:00:12Z", "type": "response_item",
        "payload": {"type": "function_call_output", "call_id": "call-refuse",
            "output": "{\"output\":\"command rejected by policy\",\"metadata\":{\"exit_code\":0}}"}})));
    fs::write(Path::new(&codex_root).join("rollout-a05.jsonl"), codex).unwrap();

    let mut claude = String::new();
    claude.push_str(&line(&claude_user(
        "claude-a05",
        "u1",
        "use the tool",
        "2026-10-01T01:00:00Z",
    )));
    claude.push_str(&line(&claude_tool_use(
        "claude-a05",
        "a1",
        "tool-x",
        "Bash",
        serde_json::json!({"command": "false"}),
        "2026-10-01T01:00:01Z",
    )));
    claude.push_str(&line(&claude_tool_result(
        "claude-a05",
        "u2",
        "tool-x",
        "command failed",
        true,
        "2026-10-01T01:00:02Z",
    )));
    // Same call id but different session: must never pair across sessions.
    claude.push_str(&line(&claude_tool_use(
        "claude-a05-other",
        "a2",
        "tool-x",
        "Bash",
        serde_json::json!({"command": "true"}),
        "2026-10-01T01:00:03Z",
    )));
    fs::write(Path::new(&claude_root).join("claude-a05.jsonl"), claude).unwrap();

    let input = Path::new(&codex_root).join("rollout-a05.jsonl");
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
    let _find = |text: &str| events.iter().find(|e| ev_text(e) == text).unwrap();
    // Tool calls carry their arguments as `text`; identify them by call_id instead.
    let by_call = |call_id: &str, kind: &str| {
        events
            .iter()
            .find(|e| e["call_id"].as_str() == Some(call_id) && ev_kind(e) == kind)
            .unwrap_or_else(|| panic!("missing {kind} for {call_id}"))
    };

    let call_a = by_call("call-a", "tool_call");
    let a_pairs: Vec<&str> = call_a["paired_event_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(a_pairs.len(), 2, "call-a has two outputs");
    assert_eq!(
        call_a.get("pair_state").and_then(Value::as_str),
        Some("paired")
    );

    let call_b = by_call("call-b", "tool_call");
    let b_pairs: Vec<&str> = call_b["paired_event_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(b_pairs.len(), 1);
    assert_eq!(
        call_b.get("pair_state").and_then(Value::as_str),
        Some("paired")
    );

    let call_c = by_call("call-c", "tool_call");
    assert_eq!(
        call_c.get("pair_state").and_then(Value::as_str),
        Some("pending_result")
    );

    let custom = by_call("custom-1", "tool_call");
    assert_eq!(
        custom.get("pair_state").and_then(Value::as_str),
        Some("paired")
    );
    assert_eq!(
        custom.get("tool_name").and_then(Value::as_str),
        Some("apply_patch")
    );

    let orphan = events
        .iter()
        .find(|e| e["call_id"].as_str() == Some("call-orphan"))
        .expect("orphan output visible");
    assert_eq!(
        orphan.get("pair_state").and_then(Value::as_str),
        Some("orphan")
    );

    // Refusal body retained; observed status not converted to task success. The output text is
    // the opaque provider payload, so match the call id and a substring rather than exact text.
    let refuse_out = events
        .iter()
        .find(|e| {
            e["call_id"].as_str() == Some("call-refuse")
                && ev_text(e).contains("command rejected by policy")
        })
        .expect("refusal output visible");
    assert!(refuse_out.get("tool_observed").is_some());

    let meta = out.metadata();
    assert!(meta["scan"]["with_pair"]["read_bytes"].as_u64().unwrap() > 0);
    assert!(meta["scan"]["with_pair"]["passes"].as_u64().unwrap() <= 2);

    // Budget: with a tiny --max-events the paired result cannot be rendered => not_rendered_budget.
    let budget = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--max-events",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let meta = budget.metadata();
    assert!(
        meta["counts"]["pairs"]["not_rendered_budget"]
            .as_u64()
            .unwrap()
            >= 1,
        "{meta:#?}"
    );

    // Claude: is_error preserved, namespace not crossed.
    let claude_out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            Path::new(&claude_root)
                .join("claude-a05.jsonl")
                .to_str()
                .unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let claude_events = claude_out.events();
    let result = claude_events
        .iter()
        .find(|e| ev_text(e) == "command failed")
        .unwrap();
    assert_eq!(result["tool_observed"]["is_error"].as_bool(), Some(true));
    let use_event = claude_events
        .iter()
        .find(|e| e["call_id"].as_str() == Some("tool-x") && ev_kind(e) == "tool_call")
        .unwrap();
    assert_eq!(
        use_event.get("session_id").and_then(Value::as_str),
        Some("claude-a05")
    );
    let pairs: Vec<&str> = use_event["paired_event_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(pairs.len(), 1, "only the same-session result may pair");
}

/// One Claude physical row that expands to three text subevents.
fn claude_three_subevent_row(texts: [&str; 3], ts: &str) -> Value {
    serde_json::json!({
        "type": "assistant", "uuid": "multi-1", "sessionId": "claude-multi",
        "timestamp": ts,
        "message": {"model": "claude-sonnet", "content": [
            {"type": "text", "text": texts[0]},
            {"type": "text", "text": texts[1]},
            {"type": "text", "text": texts[2]}
        ]}
    })
}

// A06 -----------------------------------------------------------------------

#[test]
fn a06_provenance_subevents_unknown_and_cut() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&claude_user(
        "claude-multi",
        "u1",
        "provenance question",
        "2026-10-01T00:00:00Z",
    )));
    let row =
        claude_three_subevent_row(["три", "second part", "third part"], "2026-10-01T00:00:01Z");
    let row_text = line(&row);
    let row_start = body.len() as u64;
    body.push_str(&row_text);
    // Unknown raw record type.
    let unknown_start = body.len() as u64;
    let unknown_line =
        line(&serde_json::json!({"type": "mystery-record", "payload": {"weird": true}}));
    body.push_str(&unknown_line);
    let input = sandbox.write("a06.jsonl", &body);

    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--output",
            "jsonl",
            "--max-output-bytes",
            "32768",
        ],
    );
    let events = out.events();
    // Fixture line layout: 1 = user question, 2 = the multi-block assistant row, 3 = unknown.
    // The earlier expectation of line 3 was a fixture/counting mistake, not a product defect.
    let multi: Vec<&Value> = events.iter().filter(|e| raw_line(e) == 2).collect();
    assert_eq!(multi.len(), 3, "one physical row -> three text subevents");
    for (index, event) in multi.iter().enumerate() {
        assert_eq!(sub_index(event), index as u64);
        let (start, end) = byte_range(event);
        assert_eq!(start, row_start);
        assert_eq!(end, row_start + row_text.len() as u64);
        assert!(!ev_id(event).is_empty(), "every event needs a stable id");
    }
    // The three IDs must be distinct and stable across a rerun.
    let ids: Vec<String> = multi.iter().map(|e| ev_id(e).to_string()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    let again = ok(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    let ids2: Vec<String> = again
        .events()
        .iter()
        .filter(|e| raw_line(e) == 2)
        .map(|e| ev_id(e).to_string())
        .collect();
    assert_eq!(ids, ids2);

    // Unknown record: counted, with a raw pointer, and NOT a silent zero.
    let meta = out.metadata();
    assert_eq!(count_field(&meta, &["counts", "unknown"], "events"), 1);
    let unknown = meta["counts"]["unknown"]["samples"][0].clone();
    assert_eq!(unknown["raw_line"].as_u64(), Some(3));
    let (ustart, uend) = (
        unknown["byte_start"].as_u64().unwrap(),
        unknown["byte_end"].as_u64().unwrap(),
    );
    assert_eq!(ustart, unknown_start);
    assert_eq!(uend, unknown_start + unknown_line.len() as u64);
    assert_eq!(status(&meta), "ok");

    // Display cut is marked: text_complete=false but source range still the whole row.
    let cut = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--event",
            &ids[1],
            "--output",
            "jsonl",
            "--max-output-bytes",
            "8192",
        ],
    );
    let cut_events = cut.events();
    assert_eq!(cut_events.len(), 1);
    assert!(cut_events[0]["text_complete"].is_boolean());

    // No text dedup: identical text in a different event id stays.
    let dedup_body = [
        line(&codex_session_meta(
            "dedup",
            "/repoA",
            "2026-10-01T00:00:00Z",
        )),
        line(&codex_message(
            "user",
            "repeat me",
            "2026-10-01T00:00:01Z",
            None,
        )),
        line(&codex_message(
            "assistant",
            "repeat me",
            "2026-10-01T00:00:02Z",
            Some("final"),
        )),
    ]
    .concat();
    let dedup_input = sandbox.write("dedup.jsonl", &dedup_body);
    let repeated = ok(
        &sandbox,
        &[
            "view",
            "-i",
            dedup_input.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        repeated
            .events()
            .iter()
            .filter(|e| ev_text(e) == "repeat me")
            .count(),
        2
    );
}

// A07 -----------------------------------------------------------------------

#[test]
fn a07_cursor_pages_through_subevents_then_appends() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&claude_user(
        "claude-multi",
        "u1",
        "first question",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&claude_three_subevent_row(
        ["p1", "p2", "p3"],
        "2026-10-01T00:00:01Z",
    )));
    let input = sandbox.write("a07.jsonl", &body);

    let mut cursor: Option<String> = None;
    let mut seen: Vec<String> = Vec::new();
    let mut pages = 0;
    loop {
        let mut args = vec![
            "view",
            "-i",
            input.to_str().unwrap(),
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
        pages += 1;
        assert!(pages <= 10, "pagination must terminate");
        let events = out.events();
        assert!(events.len() <= 1);
        for event in &events {
            seen.push(ev_id(event).to_string());
        }
        let meta = out.metadata();
        cursor = meta
            .get("page_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            assert_eq!(meta["exhausted"].as_bool(), Some(true));
            break;
        }
        // No append cursor before the snapshot is fully consumed.
        assert!(
            meta.get("append_cursor").and_then(Value::as_str).is_none(),
            "append cursor must not be issued mid-snapshot"
        );
    }
    assert_eq!(
        seen.len(),
        4,
        "three page events + one first question, no duplicates: {seen:?}"
    );
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(
        unique.len(),
        4,
        "duplicate event ids across pages: {seen:?}"
    );

    // Cut must not skip subevents: limit1 across the 3-subevent row yields indices 0,1,2 exactly once.
    let mut observed: Vec<(u64, u64)> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut args = vec![
            "view",
            "-i",
            input.to_str().unwrap(),
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
        for event in out.events() {
            // Fixture layout: 1 = user question, 2 = the 3-text block row. The earlier "3"
            // was an oracle line-number slip (fixture reality is 2), not a product defect.
            if raw_line(&event) == 2 {
                observed.push((raw_line(&event), sub_index(&event)));
            }
        }
        let meta = out.metadata();
        cursor = meta
            .get("page_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(observed, vec![(2, 0), (2, 1), (2, 2)]);

    // Full-snapshot tail issues an append cursor, then appends are read incrementally.
    let full = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--tail",
            "2",
            "--output",
            "jsonl",
        ],
    );
    let append = full
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .expect("tail after full scan issues append cursor")
        .to_string();

    // Append one complete row -> one new event.
    sandbox.append(
        &input,
        &line(&claude_assistant_text(
            "claude-multi",
            "a2",
            "appended one",
            "2026-10-01T00:00:05Z",
        )),
    );
    let first = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &append,
            "--output",
            "jsonl",
        ],
    );
    let first_events = first.events();
    assert_eq!(first_events.len(), 1, "{first:#?}");
    assert_eq!(ev_text(&first_events[0]), "appended one");
    let append = first
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .expect("reusable new append cursor")
        .to_string();

    // Append a partial line -> pending, zero events, checkpoint stays before the partial line.
    sandbox.append(
        &input,
        "{\"type\":\"assistant\",\"uuid\":\"a3\",\"sessionId\":\"claude-multi\",",
    );
    let pending = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &append,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(pending.events().len(), 0, "{pending:#?}");
    let meta = pending.metadata();
    assert_eq!(meta["pending_tail"].as_bool(), Some(true));
    assert_eq!(status(&meta), "ok");
    let append = meta
        .get("append_cursor")
        .and_then(Value::as_str)
        .expect("append cursor retained")
        .to_string();

    // Finish the partial line -> exactly one new event, no duplicate of the pending bytes.
    sandbox.append(&input, "\"timestamp\":\"2026-10-01T00:00:06Z\",\"message\":{\"model\":\"claude-sonnet\",\"content\":[{\"type\":\"text\",\"text\":\"appended two\"}]}}\n");
    let second = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &append,
            "--output",
            "jsonl",
        ],
    );
    let second_events = second.events();
    assert_eq!(second_events.len(), 1, "{second:#?}");
    assert_eq!(ev_text(&second_events[0]), "appended two");

    // Zero new appends still yields a valid reusable cursor.
    let next = second
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let none = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &next,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(none.events().len(), 0);
    assert!(none
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .is_some());
}

// A08 -----------------------------------------------------------------------

#[test]
fn a08_invalid_cursors_are_typed_errors() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("a08.jsonl", &simple_codex_file("a08", "/repoA"));
    let base = ok(
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
    let cursor = base
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .expect("page cursor")
        .to_string();

    let expect_invalid = |out: &Output, reason: &str| {
        assert_eq!(out.code, 1, "cursor errors are rc1: {out:#?}");
        let meta = out.metadata();
        assert_eq!(status(&meta), "error", "{meta:#?}");
        assert_eq!(
            meta["error"]["type"].as_str(),
            Some("cursor_invalid"),
            "{meta:#?}"
        );
        assert_eq!(meta["error"]["reason"].as_str(), Some(reason), "{meta:#?}");
    };

    // Garbage token.
    let bad = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            "not-a-cursor",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(bad.code, 1);
    assert_eq!(
        bad.metadata()["error"]["type"].as_str(),
        Some("cursor_invalid")
    );

    // Forged offsets / subevent.
    let forged = cursor.replace("\"subevent_index\":0", "\"subevent_index\":99");
    if forged != cursor {
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
        expect_invalid(&out, "offset_out_of_range");
    }
    let forged_line = cursor.replace("\"raw_line\":2", "\"raw_line\":999");
    if forged_line != cursor {
        let out = run(
            &sandbox,
            &[
                "view",
                "-i",
                input.to_str().unwrap(),
                "--cursor",
                &forged_line,
                "--output",
                "jsonl",
            ],
        );
        expect_invalid(&out, "offset_out_of_range");
    }

    // Wrong kind: append cursor used as page cursor.
    let full = ok(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    let append = full
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let wrong = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &append,
            "--output",
            "jsonl",
        ],
    );
    expect_invalid(&wrong, "wrong_cursor_kind");
    let wrong2 = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &cursor,
            "--output",
            "jsonl",
        ],
    );
    expect_invalid(&wrong2, "wrong_cursor_kind");

    // Conflicting semantic filter.
    let conflict = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &cursor,
            "--role",
            "assistant",
            "--output",
            "jsonl",
        ],
    );
    expect_invalid(&conflict, "filter_mismatch");

    // Truncated source.
    let truncated = sandbox.path().join("a08-trunc.jsonl");
    let body = fs::read_to_string(&input).unwrap();
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    // Whole-line prefixes only: a mid-line cut is a pending tail, not a truncated snapshot.
    let three_lines: String = lines.iter().take(3).copied().collect();
    let one_line: String = lines.iter().take(1).copied().collect();
    fs::write(&truncated, &three_lines).unwrap();
    let trunc_base = ok(
        &sandbox,
        &[
            "view",
            "-i",
            truncated.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    let trunc_cursor = trunc_base
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    fs::write(&truncated, &one_line).unwrap();
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            truncated.to_str().unwrap(),
            "--after-cursor",
            &trunc_cursor,
            "--output",
            "jsonl",
        ],
    );
    expect_invalid(&out, "truncated");

    // Same inode, anchor bytes edited in place (source replaced under the cursor).
    let anchor_input = sandbox.path().join("a08-anchor.jsonl");
    fs::write(&anchor_input, &body).unwrap();
    let staged = ok(
        &sandbox,
        &[
            "view",
            "-i",
            anchor_input.to_str().unwrap(),
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let anchor_cursor = staged
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let mut edited = body.clone();
    // Replace bytes just before the checkpoint with same-length different content.
    if let Some(pos) = edited.find("first question") {
        edited.replace_range(pos..pos + 3, "zzz");
    }
    fs::write(&anchor_input, &edited).unwrap();
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            anchor_input.to_str().unwrap(),
            "--cursor",
            &anchor_cursor,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        out.code, 1,
        "source replacement under page cursor must fail: {out:#?}"
    );
    assert_eq!(
        out.metadata()["error"]["type"].as_str(),
        Some("cursor_invalid")
    );

    // Copy+replace to a different inode -> generation mismatch.
    let replaced = sandbox.path().join("a08-replaced.jsonl");
    fs::write(&replaced, &body).unwrap();
    let rep_base = ok(
        &sandbox,
        &[
            "view",
            "-i",
            replaced.to_str().unwrap(),
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let rep_cursor = rep_base
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    fs::remove_file(&replaced).unwrap();
    fs::write(&replaced, &body).unwrap();
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            replaced.to_str().unwrap(),
            "--cursor",
            &rep_cursor,
            "--output",
            "jsonl",
        ],
    );
    expect_invalid(&out, "generation_mismatch");

    // No silent rewind: the source file is not modified by any cursor failure.
    assert_eq!(fs::read_to_string(&input).unwrap(), body);
}

// A09 -----------------------------------------------------------------------

#[test]
fn a09_budgets_record_limit_and_retry() {
    let sandbox = Sandbox::new();
    let huge_body = format!("{}\nTAIL_HIT_IN_HUGE_ROW", "y".repeat(1_048_576));
    let huge_text = format!("{}{}", huge_body, "END");
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "a09",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "before huge",
        "2026-10-01T00:00:01Z",
        None,
    )));
    let huge = line(&codex_message(
        "assistant",
        &huge_text,
        "2026-10-01T00:00:02Z",
        Some("final"),
    ));
    body.push_str(&huge);
    body.push_str(&line(&codex_message(
        "user",
        "after huge",
        "2026-10-01T00:00:03Z",
        None,
    )));
    let input = sandbox.write("a09.jsonl", &body);
    // Raw offsets computed independently from the serialized lines (never from product output).
    let line1_bytes = line(&codex_session_meta("a09", "/repoA", "2026-10-01T00:00:00Z"));
    let line2_bytes = line(&codex_message(
        "user",
        "before huge",
        "2026-10-01T00:00:01Z",
        None,
    ));
    let huge_start_expected = (line1_bytes.len() + line2_bytes.len()) as u64;
    let huge_end_expected = huge_start_expected + huge.len() as u64;
    let _ = huge;

    // Default record cap (1MiB) rejects the huge row: partial rc1 with uninspected range.
    let out = run(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(out.code, 1, "partial must be rc1: {out:#?}");
    let meta = out.metadata();
    assert_eq!(status(&meta), "partial");
    assert!(count_field(&meta, &["counts", "uninspected"], "records") >= 1);
    let uninspected = &meta["counts"]["uninspected"]["samples"][0];
    assert_eq!(uninspected["raw_line"].as_u64(), Some(3));
    assert!(uninspected["byte_start"].as_u64().unwrap() > 0);
    assert!(uninspected
        .get("retry_cursor")
        .and_then(Value::as_str)
        .is_some());
    assert!(
        meta.get("append_cursor").and_then(Value::as_str).is_none(),
        "no append cursor past an uninspected record"
    );
    // The event after the huge row is still NOT claimed as processed.
    assert!(!out.events().iter().any(|e| ev_text(e) == "after huge"));

    // 0.3 MVP has no explicit skip: the flag is rejected by clap as a usage error (rc2), so
    // there is no structured metadata to inspect — the contract is the exit code and a bounded
    // non-empty diagnostic, not a JSON row.
    let rejected = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--skip-uninspected",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(rejected.code, 2, "{rejected:#?}");
    assert!(
        !rejected.stderr.trim().is_empty() && rejected.stderr.len() < 2048,
        "clap usage error must be a bounded diagnostic: {rejected:#?}"
    );
    assert!(
        rejected.stdout.trim().is_empty(),
        "rc2 argument error must not emit partial JSONL on stdout: {rejected:#?}"
    );
    assert!(!out.events().iter().any(|e| ev_text(e) == "after huge"));

    // Raise the record limit: the omitted record is read from its original range.
    let retried = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--max-record-bytes",
            "2097152",
            "--output",
            "jsonl",
        ],
    );
    // Locate the huge record by its KNOWN raw line (3) and exact independently computed range;
    // a bounded preview legitimately omits the tail, so the event is located by provenance.
    let huge_event = retried
        .events()
        .into_iter()
        .find(|e| raw_line(e) == 3)
        .expect("huge row readable at raw line 3");
    let (huge_start, huge_end) = byte_range(&huge_event);
    assert_eq!(
        (huge_start, huge_end),
        (huge_start_expected, huge_end_expected),
        "huge record must report its original byte range"
    );
    // Display is bounded: the far tail is not claimed as fully rendered.
    let huge_complete = huge_event
        .get("text_complete")
        .and_then(Value::as_bool)
        .expect("text_complete");
    assert!(
        !huge_complete,
        "a >1MiB body cannot be fully rendered in the default budget: {huge_event:#?}"
    );
    // Raw read-back: slice the source file with the reported range and prove END + late hit are
    // really there — the pointer is usable, not merely well-formed.
    let raw_bytes = fs::read(&input).unwrap();
    let raw_slice = &raw_bytes[huge_start as usize..huge_end as usize];
    let raw_text = String::from_utf8_lossy(raw_slice);
    assert!(
        raw_text.contains("TAIL_HIT_IN_HUGE_ROW"),
        "reported range must contain the late hit"
    );
    assert!(
        raw_text.contains("END"),
        "reported range must contain the record END"
    );
    assert!(
        raw_slice.ends_with(b"\n"),
        "raw range must cover the complete physical line including its newline"
    );
    assert!(retried.metadata().get("append_cursor").is_some());
    // Whole-text search finds the late hit once the record is processable, and points at the
    // original text via an independent matched range (beyond the 32 KiB preview).
    let searched = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "TAIL_HIT_IN_HUGE_ROW",
            "--max-record-bytes",
            "2097152",
            "--output",
            "jsonl",
        ],
    );
    let hits = searched.events();
    assert_eq!(hits.len(), 1, "{searched:#?}");
    let hit = &hits[0];
    let ranges = hit["matched_ranges"].as_array().expect("matched_ranges");
    assert_eq!(ranges.len(), 1, "{hit:#?}");
    let m_start = ranges[0]["start"].as_u64().unwrap();
    // The text is "y"*1MiB + "\n" + "TAIL_HIT_IN_HUGE_ROW" + "END"; the hit starts right after
    // those bytes, i.e. beyond any 32 KiB preview cut.
    assert_eq!(
        m_start,
        (1_048_576 + 1) as u64,
        "matched range must point into the original text past the preview"
    );
    assert!(
        m_start as usize > 32 * 1024,
        "hit must lie beyond the 32 KiB preview window"
    );
    // The preview itself does not contain the needle: the match came from whole-record search.
    assert!(
        !ev_text(hit).contains("TAIL_HIT_IN_HUGE_ROW"),
        "preview intentionally omits the far hit; only matched_ranges locates it"
    );

    // Output budget: stdout+stderr within --max-output-bytes and all JSONL valid.
    // The record cap must be >= the >1MiB row, otherwise partiality (not budget) would be measured.
    for budget in ["8192", "32768"] {
        let out = ok(
            &sandbox,
            &[
                "view",
                "-i",
                input.to_str().unwrap(),
                "--max-record-bytes",
                "2097152",
                "--max-output-bytes",
                budget,
                "--output",
                "jsonl",
                "--tail",
                "5",
            ],
        );
        assert!(
            out.total_bytes() <= budget.parse::<usize>().unwrap(),
            "{budget}: {} bytes",
            out.total_bytes()
        );
        assert!(
            !out.stdout.trim().is_empty(),
            "{budget}: a successful bounded response must not be silently empty"
        );
        let _ = out.metadata();
    }
    // Budget must ALSO be honest when the row is not processable: partial rc1, never a truncated
    // JSONL or a fake rc0 under the default 1MiB cap.
    let under_cap = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--max-output-bytes",
            "8192",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        under_cap.code, 1,
        "unprocessable row under the default cap is partial rc1: {under_cap:#?}"
    );
    assert_eq!(status(&under_cap.metadata()), "partial", "{under_cap:#?}");
    assert!(under_cap.total_bytes() <= 8192, "{under_cap:#?}");
    for l in under_cap.stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            serde_json::from_str::<Value>(l).is_ok(),
            "partial output must stay valid JSONL: {l}"
        );
    }

    // Long query + long name payload: the exact envelope may legitimately exceed 8192, in which
    // case the product must refuse with bounded `budget_too_small` rc2 (never silent empty or
    // truncated JSON); 32768 must then succeed.
    let long_q = "q".repeat(1800);
    let long_args = [
        "search",
        "-i",
        input.to_str().unwrap(),
        "--query",
        &long_q,
        "--max-record-bytes",
        "2097152",
        "--max-output-bytes",
        "8192",
        "--output",
        "jsonl",
    ];
    let narrow = run(&sandbox, &long_args);
    match narrow.code {
        0 => {
            assert!(
                narrow.total_bytes() <= 8192,
                "long query envelope: {} bytes",
                narrow.total_bytes()
            );
            assert_eq!(status(&narrow.metadata()), "ok");
        }
        2 => {
            // Exact-envelope refusal must be explicit and bounded, not a silent empty success.
            let diag = format!("{}{}", narrow.stderr, narrow.stdout);
            assert!(
                !diag.trim().is_empty() && diag.len() < 2048,
                "budget refusal needs a bounded diagnostic: {narrow:#?}"
            );
            assert!(
                narrow.stdout.trim().is_empty(),
                "budget refusal must not emit partial JSONL: {narrow:#?}"
            );
        }
        other => panic!("long query must be rc0 within budget or rc2 budget_too_small, got {other}: {narrow:#?}"),
    }
    let wide = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            &long_q,
            "--max-record-bytes",
            "2097152",
            "--max-output-bytes",
            "32768",
            "--output",
            "jsonl",
        ],
    );
    assert!(
        !wide.stdout.trim().is_empty(),
        "32768 must succeed with a structured response: {wide:#?}"
    );
    let _ = wide.metadata();

    // Tiny budget rejected before reading (rc2), with a short bounded diagnostic.
    let tiny = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--max-output-bytes",
            "1024",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(tiny.code, 2, "{tiny:#?}");
    assert!(tiny.stderr.len() < 2048);

    // Display cut (limit) is not input partiality, but a bounded lookahead only KNOWS about the
    // next records: require a page continuation plus a declared remainder, not an exact count.
    let display = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--max-record-bytes",
            "2097152",
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let meta = display.metadata();
    assert_eq!(status(&meta), "ok");
    assert!(
        meta.get("page_cursor").and_then(Value::as_str).is_some(),
        "limit 1 with remaining records must offer a continuation: {meta:#?}"
    );
    assert_eq!(
        meta["exhausted"].as_bool(),
        Some(false),
        "exhausted must be false when the snapshot was not fully consumed: {meta:#?}"
    );
    assert!(
        meta["not_rendered"]["primary_events"].as_u64().unwrap() >= 1,
        "at least one known unrendered record must be declared: {meta:#?}"
    );
}

// A10 -----------------------------------------------------------------------

#[test]
fn a10_pipe_empty_error_and_broken_pipe() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("a10.jsonl", &simple_codex_file("a10", "/repoA"));

    // Legitimate empty result: rc0 + metadata only.
    let empty = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--role",
            "system",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(empty.events().len(), 0);
    let meta = empty.metadata();
    assert_eq!(status(&meta), "ok");
    assert_eq!(meta["exhausted"].as_bool(), Some(true));

    // Missing source: rc1 with machine-readable error, no prose on stdout.
    let missing = run(
        &sandbox,
        &[
            "view",
            "-i",
            "/nonexistent/session.jsonl",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(missing.code, 1);
    assert_eq!(status(&missing.metadata()), "error");
    assert!(missing
        .stdout
        .lines()
        .all(|l| serde_json::from_str::<Value>(l).is_ok()));

    // Query syntax error: rc2.
    let syntax = run(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--limit", "abc"],
    );
    assert_eq!(syntax.code, 2);

    // Query-local rejection of legacy output flags: rc2 (while legacy path accepts them).
    let legacy_flag = run(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--format-out", "md"],
    );
    assert_eq!(legacy_flag.code, 2, "{legacy_flag:#?}");
    let legacy_dir = sandbox.path().join("legacy_out");
    let legacy_ok = ok(
        &sandbox,
        &[
            "-i",
            input.to_str().unwrap(),
            "-o",
            legacy_dir.to_str().unwrap(),
            "--format-out",
            "md",
        ],
    );
    assert!(legacy_ok.stdout.contains("format: codex"));
    assert!(legacy_dir.join("a10.transcript.md").exists());

    // Broken pipe: consumer closes stdout early; must not print a panic.
    let mut child = Command::new(BIN)
        .env("HOME", sandbox.home())
        .args([
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "question",
            "--output",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdout = child.stdout.take().unwrap();
        let mut buf = [0u8; 16];
        let _ = stdout.read(&mut buf).unwrap();
        drop(stdout);
    }
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "panic on broken pipe: {stderr}"
    );

    // Diagnostics stay bounded even for a malformed source.
    let malformed = sandbox.write("malformed.jsonl", "{bad}\n{badder}\n");
    let mal = run(
        &sandbox,
        &[
            "view",
            "-i",
            malformed.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(mal.code, 1);
    assert!(mal.total_bytes() <= 32768);
}

// A11 -----------------------------------------------------------------------

#[test]
fn a11_legacy_outputs_and_exit_codes_stable() {
    let sandbox = Sandbox::new();
    for (fixture, expected_rows) in [
        ("codex_sample.jsonl", 5usize),
        ("claude_sample.jsonl", 5),
        ("opencode_sample.jsonl", 8),
    ] {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let out_dir = sandbox.path().join(format!("legacy-{fixture}"));
        let out = ok(
            &sandbox,
            &["-i", src.to_str().unwrap(), "-o", out_dir.to_str().unwrap()],
        );
        assert!(out.stdout.contains("format:"), "{fixture}: {}", out.stdout);
        let base = fixture.trim_end_matches(".jsonl");
        let clean = fs::read_to_string(out_dir.join(format!("{base}.clean.jsonl"))).unwrap();
        let rows = clean.lines().count();
        assert_eq!(rows, expected_rows, "{fixture} kept_events changed");
        assert!(clean.contains("\"ts\""), "legacy clean schema must keep ts");
        assert!(
            !clean.contains("\"record_type\""),
            "legacy schema must not gain viewer fields"
        );
        assert!(!clean.contains("schema_version"), "legacy schema untouched");
    }
    // The default output is clean JSONL only.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_sample.jsonl");
    let default_dir = sandbox.path().join("legacy-default");
    ok(
        &sandbox,
        &[
            "-i",
            src.to_str().unwrap(),
            "-o",
            default_dir.to_str().unwrap(),
        ],
    );
    assert!(default_dir.join("codex_sample.clean.jsonl").exists());
    assert!(!default_dir.join("codex_sample.transcript.md").exists());
    assert!(!default_dir.join("codex_sample.summary.json").exists());

    // Explicit output selection: clean, transcript, summary.
    let cases: [(&[&str], bool, bool, bool); 9] = [
        (&["--format-out", "jsonl"], true, false, false),
        (&["--format-out", "md"], false, true, false),
        (&["--format-out", "both"], true, true, false),
        (&["--summary"], true, false, true),
        (&["--format-out", "both", "--summary"], true, true, true),
        (&["--format-out", "md", "--summary"], false, true, true),
        (&["--channel", "api"], true, false, false),
        (&["--no-dedup"], true, false, false),
        (&["--msg-chars", "10"], true, false, false),
    ];
    for (index, (extra, clean, markdown, summary)) in cases.iter().enumerate() {
        let out_dir = sandbox.path().join(format!("flag-{index}"));
        let mut args = vec!["-i", src.to_str().unwrap(), "-o", out_dir.to_str().unwrap()];
        args.extend(extra.iter().copied());
        ok(&sandbox, &args);
        assert_eq!(
            out_dir.join("codex_sample.clean.jsonl").exists(),
            *clean,
            "clean output for {extra:?}"
        );
        assert_eq!(
            out_dir.join("codex_sample.transcript.md").exists(),
            *markdown,
            "transcript output for {extra:?}"
        );
        assert_eq!(
            out_dir.join("codex_sample.summary.json").exists(),
            *summary,
            "summary output for {extra:?}"
        );
    }
    // Missing input keeps legacy error exit 1.
    let missing = run(
        &sandbox,
        &["-i", "/nope.jsonl", "-o", sandbox.path().to_str().unwrap()],
    );
    assert_eq!(missing.code, 1);
}

fn installer_home(case: &str, claude: bool, codex: bool, existing: &str) -> (Sandbox, String) {
    let sandbox = Sandbox::new();
    let home = sandbox.home();
    for (present, dir) in [(claude, ".claude"), (codex, ".codex")] {
        if present {
            let skill = home.join(dir).join("skills/agent-jsonl-compact-reader");
            fs::create_dir_all(&skill).unwrap();
            if !existing.is_empty() {
                fs::write(skill.join("SKILL.md"), existing).unwrap();
            }
        }
    }
    let _ = case;
    let home_str = home.to_string_lossy().into_owned();
    (sandbox, home_str)
}

#[test]
fn a11_installer_matrix_nine_cases() {
    // Nine isolated homes covering: both present, codex only, claude only, absent, explicit
    // flags, and preservation of existing content.
    let cases: Vec<(&str, bool, bool, &str, Option<&str>)> = vec![
        ("both-present", true, true, "", None),
        ("codex-only-home", false, true, "", None),
        ("claude-only-home", true, false, "", None),
        (
            "neither-explicit-claude",
            false,
            false,
            "",
            Some("--claude-only"),
        ),
        (
            "neither-explicit-codex",
            false,
            false,
            "",
            Some("--codex-only"),
        ),
        ("neither-default", false, false, "", None),
        ("same-content", true, true, "SENTINEL-SAME", None),
        ("different-content", true, true, "CUSTOM-USER-SKILL", None),
        ("claude-present-codex-absent", true, false, "", None),
    ];
    assert_eq!(cases.len(), 9);
    for (case, claude_present, codex_present, existing, flag) in cases {
        let (sandbox, home) = installer_home(case, claude_present, codex_present, existing);
        let mut args = vec!["install-skills"];
        if let Some(flag) = flag {
            args.push(flag);
        }
        let out = run(&sandbox, &args);
        let claude_skill =
            Path::new(&home).join(".claude/skills/agent-jsonl-compact-reader/SKILL.md");
        let codex_skill =
            Path::new(&home).join(".codex/skills/agent-jsonl-compact-reader/SKILL.md");
        let explicit = flag.is_some();
        let expect_claude = !args.contains(&"--codex-only") && (claude_present || explicit);
        let expect_codex = !args.contains(&"--claude-only") && (codex_present || explicit);
        assert_eq!(out.code, 0, "{case}: {}", out.stderr);
        assert_eq!(claude_skill.exists(), expect_claude, "{case} claude");
        assert_eq!(codex_skill.exists(), expect_codex, "{case} codex");
        if claude_skill.exists() {
            let body = fs::read_to_string(&claude_skill).unwrap();
            if existing.is_empty() {
                assert!(body.contains("agent-jsonl-compact"), "{case}");
            } else {
                assert_eq!(body, existing, "{case}");
            }
        }
        if !expect_claude && claude_present {
            assert!(out.stdout.contains("skipped"), "{case}: {}", out.stdout);
        }
    }
    // Preserve user edits unless replacement is explicitly requested.
    let (sandbox, home) = installer_home("different-content", true, false, "CUSTOM-USER-SKILL");
    ok(&sandbox, &["install-skills"]);
    let body = fs::read_to_string(
        Path::new(&home).join(".claude/skills/agent-jsonl-compact-reader/SKILL.md"),
    )
    .unwrap();
    assert_eq!(body, "CUSTOM-USER-SKILL");
    ok(&sandbox, &["install-skills", "--force"]);
    let body = fs::read_to_string(
        Path::new(&home).join(".claude/skills/agent-jsonl-compact-reader/SKILL.md"),
    )
    .unwrap();
    assert!(body.contains("name: agent-jsonl-compact-reader"));
}

#[test]
fn a11_version_consistency() {
    let sandbox = Sandbox::new();
    let cargo_version = env!("CARGO_PKG_VERSION");
    let plain = ok(&sandbox, &["--version"]);
    assert_eq!(
        plain.stdout.trim(),
        format!("agent-jsonl-compact {cargo_version}")
    );
    let json = ok(&sandbox, &["version", "--output", "json"]);
    let value: Value = serde_json::from_str(json.stdout.trim()).unwrap();
    assert_eq!(value["version"].as_str(), Some(cargo_version));
    assert_eq!(value["source_release_date"].as_str(), Some("2026-10-23"));
    assert_eq!(value["query_schema_version"].as_u64(), Some(1));
    let changelog =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains(cargo_version),
        "CHANGELOG must mention {cargo_version}"
    );
    assert!(changelog.contains("2026-10-23"));
    let cargo_toml =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains(&format!("version = \"{cargo_version}\"")));
}

// A12 -----------------------------------------------------------------------

#[test]
fn a12_integrity_bad_middle_pending_tail_and_unknown_format() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "a12",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "valid before",
        "2026-10-01T00:00:01Z",
        None,
    )));
    body.push_str("{not json at all}\n");
    body.push_str(&line(&codex_message(
        "user",
        "valid after",
        "2026-10-01T00:00:03Z",
        None,
    )));
    let input = sandbox.write("a12-bad.jsonl", &body);

    let before_bytes = fs::read(&input).unwrap();
    let before_hash = sha256_hex(&before_bytes);
    let before_mtime = fs::metadata(&input).unwrap().modified().unwrap();

    let out = run(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(
        out.code, 1,
        "invalid middle row is a failure, not silent success: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "partial");
    assert_eq!(count_field(&meta, &["counts", "invalid"], "records"), 1);
    assert_eq!(
        meta["counts"]["invalid"]["samples"][0]["raw_line"].as_u64(),
        Some(3)
    );
    assert!(
        meta.get("append_cursor").and_then(Value::as_str).is_none(),
        "invalid middle row blocks append cursor"
    );

    // Pending tail is distinguishable and does not poison the checkpoint.
    let pending = sandbox.write(
        "a12-pending.jsonl",
        &format!(
            "{}{}",
            line(&codex_session_meta(
                "a12p",
                "/repoA",
                "2026-10-01T00:00:00Z"
            )),
            "{\"type\":\"response_item\""
        ),
    );
    let pend = ok(
        &sandbox,
        &["view", "-i", pending.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(pend.metadata()["pending_tail"].as_bool(), Some(true));
    assert_eq!(status(&pend.metadata()), "ok");

    // Unknown-only format: format_uncertain, never a silent Codex 0-event success.
    // This file contains NO recognised provider record, so rc1 is correct.
    let mut unknown = String::new();
    for i in 0..50 {
        unknown.push_str(&line(&serde_json::json!({"kind": "mystery", "n": i})));
    }
    let unknown_path = sandbox.write("a12-unknown.jsonl", &unknown);
    let auto = run(
        &sandbox,
        &[
            "view",
            "-i",
            unknown_path.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        auto.code, 1,
        "uncertain format must not be rc0 success: {auto:#?}"
    );
    let meta = auto.metadata();
    assert_eq!(status(&meta), "error");
    assert_eq!(meta["error"]["type"].as_str(), Some("format_uncertain"));
    assert_eq!(
        count_field(&meta, &["counts", "unknown"], "events"),
        50,
        "all 50 unknown records must be counted: {meta:#?}"
    );
    assert_eq!(
        meta["counts"]["unknown"]["samples"]
            .as_array()
            .map(Vec::len),
        Some(8),
        "unknown raw samples must be exposed (bounded): {meta:#?}"
    );
    // Every sample must carry a positive, in-range raw pointer.
    for sample in meta["counts"]["unknown"]["samples"].as_array().unwrap() {
        let line = sample["raw_line"].as_u64().expect("sample raw_line");
        assert!((1..=50).contains(&line), "sample outside file: {sample:#?}");
        let start = sample["byte_start"].as_u64().expect("sample byte_start");
        let end = sample["byte_end"].as_u64().expect("sample byte_end");
        assert!(end > start, "sample range must be non-empty: {sample:#?}");
    }

    // 50 unknown records FOLLOWED BY a real Claude body is a different file: auto must
    // recognise Claude, inspect every record, and succeed (rc0) with body available and
    // the unknown count/origin still reported. Unknown metadata alone is not a provider conflict.
    let mut unknown_then_claude = unknown.clone();
    unknown_then_claude.push_str(&line(&claude_user(
        "claude-after-unknown",
        "u1",
        "claude body after 50 unknowns",
        "2026-10-01T00:00:00Z",
    )));
    let delayed_path = sandbox.write("a12-unknown-then-claude.jsonl", &unknown_then_claude);
    let delayed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            delayed_path.to_str().unwrap(),
            "--output",
            "jsonl",
        ],
    );
    assert!(
        delayed
            .events()
            .iter()
            .any(|e| ev_text(e) == "claude body after 50 unknowns"),
        "auto must find the Claude body after 50 unknown rows: {delayed:#?}"
    );
    let delayed_meta = delayed.metadata();
    assert_eq!(
        status(&delayed_meta),
        "ok",
        "all records inspected, one recognised provider -> ok: {delayed_meta:#?}"
    );
    assert_eq!(
        count_field(&delayed_meta, &["counts", "unknown"], "events"),
        50,
        "unknown coverage must survive a recognised provider: {delayed_meta:#?}"
    );

    // Explicit format resolves the same file and shows the Claude body.
    let explicit = ok(
        &sandbox,
        &[
            "view",
            "-i",
            delayed_path.to_str().unwrap(),
            "--format",
            "claude_code",
            "--output",
            "jsonl",
        ],
    );
    assert!(explicit
        .events()
        .iter()
        .any(|e| ev_text(e) == "claude body after 50 unknowns"));

    // Conflicting providers in one file -> explicit inconsistency, not a guess.
    let mixed = sandbox.write(
        "a12-mixed.jsonl",
        &format!(
            "{}{}{}",
            line(&codex_session_meta("mix", "/repoA", "2026-10-01T00:00:00Z")),
            line(&codex_message(
                "user",
                "codex body",
                "2026-10-01T00:00:01Z",
                None
            )),
            line(&claude_user(
                "claude-mix",
                "u1",
                "claude body",
                "2026-10-01T00:00:02Z"
            ))
        ),
    );
    let mix = run(
        &sandbox,
        &["view", "-i", mixed.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(mix.code, 1);
    let meta = mix.metadata();
    assert_eq!(
        meta["error"]["type"].as_str(),
        Some("format_inconsistent"),
        "{meta:#?}"
    );

    // Source bytes/hash/mtime unchanged by reading; no cache/index files created.
    let after_bytes = fs::read(&input).unwrap();
    assert_eq!(sha256_hex(&after_bytes), before_hash);
    assert_eq!(
        fs::metadata(&input).unwrap().modified().unwrap(),
        before_mtime
    );
    let mut entries: Vec<String> = fs::read_dir(sandbox.home())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert!(entries.is_empty(), "no files created in HOME: {entries:?}");

    // Observable concurrent mutation -> inconsistent rc1.
    let mutable = sandbox.write("a12-mutable.jsonl", &simple_codex_file("a12m", "/repoA"));
    let staged = ok(
        &sandbox,
        &[
            "view",
            "-i",
            mutable.to_str().unwrap(),
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let cursor = staged
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let _ = fs::OpenOptions::new().append(true).open(&mutable);
    fs::write(&mutable, fs::read_to_string(&mutable).unwrap()).unwrap();
    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            mutable.to_str().unwrap(),
            "--cursor",
            &cursor,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 1);
    assert_eq!(
        out.metadata()["error"]["type"].as_str(),
        Some("cursor_invalid")
    );
}

// F06 regressions (test-oracles-1021) ---------------------------------------
//
// These cover already-agreed F06 behaviour and are kept independent of the
// concrete cursor-token encoding: they only pass the token back verbatim and
// assert on metadata/events. No token is constructed or forged here.

/// A mid-file append between two page requests must not leak into the in-flight snapshot;
/// the continuation still serves the OLD snapshot, and only a later `--after-cursor`
/// request (taking a new snapshot) sees the appended rows.
#[test]
fn f06_append_between_pages_preserves_old_snapshot_then_append_reads_new() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "f06-append",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for i in 0..4 {
        body.push_str(&line(&codex_message(
            "user",
            &format!("page body {i}"),
            &format!("2026-10-01T00:00:0{}Z", i + 1),
            None,
        )));
    }
    let input = sandbox.write("f06-append.jsonl", &body);
    let snapshot_len = fs::metadata(&input).unwrap().len();

    let first = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--limit",
            "2",
            "--output",
            "jsonl",
        ],
    );
    let first_events = first.events();
    let first_ids: Vec<String> = first_events.iter().map(|e| ev_id(e).to_string()).collect();
    assert_eq!(first_ids.len(), 2, "{first:#?}");
    let first_meta = first.metadata();
    let page_cursor = first_meta
        .get("page_cursor")
        .and_then(Value::as_str)
        .expect("page cursor after limit 2 of 5 rows")
        .to_string();
    assert!(
        first_meta
            .get("append_cursor")
            .and_then(Value::as_str)
            .is_none(),
        "append cursor must not be issued while pages remain"
    );

    // Concurrent append AFTER the first page: must not appear in the in-flight snapshot.
    sandbox.append(
        &input,
        &line(&codex_message(
            "user",
            "appended between pages",
            "2026-10-01T00:01:00Z",
            None,
        )),
    );

    let mut seen: Vec<String> = first_ids.clone();
    let mut cursor = Some(page_cursor);
    let mut pages = 1;
    let mut final_append_cursor = None;
    while let Some(token) = cursor {
        pages += 1;
        assert!(pages <= 10, "pagination must terminate: {seen:?}");
        let out = ok(
            &sandbox,
            &[
                "view",
                "-i",
                input.to_str().unwrap(),
                "--cursor",
                &token,
                "--output",
                "jsonl",
            ],
        );
        for event in out.events() {
            assert_ne!(
                ev_text(&event),
                "appended between pages",
                "append leaked into the old snapshot: {event:#?}"
            );
            seen.push(ev_id(&event).to_string());
        }
        let meta = out.metadata();
        cursor = meta
            .get("page_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            final_append_cursor = meta
                .get("append_cursor")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }
    let unique: std::collections::BTreeSet<&String> = seen.iter().collect();
    assert_eq!(
        unique.len(),
        seen.len(),
        "duplicate ids across pages: {seen:?}"
    );
    assert_eq!(seen.len(), 4, "old snapshot has exactly 4 bodies: {seen:?}");
    let append_cursor = final_append_cursor.expect("append cursor once snapshot exhausted");

    // The append cursor was taken at the old snapshot boundary (no page continuation left).
    // Reading it now sees exactly the appended row.
    let appended = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &append_cursor,
            "--output",
            "jsonl",
        ],
    );
    let appended_events = appended.events();
    let appended_texts: Vec<&str> = appended_events.iter().map(ev_text).collect();
    assert_eq!(
        appended_texts,
        vec!["appended between pages"],
        "append must yield exactly the new row: {appended:#?}"
    );
    // The source file is now larger than the snapshot the earlier cursor was built on.
    assert!(fs::metadata(&input).unwrap().len() > snapshot_len);
}

/// A search cursor resumes with the stored query when `--query` is omitted; supplying a
/// different query must be rejected as a semantic-filter conflict.
#[test]
fn f06_search_cursor_reuses_query_and_rejects_conflicting_query() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "f06-search",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for (i, text) in ["alpha one", "alpha two", "alpha three", "plain body"]
        .iter()
        .enumerate()
    {
        body.push_str(&line(&codex_message(
            "user",
            text,
            &format!("2026-10-01T00:00:0{}Z", i + 1),
            None,
        )));
    }
    let input = sandbox.write("f06-search.jsonl", &body);

    let first = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "alpha",
            "--limit",
            "2",
            "--output",
            "jsonl",
        ],
    );
    let first_events = first.events();
    let first_hits: Vec<&str> = first_events.iter().map(ev_text).collect();
    assert_eq!(first_hits, vec!["alpha one", "alpha two"], "{first:#?}");
    let cursor = first
        .metadata()
        .get("page_cursor")
        .and_then(Value::as_str)
        .expect("search page cursor")
        .to_string();

    // Omitted --query: the stored filter is reused.
    let resumed = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &cursor,
            "--output",
            "jsonl",
        ],
    );
    let resumed_events = resumed.events();
    let resumed_hits: Vec<&str> = resumed_events.iter().map(ev_text).collect();
    assert_eq!(
        resumed_hits,
        vec!["alpha three"],
        "resume must reuse query=alpha: {resumed:#?}"
    );

    // Explicit different query: semantic-filter conflict, not a silent re-evaluation.
    let conflict = run(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &cursor,
            "--query",
            "beta",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(conflict.code, 1, "conflicting query is rc1: {conflict:#?}");
    let meta = conflict.metadata();
    assert_eq!(
        meta["error"]["type"].as_str(),
        Some("cursor_invalid"),
        "{meta:#?}"
    );
    assert_eq!(
        meta["error"]["reason"].as_str(),
        Some("filter_mismatch"),
        "{meta:#?}"
    );
    // The same explicit-but-identical query is accepted (no false conflict).
    let same = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            &cursor,
            "--query",
            "alpha",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        same.events().iter().map(ev_text).collect::<Vec<_>>(),
        vec!["alpha three"]
    );
}

/// Truncation that cuts a line in the middle (not only on a whole-line boundary) must still
/// be detected as a truncated source rather than silently re-read or reported as pending.
#[test]
fn f06_cursor_truncation_mid_line_is_truncated() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "f06-trunc",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for i in 0..4 {
        body.push_str(&line(&codex_message(
            "user",
            &format!("trunc body {i}"),
            &format!("2026-10-01T00:00:0{}Z", i + 1),
            None,
        )));
    }
    let input = sandbox.write("f06-trunc.jsonl", &body);

    let base = ok(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    let append_cursor = base
        .metadata()
        .get("append_cursor")
        .and_then(Value::as_str)
        .expect("append cursor after full scan")
        .to_string();
    let original = fs::read_to_string(&input).unwrap();

    // Cut in the MIDDLE of the 3rd line (not on a '\n' boundary).
    let lines: Vec<&str> = original.split_inclusive('\n').collect();
    let cut_at = lines[0].len() + lines[1].len() + lines[2].len() / 2;
    assert!(
        !original.is_char_boundary(cut_at) || original.as_bytes()[cut_at - 1] != b'\n',
        "fixture must cut mid-line"
    );
    fs::write(&input, &original[..cut_at]).unwrap();

    let out = run(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &append_cursor,
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(out.code, 1, "mid-line truncation is rc1: {out:#?}");
    let meta = out.metadata();
    assert_eq!(
        meta["error"]["type"].as_str(),
        Some("cursor_invalid"),
        "{meta:#?}"
    );
    assert_eq!(
        meta["error"]["reason"].as_str(),
        Some("truncated"),
        "mid-line as well as whole-line cuts must report truncated: {meta:#?}"
    );
}

/// Metadata scanning must compute "latest activity" from the whole file, not a bounded
/// preview: a >256 KiB first record followed by a late update must still be visible, and a
/// huge first record must not be silently dropped under the default record cap.
#[test]
fn f06_metadata_latest_activity_beyond_preview_and_huge_first_record() {
    let sandbox = Sandbox::new();
    let (codex_root, _) = roots(&sandbox);

    // 300 KiB first record (well above any 256 KiB preview window, below the 1 MiB record cap).
    let huge = "H".repeat(300 * 1024);
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "f06-huge",
        "/repoHuge",
        "2026-09-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        &huge,
        "2026-09-01T00:00:01Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "late answer",
        "2026-10-21T11:00:00Z",
        Some("final"),
    )));
    fs::write(Path::new(&codex_root).join("rollout-f06-huge.jsonl"), body).unwrap();

    let out = ok(
        &sandbox,
        &[
            "sessions",
            "--root",
            &codex_root,
            "--provider",
            "codex",
            "--cwd",
            "/repoHuge",
            "--output",
            "jsonl",
        ],
    );
    let sessions: Vec<Value> = out
        .jsonl()
        .into_iter()
        .filter(|row| row.get("record_type").and_then(Value::as_str) == Some("session"))
        .collect();
    assert_eq!(sessions.len(), 1, "{out:#?}");
    let session = &sessions[0];
    let started = session
        .get("started_at")
        .and_then(Value::as_str)
        .unwrap_or("");
    let latest = session
        .get("last_event_at")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        started.starts_with("2026-09-01"),
        "start must be the old session start: {session:#?}"
    );
    assert!(
        latest.starts_with("2026-10-21"),
        "latest activity after >256 KiB preview must not be a stale preview value: {session:#?}"
    );

    // The huge first record is within the default cap and must not be dropped as uninspected.
    let viewed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            Path::new(&codex_root)
                .join("rollout-f06-huge.jsonl")
                .to_str()
                .unwrap(),
            "--output",
            "jsonl",
        ],
    );
    let meta = viewed.metadata();
    assert_eq!(status(&meta), "ok", "{meta:#?}");
    assert!(
        count_field(&meta, &["counts", "uninspected"], "records") == 0,
        "300 KiB record is under the 1 MiB default cap and must be inspected: {meta:#?}"
    );
    // The late assistant answer survives the huge body.
    assert!(viewed.events().iter().any(|e| ev_text(e) == "late answer"));
}

// ---------------------------------------------------------------------------
// F06 explicit-vs-omitted filter reuse (CURSOR_EXPLICIT_FILTER_GATE_1021)
// ---------------------------------------------------------------------------

fn explicit_filter_codex() -> String {
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "explicit",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "m one",
        "2026-10-01T00:01:00Z",
        None,
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "c1",
        "{\"cmd\":\"a\"}",
        "2026-10-01T00:02:00Z",
    )));
    body.push_str(&line(&codex_function_output(
        "c1",
        "out one",
        "2026-10-01T00:03:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "m two",
        "2026-10-01T00:04:00Z",
        None,
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "c2",
        "{\"cmd\":\"b\"}",
        "2026-10-01T00:05:00Z",
    )));
    body.push_str(&line(&codex_function_output(
        "c2",
        "out two",
        "2026-10-01T00:06:00Z",
    )));
    // After the stored --until (00:05:00Z): must not leak if until were dropped on resume.
    body.push_str(&line(&codex_message(
        "user",
        "late leak",
        "2026-10-01T00:07:00Z",
        None,
    )));
    body
}

#[test]
fn f06_cursor_explicit_filters_omitted_vs_explicit() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("f06-explicit.jsonl", &explicit_filter_codex());
    let path = input.to_str().unwrap().to_string();

    // 1) tools-scope cursor resumed WITHOUT --scope must reuse tools (not default messages).
    let first = ok(
        &sandbox,
        &[
            "view", "-i", &path, "--scope", "tools", "--limit", "1", "--output", "jsonl",
        ],
    );
    assert_eq!(first.events().len(), 1, "{first:#?}");
    assert_eq!(ev_kind(&first.events()[0]), "tool_call");
    let cursor = first.metadata()["page_cursor"]
        .as_str()
        .expect("page cursor")
        .to_string();

    let reused = ok(
        &sandbox,
        &[
            "view", "-i", &path, "--cursor", &cursor, "--limit", "1", "--output", "jsonl",
        ],
    );
    assert!(
        reused
            .events()
            .iter()
            .all(|e| ev_kind(e).starts_with("tool")),
        "omitted --scope must reuse the stored tools scope: {reused:#?}"
    );
    assert!(
        !reused.events().is_empty(),
        "positive remaining tool calls required"
    );

    // 2) explicit --scope messages against the stored tools cursor is a conflict (rc1).
    let conflict = run(
        &sandbox,
        &[
            "view", "-i", &path, "--cursor", &cursor, "--scope", "messages", "--output", "jsonl",
        ],
    );
    assert_eq!(conflict.code, 1, "{conflict:#?}");
    assert_eq!(
        conflict.metadata()["error"]["type"].as_str(),
        Some("cursor_invalid")
    );
    assert_eq!(
        conflict.metadata()["error"]["reason"].as_str(),
        Some("filter_mismatch")
    );

    // 3) since+until cursor: repeating only the identical --since must reuse stored --until.
    let windowed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--scope",
            "all",
            "--since",
            "2026-10-01T00:02:00Z",
            "--until",
            "2026-10-01T00:05:00Z",
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let time_cursor = windowed.metadata()["page_cursor"]
        .as_str()
        .expect("time page cursor")
        .to_string();
    let only_since = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            &time_cursor,
            "--since",
            "2026-10-01T00:02:00Z",
            "--output",
            "jsonl",
        ],
    );
    // Stored until is 00:05:00Z (exclusive): the "late leak" record (00:07) must be absent.
    assert!(
        !only_since
            .events()
            .iter()
            .any(|e| ev_text(e) == "late leak"),
        "omitted --until must be reused, not dropped: {only_since:#?}"
    );
    assert!(
        only_since.metadata()["filters"]["until_ms"]
            .as_i64()
            .is_some(),
        "resumed filters must still carry the stored until bound"
    );
    // A genuinely changed boundary is still a conflict.
    let changed = run(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            &time_cursor,
            "--since",
            "2026-10-01T00:01:00Z",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(changed.code, 1, "{changed:#?}");
    assert_eq!(
        changed.metadata()["error"]["reason"].as_str(),
        Some("filter_mismatch")
    );

    // 4) explicit foreign --format against the stored provider must fail, not be ignored.
    let foreign = run(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            &cursor,
            "--format",
            "claude_code",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(foreign.code, 1, "{foreign:#?}");
    assert_eq!(
        foreign.metadata()["error"]["reason"].as_str(),
        Some("filter_mismatch")
    );

    // 5) explicit --regex true against a stored literal search cursor must not be overridden.
    let search = ok(
        &sandbox,
        &["search", "-i", &path, "--query", "out", "--output", "jsonl"],
    );
    let search_meta = search.metadata();
    let search_cursor = search_meta["page_cursor"].as_str();
    if let Some(search_cursor) = search_cursor {
        let regex_conflict = run(
            &sandbox,
            &[
                "search",
                "-i",
                &path,
                "--cursor",
                search_cursor,
                "--regex",
                "--output",
                "jsonl",
            ],
        );
        assert_eq!(regex_conflict.code, 1, "{regex_conflict:#?}");
        assert_eq!(
            regex_conflict.metadata()["error"]["reason"].as_str(),
            Some("filter_mismatch")
        );
    }
}

// ---------------------------------------------------------------------------
// F07/N03 rejected-position retry cursor (RETRY_VALIDATION_GATE_1021)
// ---------------------------------------------------------------------------

#[test]
fn f07_retry_cursor_reads_rejected_row_without_prefix() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&claude_user(
        "retry",
        "p1",
        "prefix body",
        "2026-10-01T00:00:00Z",
    )));
    // One over-limit record (default cap 1 MiB) followed by valid rows.
    let huge = format!("{}HUGETAIL_7c1f", "z".repeat(1_100_000));
    body.push_str(&line(&json!({
        "type": "assistant", "uuid": "huge", "sessionId": "retry",
        "timestamp": "2026-10-01T00:00:01Z",
        "message": {"model": "synthetic", "content": [{"type": "text", "text": huge}]}
    })));
    body.push_str(&line(&claude_user(
        "retry",
        "p3",
        "after huge one",
        "2026-10-01T00:00:02Z",
    )));
    body.push_str(&line(&claude_user(
        "retry",
        "p4",
        "after huge two",
        "2026-10-01T00:00:03Z",
    )));
    let input = sandbox.write("retry.jsonl", &body);

    // Default cap: the huge row is rejected, partial rc1, and a REAL reusable token is returned.
    let first = run(
        &sandbox,
        &["view", "-i", input.to_str().unwrap(), "--output", "jsonl"],
    );
    assert_eq!(first.code, 1, "{first:#?}");
    let meta = first.metadata();
    assert_eq!(status(&meta), "partial");
    let sample = &meta["counts"]["uninspected"]["samples"][0];
    assert_eq!(sample["raw_line"].as_u64(), Some(2));
    let retry = sample["retry_cursor"]
        .as_str()
        .expect("retry_cursor must be an opaque token, not a hint string");
    assert!(
        retry.starts_with("c1:"),
        "retry_cursor must be a real reusable cursor: {retry}"
    );
    assert!(sample.get("retry_hint").and_then(Value::as_str).is_some());
    assert!(meta.get("append_cursor").and_then(Value::as_str).is_none());

    // Resuming the token with a larger cap reads the rejected row and later rows, from the
    // rejected position: the already-processed prefix is NOT re-emitted.
    let retried = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--cursor",
            retry,
            "--max-record-bytes",
            "4194304",
            "--output",
            "jsonl",
        ],
    );
    let retried_events = retried.events();
    let texts: Vec<&str> = retried_events.iter().map(ev_text).collect();
    assert!(
        !texts.contains(&"prefix body"),
        "retry must not re-emit the processed prefix: {texts:?}"
    );
    // The huge row is read from its original position. Its display text may be preview-cut
    // (text_complete=false) — that is legitimate; what matters is that the row exists and was
    // not reported uninspected any more.
    let huge_event = retried_events
        .iter()
        .find(|e| raw_line(e) == 2)
        .expect("huge row must be read at raw line 2");
    assert!(
        !ev_text(huge_event).is_empty(),
        "huge row body must be present (possibly cut)"
    );
    assert_eq!(
        retried.metadata()["counts"]["uninspected"]["records"].as_u64(),
        Some(0)
    );
    assert!(texts.contains(&"after huge one"));
    assert!(texts.contains(&"after huge two"));

    // Accounting: the retry reads only from the rejected offset, and validation is disclosed.
    let meta = retried.metadata();
    assert_eq!(status(&meta), "ok");
    let breakdown = &meta["scan"]["read_bytes_breakdown"];
    assert!(
        breakdown["validation"].as_u64().unwrap() > 0,
        "cursor validation bytes must be accounted: {breakdown:#?}"
    );
    let main = breakdown["main"].as_u64().unwrap();
    let total = meta["scan"]["total_read_bytes"].as_u64().unwrap();
    let sum: u64 = ["main", "detect", "metadata", "anchor", "validation", "pair"]
        .iter()
        .map(|k| breakdown[*k].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(total, sum, "total must equal the disclosed breakdown");
    let file_size = fs::metadata(&input).unwrap().len();
    assert!(
        main < file_size,
        "retry must not read the whole prefix: main={main} size={file_size}"
    );
}

// ---------------------------------------------------------------------------
// F05 source-generation identity (SOURCE_GENERATION_GATE_1021)
// ---------------------------------------------------------------------------

#[test]
fn f05_source_generation_stable_on_append_changes_on_replace() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("gen.jsonl", &simple_codex_file("gen", "/repoA"));
    let path = input.to_str().unwrap().to_string();

    let first = ok(&sandbox, &["view", "-i", &path, "--output", "jsonl"]);
    let first_events = first.events();
    let ids: Vec<String> = first_events.iter().map(|e| ev_id(e).to_string()).collect();
    assert!(!ids.is_empty());
    let old_id = ids[0].clone();

    // Append to the SAME inode: event ids for the already-existing events must not change.
    sandbox.append(
        &input,
        &line(&codex_message(
            "user",
            "appended row",
            "2026-10-01T00:00:09Z",
            None,
        )),
    );
    let after_append = ok(&sandbox, &["view", "-i", &path, "--output", "jsonl"]);
    let after_ids: Vec<String> = after_append
        .events()
        .iter()
        .map(|e| ev_id(e).to_string())
        .collect();
    for id in &ids {
        assert!(after_ids.contains(id), "append must preserve event id {id}");
    }

    // Replace the path with a NEW inode (rename), same length/content shape.
    let replacement = sandbox.path().join("gen-new.jsonl");
    fs::write(&replacement, simple_codex_file("gen", "/repoA")).unwrap();
    fs::rename(&replacement, &input).unwrap();

    let replaced = ok(&sandbox, &["view", "-i", &path, "--output", "jsonl"]);
    let replaced_ids: Vec<String> = replaced
        .events()
        .iter()
        .map(|e| ev_id(e).to_string())
        .collect();
    assert!(
        !replaced_ids.contains(&old_id),
        "replacement must change source identity/event ids: {replaced_ids:?}"
    );

    // Re-entering the OLD id must fail rather than present new content as old evidence.
    let old_lookup = run(
        &sandbox,
        &["view", "-i", &path, "--event", &old_id, "--output", "jsonl"],
    );
    assert_eq!(old_lookup.code, 1, "{old_lookup:#?}");
    assert_eq!(
        old_lookup.metadata()["error"]["type"].as_str(),
        Some("event_not_found")
    );
}

// ---------------------------------------------------------------------------
// F02 explicit private message channel (PRIVATE_MESSAGE_GATE_1021)
// ---------------------------------------------------------------------------

#[test]
fn f02_private_channel_message_is_hard_scope_excluded() {
    let sandbox = Sandbox::new();
    const SENTINEL: &str = "PRIVATE_CHANNEL_SENTINEL_5b2e";
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "priv",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:01Z", "type": "response_item",
        "payload": {"type": "message", "role": "assistant", "channel": "analysis",
            "content": [{"type": "input_text", "text": format!("{SENTINEL} response_item")}]}
    })));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:02Z", "type": "event_msg",
        "payload": {"type": "agent_message", "phase": "analysis",
            "message": format!("{SENTINEL} legacy")}
    })));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:03Z", "type": "response_item",
        "payload": {"type": "message", "role": "assistant", "phase": "commentary",
            "content": [{"type": "input_text", "text": "public commentary"}]}
    })));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:04Z", "type": "response_item",
        "payload": {"type": "message", "role": "assistant", "phase": "final",
            "content": [{"type": "input_text", "text": "public final"}]}
    })));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:05Z", "type": "response_item",
        "payload": {"type": "message", "role": "assistant", "phase": "future_phase_xyz",
            "content": [{"type": "input_text", "text": "unknown future phase preserved"}]}
    })));
    let input = sandbox.write("priv.jsonl", &body);
    let path = input.to_str().unwrap();

    for args in [
        vec!["view", "-i", path, "--output", "jsonl"],
        vec!["view", "-i", path, "--scope", "all", "--output", "jsonl"],
        vec![
            "search",
            "-i",
            path,
            "--query",
            SENTINEL,
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    ] {
        let out = run(&sandbox, &args);
        assert!(
            !out.stdout.contains(SENTINEL),
            "private marked body must never be exposed: {args:?}\n{}",
            out.stdout
        );
        // JSON is still well formed and diagnostics stay off stdout prose.
        let _ = out.jsonl();
    }

    let default_view = ok(&sandbox, &["view", "-i", path, "--output", "jsonl"]);
    let default_events = default_view.events();
    let texts: Vec<&str> = default_events.iter().map(ev_text).collect();
    assert!(texts.contains(&"public commentary"));
    assert!(texts.contains(&"public final"));
    assert!(
        texts.contains(&"unknown future phase preserved"),
        "unknown future phase must be preserved: {texts:?}"
    );
}

// ---------------------------------------------------------------------------
// Absolute raw ordering across page/append scans (PAIR_RAW_ORDER_NOTE_1021)
// ---------------------------------------------------------------------------

#[test]
fn f06_paired_append_renders_absolute_raw_order() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "order",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "B",
        "{\"cmd\":\"b\"}",
        "2026-10-01T00:00:01Z",
    )));
    let input = sandbox.write("order.jsonl", &body);

    // Full scan (tail) issues the EOF append cursor while call B is still pending.
    let full = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--tail",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let cursor = full.metadata()["append_cursor"]
        .as_str()
        .expect("append cursor")
        .to_string();

    // Append: result B, then call C and result C.
    sandbox.append(
        &input,
        &line(&codex_function_output(
            "B",
            "result b",
            "2026-10-01T00:00:02Z",
        )),
    );
    sandbox.append(
        &input,
        &line(&codex_function_call(
            "shell",
            "C",
            "{\"cmd\":\"c\"}",
            "2026-10-01T00:00:03Z",
        )),
    );
    sandbox.append(
        &input,
        &line(&codex_function_output(
            "C",
            "result c",
            "2026-10-01T00:00:04Z",
        )),
    );

    let resumed = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &cursor,
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let events = resumed.events();
    // Absolute raw order: old call B (line 2) must come before the appended rows (lines 3-5).
    let order: Vec<(u64, u64)> = events.iter().map(|e| (raw_line(e), sub_index(e))).collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(
        order, sorted,
        "rendered order must be absolute raw order: {order:?}"
    );
    let first = events.first().expect("at least one event");
    assert_eq!(
        ev_kind(first),
        "tool_call",
        "old pending call B must render first: {order:?}"
    );
    assert_eq!(first["call_id"].as_str(), Some("B"));
    assert!(events.iter().any(|e| ev_text(e) == "result b"));
    assert!(events.iter().any(|e| ev_text(e) == "result c"));
}

// ---------------------------------------------------------------------------
// Hard-scope context separation (SELECTOR_HARD_SCOPE_CAP_1021)
// ---------------------------------------------------------------------------

#[test]
fn f03_context_never_crosses_hard_scope_kind() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "hardscope",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "alpha",
        "2026-10-01T00:00:01Z",
        None,
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "c1",
        "{\"cmd\":\"t\"}",
        "2026-10-01T00:00:02Z",
    )));
    body.push_str(&line(&codex_function_output(
        "c1",
        "tool result",
        "2026-10-01T00:00:03Z",
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "beta",
        "2026-10-01T00:00:04Z",
        Some("final"),
    )));
    let input = sandbox.write("hardscope.jsonl", &body);
    let path = input.to_str().unwrap();

    // messages search with context: beta is context, no tool event may appear.
    let msg = ok(
        &sandbox,
        &[
            "search",
            "-i",
            path,
            "--query",
            "alpha",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let msg_events = msg.events();
    assert!(
        msg_events.iter().any(|e| ev_text(e) == "beta"),
        "beta must be context"
    );
    assert!(
        msg_events.iter().all(|e| !ev_kind(e).starts_with("tool")),
        "tools must not leak into messages context: {:?}",
        msg_events
            .iter()
            .map(|e| (ev_kind(e), ev_text(e)))
            .collect::<Vec<_>>()
    );

    // tools search with context: neighbouring tool results stay, user messages never appear.
    let tools = ok(
        &sandbox,
        &[
            "search",
            "-i",
            path,
            "--scope",
            "tools",
            "--query",
            "cmd",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let tool_events = tools.events();
    assert!(
        tool_events.iter().any(|e| ev_text(e) == "tool result"),
        "tool neighbour must be context: {:?}",
        tool_events.iter().map(ev_text).collect::<Vec<_>>()
    );
    assert!(
        tool_events
            .iter()
            .all(|e| ev_text(e) != "alpha" && ev_text(e) != "beta"),
        "messages must not leak into tools context: {:?}",
        tool_events.iter().map(ev_text).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Final audit regressions (F-AUD-01/02, R-01/R-02, --event scope)
// ---------------------------------------------------------------------------

fn audit_hit_file(rows: usize, hits: usize) -> String {
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "audit",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for i in 0..rows {
        let text = if i < hits {
            format!("HIT {i:03}")
        } else {
            format!("plain {i:03}")
        };
        body.push_str(&line(&codex_message(
            "user",
            &text,
            "2026-10-01T00:00:01Z",
            None,
        )));
    }
    body
}

#[test]
fn f_aud01_max_events_never_drops_hits_without_cursor() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("hit.jsonl", &audit_hit_file(60, 30));
    let path = input.to_str().unwrap().to_string();

    let mut cursor: Option<String> = None;
    let mut primary_ids: Vec<String> = Vec::new();
    for _page in 0..10 {
        let mut args = vec![
            "search",
            "-i",
            &path,
            "--query",
            "HIT",
            "--context",
            "1",
            "--limit",
            "20",
            "--max-events",
            "20",
            "--output",
            "jsonl",
        ];
        if let Some(token) = &cursor {
            args.push("--cursor");
            args.push(token);
        }
        let out = ok(&sandbox, &args);
        let events = out.events();
        assert!(
            events.len() <= 20,
            "per-page total events must respect the cap: {}",
            events.len()
        );
        for event in &events {
            let is_hit = event
                .get("matched_queries")
                .and_then(Value::as_array)
                .map(|q| !q.is_empty())
                .unwrap_or(false);
            if is_hit {
                primary_ids.push(ev_id(event).to_string());
            }
        }
        let meta = out.metadata();
        assert_eq!(meta["status"].as_str(), Some("ok"));
        cursor = meta
            .get("page_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    let unique: std::collections::BTreeSet<&String> = primary_ids.iter().collect();
    assert_eq!(
        primary_ids.len(),
        30,
        "all 30 hits must be reachable: {:?}",
        primary_ids.len()
    );
    assert_eq!(unique.len(), 30, "no duplicate primary across pages");
}

#[test]
fn f_aud02_uninspected_result_is_unknown_pair_not_pending() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "aud02",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "c1",
        "{\"cmd\":\"x\"}",
        "2026-10-01T00:00:01Z",
    )));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:02Z", "type": "response_item",
        "payload": {"type": "function_call_output", "call_id": "c1", "output": "z".repeat(1_200_000)}
    })));
    let input = sandbox.write("aud02.jsonl", &body);

    let out = run(
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
    assert_eq!(
        out.code, 1,
        "unread result range must make the query partial: {out:#?}"
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "partial");
    assert!(meta["counts"]["uninspected"]["records"].as_u64().unwrap() >= 1);
    let call = out
        .events()
        .into_iter()
        .find(|e| e["call_id"].as_str() == Some("c1") && ev_kind(e) == "tool_call")
        .expect("call visible");
    assert_eq!(call["pair_state"].as_str(), Some("unknown_pair"));
    let with_pair = &meta["scan"]["with_pair"];
    assert_eq!(with_pair["incomplete"].as_bool(), Some(true));
    assert!(with_pair["uninspected"].as_u64().unwrap() >= 1);
    assert!(!with_pair["gaps"].as_array().unwrap().is_empty());
}

#[test]
fn r01_time_exclusion_is_not_role_exclusion() {
    let sandbox = Sandbox::new();
    let input = sandbox.write("r01.jsonl", &audit_hit_file(60, 0));
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--since",
            "2026-06-01T00:00:00Z",
            "--role",
            "assistant",
            "--output",
            "jsonl",
        ],
    );
    let meta = out.metadata();
    assert_eq!(
        count_field(&meta, &["counts", "excluded_time"], "events"),
        0
    );
    assert_eq!(
        count_field(&meta, &["counts", "excluded_role"], "events"),
        60
    );
}

#[test]
fn r02_counterpart_total_is_exact_with_truncated_ids() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "r02",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "fan",
        "{\"cmd\":\"x\"}",
        "2026-10-01T00:00:01Z",
    )));
    for i in 0..40 {
        body.push_str(&line(&codex_function_output(
            "fan",
            &format!("result {i}"),
            "2026-10-01T00:00:02Z",
        )));
    }
    let input = sandbox.write("r02.jsonl", &body);
    let out = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--max-events",
            "5",
            "--output",
            "jsonl",
        ],
    );
    let call = out
        .events()
        .into_iter()
        .find(|e| e["call_id"].as_str() == Some("fan") && ev_kind(e) == "tool_call")
        .expect("call visible");
    assert_eq!(
        call["paired_total"].as_u64(),
        Some(40),
        "exact total, not the capped list"
    );
    assert_eq!(call["paired_ids_truncated"].as_bool(), Some(true));
    assert!(call["pair_reentry"]
        .get("raw_line")
        .and_then(Value::as_u64)
        .is_some());
}

#[test]
fn event_lookup_respects_scope_and_privacy_and_pairs() {
    let sandbox = Sandbox::new();
    const SENTINEL: &str = "PRIVATE_EVENT_SENTINEL_77aa";
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "ev",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "ev-call",
        "{\"cmd\":\"x\"}",
        "2026-10-01T00:00:01Z",
    )));
    body.push_str(&line(&codex_function_output(
        "ev-call",
        "tool ok",
        "2026-10-01T00:00:02Z",
    )));
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:03Z", "type": "response_item",
        "payload": {"type": "message", "role": "assistant", "channel": "analysis",
            "content": [{"type": "input_text", "text": format!("{SENTINEL} body")}]}
    })));
    let input = sandbox.write("ev.jsonl", &body);
    let path = input.to_str().unwrap();

    let all = ok(
        &sandbox,
        &["view", "-i", path, "--scope", "tools", "--output", "jsonl"],
    );
    let call_id = all
        .events()
        .iter()
        .find(|e| ev_kind(e) == "tool_call")
        .map(|e| ev_id(e).to_string())
        .expect("call id");
    let found = ok(
        &sandbox,
        &[
            "view",
            "-i",
            path,
            "--event",
            &call_id,
            "--scope",
            "tools",
            "--with-pair",
            "--output",
            "jsonl",
        ],
    );
    let meta = found.metadata();
    assert_eq!(status(&meta), "ok");
    let call = found
        .events()
        .into_iter()
        .find(|e| ev_id(e) == call_id)
        .expect("event returned");
    assert_eq!(call["call_id"].as_str(), Some("ev-call"));
    assert!(call["paired_event_ids"].as_array().is_some());

    // A private analysis event id must not become public through --event.
    let private_id = all
        .events()
        .iter()
        .find(|e| ev_text(e).contains(SENTINEL))
        .map(|e| ev_id(e).to_string());
    if let Some(private_id) = private_id {
        for scope in ["messages", "all"] {
            let out = run(
                &sandbox,
                &[
                    "view",
                    "-i",
                    path,
                    "--event",
                    &private_id,
                    "--scope",
                    scope,
                    "--output",
                    "jsonl",
                ],
            );
            assert_eq!(
                out.code, 1,
                "private event must not be publicly emitted ({scope}): {out:#?}"
            );
            assert!(!out.stdout.contains(SENTINEL));
        }
    }
}

#[test]
fn cap_leading_context_cannot_starve_a_hit() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "cap",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "context before",
        "2026-10-01T00:00:01Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "user",
        "HIT target",
        "2026-10-01T00:00:02Z",
        None,
    )));
    let input = sandbox.write("cap.jsonl", &body);

    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            input.to_str().unwrap(),
            "--query",
            "HIT",
            "--context",
            "1",
            "--limit",
            "1",
            "--max-events",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let events = out.events();
    assert!(events.len() <= 1, "cap is 1: {events:#?}");
    assert!(
        events.iter().any(|e| ev_text(e) == "HIT target"),
        "the reserved primary slot must go to the hit, not to leading context: {:?}",
        events.iter().map(ev_text).collect::<Vec<_>>()
    );
}

#[test]
fn cap_preceding_counterpart_does_not_block_append_progress() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "prog",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_function_call(
        "shell",
        "B",
        "{\"cmd\":\"b\"}",
        "2026-10-01T00:00:01Z",
    )));
    let input = sandbox.write("prog.jsonl", &body);

    let full = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--scope",
            "tools",
            "--tail",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let cursor = full.metadata()["append_cursor"]
        .as_str()
        .expect("append cursor")
        .to_string();
    sandbox.append(
        &input,
        &line(&codex_function_output(
            "B",
            "result b",
            "2026-10-01T00:00:02Z",
        )),
    );
    sandbox.append(
        &input,
        &line(&codex_function_call(
            "shell",
            "C",
            "{\"cmd\":\"c\"}",
            "2026-10-01T00:00:03Z",
        )),
    );
    sandbox.append(
        &input,
        &line(&codex_function_output(
            "C",
            "result c",
            "2026-10-01T00:00:04Z",
        )),
    );

    // With a total cap of 1 the old pending call B must not occupy the only slot forever.
    let first = ok(
        &sandbox,
        &[
            "view",
            "-i",
            input.to_str().unwrap(),
            "--after-cursor",
            &cursor,
            "--scope",
            "tools",
            "--with-pair",
            "--max-events",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let first_events = first.events();
    assert_eq!(first_events.len(), 1, "exactly the cap: {first_events:#?}");
    assert!(
        first_events
            .iter()
            .all(|e| !(e["call_id"].as_str() == Some("B") && ev_kind(e) == "tool_call")),
        "already-known old call B must not be re-rendered instead of new data: {first_events:#?}"
    );
    let meta = first.metadata();
    assert!(
        meta.get("page_cursor").and_then(Value::as_str).is_some()
            || meta.get("append_cursor").and_then(Value::as_str).is_some(),
        "progress must expose a continuation: {meta:#?}"
    );
}

#[test]
fn event_absent_only_when_snapshot_fully_inspected() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "evsnap",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    // 1.1 MB predecessor: over the default cap, so the target row is not reached.
    body.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:01Z", "type": "response_item",
        "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "z".repeat(1_100_000)}]}
    })));
    body.push_str(&line(&codex_message(
        "user",
        "target after big",
        "2026-10-01T00:00:02Z",
        None,
    )));
    let input = sandbox.write("evsnap.jsonl", &body);
    let path = input.to_str().unwrap().to_string();

    // Discover the target id with a large cap.
    let wide = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--max-record-bytes",
            "2000000",
            "--output",
            "jsonl",
        ],
    );
    let target_id = wide
        .events()
        .iter()
        .find(|e| ev_text(e) == "target after big")
        .map(|e| ev_id(e).to_string())
        .expect("target visible with a large cap");

    // Default cap: the id is behind an uninspected row, so absence must NOT be claimed.
    let out = run(
        &sandbox,
        &[
            "view", "-i", &path, "--event", &target_id, "--output", "jsonl",
        ],
    );
    assert_eq!(out.code, 1, "{out:#?}");
    let meta = out.metadata();
    assert_eq!(
        status(&meta),
        "partial",
        "unread range must be partial, not event_not_found"
    );
    assert_ne!(meta["error"]["type"].as_str(), Some("event_not_found"));
    let sample = &meta["counts"]["uninspected"]["samples"][0];
    assert!(sample["byte_start"].as_u64().unwrap() > 0);
    let retry = sample["retry_cursor"].as_str().expect("real retry cursor");

    // Retrying with a larger cap reaches the same event without re-emitting the prefix.
    let retried = ok(
        &sandbox,
        &[
            "view",
            "-i",
            &path,
            "--cursor",
            retry,
            "--max-record-bytes",
            "2000000",
            "--output",
            "jsonl",
        ],
    );
    let retried_events = retried.events();
    let texts: Vec<&str> = retried_events
        .iter()
        .map(|e| e.get("text").and_then(Value::as_str).unwrap_or(""))
        .collect();
    assert!(texts.contains(&"target after big"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.starts_with("zzzz")),
        "the already-processed prefix must not be re-emitted"
    );
}

/// Cursor profile: `--context` is semantic, so an omitted value must reuse the stored profile and
/// an explicit conflict must be a typed error (context-profile-1021).
#[test]
fn f06_cursor_context_profile_is_inherited_and_conflicts_rejected() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "ctx",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for text in ["before", "HIT first", "middle", "HIT second", "after"] {
        body.push_str(&line(&codex_message(
            "user",
            text,
            "2026-10-01T00:00:01Z",
            None,
        )));
    }
    let input = sandbox.write("ctx.jsonl", &body);
    let path = input.to_str().unwrap().to_string();

    let first = ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "HIT",
            "--context",
            "1",
            "--limit",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let first_events = first.events();
    let first_texts: Vec<&str> = first_events.iter().map(ev_text).collect();
    assert_eq!(
        first_texts,
        vec!["before", "HIT first", "middle"],
        "{first_texts:?}"
    );
    let cursor = first.metadata()["page_cursor"]
        .as_str()
        .expect("page cursor")
        .to_string();

    // Omitted --context: the stored profile (1) is reused, so the trailing neighbour is returned.
    let resumed = ok(
        &sandbox,
        &[
            "search", "-i", &path, "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    let resumed_events = resumed.events();
    let resumed_texts: Vec<&str> = resumed_events.iter().map(ev_text).collect();
    assert_eq!(
        resumed_texts,
        vec!["HIT second", "after"],
        "{resumed_texts:?}"
    );
    let after = resumed_events
        .iter()
        .find(|e| ev_text(e) == "after")
        .unwrap();
    assert_eq!(after.get("is_context").and_then(Value::as_bool), Some(true));
    assert_eq!(
        resumed.metadata()["windows"]["context"].as_u64(),
        Some(1),
        "resumed metadata must disclose the inherited context profile"
    );

    // Primary ids are not duplicated across pages.
    let first_primary: Vec<String> = first_events
        .iter()
        .filter(|e| e.get("is_context").and_then(Value::as_bool) != Some(true))
        .map(|e| ev_id(e).to_string())
        .collect();
    let resumed_primary: Vec<String> = resumed_events
        .iter()
        .filter(|e| e.get("is_context").and_then(Value::as_bool) != Some(true))
        .map(|e| ev_id(e).to_string())
        .collect();
    assert!(first_primary.iter().all(|id| !resumed_primary.contains(id)));

    // Explicit same value is accepted.
    ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--cursor",
            &cursor,
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );

    // Explicit different values are typed conflicts.
    for bad in ["0", "2"] {
        let out = run(
            &sandbox,
            &[
                "search",
                "-i",
                &path,
                "--cursor",
                &cursor,
                "--context",
                bad,
                "--output",
                "jsonl",
            ],
        );
        assert_eq!(out.code, 1, "context {bad} must conflict: {out:#?}");
        assert_eq!(
            out.metadata()["error"]["type"].as_str(),
            Some("cursor_invalid")
        );
        assert_eq!(
            out.metadata()["error"]["reason"].as_str(),
            Some("filter_mismatch")
        );
    }

    // The retry token from an uninspected row keeps the same context profile.
    let mut big = String::new();
    big.push_str(&line(&codex_session_meta(
        "ctx2",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    big.push_str(&line(&json!({
        "timestamp": "2026-10-01T00:00:01Z", "type": "response_item",
        "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "z".repeat(1_100_000)}]}
    })));
    big.push_str(&line(&codex_message(
        "user",
        "late HIT",
        "2026-10-01T00:00:02Z",
        None,
    )));
    let big_input = sandbox.write("ctx-big.jsonl", &big);
    let partial = run(
        &sandbox,
        &[
            "search",
            "-i",
            big_input.to_str().unwrap(),
            "--query",
            "HIT",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(partial.code, 1);
    let retry = partial.metadata()["counts"]["uninspected"]["samples"][0]["retry_cursor"]
        .as_str()
        .expect("retry cursor")
        .to_string();
    let retried = ok(
        &sandbox,
        &[
            "search",
            "-i",
            big_input.to_str().unwrap(),
            "--cursor",
            &retry,
            "--max-record-bytes",
            "2000000",
            "--output",
            "jsonl",
        ],
    );
    assert_eq!(
        retried.metadata()["windows"]["context"].as_u64(),
        Some(1),
        "retry cursor must keep the same semantic context profile"
    );
}

/// Regex mode is part of the cursor's semantic profile: an omitted `--regex` inherits the stored
/// mode even when the query is repeated explicitly (regex-profile-confirmed-1021).
#[test]
fn f06_cursor_regex_mode_is_inherited_independently_of_query() {
    let sandbox = Sandbox::new();
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        "rx",
        "/repoA",
        "2026-10-01T00:00:00Z",
    )));
    for text in ["before", "HxT first", "middle", "HxT second", "after"] {
        body.push_str(&line(&codex_message(
            "user",
            text,
            "2026-10-01T00:00:01Z",
            None,
        )));
    }
    let input = sandbox.write("rx.jsonl", &body);
    let path = input.to_str().unwrap().to_string();

    // Regex search: `.` matches `x`, so this is a real regex hit.
    let first = ok(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "H.T", "--regex", "--limit", "1", "--output", "jsonl",
        ],
    );
    assert!(
        first.events().iter().any(|e| ev_text(e) == "HxT first"),
        "regex must match: {:?}",
        first.events().iter().map(ev_text).collect::<Vec<_>>()
    );
    let cursor = first.metadata()["page_cursor"]
        .as_str()
        .expect("page cursor")
        .to_string();

    // Same explicit query, --regex omitted: stored regex mode is reused (previously rc1).
    let resumed = ok(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "H.T", "--cursor", &cursor, "--output", "jsonl",
        ],
    );
    let resumed_events = resumed.events();
    let texts: Vec<&str> = resumed_events.iter().map(ev_text).collect();
    assert!(
        texts.contains(&"HxT second"),
        "stored regex mode must still match after resume: {texts:?}"
    );
    assert_eq!(
        resumed.metadata()["filters"]["event"].as_str(),
        None,
        "no bogus event filter"
    );

    // Explicit same mode accepted.
    ok(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "H.T", "--regex", "--cursor", &cursor, "--output",
            "jsonl",
        ],
    );

    // Explicit literal mode against a stored regex profile is a typed conflict.
    let literal = run(
        &sandbox,
        &[
            "search", "-i", &path, "--query", "H.T", "--regex", "false", "--cursor", &cursor,
            "--output", "jsonl",
        ],
    );
    assert_eq!(literal.code, 1, "{literal:#?}");
    assert_eq!(
        literal.metadata()["error"]["type"].as_str(),
        Some("cursor_invalid")
    );
    assert_eq!(
        literal.metadata()["error"]["reason"].as_str(),
        Some("filter_mismatch")
    );
}

#[test]
fn real_sanitized_claude_desktop_fixture_view_search_and_pending_pair() {
    let sandbox = Sandbox::new();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_desktop_sample.jsonl");
    let path = path.to_string_lossy().into_owned();

    let out = ok(
        &sandbox,
        &["view", "-i", &path, "--tail", "20", "--output", "jsonl"],
    );
    let meta = out.metadata();
    assert_eq!(status(&meta), "ok");
    assert_eq!(meta["provider"].as_str(), Some("claude_code"));
    assert_eq!(
        meta["source"]["session_id"].as_str(),
        Some("00000000-0000-4000-8000-000000000001")
    );
    assert!(out.stdout.contains("Userさん"));
    assert!(!out.stdout.contains("Yoshi-yu"));
    assert!(!out.stdout.contains("Asia/Tokyo"));
    assert!(!out.stdout.contains("/home/claude"));
    assert!(!out.stdout.contains("a79584e2"));

    let out = ok(
        &sandbox,
        &[
            "search",
            "-i",
            &path,
            "--query",
            "会話",
            "--context",
            "1",
            "--output",
            "jsonl",
        ],
    );
    assert!(out.events().iter().any(|event| {
        event.get("kind").and_then(Value::as_str) == Some("user")
            && event
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .contains("会話")
    }));
    assert!(!out.stdout.contains("Yoshi-yu"));

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
    assert!(events.iter().any(|event| {
        event.get("pair_state").and_then(Value::as_str) == Some("pending_result")
            && event.get("call_id").and_then(Value::as_str)
                == Some("toolu_000000000000000000000003")
    }));
    assert!(events
        .iter()
        .any(|event| event.get("pair_state").and_then(Value::as_str) == Some("paired")));
}
