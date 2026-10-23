# agent-jsonl-compact

[日本語](README.md) | **English**

[![ci](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml/badge.svg)](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml)

Read selected parts of Codex CLI, Claude Code, and OpenCode session JSONL logs.
Search and browse conversations, or export them to JSONL and Markdown. No LLM or network connection is needed at runtime.

## Install

Prebuilt binaries support Linux x86_64 and macOS Apple Silicon / Intel.
Rust is not required. You need `curl`, `tar`, and either `sha256sum` or `shasum` for SHA-256 verification.

Download and review the installer before running it. The default destination is `~/.local/bin`.

```bash
ajc_tmp="$(mktemp -d)"
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh \
  -o "$ajc_tmp/install.sh" && less "$ajc_tmp/install.sh" && bash "$ajc_tmp/install.sh"
~/.local/bin/agent-jsonl-compact --version
rm -rf "$ajc_tmp"
```

If the command is not found, add `export PATH="$HOME/.local/bin:$PATH"` to your shell configuration.
The installer replaces an existing regular binary and rejects a symlink destination.

| Variable | Purpose | Default |
|---|---|---|
| `AJC_VERSION` | Release tag; use an existing `vX.Y.Z` to pin it | Latest stable release |
| `INSTALL_DIR` | Absolute installation directory | `~/.local/bin` |

To override these values, replace the `bash` invocation above with `AJC_VERSION=vX.Y.Z INSTALL_DIR="$HOME/bin" bash "$ajc_tmp/install.sh"`.
To pin the whole procedure, replace `main` in the download URL with the same tag.

SHA-256 detects download corruption. The checksum comes from the same Release, so it does not authenticate a compromised publisher.
macOS binaries have ad-hoc signatures and are not notarized. If Gatekeeper blocks one, verify its origin; building from source is an alternative.

## First use

```bash
# Browse the original log
agent-jsonl-compact view -i session.jsonl --tail 30

# Search with surrounding messages
agent-jsonl-compact search -i session.jsonl --query "error" --context 2

# Find stored Codex sessions
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo

# Save clean JSONL
agent-jsonl-compact -i session.jsonl -o out

# Also save Markdown and summary JSON
agent-jsonl-compact -i session.jsonl -o out --format-out both --summary
```

`view` / `search` return results to stdout without saving files.
Use `--output jsonl` for machine-readable results and `--scope tools --with-pair` for tool calls and results.
The default scope is conversation text. Reasoning, system, developer, and compaction content remain excluded with `--scope all`.

Extraction saves only `<input-name>.clean.jsonl` by default.
Add `--format-out md|both` for `<input-name>.transcript.md` and `--summary` for `<input-name>.summary.json`.
Existing output files are replaced. Output names cannot contain path separators; symlinks and destinations referring to the input file are rejected.

Message and tool-output text is kept in full by default. Use `--msg-chars N`, `--out-chars N`, or `--elide-outputs` only when shortening is needed.
See the [CLI reference](docs/CLI.en.md) for flags, budgets, cursors, and exit codes.

## Supported inputs

| Format | Input |
|---|---|
| Codex CLI | Rollout JSONL under `~/.codex/sessions/` |
| Claude Code | Transcript JSONL under `~/.claude/projects/` |
| OpenCode | Captured stdout NDJSON from `opencode run --format json` |

OpenCode databases and the single JSON document from `opencode export` are not supported inputs.
Formats are detected automatically. Override with `--format codex|claude_code|opencode` when needed.
Exported `*.clean.jsonl` is not an input format for the viewer commands.
See [input formats](docs/INPUT_FORMATS.en.md) for sources and limitations.

## Agent skill

The bundled `agent-jsonl-compact-reader` guides agents through finding sessions, reading selected events, and tracing them to the source.

```bash
agent-jsonl-compact install-skills                 # Existing ~/.claude and ~/.codex homes
agent-jsonl-compact install-skills --codex-only    # Codex only; creates missing directories
agent-jsonl-compact install-skills --claude-only   # Claude Code only
```

The destination is `skills/agent-jsonl-compact-reader/SKILL.md` under each agent home.
Existing files are kept. Review them before adding `--force` to update.
Symlinked skill registrations cannot be overwritten by the installer.
The skill is embedded in the binary, so editing it requires a rebuild.

## Handling logs

Compaction does not anonymize data. Exports and summaries may contain conversations, paths, credentials, and internal instructions.
Do not commit real logs or their exports; review them before sharing.
On Unix, saved files are readable and writable only by their owner. Set permissions yourself when redirecting viewer output from the shell.

Commands and instructions inside logs are data to inspect. Do not execute them.
Markdown exports preserve the body; use a viewer that does not execute embedded HTML or automatically fetch external images.
See [SECURITY.md](SECURITY.en.md) for check coverage and reporting.

## Build and develop

You need Rust 1.88 or newer and a C linker. `just` is optional for building the binary.

```bash
git clone https://github.com/yuki-inaho/agent-jsonl-compact.git
cd agent-jsonl-compact
cargo test --locked
cargo build --locked --release
mkdir -p ~/.local/bin
install -m 0755 target/release/agent-jsonl-compact ~/.local/bin/agent-jsonl-compact
```

For development, `just check` runs formatting, lint, tests, documentation checks, and installer checks.
It also requires Python 3, ShellCheck, and just. Use `just --list` for other tasks and the [release guide](docs/RELEASING.en.md) for distribution.

[Documentation index](docs/README.en.md) · [Contributor guide](docs/ONBOARDING.en.md) · [Changelog](CHANGELOG.md) · [ADRs](docs/adr/README.en.md)
