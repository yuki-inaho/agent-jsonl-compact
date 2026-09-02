use crate::cli::SessionFormat;
use crate::util::for_each_jsonl_record_until;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

const CODEX_TYPES: &[&str] = &[
    "session_meta",
    "response_item",
    "event_msg",
    "turn_context",
    "compacted",
];

const CLAUDE_TYPES: &[&str] = &[
    "user",
    "assistant",
    "system",
    "summary",
    "ai-title",
    "mode",
    "permission-mode",
    "last-prompt",
    "attachment",
    "file-history-snapshot",
];

const OPENCODE_TYPES: &[&str] = &[
    "step_start",
    "text",
    "reasoning",
    "tool_use",
    "step_finish",
    "error",
];

pub fn detect_format(path: &Path, sample: usize) -> Result<SessionFormat> {
    let mut seen = 0usize;
    let mut codex_hits = 0usize;
    let mut claude_hits = 0usize;
    let mut opencode_hits = 0usize;

    for_each_jsonl_record_until(path, |object| {
        if seen >= sample {
            return Ok(false);
        }
        seen += 1;

        if object.contains_key("_parse_error") || object.contains_key("_nonobject") {
            return Ok(true);
        }

        let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
        if object.contains_key("payload") && CODEX_TYPES.contains(&record_type) {
            codex_hits += 1;
        }
        if !object.contains_key("payload")
            && (object.contains_key("sessionId")
                || object.contains_key("uuid")
                || object.contains_key("parentUuid"))
            && CLAUDE_TYPES.contains(&record_type)
        {
            claude_hits += 1;
        }
        if object.contains_key("sessionID")
            && OPENCODE_TYPES.contains(&record_type)
            && (object.contains_key("part") || object.contains_key("error"))
        {
            opencode_hits += 1;
        }

        Ok(true)
    })?;

    if opencode_hits > codex_hits && opencode_hits > claude_hits {
        Ok(SessionFormat::OpenCode)
    } else if claude_hits > codex_hits {
        Ok(SessionFormat::ClaudeCode)
    } else if codex_hits > 0 {
        Ok(SessionFormat::Codex)
    } else if claude_hits > 0 {
        Ok(SessionFormat::ClaudeCode)
    } else if opencode_hits > 0 {
        Ok(SessionFormat::OpenCode)
    } else {
        // 内容から判定できないとき(空 / 未知形式 / meta 行のみ)に限り、入力パスの
        // ヒントで補う。内容で決まる限りパスは見ない(リネーム・移動に強い既存挙動)。
        Ok(hint_from_path(path).unwrap_or(SessionFormat::Codex))
    }
}

/// 入力パスから形式を推定する補助シグナル。内容判定が不能なときだけ使う。
/// Codex: `rollout-*.jsonl` または `~/.codex/` 配下。
/// Claude Code: `~/.claude/` 配下(`projects/<id>.jsonl` 等)。
/// OpenCode: `opencode-*.jsonl` / `opencode-*.ndjson`。
fn hint_from_path(path: &Path) -> Option<SessionFormat> {
    let full = path.to_string_lossy();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.starts_with("rollout-") || full.contains("/.codex/") {
        Some(SessionFormat::Codex)
    } else if full.contains("/.claude/") {
        Some(SessionFormat::ClaudeCode)
    } else if name.starts_with("opencode-")
        && (name.ends_with(".jsonl") || name.ends_with(".ndjson"))
    {
        Some(SessionFormat::OpenCode)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn path_hint_detects_codex_and_claude() {
        assert_eq!(
            hint_from_path(&PathBuf::from(
                "/home/u/.codex/sessions/2026/06/07/rollout-abc.jsonl"
            )),
            Some(SessionFormat::Codex)
        );
        assert_eq!(
            hint_from_path(&PathBuf::from("rollout-abc.jsonl")),
            Some(SessionFormat::Codex)
        );
        assert_eq!(
            hint_from_path(&PathBuf::from("/home/u/.claude/projects/foo/1234.jsonl")),
            Some(SessionFormat::ClaudeCode)
        );
        assert_eq!(hint_from_path(&PathBuf::from("/tmp/random.jsonl")), None);
        assert_eq!(
            hint_from_path(&PathBuf::from("/tmp/opencode-session.ndjson")),
            Some(SessionFormat::OpenCode)
        );
    }

    #[test]
    fn empty_file_uses_path_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-empty.jsonl");
        std::fs::File::create(&path).unwrap();
        assert_eq!(detect_format(&path, 50).unwrap(), SessionFormat::Codex);
    }
}
