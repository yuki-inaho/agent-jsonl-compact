use crate::cli::SessionFormat;
use crate::util::for_each_jsonl_record_until;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub fn detect_format(path: &Path, sample: usize) -> Result<SessionFormat> {
    let codex_types: HashSet<&'static str> = [
        "session_meta",
        "response_item",
        "event_msg",
        "turn_context",
        "compacted",
    ]
    .into_iter()
    .collect();
    let claude_types: HashSet<&'static str> = [
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
    ]
    .into_iter()
    .collect();

    let mut seen = 0usize;
    let mut codex_hits = 0usize;
    let mut claude_hits = 0usize;

    for_each_jsonl_record_until(path, |object| {
        if seen >= sample {
            return Ok(false);
        }
        seen += 1;

        if object.contains_key("_parse_error") || object.contains_key("_nonobject") {
            return Ok(true);
        }

        let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
        if object.contains_key("payload") && codex_types.contains(record_type) {
            codex_hits += 1;
        }
        if !object.contains_key("payload")
            && (object.contains_key("sessionId")
                || object.contains_key("uuid")
                || object.contains_key("parentUuid"))
            && claude_types.contains(record_type)
        {
            claude_hits += 1;
        }

        Ok(true)
    })?;

    if claude_hits > codex_hits {
        Ok(SessionFormat::ClaudeCode)
    } else if codex_hits > 0 {
        Ok(SessionFormat::Codex)
    } else if claude_hits > 0 {
        Ok(SessionFormat::ClaudeCode)
    } else {
        Ok(SessionFormat::Codex)
    }
}
