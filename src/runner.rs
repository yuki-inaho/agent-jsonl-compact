use crate::classify::classify;
use crate::clean::{keep_set, kind_of, Cleaner};
use crate::cli::{Cli, OutputFormat, SessionFormat};
use crate::counter::Counter;
use crate::detect::detect_format;
use crate::render::render_markdown;
use crate::util::{
    clone_or_null, event, for_each_jsonl_record, json_text, json_truthy, number_u64, string, Event,
    JsonObject,
};
use anyhow::{bail, Context, Result};
use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Stats(StatsReport),
    Extract(ExtractReport),
}

impl RunOutcome {
    pub fn print(&self) {
        match self {
            RunOutcome::Stats(report) => report.print(),
            RunOutcome::Extract(report) => report.print(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatsReport {
    pub format: SessionFormat,
    pub raw_type_counter: Counter,
    pub kind_counter_all: Counter,
    pub kept_kind_counter: Counter,
}

impl StatsReport {
    pub fn print(&self) {
        println!("detected format: {}\n", self.format.as_str());
        println!("== 生レコード型 ==");
        for (key, count) in self.raw_type_counter.most_common() {
            println!("{count:>7}  {}", key.replace('/', " / "));
        }
        println!("\n== 正規化 kind(全件) ==");
        for (key, count) in self.kind_counter_all.most_common() {
            println!("{count:>7}  {key}");
        }
        println!("\n== 出力された kind(フィルタ後) ==");
        for (key, count) in self.kept_kind_counter.most_common() {
            println!("{count:>7}  {key}");
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractReport {
    pub format: SessionFormat,
    pub input_bytes: u64,
    pub total_lines: usize,
    pub kept_events: usize,
    pub output_bytes: u64,
    pub written: Vec<PathBuf>,
}

impl ExtractReport {
    pub fn print(&self) {
        println!("format: {}", self.format.as_str());
        println!(
            "input : {:.1} MB / {} lines",
            self.input_bytes as f64 / 1e6,
            self.total_lines
        );
        println!("kept  : {} events", self.kept_events);
        let ratio = 100.0 * self.output_bytes as f64 / self.input_bytes.max(1) as f64;
        println!(
            "output: {:.2} MB  ({:.2}% of input)",
            self.output_bytes as f64 / 1e6,
            ratio
        );
        for path in &self.written {
            let size_kb = fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0);
            println!("  - {}  ({size_kb} KB)", path.display());
        }
    }
}

pub fn run(args: Cli) -> Result<RunOutcome> {
    if !args.input.exists() {
        bail!("input not found: {}", args.input.display());
    }

    let format = match args.format {
        SessionFormat::Auto => detect_format(&args.input, 50)?,
        other => other,
    };
    let keep = keep_set(format, args.channel, args.keep_token_count);
    let mut cleaner = Cleaner::new(
        keep,
        args.msg_chars,
        args.out_chars,
        !args.no_dedup,
        args.elide_outputs,
    );

    let mut raw_type_counter = Counter::default();
    let mut kind_counter_all = Counter::default();
    let mut kept_kind_counter = Counter::default();
    let mut models = BTreeSet::new();
    let mut kept: Vec<Event> = Vec::new();
    let mut claude_session_done = false;

    for_each_jsonl_record(&args.input, |object| {
        raw_type_counter.inc(raw_type_key(&object, format));

        if format == SessionFormat::ClaudeCode
            && !claude_session_done
            && json_truthy(object.get("cwd"))
        {
            claude_session_done = true;
            let session_event = event([
                ("ts", clone_or_null(object.get("timestamp"))),
                ("kind", string("session")),
                ("id", clone_or_null(object.get("sessionId"))),
                ("cwd", clone_or_null(object.get("cwd"))),
                ("cli_version", clone_or_null(object.get("version"))),
                ("git_branch", clone_or_null(object.get("gitBranch"))),
                ("originator", string("claude-code")),
            ]);
            kind_counter_all.inc("session");
            if let Some(accepted) = cleaner.accept(session_event) {
                kept_kind_counter.inc(kind_of(&accepted).unwrap_or(""));
                kept.push(accepted);
            }
        }

        for event in classify(&object, format) {
            let kind = kind_of(&event).unwrap_or("").to_string();
            kind_counter_all.inc(kind.clone());
            if let Some(model) = event.get("model").and_then(Value::as_str) {
                models.insert(model.to_string());
            }
            if let Some(accepted) = cleaner.accept(event) {
                kept_kind_counter.inc(kind_of(&accepted).unwrap_or(""));
                kept.push(accepted);
            }
        }

        Ok(())
    })?;

    if args.stats {
        return Ok(RunOutcome::Stats(StatsReport {
            format,
            raw_type_counter,
            kind_counter_all,
            kept_kind_counter,
        }));
    }

    let out_dir = args.out_dir.clone().unwrap_or(std::env::current_dir()?);
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create output directory {}", out_dir.display()))?;
    let base = args.name.clone().unwrap_or_else(|| {
        args.input
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "session".to_string())
    });

    let summary = build_summary(
        &args,
        format,
        &raw_type_counter,
        &kind_counter_all,
        &kept_kind_counter,
        &models,
        &kept,
    )?;

    let mut written = Vec::new();
    if matches!(args.format_out, OutputFormat::Jsonl | OutputFormat::Both) {
        let path = out_dir.join(format!("{base}.clean.jsonl"));
        let mut file =
            File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
        for event in &kept {
            writeln!(file, "{}", serde_json::to_string(event)?)?;
        }
        written.push(path);
    }

    if matches!(args.format_out, OutputFormat::Md | OutputFormat::Both) {
        let path = out_dir.join(format!("{base}.transcript.md"));
        fs::write(&path, render_markdown(&kept, format))
            .with_context(|| format!("failed to write {}", path.display()))?;
        written.push(path);
    }

    let summary_path = out_dir.join(format!("{base}.summary.json"));
    fs::write(
        &summary_path,
        format!("{}\n", serde_json::to_string_pretty(&summary)?),
    )
    .with_context(|| format!("failed to write {}", summary_path.display()))?;
    written.push(summary_path);

    let output_bytes = written
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();

    Ok(RunOutcome::Extract(ExtractReport {
        format,
        input_bytes: fs::metadata(&args.input)?.len(),
        total_lines: raw_type_counter.total(),
        kept_events: kept.len(),
        output_bytes,
        written,
    }))
}

fn raw_type_key(object: &JsonObject, format: SessionFormat) -> String {
    let record_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "None".to_string());
    if format == SessionFormat::Codex {
        let payload_type = object
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str);
        if let Some(payload_type) = payload_type {
            return format!("{record_type}/{payload_type}");
        }
    }
    record_type
}

fn build_summary(
    args: &Cli,
    format: SessionFormat,
    raw_type_counter: &Counter,
    kind_counter_all: &Counter,
    kept_kind_counter: &Counter,
    models: &BTreeSet<String>,
    kept: &[Event],
) -> Result<Value> {
    let input_bytes = fs::metadata(&args.input)?.len();
    let goals = unique_goals(kept);
    let session_info = first_session_info(kept).unwrap_or(Value::Null);

    let mut object = Map::new();
    object.insert(
        "input".to_string(),
        Value::String(args.input.to_string_lossy().to_string()),
    );
    object.insert(
        "format".to_string(),
        Value::String(format.as_str().to_string()),
    );
    object.insert("input_bytes".to_string(), number_u64(input_bytes));
    object.insert(
        "total_lines".to_string(),
        Value::Number(Number::from(raw_type_counter.total() as u64)),
    );
    object.insert(
        "kept_events".to_string(),
        Value::Number(Number::from(kept.len() as u64)),
    );
    object.insert(
        "channel".to_string(),
        if format == SessionFormat::Codex {
            Value::String(args.channel.as_str().to_string())
        } else {
            Value::Null
        },
    );
    object.insert("session_info".to_string(), session_info);
    object.insert(
        "models".to_string(),
        Value::Array(models.iter().cloned().map(Value::String).collect()),
    );
    object.insert(
        "goals".to_string(),
        Value::Array(goals.into_iter().map(Value::String).collect()),
    );
    object.insert(
        "raw_type_counts".to_string(),
        raw_type_counter.to_json_object_by_count(),
    );
    object.insert(
        "kind_counts_all".to_string(),
        kind_counter_all.to_json_object_by_count(),
    );
    object.insert(
        "kept_kind_counts".to_string(),
        kept_kind_counter.to_json_object_by_count(),
    );

    Ok(Value::Object(object))
}

fn unique_goals(kept: &[Event]) -> Vec<String> {
    let mut seen = Vec::new();
    for event in kept {
        if kind_of(event) == Some("goal") {
            if let Some(objective) = event.get("objective").and_then(Value::as_str) {
                if !seen.iter().any(|item| item == objective) {
                    seen.push(objective.to_string());
                }
            }
        }
    }
    seen
}

fn first_session_info(kept: &[Event]) -> Option<Value> {
    for event in kept {
        if kind_of(event) == Some("session") {
            let mut info = Map::new();
            for key in ["id", "forked_from_id", "cwd", "originator", "cli_version"] {
                info.insert(
                    key.to_string(),
                    event.get(key).cloned().unwrap_or(Value::Null),
                );
            }
            return Some(Value::Object(info));
        }
    }
    None
}

#[allow(dead_code)]
fn compact_json_preview(value: &Value, chars: usize) -> String {
    json_text(value).chars().take(chars).collect()
}
