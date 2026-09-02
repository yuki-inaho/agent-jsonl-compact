use crate::cli::SessionFormat;
use crate::util::{
    clone_or_null, command_from_args, event, function_output_text, json_truthy, map_get,
    number_usize, object_field, parse_arguments_field, string, text_from, value_as_optional_string,
    value_as_string, Event, JsonObject,
};
use serde_json::Value;

pub fn classify(object: &JsonObject, format: SessionFormat) -> Vec<Event> {
    match format {
        SessionFormat::ClaudeCode => classify_claude(object),
        SessionFormat::OpenCode => classify_opencode(object),
        SessionFormat::Codex | SessionFormat::Auto => classify_codex(object),
    }
}

fn classify_opencode(object: &JsonObject) -> Vec<Event> {
    if let Some(err) = object.get("_parse_error").and_then(Value::as_str) {
        return vec![event([
            ("ts", Value::Null),
            ("kind", string("parse_error")),
            ("error", string(err)),
        ])];
    }

    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = clone_or_null(object.get("timestamp"));
    let part = object.get("part").and_then(Value::as_object);

    match record_type {
        "step_start" => vec![event([
            ("ts", ts),
            ("kind", string("turn_start")),
            ("turn_id", clone_or_null(map_get(part, "messageID"))),
            ("step_id", clone_or_null(map_get(part, "id"))),
        ])],
        "step_finish" => vec![event([
            ("ts", ts),
            ("kind", string("turn_end")),
            ("turn_id", clone_or_null(map_get(part, "messageID"))),
            ("step_id", clone_or_null(map_get(part, "id"))),
            ("reason", clone_or_null(map_get(part, "reason"))),
            ("cost", clone_or_null(map_get(part, "cost"))),
            ("tokens", clone_or_null(map_get(part, "tokens"))),
        ])],
        "text" => vec![event([
            ("ts", ts),
            ("kind", string("assistant")),
            ("text", string(value_as_string(map_get(part, "text"), ""))),
        ])],
        "reasoning" => vec![event([
            ("ts", ts),
            ("kind", string("reasoning")),
            ("text", string(value_as_string(map_get(part, "text"), ""))),
        ])],
        "tool_use" => classify_opencode_tool(ts, part),
        "error" => {
            let raw = object.get("error");
            vec![event([
                ("ts", ts),
                ("kind", string("error")),
                ("text", string(opencode_error_text(raw))),
                ("error", clone_or_null(raw)),
            ])]
        }
        _ => vec![event([
            ("ts", ts),
            ("kind", string("oc_other")),
            ("oc_type", clone_or_null(object.get("type"))),
        ])],
    }
}

fn classify_opencode_tool(ts: Value, part: Option<&JsonObject>) -> Vec<Event> {
    let state = object_field(part, "state");
    let args = clone_or_null(map_get(state, "input"));
    let cmd = command_from_args(&args);
    let call_id = map_get(part, "callID").or_else(|| map_get(part, "id"));
    let status = map_get(state, "status")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut out = vec![event([
        ("ts", ts.clone()),
        ("kind", string("tool_call")),
        ("tool", clone_or_null(map_get(part, "tool"))),
        ("call_id", clone_or_null(call_id)),
        ("cmd", cmd.map(string).unwrap_or(Value::Null)),
        ("args", args),
        ("status", string(status)),
    ])];

    let output = match status {
        "completed" => map_get(state, "output").map(|value| value_as_string(Some(value), "")),
        "error" => map_get(state, "error").map(|value| value_as_string(Some(value), "")),
        _ => None,
    };
    if let Some(output) = output {
        out.push(event([
            ("ts", ts),
            ("kind", string("tool_output")),
            ("call_id", clone_or_null(call_id)),
            ("output", string(output)),
            ("is_error", Value::Bool(status == "error")),
        ]));
    }
    out
}

fn opencode_error_text(error: Option<&Value>) -> String {
    let object = error.and_then(Value::as_object);
    let data = object_field(object, "data");
    if let Some(value) = [
        map_get(data, "message"),
        map_get(object, "message"),
        map_get(object, "name"),
    ]
    .into_iter()
    .flatten()
    .next()
    {
        return value_as_string(Some(value), "");
    }
    value_as_string(error, "")
}

