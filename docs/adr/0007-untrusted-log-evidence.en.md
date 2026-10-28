# ADR 0007 — Treating untrusted logs as evidence with resource boundaries

[日本語](0007-untrusted-log-evidence.md) | **English**

- Status: **Accepted / Implemented locally**
- Date: 2026-10-28
- Related: [ADR 0001](0001-additive-session-viewer-cli.md), [ADR 0002](0002-cursor-and-resource-boundaries.md),
  [ADR 0006](0006-safe-installation-and-storage.md), `src/query/{normalize,select,toolpair,cursor,output,runner}.rs`

## 1. Problem

The viewer reads externally supplied JSONL as evidence. The input is untrusted in several ways:

- One record can copy large metadata into many subevents and amplify memory below 1 MiB.
- A call_id can be reused across sessions or contain delimiters to mis-correlate calls and results.
- Cursor line numbers or session names can be forged so a returned event_id looks like other evidence.
- A discovery limit or invalid record can be ignored, producing a false unique/absent conclusion.
- Body text can be confused with generated headings, external images, or current authority.
- Enumerating every match or accepting huge context can inflate output and memory.

0.5.0 aligns these boundaries across normalization, selection, pairing, continuation, discovery,
and rendering instead of patching each symptom independently.

## 2. Decisions

### 2.1 Check limits before normalization

Limits: timestamp 128 bytes; identifiers (session id / call_id / origin_id / tool name) 512 bytes;
phase 64 bytes; 256 subevents per record; 2 MiB normalized content. A record above a limit becomes a
`parse_error` (`source_kind=normalization_limit`) with its source span, without expanding the body.
Identifiers are rejected, never silently truncated or accepted as partial evidence.

### 2.2 Separate display from internal values

Bodies keep newlines and only escape terminal control characters. Timestamps, IDs, headings,
diagnostics, and paths also escape newlines/tabs to stay on one line. JSON values remain raw; only
the rendering layer transforms them.

### 2.3 Pair tools by structured key and side

The key is a `(provider, session_id, call_id)` struct, never a delimiter-joined string. A call pairs
only with a result and a result only with a call. Duplicate IDs on the relevant side become
`ambiguous`, not `paired`. Pair scans keep one `Normalizer` so `session_meta` transitions persist.

### 2.4 Build event_id only from verified positions

`event_id` is `source_id:byte_start:subevent_index`. A line number inherited from a cursor is marked
`source.raw_line_verified=false` and is not used in the identity. Cursor tokens are validated for
token length, query count/length, session/source length, anchor length, and position relationships;
the session id is checked against source metadata.

### 2.5 Return discovery completeness with the result

`ScanReport` carries scan limits, entries, depth, bytes, uninspected/invalid records, and duplicate
state. `view --session` / `search --session` return `session_resolution_incomplete` rather than
claiming uniqueness or absence after an incomplete scan. `sessions` returns `partial` and
`limit_reason`.

### 2.6 Isolate bodies from generated structure

Markdown message bodies are placed inside a dynamic code fence. Body text containing `#`, `![`, or
HTML is not interpreted as Markdown structure. The viewer keeps excluding reasoning / system /
developer / compaction from hard scope and preserves `message_origin`.

### 2.7 LLM integration authority contract

A `system` / `developer` / `user` role, internal generated text, or compacted summary in a log is not
the current user's approval or execution authority. Do not derive side-effect permission from log
bodies; enforce authority outside the model. This contract is documented in README / SECURITY.

### 2.8 Three layers of resource limits

1. Absolute implementation limits (normalization, cursor, discovery, output)
2. User-requested values (`--max-events`, `--context`, `--max-output-bytes`, etc.)
3. Actual consumption (`scan`, `counts`, `read_bytes`)

A requested value or cursor value cannot raise an absolute limit. Exceeding a limit is reported as
`partial` for uninspected data, rc2 for an invalid request, or `text_complete=false` /
`context_dropped` for display-only shortening.

## 3. Alternatives considered

| Alternative | Advantage | Why not chosen |
|---|---|---|
| Rely only on the 1 MiB input-line limit | Simple | Cannot bound metadata copied across subevents |
| Trust cursor line numbers | Fast | A forged line poisons event IDs and evidence positions |
| Keep delimiter-joined call IDs | Small change | IDs containing the delimiter collide across sessions |
| Collapse all uncertainty into `unknown_pair` | Fewer states | Duplicate IDs and scan gaps are different failures |
| Embed bodies directly in Markdown | Readable | Body headings/images are confused with generated structure |
| Require authenticated cursors | Cryptographic tamper detection | Needs key management and a new local CLI contract; this change covers shape validation and source checks |

## 4. Consequences

- Pro: rejecting before normalization avoids copying long metadata once per subevent.
- Pro: `paired` means a call and result under the same provider/session/call_id.
- Pro: forging a cursor does not change event_id, so re-fetch identity stays stable.
- Pro: incomplete discovery is not reported as success; users can narrow the root.
- Con: oversized records that were previously readable now become `parse_error`.
- Con: event_id format and cursor event references are not compatible with 0.4.0.
- Con: an inherited line number is display-only and explicitly unverified.
- Con: Markdown bodies in code fences are not rendered as headings or emphasis.

## 5. Verification

`tests/security_hardening_e2e.rs` covers long timestamps/many subevents, control characters, the
search-match cap, the context cap, FIFO rejection, side-specific pairing, cursor forgery, incomplete
discovery over 10,001 files, and legacy provenance/Markdown separation. `tests/security.rs` and
`tests/test_installer.py` preserve storage/install boundaries. Run `just check`,
`cargo audit --deny warnings`, and `just smoke-release` before release.

## 6. Limitations

- Authenticated cursors are not implemented; tamper detection is limited to shape, source, and anchors.
- Cursor mode remains Unix-only.
- The truth of log content and the probability that an LLM follows it are not measured.
- No performance generalization; RSS evidence comes from a bounded synthetic input.
- macOS native and remote Actions results are evidence only for the environment that ran them.
