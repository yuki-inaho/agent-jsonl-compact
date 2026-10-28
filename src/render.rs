use crate::cli::SessionFormat;
use crate::query::output::escape_controls_singleline;
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
                    display_line(event, "id"),
                    display_line(event, "cwd"),
                    display_line(event, "cli_version"),
                    display_line(event, "originator"),
                );
                if let Some(branch) = str_field(event, "git_branch") {
                    if !branch.is_empty() {
                        line.push_str(&format!(" branch={}", escape_controls_singleline(&branch)));
                    }
                }
                if let Some(forked_from) = str_field(event, "forked_from_id") {
                    if !forked_from.is_empty() {
                        line.push_str(&format!(
                            " forked_from={}",
                            escape_controls_singleline(&forked_from)
                        ));
                    }
                }
                lines.push(line);
            }
            "goal" => lines.push(format!(
                "\n## 🎯 GOAL [{}]\n{}\n",
                display_line(event, "status"),
                fence(&display_body(event, "objective"), "text")
            )),
            "turn_start" => lines.push(format!("\n--- turn start ({ts}) ---")),
            "turn_end" => {
                let reason = str_field(event, "reason")
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" reason={}", escape_controls_singleline(&value)))
                    .unwrap_or_default();
                let cost = str_field(event, "cost")
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" cost={}", escape_controls_singleline(&value)))
                    .unwrap_or_default();
                lines.push(format!("--- turn end ({ts}){reason}{cost} ---"));
            }
            "turn_aborted" => lines.push(format!(
                "\n--- turn aborted: {} ---",
                display_line(event, "reason")
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
                    .map(|value| format!(" [{}]", escape_controls_singleline(&value)))
                    .unwrap_or_default();
                lines.push(format!(
                    "\n### {role}{phase}{sub} ({ts})\n{}\n",
                    fence(&display_body(event, "text"), "text")
                ));
            }
            "injected" => {
                lines.push(format!(
                    "\n### 🛠 FRAMEWORK(meta){sub} ({ts})\n{}\n",
                    fence(&display_body(event, "text"), "text")
                ));
            }
            "thinking" => {
                let body = display_body(event, "text");
                if body.is_empty() {
                    lines.push("\n*🧠 thinking: [empty]*".to_string());
                } else {
                    lines.push(format!("\n*🧠 thinking{sub}:*\n{}", fence(&body, "text")));
                }
            }
            "reasoning" => {
                let body = display_body(event, "text");
                if !body.is_empty() {
                    lines.push(format!(
                        "\n*🧠 reasoning(summary):*\n{}",
                        fence(&body, "text")
                    ));
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
                        .map(|value| escape_controls_singleline(&value))
                        .unwrap_or_default();
                    lines.push(format!(
                        "\n🔧 **{}**{sub} {args}",
                        display_line(event, "tool")
                    ));
                }
            }
            "tool_output" => lines.push(fence(&display_body(event, "output"), "")),
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
                    fence(display_body(event, "stdout").trim(), "")
                ));
            }
            "web_search" => lines.push(format!(
                "\n🔎 web_search: `{}`",
                display_line(event, "query")
            )),
            "mcp_tool" => lines.push(format!(
                "\n🧩 mcp: {}/{}",
                display_line(event, "server"),
                display_line(event, "tool")
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
                display_line(event, "num_turns")
            )),
            "item_completed" => lines.push(format!(
                "\n📦 item_completed: {}",
                display_line(event, "item_type")
            )),
            "token_count" => lines.push("· token_count".to_string()),
            "error" => lines.push(format!(
                "\n❌ **error** ({ts})\n{}",
                fence(&display_body(event, "text"), "text")
            )),
            other => {
                let mut extra = Map::new();
                for (key, value) in event {
                    if key != "ts" && key != "kind" {
                        extra.insert(key.clone(), value.clone());
                    }
                }
                let extra_text = escape_controls_singleline(
                    &json_text(&Value::Object(extra))
                        .chars()
                        .take(200)
                        .collect::<String>(),
                );
                lines.push(format!(
                    "\n• {}: {extra_text}",
                    escape_controls_singleline(other)
                ));
            }
        }
    }

    format!("{}\n", lines.join("\n"))
}

pub fn fence(body: &str, lang: &str) -> String {
    let body = crate::query::output::escape_controls(body);
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
    match value {
        Some(Value::String(ts)) => escape_controls_singleline(ts.get(11..19).unwrap_or("")),
        Some(Value::Number(ts)) => ts
            .as_u64()
            .map(|millis| {
                let seconds = (millis / 1000) % 86_400;
                format!(
                    "{:02}:{:02}:{:02}",
                    seconds / 3600,
                    (seconds / 60) % 60,
                    seconds % 60
                )
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
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

fn display_line(event: &Event, key: &str) -> String {
    escape_controls_singleline(&str_field(event, key).unwrap_or_default())
}

fn display_body(event: &Event, key: &str) -> String {
    str_field(event, key).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{fence, hhmmss};
    use serde_json::json;

    #[test]
    fn fence_uses_longer_delimiter_when_body_contains_backticks() {
        let rendered = fence("a ``` b", "");
        assert!(rendered.starts_with("````\n"));
    }

    #[test]
    fn hhmmss_accepts_unix_milliseconds_from_opencode() {
        assert_eq!(hhmmss(Some(&json!(3_723_000))), "01:02:03");
    }
}