fn classify_codex(object: &JsonObject) -> Vec<Event> {
    if let Some(err) = object.get("_parse_error").and_then(Value::as_str) {
        return vec![event([
            ("ts", Value::Null),
            ("kind", string("parse_error")),
            ("error", string(err)),
        ])];
    }

    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = clone_or_null(object.get("timestamp"));
    let payload = object.get("payload").and_then(Value::as_object);

    match record_type {
        "session_meta" => vec![event([
            ("ts", ts),
            ("kind", string("session")),
            ("id", clone_or_null(map_get(payload, "id"))),
            (
                "forked_from_id",
                clone_or_null(map_get(payload, "forked_from_id")),
            ),
            ("cwd", string(value_as_string(map_get(payload, "cwd"), ""))),
            ("originator", clone_or_null(map_get(payload, "originator"))),
            (
                "cli_version",
                clone_or_null(map_get(payload, "cli_version")),
            ),
        ])],
        "turn_context" => vec![event([
            ("ts", ts),
            ("kind", string("turn_context")),
            ("model", clone_or_null(map_get(payload, "model"))),
        ])],
        "compacted" => {
            let replacement_history_len = map_get(payload, "replacement_history")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            vec![event([
                ("ts", ts),
                ("kind", string("compacted")),
                (
                    "message",
                    string(value_as_string(map_get(payload, "message"), "")),
                ),
                (
                    "replacement_history_len",
                    number_usize(replacement_history_len),
                ),
            ])]
        }
        "event_msg" => classify_codex_event_msg(ts, payload),
        "response_item" => classify_codex_response_item(ts, payload),
        _ => vec![event([
            ("ts", ts),
            ("kind", string("unknown")),
            ("rtype", clone_or_null(object.get("type"))),
        ])],
    }
}

fn classify_codex_event_msg(ts: Value, payload: Option<&JsonObject>) -> Vec<Event> {
    let event_type = map_get(payload, "type")
        .and_then(Value::as_str)
        .unwrap_or("");
    match event_type {
        "user_message" => vec![event([
            ("ts", ts),
            ("kind", string("user")),
            (
                "text",
                string(value_as_string(map_get(payload, "message"), "")),
            ),
        ])],
        "agent_message" => vec![event([
            ("ts", ts),
            ("kind", string("assistant")),
            ("phase", clone_or_null(map_get(payload, "phase"))),
            (
                "text",
                string(value_as_string(map_get(payload, "message"), "")),
            ),
        ])],
        "patch_apply_end" => vec![event([
            ("ts", ts),
            ("kind", string("patch")),
            ("success", clone_or_null(map_get(payload, "success"))),
            (
                "stdout",
                string(value_as_string(map_get(payload, "stdout"), "")),
            ),
        ])],
        "web_search_end" => vec![event([
            ("ts", ts),
            ("kind", string("web_search")),
            ("query", clone_or_null(map_get(payload, "query"))),
        ])],
        "mcp_tool_call_end" => {
            let invocation = object_field(payload, "invocation");
            vec![event([
                ("ts", ts),
                ("kind", string("mcp_tool")),
                ("server", clone_or_null(map_get(invocation, "server"))),
                ("tool", clone_or_null(map_get(invocation, "tool"))),
            ])]
        }
        "thread_goal_updated" => {
            let goal = object_field(payload, "goal");
            vec![event([
                ("ts", ts),
                ("kind", string("goal")),
                ("objective", clone_or_null(map_get(goal, "objective"))),
                ("status", clone_or_null(map_get(goal, "status"))),
            ])]
        }
        "task_started" | "turn_started" => vec![event([
            ("ts", ts),
            ("kind", string("turn_start")),
            ("turn_id", clone_or_null(map_get(payload, "turn_id"))),
        ])],
        "task_complete" => vec![event([
            ("ts", ts),
            ("kind", string("turn_end")),
            ("turn_id", clone_or_null(map_get(payload, "turn_id"))),
        ])],
        "turn_aborted" => vec![event([
            ("ts", ts),
            ("kind", string("turn_aborted")),
            ("reason", clone_or_null(map_get(payload, "reason"))),
        ])],
        "context_compacted" => vec![event([("ts", ts), ("kind", string("context_compacted"))])],
        "thread_rolled_back" => vec![event([
            ("ts", ts),
            ("kind", string("thread_rolled_back")),
            ("num_turns", clone_or_null(map_get(payload, "num_turns"))),
        ])],
        "item_completed" => {
            let item = object_field(payload, "item");
            vec![event([
                ("ts", ts),
                ("kind", string("item_completed")),
                ("item_type", clone_or_null(map_get(item, "type"))),
            ])]
        }
        "token_count" => vec![event([("ts", ts), ("kind", string("token_count"))])],
        _ => vec![event([
            ("ts", ts),
            ("kind", string("event_other")),
            ("event_type", clone_or_null(map_get(payload, "type"))),
        ])],
    }
}

