use crate::cli::{Channel, SessionFormat};
use crate::util::{json_text, string, Event};
use serde_json::Value;
use std::collections::HashSet;

const TERMINAL_KINDS: &[&str] = &[
    "session",
    "user",
    "assistant",
    "thinking",
    "reasoning",
    "tool_call",
    "tool_output",
    "patch",
    "web_search",
    "mcp_tool",
    "goal",
    "turn_start",
    "turn_end",
    "turn_aborted",
    "compacted",
    "context_compacted",
    "thread_rolled_back",
    "item_completed",
    "parse_error",
];

const API_KINDS: &[&str] = &[
    "session",
    "api_user",
    "api_assistant",
    "api_developer",
    "reasoning",
    "tool_call",
    "tool_output",
    "web_search",
    "goal",
    "turn_start",
    "turn_end",
    "turn_aborted",
    "compacted",
    "parse_error",
];

const CLAUDE_KINDS: &[&str] = &[
    "session",
    "user",
    "assistant",
    "thinking",
    "tool_call",
    "tool_output",
    "compacted",
    "parse_error",
];

const DEDUP_TEXT_KINDS: &[&str] = &["user", "assistant", "api_user", "api_assistant"];

pub fn keep_set(
    format: SessionFormat,
    channel: Channel,
    keep_token_count: bool,
) -> HashSet<String> {
    let mut keep: HashSet<String> = match format {
        SessionFormat::ClaudeCode => CLAUDE_KINDS.iter().map(|v| (*v).to_string()).collect(),
        SessionFormat::Codex | SessionFormat::Auto => match channel {
            Channel::Terminal => TERMINAL_KINDS.iter().map(|v| (*v).to_string()).collect(),
            Channel::Api => API_KINDS.iter().map(|v| (*v).to_string()).collect(),
            Channel::Both => TERMINAL_KINDS
                .iter()
                .chain(API_KINDS.iter())
                .map(|v| (*v).to_string())
                .collect(),
        },
    };

    if keep_token_count {
        keep.insert("token_count".to_string());
    }
    keep
}

#[derive(Debug, Clone)]
pub struct Cleaner {
    keep: HashSet<String>,
    msg_chars: usize,
    out_chars: usize,
    dedup: bool,
    elide_outputs: bool,
    seen_session: bool,
    last_text_key: Option<(String, Option<String>)>,
    seen_web_search: HashSet<Option<String>>,
}

impl Cleaner {
    pub fn new(
        keep: HashSet<String>,
        msg_chars: usize,
        out_chars: usize,
        dedup: bool,
        elide_outputs: bool,
    ) -> Self {
        Self {
            keep,
            msg_chars,
            out_chars,
            dedup,
            elide_outputs,
            seen_session: false,
            last_text_key: None,
            seen_web_search: HashSet::new(),
        }
    }

    pub fn accept(&mut self, mut event: Event) -> Option<Event> {
        let kind = kind_of(&event)?.to_string();
        if !self.keep.contains(&kind) {
            return None;
        }

        if kind == "session" {
            if self.seen_session {
                return None;
            }
            self.seen_session = true;
        }

        if event.contains_key("text") {
            transform_text_field(&mut event, "text", self.msg_chars);
        }
        for field in ["output", "stdout"] {
            if event.contains_key(field) {
                if self.elide_outputs {
                    elide_field(&mut event, field);
                } else {
                    transform_text_field(&mut event, field, self.out_chars);
                }
            }
        }

        if self.dedup {
            if DEDUP_TEXT_KINDS.contains(&kind.as_str()) {
                let text = optional_text(event.get("text"));
                let key = (kind.clone(), text);
                if Some(key.clone()) == self.last_text_key {
                    return None;
                }
                self.last_text_key = Some(key);
            } else if kind == "web_search" {
                let query = optional_text(event.get("query"));
                if self.seen_web_search.contains(&query) {
                    return None;
                }
                self.seen_web_search.insert(query);
            }
        }

        Some(event)
    }
}

pub fn kind_of(event: &Event) -> Option<&str> {
    event.get("kind").and_then(Value::as_str)
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(Value::Bool(flag)) => Some(flag.to_string()),
        Some(Value::Null) | None => None,
        Some(other) => Some(json_text(other)),
    }
}

fn transform_text_field(event: &mut Event, field: &str, limit: usize) {
    let Some(value) = event.get(field) else {
        return;
    };
    if matches!(value, Value::Null) {
        return;
    }
    let text = optional_text(Some(value)).unwrap_or_default();
    event.insert(field.to_string(), string(truncate(&text, limit)));
}

fn truncate(text: &str, limit: usize) -> String {
    let normalized = text.replace("\r\n", "\n");
    let total_chars = normalized.chars().count();
    if limit > 0 && total_chars > limit {
        let head: String = normalized.chars().take(limit).collect();
        format!("{head}\n…[+{} chars truncated]", total_chars - limit)
    } else {
        normalized
    }
}

fn elide_field(event: &mut Event, field: &str) {
    let Some(value) = event.get(field) else {
        return;
    };
    if matches!(value, Value::Null) {
        return;
    }
    let text = optional_text(Some(value)).unwrap_or_default();
    event.insert(field.to_string(), string(elide_body(&text, 200)));
}

fn elide_body(body: &str, threshold: usize) -> String {
    let body_len = body.chars().count();
    if body.is_empty() || body_len <= threshold {
        return body.to_string();
    }
    let line_count = body.lines().count();
    let first = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(100)
        .collect::<String>();
    format!("[output elided: {line_count} lines / {body_len} chars] {first}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{event, string};

    #[test]
    fn truncate_preserves_full_text_when_limit_is_zero() {
        assert_eq!(truncate("abc", 0), "abc");
    }

    #[test]
    fn cleaner_drops_duplicate_text() {
        let keep = ["user"]
            .into_iter()
            .map(|value| value.to_string())
            .collect();
        let mut cleaner = Cleaner::new(keep, 0, 0, true, false);
        assert!(cleaner
            .accept(event([("kind", string("user")), ("text", string("hi"))]))
            .is_some());
        assert!(cleaner
            .accept(event([("kind", string("user")), ("text", string("hi"))]))
            .is_none());
    }
}
