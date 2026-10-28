# CLI reference

[日本語](CLI.md) | English

## Extraction options

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

`sessions` / `view` / `search` print results to standard output (stdout) without saving files.
They leave input files unchanged and create no cache or index by default. See [ADR 0001](adr/0001-additive-session-viewer-cli.md) /
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

`version --output json` returns `name`, `version`, the fixed `source_release_date`, `source_commit`, `source_commit_dirty`,
`query_schema_version`, `cursor_version`, and `source_date_kind`. The fixed release date is defined in the source;
`source_date_kind` indicates that it is not the build time. `source_commit` is the Git commit captured at build time (40 or 64
hexadecimal digits), or `unknown` when no Git metadata was available. `source_commit_dirty` reports uncommitted changes
present at build time, and is `null` (unknown) when Git was unavailable; unknown is never reported as clean. The build
script checks Git on every build so incremental builds reflect branch advances and worktree edits. The update date
differs from the build time and the GitHub Release update time ([ADR 0003](adr/0003-version-and-release-metadata.md)).

### 1. Find candidates — `sessions`

```bash
# exact cwd match (default). Add --cwd-children to include child directories.
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# include Claude Code / declare an explicit search root
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- The command searches each tool's default directories, or the directories specified with `--root`.
  To find OpenCode run JSONL, specify `--root`. OpenCode logs have no default search directory,
  and the command does not guess the location of `opencode.db`.
- `--provider all` targets codex / claude_code / opencode.
- An absent default root is intentionally skipped. A missing `--root` or a root that cannot be
  read (for example because of permissions) is `status: partial` (`scan_io`).
- Symlink entries inside a root are not followed. Each skipped symlink is counted in
  `symlinks_skipped` and makes the result `status: partial` (`scan_symlink`) because the target
  was not inspected. A root supplied as a symlink to a directory is followed.
- A file above 64 MiB is skipped as `scan_file_bytes`; a scan above 256 MiB stops as `scan_bytes`.
  Both are `status: partial`.
- If `--scan-limit` (default 10000) is reached, the result reports `status: partial` and
  the unscanned remainder.
- `--session` matches only the requested session id, so an unrelated duplicate does not block a
  unique target. The `sessions` listing keeps `session_ambiguous` when a complete scan finds any
  duplicate id.
- Output reports the start time (`started_at`) and the last event time
  (`last_event_at`) separately. The command does not use file modification time (mtime)
  to determine whether a session is running or stopped.

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
- `view` displays 50 events by default; `search` returns 20 hits by default.
  `--max-events` defaults to 200 (maximum 10,000) and counts selected events (primary),
  surrounding events (context), and matching tool calls or results (pair).
- `--with-pair` adds results whose `call_id` matches the selected tool call.
  It reports one of six pairing states:

  | State | Meaning |
  |---|---|
  | `paired` | A call and result match |
  | `pending_result` | A complete scan confirmed that the result is absent |
  | `unknown_pair` | The scan is incomplete, so the result's absence is unconfirmed |
  | `orphan` | A result has no matching call |
  | `not_rendered_budget` | A matching event exists, but the output limit prevented its display |
  | `ambiguous` | The same call_id appears more than once on the relevant side; `pair_reason` explains it |

- `paired_total` is the total number of matching events; `paired_event_ids` lists the retained IDs.
  If the ID list is truncated, `paired_ids_truncated: true` marks the truncation and `pair_reentry`
  gives the position for another read. The array length may differ from the total.
  `scan.with_pair.incomplete` means the scan for matching events was incomplete.
- `text_complete: false` means the displayed text was shortened. It does not mean the scan was incomplete.

### 3. Search the body — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# regex only when explicitly requested
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- Repeated `--query` options match any of the queries (OR). Each hit lists its matching queries in `matched_queries`.
- `matched_ranges` are UTF-8 byte offsets into the original text. They refer to the
  source even when the displayed text is cut.
- `--event EVENT_ID` narrows the target events without changing output limits.
  If you need full text, raise `--max-output-bytes` (and
  `--max-record-bytes` if needed), or read the raw range via `source.byte_start` /
  `byte_end`.
- `--context K` adds K events before and after within the allowed display scope (hard scope).
  Neighboring events may fall outside the role or time filters (soft filters) and are marked
  `is_context: true`. K is capped at 10,000. Context beyond the retention cap is omitted and
  counted in `counts.context_dropped`.
- `--phase` filters on comma-separated phase names recorded in the log; `--until` excludes events
  at or after the given time. `--since` / `--until` require RFC3339 with an offset.
- `search --with-pair` also adds the counterpart of a matching tool call or result.
- If an event is both a hit and context for another hit, it is emitted once as primary.
- At most 256 match ranges are retained per event. Above that, the event reports
  `matched_ranges_truncated: true` and `matched_ranges_total`. Without truncation the total is
  exact; when truncated it is a lower bound. Regex enumeration stops after 257 matches per query
  (the smallest observation proving that 256 was exceeded). `matched_queries` is not dropped.

### 4. Locate events in the source log

Each JSONL event line includes fields for locating the original log entry:

| field | meaning |
|---|---|
| `event_id` | stable ID built from file identity, the original start position, and the event's position within its source line |
| `source.raw_line` | raw line number (1-based) |
| `source.byte_start` / `byte_end` | raw byte range (0-based, end exclusive) |
| `source.subevent_index` | index when one raw line expands to multiple events |
| `text_complete` | whether the displayed text is complete |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`. A user role is not necessarily human |

