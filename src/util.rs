use anyhow::{Context, Result};
use serde_json::{Map, Number, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub type JsonObject = Map<String, Value>;
pub type Event = JsonObject;

pub fn for_each_jsonl_record<F>(path: &Path, mut callback: F) -> Result<()>
where
    F: FnMut(JsonObject) -> Result<()>,
{
    for_each_jsonl_record_until(path, |record| {
        callback(record)?;
        Ok(true)
    })
}

pub fn for_each_jsonl_record_until<F>(path: &Path, mut callback: F) -> Result<()>
where
    F: FnMut(JsonObject) -> Result<bool>,
{
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut lineno: u64 = 0;

    loop {
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if bytes == 0 {
            break;
        }

        let raw = String::from_utf8_lossy(&line);
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let mut record = parse_record(trimmed, lineno);
            if !callback(std::mem::take(&mut record))? {
                break;
            }
        }
        lineno += 1;
    }

    Ok(())
}

fn parse_record(raw: &str, lineno: u64) -> JsonObject {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(mut object)) => {
            object.insert("_lineno".to_string(), Value::Number(Number::from(lineno)));
            object
        }
        Ok(value) => {
            let mut object = JsonObject::new();
            object.insert("_lineno".to_string(), Value::Number(Number::from(lineno)));
            object.insert("_nonobject".to_string(), value);
            object
        }
        Err(err) => {
            let mut object = JsonObject::new();
            object.insert("_lineno".to_string(), Value::Number(Number::from(lineno)));
            object.insert("_parse_error".to_string(), Value::String(err.to_string()));
            object
        }
    }
}

pub fn event(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Event {
    let mut object = Event::new();
    for (key, value) in pairs {
        object.insert(key.to_string(), value);
    }
    object
}

pub fn null() -> Value {
    Value::Null
}

pub fn string<S: Into<String>>(value: S) -> Value {
    Value::String(value.into())
}

pub fn number_usize(value: usize) -> Value {
    Value::Number(Number::from(value as u64))
}

pub fn number_u64(value: u64) -> Value {
    Value::Number(Number::from(value))
}

pub fn clone_or_null(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

pub fn map_get<'a>(map: Option<&'a JsonObject>, key: &str) -> Option<&'a Value> {
    map.and_then(|m| m.get(key))
}

pub fn object_field<'a>(map: Option<&'a JsonObject>, key: &str) -> Option<&'a JsonObject> {
    map_get(map, key).and_then(Value::as_object)
}

pub fn value_as_string(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => default.to_string(),
        Some(other) => json_text(other),
    }
}

pub fn value_as_optional_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        Some(Value::Null) | None => None,
        Some(other) => Some(json_text(other)),
    }
}

pub fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::new())
}

pub fn text_from(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                let Some(object) = item.as_object() else {
                    continue;
                };
                let item_type = object.get("type").and_then(Value::as_str).unwrap_or("");
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    if !item_type.contains("image") {
                        parts.push(text.to_string());
                    }
                } else if item_type.contains("image") {
                    parts.push("[image]".to_string());
                }
            }
            parts.join("\n")
        }
        Some(Value::Null) | None => String::new(),
        Some(other) => json_text(other),
    }
}

pub fn json_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(v)) => *v,
        Some(Value::Number(v)) => v.as_i64().map(|n| n != 0).unwrap_or(true),
        Some(Value::String(v)) => !v.is_empty(),
        Some(Value::Array(v)) => !v.is_empty(),
        Some(Value::Object(v)) => !v.is_empty(),
    }
}

pub fn parse_arguments_field(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(raw)) => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        Some(other) => other.clone(),
        None => Value::Null,
    }
}

pub fn function_output_text(output: Option<&Value>) -> String {
    match output {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(object)) => {
            if let Some(content) = object.get("content").and_then(Value::as_str) {
                return content.to_string();
            }
            if let Some(items) = object
                .get("content_items")
                .or_else(|| object.get("content"))
            {
                if items.is_array() {
                    return text_from(Some(items));
                }
            }
            json_text(&Value::Object(object.clone()))
        }
        Some(Value::Null) | None => String::new(),
        Some(other) => value_as_string(Some(other), ""),
    }
}

pub fn command_from_args(args: &Value) -> Option<String> {
    let object = args.as_object()?;
    let candidate = object.get("cmd").or_else(|| object.get("command"))?;
    match candidate {
        Value::Array(items) => Some(
            items
                .iter()
                .map(plain_value_to_string)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Value::String(text) => Some(text.clone()),
        Value::Null => None,
        other => Some(plain_value_to_string(other)),
    }
}

pub fn plain_value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        other => json_text(other),
    }
}

pub fn trim_chars(value: &str, limit: usize) -> String {
    if limit == 0 {
        return value.to_string();
    }
    value.chars().take(limit).collect()
}
