# Changelog

All notable changes to `agent-jsonl-compact` are recorded here.

## 0.3.0 — 2026-10-21

Source release date: **2026-10-21** (fixed release date; not a build timestamp).

### Added

- `sessions` — find conversation candidates under declared provider roots
  (`--root`, `--provider`, `--cwd`, `--cwd-children`, `--scan-limit`).
- `view` — read a session or a file with bounded output, filters
  (`--scope`, `--role`, `--phase`, `--since`, `--until`), `--tail`, `--event`,
  `--with-pair`, and page/append cursors (`--cursor`, `--after-cursor`).
- `search` — literal (OR) or opt-in `--regex` search with `--context`, returning
  `matched_queries` and `matched_ranges` against the original text.
- `version --output json` — machine-readable version, fixed `source_release_date`,
  and query schema version.
- Viewer output carries provenance: raw line, byte range, subevent index, provider
  session id, message origin, tool pair state and coverage counters.

### Compatibility

- Legacy extraction (`-i/-o`, `--format-out`, `--channel`, `--name`, `--stats`,
  `--no-dedup`, `--elide-outputs`, `--msg-chars`, `--out-chars`) keeps its existing
  schema, defaults and exit codes.
- New viewer commands use their own exit contract: `0` processed (including a
  legitimate empty result), `1` source/decode/format-uncertain/partial/cursor error,
  `2` argument or query syntax error.

### Notes

- Query commands never write to the source and create no cache or index by default.
- Memory is bounded by the requested window, the per-record cap and the output
  budget, not by the number of events in the input.
- Cursor mode requires Unix file-generation identity (`dev`+`inode`); other platforms
  reject it with `cursor_unsupported_platform`.
