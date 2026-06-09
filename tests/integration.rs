use session_jsonl_compact::cli::{Channel, Cli, OutputFormat, SessionFormat};
use session_jsonl_compact::{run, RunOutcome};
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn base_cli(input: PathBuf, out_dir: PathBuf, name: &str) -> Cli {
    Cli {
        input,
        out_dir: Some(out_dir),
        name: Some(name.to_string()),
        format: SessionFormat::Auto,
        msg_chars: 0,
        out_chars: 0,
        channel: Channel::Terminal,
        keep_token_count: false,
        no_dedup: false,
        elide_outputs: false,
        format_out: OutputFormat::Both,
        stats: false,
    }
}

#[test]
fn extracts_codex_sample() {
    let tmp = tempfile::tempdir().unwrap();
    let outcome = run(base_cli(
        fixture("codex_sample.jsonl"),
        tmp.path().to_path_buf(),
        "codex",
    ))
    .unwrap();
    let RunOutcome::Extract(report) = outcome else {
        panic!("expected extract outcome");
    };
    assert_eq!(report.format, SessionFormat::Codex);
    assert_eq!(report.kept_events, 5);

    let clean = fs::read_to_string(tmp.path().join("codex.clean.jsonl")).unwrap();
    assert!(clean.contains("\"kind\":\"user\""));
    assert!(clean.contains("\"kind\":\"assistant\""));
    assert!(clean.contains("\"kind\":\"tool_call\""));
    assert!(!clean.contains("token_count"));
}

#[test]
fn extracts_claude_sample() {
    let tmp = tempfile::tempdir().unwrap();
    let outcome = run(base_cli(
        fixture("claude_sample.jsonl"),
        tmp.path().to_path_buf(),
        "claude",
    ))
    .unwrap();
    let RunOutcome::Extract(report) = outcome else {
        panic!("expected extract outcome");
    };
    assert_eq!(report.format, SessionFormat::ClaudeCode);
    assert_eq!(report.kept_events, 5);

    let summary = fs::read_to_string(tmp.path().join("claude.summary.json")).unwrap();
    assert!(summary.contains("claude_code"));
    assert!(summary.contains("claude-sonnet"));

    let clean = fs::read_to_string(tmp.path().join("claude.clean.jsonl")).unwrap();
    assert!(!clean.contains("cc_meta"));
}

#[test]
fn stats_does_not_write_outputs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut args = base_cli(
        fixture("codex_sample.jsonl"),
        tmp.path().to_path_buf(),
        "stats",
    );
    args.stats = true;
    let outcome = run(args).unwrap();
    assert!(matches!(outcome, RunOutcome::Stats(_)));
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}
