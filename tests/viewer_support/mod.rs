//! Shared fixture/harness helpers for the artificial viewer acceptance tests (A01–A12).
//!
//! Everything here is programmatically generated: no real home directory, no provider
//! database and no real session log is read. Each test gets an isolated HOME/XDG tree and
//! passes explicit `--root`/`-i` paths to the actual built binary.

#![allow(dead_code)]

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The real binary produced by cargo for this test target.
pub const BIN: &str = env!("CARGO_BIN_EXE_agent-jsonl-compact");

pub struct Sandbox {
    pub dir: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn home(&self) -> PathBuf {
        let home = self.path().join("home");
        fs::create_dir_all(&home).unwrap();
        home
    }

    pub fn write(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    pub fn append(&self, path: &Path, body: &str) {
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Parse stdout as JSONL; every non-empty line must be valid JSON.
    pub fn jsonl(&self) -> Vec<Value> {
        self.stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<Value>(line).unwrap_or_else(|err| {
                    panic!(
                        "stdout line is not JSON: {err}\nline={line}\nstderr={}",
                        self.stderr
                    )
                })
            })
            .collect()
    }

    pub fn events(&self) -> Vec<Value> {
        self.jsonl()
            .into_iter()
            .filter(|row| row.get("record_type").and_then(Value::as_str) == Some("event"))
            .collect()
    }

    pub fn metadata(&self) -> Value {
        let rows = self.jsonl();
        let meta: Vec<&Value> = rows
            .iter()
            .filter(|row| row.get("record_type").and_then(Value::as_str) == Some("metadata"))
            .collect();
        assert_eq!(
            meta.len(),
            1,
            "exactly one metadata record expected: {rows:#?}"
        );
        // The metadata record must be last.
        assert_eq!(
            rows.last()
                .unwrap()
                .get("record_type")
                .and_then(Value::as_str),
            Some("metadata"),
            "metadata must be the final stdout record"
        );
        meta[0].clone()
    }

    pub fn total_bytes(&self) -> usize {
        self.stdout.len() + self.stderr.len()
    }
}

fn base_command(sandbox: &Sandbox, args: &[&str]) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.path().join("xdg-config"))
        .env("XDG_DATA_HOME", sandbox.path().join("xdg-data"))
        .env("XDG_CACHE_HOME", sandbox.path().join("xdg-cache"))
        .env("XDG_STATE_HOME", sandbox.path().join("xdg-state"))
        .env("PATH", "/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .current_dir(sandbox.path());
    command.args(args);
    command
}

pub fn run(sandbox: &Sandbox, args: &[&str]) -> Output {
    let out = base_command(sandbox, args).output().expect("spawn binary");
    Output {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

pub fn ok(sandbox: &Sandbox, args: &[&str]) -> Output {
    let out = run(sandbox, args);
    assert_eq!(
        out.code, 0,
        "expected rc0 for {args:?}\nstdout={}\nstderr={}",
        out.stdout, out.stderr
    );
    out
}

/// Spawn the binary with piped stdin so a consumer can close stdout early (SIGPIPE path).
pub fn run_stdin(_sandbox: &Sandbox, args: &[&str], stdin_body: &str) -> Output {
    use std::process::Stdio;
    let mut child = base_command(_sandbox, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_body.as_bytes())
        .ok();
    let out = child.wait_with_output().expect("wait");
    Output {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Serialize one JSONL record and return (serialized bytes incl. newline, byte length).
pub fn line(value: &Value) -> String {
    let mut text = serde_json::to_string(value).unwrap();
    text.push('\n');
    text
}

/// SHA-256 for fixture expectations. Uses the same vetted crate as the product but is an
/// independent computation: expectations come from the serialized fixture bytes, never from
/// production output or the production normalizer.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// Record builders (Codex / Claude Code / OpenCode run formats)
// ---------------------------------------------------------------------------

pub fn codex_session_meta(id: &str, cwd: &str, ts: &str) -> Value {
    json!({
        "timestamp": ts,
        "type": "session_meta",
        "payload": {"id": id, "cwd": cwd, "originator": "codex-cli", "cli_version": "9.9.9"}
    })
}

pub fn codex_message(role: &str, text: &str, ts: &str, phase: Option<&str>) -> Value {
    let mut payload =
        json!({"type": "message", "role": role, "content": [{"type": "input_text", "text": text}]});
    if let Some(phase) = phase {
        payload["phase"] = Value::String(phase.to_string());
    }
    json!({"timestamp": ts, "type": "response_item", "payload": payload})
}

/// Codex terminal mirror (event_msg) for a user message.
pub fn codex_event_user(text: &str, ts: &str) -> Value {
    json!({"timestamp": ts, "type": "event_msg", "payload": {"type": "user_message", "message": text}})
}

pub fn codex_function_call(name: &str, call_id: &str, args: &str, ts: &str) -> Value {
    json!({"timestamp": ts, "type": "response_item", "payload": {
        "type": "function_call", "name": name, "call_id": call_id, "arguments": args
    }})
}

pub fn codex_function_output(call_id: &str, output: &str, ts: &str) -> Value {
    json!({"timestamp": ts, "type": "response_item", "payload": {
        "type": "function_call_output", "call_id": call_id, "output": output
    }})
}

/// Codex custom tool call (the form the legacy classifier drops; must stay visible here).
pub fn codex_custom_call(name: &str, call_id: &str, input: &str, ts: &str) -> Value {
    json!({"timestamp": ts, "type": "response_item", "payload": {
        "type": "custom_tool_call", "name": name, "call_id": call_id, "input": input
    }})
}

pub fn codex_custom_output(call_id: &str, output: &str, ts: &str) -> Value {
    json!({"timestamp": ts, "type": "response_item", "payload": {
        "type": "custom_tool_call_output", "call_id": call_id, "output": output
    }})
}

pub fn claude_user(session: &str, uuid: &str, text: &str, ts: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "sessionId": session, "timestamp": ts,
        "cwd": "/work/repo", "version": "1.0.0", "gitBranch": "main",
        "message": {"role": "user", "content": text}})
}

pub fn claude_assistant_text(session: &str, uuid: &str, text: &str, ts: &str) -> Value {
    json!({"type": "assistant", "uuid": uuid, "sessionId": session, "timestamp": ts,
        "message": {"model": "claude-sonnet", "content": [{"type": "text", "text": text}]}})
}

pub fn claude_tool_use(
    session: &str,
    uuid: &str,
    tool_id: &str,
    name: &str,
    input: Value,
    ts: &str,
) -> Value {
    json!({"type": "assistant", "uuid": uuid, "sessionId": session, "timestamp": ts,
    "message": {"model": "claude-sonnet", "content": [
        {"type": "tool_use", "id": tool_id, "name": name, "input": input}
    ]}})
}

pub fn claude_tool_result(
    session: &str,
    uuid: &str,
    tool_id: &str,
    body: &str,
    is_error: bool,
    ts: &str,
) -> Value {
    json!({"type": "user", "uuid": uuid, "sessionId": session, "timestamp": ts,
    "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": tool_id, "is_error": is_error,
         "content": [{"type": "text", "text": body}]}
    ]}})
}

