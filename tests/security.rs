//! Regression checks for filesystem boundaries. All data and homes are synthetic.
use agent_jsonl_compact::{cli::Cli, install_skills_into, run};
use clap::Parser;
use std::{fs, path::Path, process::Command};

fn extract(
    input: &Path,
    output: &Path,
    name: &str,
) -> anyhow::Result<agent_jsonl_compact::RunOutcome> {
    run(Cli::parse_from([
        "agent-jsonl-compact",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--name",
        name,
        "--format-out",
        "both",
        "--summary",
    ]))
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_sample.jsonl")
}

#[test]
fn output_name_cannot_escape_directory() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out");
    for name in [
        "../escape",
        "/absolute",
        "..\\escape",
        "nested/file",
        "",
        ".",
        "..",
        "bad\nname",
    ] {
        assert!(
            extract(&fixture(), &output, name).is_err(),
            "accepted {name:?}"
        );
    }
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert!(!temp.path().join("escape.clean.jsonl").exists());
}

#[test]
fn input_cannot_be_replaced_by_output() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("session.clean.jsonl");
    fs::copy(fixture(), &input).unwrap();
    let before = fs::read(&input).unwrap();
    assert!(extract(&input, temp.path(), "session").is_err());
    assert_eq!(fs::read(&input).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn all_outputs_are_private_even_when_replacing_public_files() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    for extension in ["clean.jsonl", "transcript.md", "summary.json"] {
        let path = temp.path().join(format!("session.{extension}"));
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    extract(&fixture(), temp.path(), "session").unwrap();
    for entry in fs::read_dir(temp.path()).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn output_links_cannot_modify_other_files_or_the_input() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("victim");
    fs::write(&victim, "keep me").unwrap();
    // Validate every destination before writing even the first output.
    let linked = temp.path().join("session.summary.json");
    symlink(&victim, &linked).unwrap();
    assert!(extract(&fixture(), temp.path(), "session").is_err());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
    assert!(!temp.path().join("session.clean.jsonl").exists());
    fs::remove_file(&linked).unwrap();
    fs::hard_link(&victim, temp.path().join("session.clean.jsonl")).unwrap();
    extract(&fixture(), temp.path(), "session").unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");

    let input = temp.path().join("source.jsonl");
    fs::copy(fixture(), &input).unwrap();
    fs::hard_link(&input, temp.path().join("alias.clean.jsonl")).unwrap();
    assert!(extract(&input, temp.path(), "alias").is_err());
    assert_eq!(fs::read(&input).unwrap(), fs::read(fixture()).unwrap());
}

#[test]
fn skill_edits_require_explicit_replacement() {
    let temp = tempfile::tempdir().unwrap();
    install_skills_into(temp.path(), true, false).unwrap();
    let path = temp
        .path()
        .join(".claude/skills/agent-jsonl-compact-reader/SKILL.md");
    fs::write(&path, "local edits").unwrap();
    let report = install_skills_into(temp.path(), true, false).unwrap();
    assert!(report.written.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "local edits");
    let result = Command::new(env!("CARGO_BIN_EXE_agent-jsonl-compact"))
        .env("HOME", temp.path())
        .args(["install-skills", "--claude-only", "--force"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("name: agent-jsonl-compact-reader"));
}

#[test]
fn terminal_statistics_and_errors_escape_log_controls() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("control.jsonl");
    fs::write(&input, "{\"type\":\"\\u001b]52;c;payload\\u0007\"}\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_agent-jsonl-compact"))
        .args([
            "-i",
            input.to_str().unwrap(),
            "--format",
            "codex",
            "--stats",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("\\x1b"));
    assert!(!stdout.contains(['\u{1b}', '\u{07}']));
    let missing = temp.path().join("missing\u{1b}[31m.jsonl");
    let result = Command::new(env!("CARGO_BIN_EXE_agent-jsonl-compact"))
        .args(["-i", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains("\\x1b"));
    assert!(!stderr.contains('\u{1b}'));
}

#[cfg(unix)]
#[test]
fn skill_install_rejects_symlink_file_and_directory_even_with_force() {
    use std::os::unix::fs::symlink;
    for link_directory in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let skills = temp.path().join(".claude/skills");
        fs::create_dir_all(&skills).unwrap();
        let victim = temp.path().join("victim");
        fs::create_dir(&victim).unwrap();
        fs::write(victim.join("SKILL.md"), "keep me").unwrap();
        let target = skills.join("agent-jsonl-compact-reader");
        if link_directory {
            symlink(&victim, &target).unwrap();
        } else {
            fs::create_dir(&target).unwrap();
            symlink(victim.join("SKILL.md"), target.join("SKILL.md")).unwrap();
        }
        let result = Command::new(env!("CARGO_BIN_EXE_agent-jsonl-compact"))
            .env("HOME", temp.path())
            .args(["install-skills", "--claude-only", "--force"])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(
            fs::read_to_string(victim.join("SKILL.md")).unwrap(),
            "keep me"
        );
    }
}