fn classify_codex_response_item(ts: Value, payload: Option<&JsonObject>) -> Vec<Event> {
    let item_type = map_get(payload, "type")
        .and_then(Value::as_str)
        .unwrap_or("");
    match item_type {
        "message" => {
            let role = map_get(payload, "role")
                .and_then(Value::as_str)
                .unwrap_or("");
            let kind = match role {
                "user" => "api_user",
                "assistant" => "api_assistant",
                "developer" => "api_developer",
                _ => "api_other",
            };
            vec![event([
                ("ts", ts),
                ("kind", string(kind)),
                ("role", clone_or_null(map_get(payload, "role"))),
                ("phase", clone_or_null(map_get(payload, "phase"))),
                ("text", string(text_from(map_get(payload, "content")))),
            ])]
        }
        "reasoning" => {
            let summary = map_get(payload, "summary");
            let text = if json_truthy(summary) {
                text_from(summary)
            } else {
                String::new()
            };
            vec![event([
                ("ts", ts),
                ("kind", string("reasoning")),
                ("text", string(text.clone())),
                (
                    "encrypted",
                    Value::Bool(json_truthy(map_get(payload, "encrypted_content"))),
                ),
            ])]
        }
        "function_call" => {
            let args = parse_arguments_field(map_get(payload, "arguments"));
            let cmd = command_from_args(&args);
            vec![event([
                ("ts", ts),
                ("kind", string("tool_call")),
                ("tool", clone_or_null(map_get(payload, "name"))),
                ("call_id", clone_or_null(map_get(payload, "call_id"))),
                ("cmd", cmd.map(string).unwrap_or(Value::Null)),
                ("args", args),
            ])]
        }
        "function_call_output" => vec![event([
            ("ts", ts),
            ("kind", string("tool_output")),
            ("call_id", clone_or_null(map_get(payload, "call_id"))),
            (
                "output",
                string(function_output_text(map_get(payload, "output"))),
            ),
        ])],
        "web_search_call" => {
            let action = object_field(payload, "action");
            vec![event([
                ("ts", ts),
                ("kind", string("web_search")),
                ("query", clone_or_null(map_get(action, "query"))),
            ])]
        }
        _ => vec![event([
            ("ts", ts),
            ("kind", string("api_other")),
            ("subtype", clone_or_null(map_get(payload, "type"))),
        ])],
    }
}

fn classify_claude(object: &JsonObject) -> Vec<Event> {
    if let Some(err) = object.get("_parse_error").and_then(Value::as_str) {
        return vec![event([
            ("ts", Value::Null),
            ("kind", string("parse_error")),
            ("error", string(err)),
        ])];
    }

    let record_type = object.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = clone_or_null(object.get("timestamp"));
    let sidechain = object
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    match record_type {
        "user" => classify_claude_user(object, ts, sidechain),
        "assistant" => classify_claude_assistant(object, ts, sidechain),
        "summary" => classify_claude_summary(object, ts),
        _ if json_truthy(object.get("isCompactSummary")) => classify_claude_summary(object, ts),
        _ => vec![event([
            ("ts", ts),
            ("kind", string("cc_meta")),
            ("cc_type", clone_or_null(object.get("type"))),
        ])],
    }
}