pub fn claude_framework_meta(session: &str, uuid: &str, ts: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "sessionId": session, "timestamp": ts,
    "isMeta": true,
    "message": {"role": "user", "content": [
        {"type": "text", "text": "framework injected reminder"}
    ]}})
}

pub fn claude_compact_summary(session: &str, uuid: &str, ts: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "sessionId": session, "timestamp": ts,
        "isCompactSummary": true,
        "message": {"role": "user", "content": "compacted summary body"}})
}

/// Legacy Codex shape (pre response_item): event_msg only, no payload "type": "message".
pub fn legacy_codex(session: &str, cwd: &str, ts_user: &str, text: &str) -> Vec<Value> {
    vec![
        json!({"timestamp": ts_user, "type": "session_meta", "payload": {
            "id": session, "cwd": cwd, "originator": "codex-cli", "cli_version": "0.1.0"
        }}),
        json!({"timestamp": ts_user, "type": "event_msg", "payload": {
            "type": "user_message", "message": text
        }}),
    ]
}

// ---------------------------------------------------------------------------
// Small assertion helpers
// ---------------------------------------------------------------------------

pub fn ev_text(event: &Value) -> &str {
    event.get("text").and_then(Value::as_str).unwrap_or("")
}

pub fn ev_kind(event: &Value) -> &str {
    event.get("kind").and_then(Value::as_str).unwrap_or("")
}

pub fn ev_id(event: &Value) -> &str {
    event.get("event_id").and_then(Value::as_str).unwrap_or("")
}

pub fn source(event: &Value) -> &Value {
    event.get("source").expect("event.source")
}

pub fn raw_line(event: &Value) -> u64 {
    source(event)
        .get("raw_line")
        .and_then(Value::as_u64)
        .expect("raw_line")
}

pub fn sub_index(event: &Value) -> u64 {
    source(event)
        .get("subevent_index")
        .and_then(Value::as_u64)
        .expect("subevent_index")
}

pub fn byte_range(event: &Value) -> (u64, u64) {
    let src = source(event);
    (
        src.get("byte_start")
            .and_then(Value::as_u64)
            .expect("byte_start"),
        src.get("byte_end")
            .and_then(Value::as_u64)
            .expect("byte_end"),
    )
}

pub fn count_field(meta: &Value, path: &[&str], key: &str) -> i64 {
    let mut node = meta;
    for step in path {
        node = node.get(*step).unwrap_or(&Value::Null);
    }
    node.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing {path:?}.{key} in {meta:#?}"))
}

pub fn status(meta: &Value) -> &str {
    meta.get("status").and_then(Value::as_str).unwrap_or("")
}

/// Generic session fixture: three messages (user/assistant/user) in one provider file.
pub fn simple_codex_file(session: &str, cwd: &str) -> String {
    let mut body = String::new();
    body.push_str(&line(&codex_session_meta(
        session,
        cwd,
        "2026-10-01T00:00:00Z",
    )));
    body.push_str(&line(&codex_message(
        "user",
        "first question",
        "2026-10-01T00:00:01Z",
        None,
    )));
    body.push_str(&line(&codex_message(
        "assistant",
        "first answer",
        "2026-10-01T00:00:02Z",
        Some("final"),
    )));
    body.push_str(&line(&codex_message(
        "user",
        "second question",
        "2026-10-01T00:00:03Z",
        None,
    )));
    body
}
