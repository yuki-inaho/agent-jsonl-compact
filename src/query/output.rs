//! Bounded output rendering: JSONL (events + one final metadata record) or text.
//!
//! The envelope is serialized and measured *before* writing. If the required metadata cannot fit
//! the requested budget the caller returns `budget_too_small` (rc2) instead of emitting invalid
//! or truncated JSON. Text previews are cut on UTF-8 boundaries and `text_complete=false`.
//! Diagnostics go to stderr and count toward the same budget.

use crate::query::model::QEvent;
use serde_json::{json, Map, Value};

pub const SCHEMA_RECORD_EVENT: &str = "event";
pub const SCHEMA_RECORD_METADATA: &str = "metadata";

/// Escape terminal control characters (ANSI, OSC, C1) so logs render safely.
/// Newlines and tabs are preserved: this is the body/multiline variant.
pub fn escape_controls(text: &str) -> String {
    escape_controls_with(text, false)
}

/// Escape terminal control characters and also newlines/tabs, keeping one logical line.
/// Use for timestamps, ids, headings, paths and diagnostics.
pub fn escape_controls_singleline(text: &str) -> String {
    escape_controls_with(text, true)
}

fn escape_controls_with(text: &str, singleline: bool) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\u{1b}' => out.push_str("\\x1b"),
            '\u{9b}' => out.push_str("\\u{9b}"),
            '\u{07}' => out.push_str("\\x07"),
            '\n' if singleline => out.push_str("\\n"),
            '\r' if singleline => out.push_str("\\r"),
            '\t' if singleline => out.push_str("\\t"),
            '\u{00}'..='\u{08}' | '\u{0b}'..='\u{1f}' | '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            c if (c as u32) >= 0x80 && (c as u32) <= 0x9f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Truncate to `max_bytes` on a UTF-8 char boundary; returns (text, complete).
pub fn cut_utf8(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, true);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], false)
}

pub fn event_to_json(event: &QEvent, text_limit: usize) -> (Value, bool) {
    let (preview, complete) = cut_utf8(&event.text, text_limit);
    let mut object = Map::new();
    object.insert(
        "schema_version".into(),
        json!(crate::query::model::QUERY_SCHEMA_VERSION),
    );
    object.insert("record_type".into(), json!(SCHEMA_RECORD_EVENT));
    object.insert("provider".into(), json!(event.provider.as_str()));
    object.insert("session_id".into(), json!(event.session_id));
    object.insert("event_id".into(), json!(event.event_id));
    object.insert(
        "timestamp".into(),
        match &event.timestamp_raw {
            Some(raw) => json!(raw),
            None => Value::Null,
        },
    );
    object.insert(
        "epoch_ms".into(),
        match event.epoch_ms {
            Some(ms) => json!(ms),
            None => Value::Null,
        },
    );
    object.insert(
        "role".into(),
        match event.role {
            Some(role) => json!(role.as_str()),
            None => Value::Null,
        },
    );
    object.insert(
        "phase".into(),
        match &event.phase {
            Some(phase) => json!(phase),
            None => Value::Null,
        },
    );
    object.insert("kind".into(), json!(event.kind.as_str()));
    object.insert("source_kind".into(), json!(event.source_kind));
    object.insert("message_origin".into(), json!(event.origin.as_str()));
    object.insert("text".into(), json!(escape_controls(preview)));
    object.insert("text_complete".into(), json!(complete));
    object.insert("text_bytes".into(), json!(event.text.len()));
    object.insert(
        "source".into(),
        json!({
            "raw_line": event.source.raw_line,
            "raw_line_verified": event.raw_line_verified,
            "byte_start": event.source.byte_start,
            "byte_end": event.source.byte_end,
            "subevent_index": event.source.subevent_index,
        }),
    );
    if let Some(tool) = &event.tool_name {
        object.insert("tool_name".into(), json!(tool));
    }
    if let Some(call_id) = &event.call_id {
        object.insert("call_id".into(), json!(call_id));
    }
    if let Some(origin_id) = &event.origin_id {
        object.insert("origin_id".into(), json!(origin_id));
    }
    if let Some(mirror_of) = &event.mirror_of {
        object.insert("mirror_of".into(), json!(mirror_of));
    }
    if let Some(relation) = &event.mirror_relation {
        object.insert("mirror_relation".into(), json!(relation));
    }
    (Value::Object(object), complete)
}

pub fn attach_observed(value: &mut Value, event: &QEvent) {
    let observed = match &event.tool_observed {
        crate::query::model::ToolObserved::None => return,
        crate::query::model::ToolObserved::Returned { is_error, status } => json!({
            "is_error": is_error,
            "status": status,
        }),
    };
    if let Some(object) = value.as_object_mut() {
        object.insert("tool_observed".into(), observed);
    }
}

/// Human-readable rendering of one prepared event record (already JSON-valued).
pub fn event_to_text(value: &Value) -> String {
    let ts = value
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
    let role = value.get("role").and_then(Value::as_str).unwrap_or("-");
    let source = value.get("source").cloned().unwrap_or(Value::Null);
    let line_no = source.get("raw_line").and_then(Value::as_u64).unwrap_or(0);
    let sub = source
        .get("subevent_index")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let complete = value
        .get("text_complete")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let context = value
        .get("is_context")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = value.get("text").and_then(Value::as_str).unwrap_or("");
    format!(
        "--- [{line_no}.{sub}] {} {}/{} {}{} ---\n{}\n",
        escape_controls_singleline(ts),
        escape_controls_singleline(kind),
        escape_controls_singleline(role),
        if context { "(context) " } else { "" },
        if complete { "" } else { "(cut)" },
        escape_controls(text),
    )
}

pub fn line(value: &Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_cut_is_safe() {
        let text = "три";
        let (cut, complete) = cut_utf8(text, 4);
        // "три" = 2+2+2 bytes: the largest boundary <= 4 is after two chars.
        assert_eq!(cut, "тр");
        assert!(!complete);
        assert!(text.is_char_boundary(cut.len()));
        let (all, complete) = cut_utf8(text, 100);
        assert_eq!(all, text);
        assert!(complete);
    }

    #[test]
    fn controls_are_escaped() {
        let escaped = escape_controls("\u{1b}[31mred\u{9b}31m\u{07}");
        assert!(!escaped.contains('\u{1b}'));
        assert!(!escaped.contains('\u{9b}'));
        assert!(escaped.contains("\\x1b"));
        assert!(escaped.contains("\\u{9b}"));
    }

    #[test]
    fn json_output_does_not_embed_raw_control_bytes() {
        let event_text = "a\u{1b}]0;title\u{07}b";
        let escaped = escape_controls(event_text);
        assert!(!escaped.contains('\u{1b}'));
    }
}