fn classify_claude_user(object: &JsonObject, ts: Value, sidechain: bool) -> Vec<Event> {
    let message = object.get("message").and_then(Value::as_object);
    let content = map_get(message, "content");
    let mut out = Vec::new();

    match content {
        Some(Value::String(text)) => out.push(event([
            ("ts", ts),
            ("kind", string("user")),
            ("text", string(text.clone())),
            ("sidechain", Value::Bool(sidechain)),
        ])),
        Some(Value::Array(items)) => {
            for item in items {
                let Some(item_object) = item.as_object() else {
                    continue;
                };
                let content_type = item_object
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match content_type {
                    "tool_result" => out.push(event([
                        ("ts", ts.clone()),
                        ("kind", string("tool_output")),
                        ("call_id", clone_or_null(item_object.get("tool_use_id"))),
                        ("output", string(text_from(item_object.get("content")))),
                        ("sidechain", Value::Bool(sidechain)),
                    ])),
                    "text" => out.push(event([
                        ("ts", ts.clone()),
                        ("kind", string("user")),
                        ("text", string(value_as_string(item_object.get("text"), ""))),
                        ("sidechain", Value::Bool(sidechain)),
                    ])),
                    other if other.contains("image") => out.push(event([
                        ("ts", ts.clone()),
                        ("kind", string("user")),
                        ("text", string("[image]")),
                        ("sidechain", Value::Bool(sidechain)),
                    ])),
                    _ => {}
                }
            }
        }
        _ => {}
    }

    out
}

fn classify_claude_assistant(object: &JsonObject, ts: Value, sidechain: bool) -> Vec<Event> {
    let message = object.get("message").and_then(Value::as_object);
    let model = clone_or_null(map_get(message, "model"));
    let mut out = Vec::new();

    let Some(Value::Array(items)) = map_get(message, "content") else {
        return out;
    };

    for item in items {
        let Some(item_object) = item.as_object() else {
            continue;
        };
        let content_type = item_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        match content_type {
            "text" => out.push(event([
                ("ts", ts.clone()),
                ("kind", string("assistant")),
                ("text", string(value_as_string(item_object.get("text"), ""))),
                ("model", model.clone()),
                ("sidechain", Value::Bool(sidechain)),
            ])),
            "thinking" => {
                let thinking = value_as_string(item_object.get("thinking"), "");
                if !thinking.is_empty() {
                    out.push(event([
                        ("ts", ts.clone()),
                        ("kind", string("thinking")),
                        ("text", string(thinking)),
                        ("sidechain", Value::Bool(sidechain)),
                    ]));
                }
            }
            "tool_use" => {
                let name = value_as_optional_string(item_object.get("name"));
                let input = clone_or_null(item_object.get("input"));
                let cmd = claude_tool_cmd(name.as_deref(), item_object.get("input"));
                out.push(event([
                    ("ts", ts.clone()),
                    ("kind", string("tool_call")),
                    ("tool", name.map(string).unwrap_or(Value::Null)),
                    ("call_id", clone_or_null(item_object.get("id"))),
                    ("cmd", cmd.map(string).unwrap_or(Value::Null)),
                    ("args", input),
                    ("sidechain", Value::Bool(sidechain)),
                ]));
            }
            _ => {}
        }
    }

    out
}

fn classify_claude_summary(object: &JsonObject, ts: Value) -> Vec<Event> {
    let message = object.get("message").and_then(Value::as_object);
    let summary = if json_truthy(object.get("summary")) {
        value_as_string(object.get("summary"), "")
    } else {
        text_from(map_get(message, "content"))
    };
    vec![event([
        ("ts", ts),
        ("kind", string("compacted")),
        ("message", string(summary)),
    ])]
}

fn claude_tool_cmd(name: Option<&str>, input: Option<&Value>) -> Option<String> {
    let object = input.and_then(Value::as_object)?;
    match name {
        Some("Bash") => value_as_optional_string(object.get("command")),
        Some("Read" | "Edit" | "Write" | "NotebookEdit") => {
            let path = value_as_string(object.get("file_path"), "");
            Some(
                format!("{} {}", name.unwrap_or_default(), path)
                    .trim()
                    .to_string(),
            )
        }
        _ => None,
    }
}
