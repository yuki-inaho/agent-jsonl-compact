use agent_jsonl_compact::cli::{Channel, Cli, OutputFormat, SessionFormat};
use agent_jsonl_compact::{install_skills_into, run, RunOutcome};
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
        command: None,
        input: Some(input),
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
fn extracts_opencode_run_sample() {
    let tmp = tempfile::tempdir().unwrap();
    let outcome = run(base_cli(
        fixture("opencode_sample.jsonl"),
        tmp.path().to_path_buf(),
        "opencode",
    ))
    .unwrap();
    let RunOutcome::Extract(report) = outcome else {
        panic!("expected extract outcome");
    };
    assert_eq!(report.format, SessionFormat::OpenCode);
    assert_eq!(report.kept_events, 8);

    let summary = fs::read_to_string(tmp.path().join("opencode.summary.json")).unwrap();
    assert!(summary.contains("\"format\": \"opencode\""));
    assert!(summary.contains("opencode-session-1"));

    let clean = fs::read_to_string(tmp.path().join("opencode.clean.jsonl")).unwrap();
    assert!(clean.contains("\"originator\":\"opencode\""));
    assert!(clean.contains("\"kind\":\"assistant\""));
    assert!(clean.contains("\"kind\":\"reasoning\""));
    assert!(clean.contains("\"kind\":\"tool_call\""));
    assert!(clean.contains("\"kind\":\"tool_output\""));
    assert!(clean.contains("\"tokens\":{\"cache\""));
    assert!(clean.contains("\"kind\":\"error\""));

    let markdown = fs::read_to_string(tmp.path().join("opencode.transcript.md")).unwrap();
    assert!(markdown.contains("format=opencode"));
    assert!(markdown.contains("$ pwd"));
    assert!(markdown.contains("reason=stop"));
    assert!(markdown.contains("synthetic failure"));
}

#[test]
fn install_skills_writes_skill_into_both_agents() {
    let home = tempfile::tempdir().unwrap();
    // 両エージェントの home を用意(無いと skip されるため)。
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::create_dir_all(home.path().join(".codex")).unwrap();

    let report = install_skills_into(home.path(), false, false).unwrap();
    assert_eq!(report.written.len(), 2);

    for agent in [".claude", ".codex"] {
        let skill = home
            .path()
            .join(agent)
            .join("skills/agent-jsonl-compact-reader/SKILL.md");
        let body = fs::read_to_string(&skill).unwrap();
        assert!(body.contains("name: agent-jsonl-compact-reader"));
    }
}

#[test]
fn install_skills_skips_absent_agent_home_unless_explicit() {
    let home = tempfile::tempdir().unwrap();
    // .codex のみ用意。.claude は存在しないので skip される。
    fs::create_dir_all(home.path().join(".codex")).unwrap();

    let report = install_skills_into(home.path(), false, false).unwrap();
    assert_eq!(report.written.len(), 1);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.written[0].starts_with(home.path().join(".codex")));
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
