# CLI reference

[日本語](CLI.md) | English

## Options (extraction)

```text
--format auto|codex|claude_code|opencode
--channel terminal|api|both      # Codex only
--msg-chars N                    # 0 keeps full text
--out-chars N                    # 0 keeps full output
--elide-outputs                  # fold large tool outputs into counts and first lines
--keep-token-count               # keep Codex token_count
--no-dedup                       # disable duplicate folding
--format-out jsonl|md|both       # default jsonl (clean only)
--summary                        # also write summary JSON
--stats                          # print format and record type distribution, then exit
--name <stem>                    # filename stem, no path separators
```

## Viewer CLI

Unlike extraction, which writes files, `sessions` / `view` / `search` are viewer commands
that return to stdout without saving. They never modify the source and create no cache
or index by default. See [ADR 0001](adr/0001-additive-session-viewer-cli.md) /
[ADR 0002](adr/0002-cursor-and-resource-boundaries.md) for the design decisions.

Basic commands

```bash
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo
agent-jsonl-compact view --session codex:SESSION_ID --tail 30 --output jsonl
agent-jsonl-compact search -i /path/rollout.jsonl --query keyword --context 2
```

### Version and update date

```bash
agent-jsonl-compact --version            # semver only (unchanged)
agent-jsonl-compact version --output json
```

`version --output json` returns `version`, the fixed `source_release_date`,
`query_schema_version`, and `cursor_version`. The update date is the fixed source
date, not the build time or the GitHub Release update time
([ADR 0003](adr/0003-version-and-release-metadata.md)).

### 1. Find candidates — `sessions`

```bash
# exact cwd match (default). Add --cwd-children to include child directories.
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# include Claude Code / declare an explicit search root
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- Only provider-specific known roots and roots explicitly passed with `--root` are
  searched. OpenCode run JSONL is not searched under the default roots (the CLI does
  not guess `opencode.db`); pass `--root` explicitly to use it.
- `--provider all` targets codex / claude_code / opencode.
- If `--scan-limit` (default 10000) is reached, the result reports `status: partial` and
  the unscanned remainder.
- Output reports the start time (`started_at`) and the last event time
  (`last_event_at`) separately. mtime is not used to decide whether a session is running
  or stopped.

### 2. Read the needed part — `view`

```bash
# last 30 events (tail after filtering, in source order)
agent-jsonl-compact view --session codex:SESSION_ID --tail 30

# file input + time range + role
agent-jsonl-compact view -i /path/rollout.jsonl \
  --since 2026-10-21T00:00:00+09:00 --role user,assistant --output jsonl

# re-read one event (display limits and text_complete are preserved)
agent-jsonl-compact view -i /path/rollout.jsonl --event EVENT_ID --output jsonl
```

- The default scope is `messages`. `--scope tools` selects tools; even `--scope all`
  never exposes private reasoning, system, developer, or compaction content.
- The default display count is 50 events; `--max-events` (primary + context + pair total)
  defaults to 200.
- `--with-pair` adds the tool result matching the selected tool call by `call_id`. States
  are `paired` / `pending_result` (confirmed absent after a complete scan) /
  `unknown_pair` (scan incomplete, absence not provable) / `orphan` (result without a
  call) / `not_rendered_budget` (exists but not displayed due to budget).
- Pair counts are split into `paired_total` (true total) and `paired_event_ids` (retained
  IDs). On truncation, `paired_ids_truncated: true` and `pair_reentry` give the re-entry
  position. Do not read an array length as the counterpart total.
  `scan.with_pair.incomplete` means the pair scan itself was incomplete.
- `text_complete: false` means "the display was cut"; it does not mean the retrieved
  range is incomplete.

### 3. Search the body — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# regex only when explicitly requested
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- Repeating `--query` is OR. Each hit carries `matched_queries`.
- `matched_ranges` are UTF-8 byte offsets into the original text. They refer to the
  source even when the displayed text is cut.
- `--event EVENT_ID` only narrows the target events; it does not guarantee full text
  under the same budget. If you need full text, raise `--max-output-bytes` (and
  `--max-record-bytes` if needed), or read the raw range via `source.byte_start` /
  `byte_end`.
- `--context K` adds K events before and after within the hard scope. Neighboring events
  that cross role/time soft filters are marked `is_context: true`, but reasoning /
  developer content is never included.
- If an event is both a hit and context for another hit, it is emitted once as
  primary.

### 4. Evidence and source positions

Each event line carries:

| field | meaning |
|---|---|
| `event_id` | stable ID built from source identity + raw start position + subevent |
| `source.raw_line` | raw line number (1-based) |
| `source.byte_start` / `byte_end` | raw byte range (0-based, end exclusive) |
| `source.subevent_index` | index when one raw line expands to multiple events |
| `text_complete` | whether the displayed text is complete |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`. A user role is not necessarily human |

