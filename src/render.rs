use crate::cli::SessionFormat;
use crate::util::{json_text, Event};
use serde_json::{Map, Value};

pub fn render_markdown(events: &[Event], format: SessionFormat) -> String {
    let mut lines = vec![format!(
        "# Session 抽出 (format={}, ターミナル相当)\n",
        format.as_str()
    )];

    for event in events {
        let kind = str_field(event, "kind");
        let ts = hhmmss(event.get("ts"));
        let sub = if event
            .get("sidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            " ↳[sub]"
        } else {
            ""
        };

        match kind.as_deref().unwrap_or("") {
            "session" => {
                let mut line = format!(
                    "> **session** `{}` cwd=`{}` cli={} originator={}",
                    display_field(event, "id"),
                    display_field(event, "cwd"),
                    display_field(event, "cli_version"),
                    display_field(event, "originator"),
                );
                if let Some(branch) = str_field(event, "git_branch") {
                    if !branch.is_empty() {
                        line.push_str(&format!(" branch={branch}"));
                    }
                }
                if let Some(forked_from) = str_field(event, "forked_from_id") {
                    if !forked_from.is_empty() {
                        line.push_str(&format!(" forked_from={forked_from}"));
                    }
                }
                lines.push(line);
            }
            "goal" => lines.push(format!(
                "\n## 🎯 GOAL [{}]\n{}\n",
                display_field(event, "status"),
                display_field(event, "objective")
            )),
            "turn_start" => lines.push(format!("\n--- turn start ({ts}) ---")),
            "turn_end" => lines.push(format!("--- turn end ({ts}) ---")),
            "turn_aborted" => lines.push(format!(
                "\n--- turn aborted: {} ---",
                display_field(event, "reason")
            )),
            "user" | "assistant" | "api_user" | "api_assistant" | "api_developer" => {
                let role = match kind.as_deref().unwrap_or("") {
                    "user" => "👤 USER",
                    "assistant" => "💬 ASSISTANT",
                    "api_user" => "👤 USER(api)",
                    "api_assistant" => "💬 ASSISTANT(api)",
                    "api_developer" => "🛠 DEVELOPER(api)",
                    _ => "MESSAGE",
                };
                let phase = str_field(event, "phase")
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" [{value}]"))
                    .unwrap_or_default();
                lines.push(format!(
                    "\n### {role}{phase}{sub} ({ts})\n{}\n",
                    display_field(event, "text")
                ));
            }
            "thinking" => {
                let body = display_field(event, "text");
                if body.is_empty() {
                    lines.push("\n*🧠 thinking: [empty]*".to_string());
                } else {
                    lines.push(format!("\n*🧠 thinking{sub}:* {body}"));
                }
            }
            "reasoning" => {
                let body = display_field(event, "text");
                if !body.is_empty() {
                    lines.push(format!("\n*🧠 reasoning(summary):* {body}"));
                } else if event
                    .get("encrypted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    lines.push("\n*🧠 reasoning: [encrypted — 本文は復元不可]*".to_string());
                } else {
                    lines.push("\n*🧠 reasoning: [本文なし]*".to_string());
                }
            }
            "tool_call" => {
                let cmd = str_field(event, "cmd").unwrap_or_default();
                if !cmd.is_empty() {
                    lines.push(format!("\n{}", fence(&format!("$ {cmd}"), "bash")));
                } else {
                    let args = event
                        .get("args")
                        .map(json_text)
                        .map(|value| value.chars().take(300).collect::<String>())
                        .unwrap_or_default();
                    lines.push(format!(
                        "\n🔧 **{}**{sub} {args}",
                        display_field(event, "tool")
                    ));
                }
            }
            "tool_output" => lines.push(fence(&display_field(event, "output"), "")),
            "patch" => {
                let ok = if event
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "✅"
                } else {
                    "❌"
                };
                lines.push(format!(
                    "\n📝 patch {ok}\n{}",
                    fence(display_field(event, "stdout").trim(), "")
                ));
            }
            "web_search" => lines.push(format!(
                "\n🔎 web_search: `{}`",
                display_field(event, "query")
            )),
            "mcp_tool" => lines.push(format!(
                "\n🧩 mcp: {}/{}",
                display_field(event, "server"),
                display_field(event, "tool")
            )),
            "compacted" | "context_compacted" => {
                let replacement = event
                    .get("replacement_history_len")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .map(|value| format!(" (replaced {value} items)"))
                    .unwrap_or_default();
                lines.push(format!("\n— context compacted —{replacement}"));
            }
            "thread_rolled_back" => lines.push(format!(
                "\n— rolled back {} turn(s) —",
                display_field(event, "num_turns")
            )),
            "item_completed" => lines.push(format!(
                "\n📦 item_completed: {}",
                display_field(event, "item_type")
            )),
            "token_count" => lines.push("· token_count".to_string()),
            other => {
                let mut extra = Map::new();
                for (key, value) in event {
                    if key != "ts" && key != "kind" {
                        extra.insert(key.clone(), value.clone());
                    }
                }
                let extra_text = json_text(&Value::Object(extra))
                    .chars()
                    .take(200)
                    .collect::<String>();
                lines.push(format!("\n• {other}: {extra_text}"));
            }
        }
    }

    format!("{}\n", lines.join("\n"))
}

pub fn fence(body: &str, lang: &str) -> String {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in body.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    let ticks = "`".repeat(std::cmp::max(3, longest + 1));
    format!("{ticks}{lang}\n{body}\n{ticks}")
}

fn hhmmss(value: Option<&Value>) -> String {
    let Some(ts) = value.and_then(Value::as_str) else {
        return String::new();
    };
    ts.get(11..19).unwrap_or("").to_string()
}

fn str_field(event: &Event, key: &str) -> Option<String> {
    match event.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(Value::Bool(flag)) => Some(flag.to_string()),
        Some(Value::Null) | None => None,
        Some(other) => Some(json_text(other)),
    }
}

fn display_field(event: &Event, key: &str) -> String {
    str_field(event, key).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::fence;

    #[test]
    fn fence_uses_longer_delimiter_when_body_contains_backticks() {
        let rendered = fence("a ``` b", "");
        assert!(rendered.starts_with("````\n"));
    }
}