### 5. Resume reads and read appends with cursors

A cursor records the previous read position and query conditions. Use a page cursor to continue
reading an existing range, or an append cursor to read additions after a completed scan.
A snapshot is the file range fixed for a read.

```bash
# continue the same fixed snapshot (page_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# only appends after a completed scan (append_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| purpose | continue the same snapshot | appends after the previous complete line end |
| issued when | primary events remain undisplayed | every line was scanned with no uninspected records |
| caution | append cursor is not issued mid-page | never advances past an uninspected record |

- A cursor preserves the filters, query, scope, and search `--context` from the original request.
  On resume, omitted options use the cursor's values. You may also specify the same values.
  A different value produces `cursor_invalid` (exit code 1, reason `filter_mismatch`).
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` are display conditions and
  are not part of cursor identity. Appends can still be read after a tail with the same
  filter.
- If the command detects file replacement (`generation_mismatch`), truncation (`truncated`),
  or a rewrite at the validation position (`anchor_mismatch`), it stops with `cursor_invalid`
  and exit code 1. It does not restart from the beginning.
- If the command detects a change during reading, it returns `status: partial` and
  `source_changed: true` with exit code 1. It does not issue an append cursor.
- Cursors assume that files change only by appending. They cannot guarantee that previously
  read content outside the validation range is unchanged.
- Cursor reads identify files by Unix `dev` and `inode` values.
  On other platforms, they fail with `cursor_unsupported_platform`.

### 6. Exit codes and output separation

| Exit code | Meaning |
|---|---|
| `0` | target range processed normally (including a legitimate empty result) |
| `1` | Input read or decode failure, unknown format, incomplete scan, invalid cursor, or a change detected during reading |
| `2` | Argument or query syntax error, such as regex compilation failure, an output limit that is too small, or misuse of legacy flags |

- JSONL output contains event lines followed by one metadata line. Diagnostics and progress
  go to standard error (stderr); stdout contains no explanatory prose.
- Zero results are confirmed only when every target was scanned. If an unscanned range
  remains, the result is `partial`.
- Text output escapes control characters such as ESC / OSC / C1. Commands inside tool
  output are never executed.

### 7. Input line and output limits

- `--max-output-bytes` (default 32768, minimum 8192) is the combined stdout + stderr
  limit, including metadata, titles, and diagnostics. If the output cannot fit, the command
  stops with `budget_too_small` (exit code 2) instead of emitting incomplete JSON.