### 5. Read only appends — page cursor and append cursor

```bash
# continue the same fixed snapshot (page_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# only appends after a completed scan (append_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| purpose | continue the same snapshot | appends after the previous complete line end |
| issued when | primary events remain unread | every line was scanned with no uninspected gap |
| caution | append cursor is not issued mid-page | never advances past an uninspected record |

- A cursor preserves the filter / query / scope at issue time, plus search `--context`,
  and reuses them on resume. If omitted, the cursor value is used; if explicitly
  specified with a different value, the result is `filter_mismatch`.
  An explicitly different filter yields `cursor_invalid` (rc1, reason
  `filter_mismatch`); specifying the same value is not a conflict.
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` are display conditions and
  are not part of cursor identity. Appends can still be read after a tail with the same
  filter.
- Replacement (`generation_mismatch`), truncation (`truncated`), and boundary rewrite
  (`anchor_mismatch`) stop with typed errors (`cursor_invalid`, rc1). There is no implicit
  re-read from the beginning.
- If an observable change happens during reading, the command returns `status: partial`
  and `source_changed: true` with rc1, and does not issue an append cursor (no silent
  completion).
- Cursors assume append-only input. They do not guarantee full immutability of a prefix
  outside the visible range.
- Cursor mode uses Unix file generation (`dev`+`inode`). Non-Unix platforms are rejected
  explicitly with `cursor_unsupported_platform`.

### 6. Exit codes and output separation

| rc | meaning |
|---|---|
| `0` | target range processed normally (including a legitimate empty result) |
| `1` | source / decode / undetermined format / partial / invalid cursor / change during read |
| `2` | argument or query syntax error (regex compile failure, too-small budget, misuse of legacy flags, etc.) |

- JSONL output is "event lines + exactly one metadata line at the end". Diagnostics and
  progress go to stderr; stdout never contains prose.
- Zero results are confirmed only when every target was scanned. If an unscanned range
  remains, the result is `partial`.
- Text output escapes control characters such as ESC / OSC / C1. Commands inside tool
  output are never executed.

### 7. Budgets and limits

- `--max-output-bytes` (default 32768, minimum 8192) is the combined stdout + stderr
  budget, including metadata, titles, and diagnostics. If it does not fit, the command
  rejects with `budget_too_small` (rc2) and never emits a truncated invalid JSON.
- Lines larger than `--max-record-bytes` (default 1 MiB) are returned as `uninspected`
  with only their raw range, `status: partial` / rc1, and no `append_cursor`. Use
  `counts.uninspected.samples[].retry_cursor` in metadata to re-read from the same
  position with a larger limit.
- Input record limits and display limits are separate. A display cut is
  `text_complete: false`, not partial.

### 8. Reading the read-byte counters

Metadata `scan` contains:

| field | meaning |
|---|---|
| `scan.read_bytes` | source bytes logically consumed by the main scan |
| `scan.total_read_bytes` | main + auxiliary scans (format detection, metadata search, anchor validation, pairing) |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | breakdown of the above |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair auxiliary scan passes and bytes |

These are logical consumed bytes, not OS physical I/O or speed. Auxiliary scans can
make `total_read_bytes` exceed the file size. With `-i` plus a cursor, the command reuses
provider/session from the cursor and reads only the anchor neighborhood and the append,
but `--with-pair` scans the same snapshot up to two extra times, so that bound does not
apply . `--session` also performs a
catalog search before using the cursor, so each call may pay that search cost; it does not
guarantee O(suffix) for appends.


Extraction and viewer flags are separate. `view --format-out md` is an argument error.