- Lines larger than `--max-record-bytes` (default 1 MiB) are returned as `uninspected`
  with only their source range, `status: partial`, exit code 1, and no `append_cursor`. Use
  `counts.uninspected.samples[].retry_cursor` in metadata to re-read from the same
  position with a larger limit.
- Input line and display limits are separate. Shortened display text sets
  `text_complete: false` without making the result `partial`.

### 8. Read-byte counters

Metadata `scan` contains:

| field | meaning |
|---|---|
| `scan.read_bytes` | source bytes logically consumed by the main scan |
| `scan.total_read_bytes` | main + auxiliary scans (format detection, metadata search, anchor validation, pairing) |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | breakdown of the above |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair auxiliary scan passes and bytes |

These counters measure bytes logically read by the CLI, not physical OS I/O or processing speed.
Auxiliary scans can make `total_read_bytes` exceed the file size.

With `-i` and a cursor, the command reuses the log format and session information from the cursor.
It reads only the validation area (anchor) in the previous snapshot and the appended content.
Adding `--with-pair` requires up to two extra scans of the same snapshot to find matching events.

`--session` searches the session catalog before using the cursor. Each call may require this search,
so processing is not guaranteed to be proportional to the appended content alone (O(suffix)).

Extraction and viewer flags are separate. `view --format-out md` is an argument error.

### 9. Output fields and failure types

#### Event records

| Field | Meaning |
|---|---|
| `event_id` | `source_id:byte_start:subevent_index`; the raw line is not part of the identity |
| `source.raw_line` | Display line number; inherited when resuming from a cursor |
| `source.raw_line_verified` | Whether this scan verified the line number; `false` after cursor inheritance |
| `timestamp` / `epoch_ms` | Original timestamp and parsed epoch milliseconds when possible |
| `matched_ranges` | UTF-8 byte ranges in the original text, capped at 256 |
| `matched_ranges_total` / `matched_ranges_truncated` | Observed match count (a lower bound when truncated; regex stops at 257 per query) and whether ranges were omitted |
| `pair_state` / `pair_reason` / `paired_event_ids` | Tool-pair state, reason, and retained counterpart IDs |
| `text_complete` | Whether displayed text was shortened (separate from an incomplete scan) |

#### Metadata record

`status` is `ok`, `partial`, or `error`. If a search exceeds its 30-second processing budget, `scan.search_timed_out` is `true` and the status is `partial`. `partial` means some range could not be confirmed:
a scan limit, uninspected record, invalid record, unreadable file, or a source change during the
read. Inspect `counts.invalid`, `counts.uninspected`, `counts.context_dropped`, and the `scan`
values.

For `sessions`, `scan` contains `files_scanned`, `scan_limit`, `scan_limit_reached`,
`entries_visited`, `scan_bytes`, `walk_errors`, `symlinks_skipped`, and `limit_reason`.
`limit_reason` is one of `scan_limit`, `scan_entries`, `scan_depth`, `scan_bytes`,
`scan_file_bytes`, `scan_io`, `scan_symlink`, or `scan_time`. A missing default root is
intentionally skipped; a missing explicit root, or a root/directory/entry that cannot be
inspected (for example because of permissions), is counted in `walk_errors` and makes `status`
`partial`. A symlink skipped without being followed is counted in `symlinks_skipped` and makes
`status` `partial` as `scan_symlink`.

#### Main errors

| Type | Meaning |
|---|---|
| `session_not_found` | No candidate exists after a complete scan of the declared roots |
| `session_ambiguous` | A complete scan found multiple files for the same session id |
| `session_resolution_incomplete` | A scan limit, unreadable file, uninspected record, or invalid record prevents a unique/absent conclusion |
| `cursor_invalid` | Cursor shape, generation, position, filters, or session does not match |
| `budget_too_small` | The output budget cannot produce a page with progress (exit code 2) |
| `cursor_too_large` | The query/filter combination would emit a cursor larger than 64 KiB (exit code 2) |
| `source_unreadable` | The input cannot be read or is not a regular file |

Discovery has bounds on entries, depth, and bytes. A truncated scan returns `partial` (exit code 1),
never a successful claim that the unscanned range was complete.
