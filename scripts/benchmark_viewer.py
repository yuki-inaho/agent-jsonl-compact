#!/usr/bin/env python3
"""Reproducible synthetic benchmark for the agent-jsonl-compact viewer CLI.

Not a Harness^2 comparison and not an empirical fit to real corpora: one provider
(Claude Code) only, fixed seed, fixed body sizes. Measures four conditions on the
*release* binary and writes a JSON receipt plus a markdown summary.

Usage:
  python3 scripts/benchmark_viewer.py --help
  python3 scripts/benchmark_viewer.py generate --out temp/viewer_bench/input.jsonl
  python3 scripts/benchmark_viewer.py measure  --input temp/viewer_bench/input.jsonl \\
      --out-dir temp/viewer_cli_20261021/step7/bench

Stdlib only. No network, no provider DB, no real session logs. Read-byte counters are
logical bytes reported by the CLI, not OS physical I/O. OS cache cold/warm is
uncontrolled and labeled accordingly.
"""

import argparse
import hashlib
import json
import os
import pathlib
import random
import shutil
import subprocess
import sys
import tempfile
import time

SEED = 42
DEFAULT_RECORDS = 100_000
BODY = 256
BIG_BODY = 16_384
BIG_EVERY = 100  # 1%
SESSION = "bench-session"
NEEDLE = "RARE_LATE_HIT_9f3c"


def _text(rng, size, tag):
    filler = "".join(rng.choice("abcdefghijklmnopqrstuvwxyz0123456789 ") for _ in range(size))
    return f"{tag} {filler}"


def generate(out: pathlib.Path, records: int = DEFAULT_RECORDS) -> dict:
    rng = random.Random(SEED)
    out.parent.mkdir(parents=True, exist_ok=True)
    counts = {"text_single": 0, "tool_pair": 0, "text_triple": 0, "big": 0}
    physical = 0
    events = 0
    t0 = 1_790_000_000
    with out.open("w", encoding="utf-8") as fh:
        for i in range(records):
            ts = t0 + i
            uuid = f"u{i}"
            iso = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts))
            big = (i % BIG_EVERY == 0) and i > 0
            size = BIG_BODY if big else BODY
            roll = rng.random()
            if roll < 0.80:
                text = _text(rng, size, f"msg{i}")
                if big:
                    counts["big"] += 1
                rec = {
                    "type": "assistant",
                    "uuid": uuid,
                    "sessionId": SESSION,
                    "timestamp": iso,
                    "message": {"model": "synthetic", "content": [{"type": "text", "text": text}]},
                }
                counts["text_single"] += 1
                events += 1
            elif roll < 0.90:
                cid = f"tool{i}"
                rec = {
                    "type": "assistant",
                    "uuid": uuid,
                    "sessionId": SESSION,
                    "timestamp": iso,
                    "message": {
                        "model": "synthetic",
                        "content": [
                            {"type": "tool_use", "id": cid, "name": "Bash",
                             "input": {"command": _text(rng, size, f"cmd{i}")}}
                        ],
                    },
                }
                physical += 1
                fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
                result = {
                    "type": "user",
                    "uuid": f"r{i}",
                    "sessionId": SESSION,
                    "timestamp": iso,
                    "message": {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": cid,
                         "content": [{"type": "text", "text": _text(rng, size, f"out{i}")}]}
                    ]},
                }
                fh.write(json.dumps(result, ensure_ascii=False) + "\n")
                physical += 1
                events += 2
                counts["tool_pair"] += 1
                continue
            else:
                parts = [_text(rng, max(1, size // 3), f"tri{i}-{k}") for k in range(3)]
                rec = {
                    "type": "assistant",
                    "uuid": uuid,
                    "sessionId": SESSION,
                    "timestamp": iso,
                    "message": {"model": "synthetic",
                                "content": [{"type": "text", "text": p} for p in parts]},
                }
                counts["text_triple"] += 1
                events += 3
            physical += 1
            fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
        # One rare late hit at the very end: the full search must scan the whole input.
        late = {
            "type": "assistant",
            "uuid": "late",
            "sessionId": SESSION,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t0 + records + 1)),
            "message": {"model": "synthetic",
                        "content": [{"type": "text", "text": f"late answer {NEEDLE}"}]},
        }
        fh.write(json.dumps(late, ensure_ascii=False) + "\n")
        physical += 1
        events += 1

    data = out.read_bytes()
    return {
        "path": str(out),
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
        "physical_records": physical,
        "events_expected": events,
        "counts": counts,
        "seed": SEED,
        "records_requested": records,
        "body_bytes": BODY,
        "big_body_bytes": BIG_BODY,
        "provider": "claude_code",
    }


def run_measured(binary: pathlib.Path, args: list, label: str) -> dict:
    """Run one CLI invocation; capture rc, streams, wall time and peak RSS."""
    with tempfile.NamedTemporaryFile("w+", suffix=".time", delete=False) as tf:
        time_file = tf.name
    cmd = ["/usr/bin/time", "-v", "-o", time_file, str(binary)] + args
    started = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True)
    wall = time.perf_counter() - started
    timing = pathlib.Path(time_file).read_text(errors="replace")
    os.unlink(time_file)
    rss_kib = None
    for line in timing.splitlines():
        if "Maximum resident set size" in line:
            rss_kib = int(line.rsplit(":", 1)[1].strip())
    rows = []
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            raise SystemExit(f"{label}: non-JSON stdout line: {line[:200]}")
    meta = [r for r in rows if r.get("record_type") == "metadata"]
    if len(meta) != 1:
        raise SystemExit(f"{label}: expected one metadata record, got {len(meta)} (rc={proc.returncode})")
    meta = meta[0]
    events = [r for r in rows if r.get("record_type") == "event"]
    return {
        "label": label,
        "argv": args,
        "returncode": proc.returncode,
        "wall_seconds": round(wall, 4),
        "peak_rss_kib": rss_kib,
        "stdout_bytes": len(proc.stdout.encode("utf-8")),
        "stderr_bytes": len(proc.stderr.encode("utf-8")),
        "event_count": len(events),
        "status": meta.get("status"),
        "exhausted": meta.get("exhausted"),
        "selected": meta.get("selected"),
        "rendered": meta.get("rendered"),
        "read_bytes": meta.get("scan", {}).get("read_bytes"),
        "total_read_bytes": meta.get("scan", {}).get("total_read_bytes"),
        "read_bytes_breakdown": meta.get("scan", {}).get("read_bytes_breakdown"),
        "with_pair": meta.get("scan", {}).get("with_pair"),
        "pair_counts": meta.get("counts", {}).get("pairs"),
        "page_cursor": bool(meta.get("page_cursor")),
        "append_cursor": meta.get("append_cursor"),
        "stderr_head": proc.stderr[:400],
        "events": events,
        "metadata": meta,
    }


def measure(binary: pathlib.Path, input_path: pathlib.Path, out_dir: pathlib.Path) -> dict:
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest = generate_identity(input_path)
    env_note = {
        "binary": str(binary),
        "binary_bytes": binary.stat().st_size,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "platform": sys.platform,
        "python": sys.version.split()[0],
        "cache": "OS cache cold/warm uncontrolled; values are labels, not guarantees",
        "read_bytes_semantics": "logical bytes reported by the CLI; not OS physical I/O",
    }

    conditions = {}

    c1 = run_measured(binary, ["view", "-i", str(input_path), "--limit", "50", "--output", "jsonl"], "first_page_50")
    conditions["first_page_50"] = {k: v for k, v in c1.items() if k != "events"}

    c2 = run_measured(binary, ["search", "-i", str(input_path), "--query", NEEDLE, "--output", "jsonl"], "full_search_late_hit")
    c2_hits = [e for e in c2["events"] if e.get("matched_queries")]
    conditions["full_search_late_hit"] = {
        **{k: v for k, v in c2.items() if k != "events"},
        "exact_hits": len(c2_hits),
        "hit_text_has_needle": any(NEEDLE in (e.get("text") or "") for e in c2_hits),
    }

    # Append: bootstrap an append cursor from a full scan, then append known rows.
    work = out_dir / "append-work.jsonl"
    shutil.copyfile(input_path, work)
    base = run_measured(binary, ["view", "-i", str(work), "--tail", "1", "--output", "jsonl"], "append_bootstrap")
    append_cursor = base["append_cursor"]
    if not append_cursor:
        raise SystemExit("append bootstrap produced no append_cursor")
    appended = 3
    with work.open("a", encoding="utf-8") as fh:
        for k in range(appended):
            fh.write(json.dumps({
                "type": "assistant",
                "uuid": f"app{k}",
                "sessionId": SESSION,
                "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(1_790_000_000 + 200_000 + k)),
                "message": {"model": "synthetic", "content": [{"type": "text", "text": f"appended {k}"}]},
            }, ensure_ascii=False) + "\n")
    c3 = run_measured(
        binary,
        ["view", "-i", str(work), "--after-cursor", append_cursor, "--output", "jsonl"],
        "append_increment",
    )
    new_texts = [e.get("text") for e in c3["events"]]
    conditions["append_increment"] = {
        **{k: v for k, v in c3.items() if k != "events"},
        "appended_expected": appended,
        "new_events": len(c3["events"]),
        "new_texts": new_texts,
        "no_duplicates": len(set(new_texts)) == len(new_texts),
        "file_input_fastpath": "yes: provider/session/profile reused from the cursor",
    }

    c4 = run_measured(
        binary,
        ["view", "-i", str(input_path), "--scope", "tools", "--with-pair", "--limit", "50", "--output", "jsonl"],
        "view_tools_with_pair",
    )
    conditions["view_tools_with_pair"] = {
        **{k: v for k, v in c4.items() if k != "events"},
        "paired_events_with_ids": sum(
            1 for e in c4["events"] if e.get("pair_state") == "paired" and (e.get("paired_event_ids") or [])
        ),
    }

    receipt = {"environment": env_note, "input": manifest, "conditions": conditions}
    (out_dir / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True), encoding="utf-8")
    return receipt


def generate_identity(path: pathlib.Path) -> dict:
    data = path.read_bytes()
    records = sum(1 for _ in path.open("rb"))
    return {
        "path": str(path),
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
        "physical_records": records,
        "seed": SEED,
        "provider": "claude_code",
        "shape": "80% single text, 10% tool_use/result pair, 10% 3-text block; 256B bodies, 1% 16384B",
        "rare_needle": NEEDLE,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    gen = sub.add_parser("generate", help="write the synthetic input")
    gen.add_argument("--out", type=pathlib.Path, required=True)
    gen.add_argument("--records", type=int, default=DEFAULT_RECORDS)
    mes = sub.add_parser("measure", help="run the four measured conditions")
    mes.add_argument("--input", type=pathlib.Path, required=True)
    mes.add_argument("--out-dir", type=pathlib.Path, required=True)
    mes.add_argument("--binary", type=pathlib.Path,
                     default=pathlib.Path("target/release/agent-jsonl-compact"))
    args = parser.parse_args()

    if args.cmd == "generate":
        info = generate(args.out, args.records)
        print(json.dumps(info, indent=2, sort_keys=True))
        return 0

    if not args.input.exists():
        raise SystemExit(f"input not found: {args.input}")
    binary = args.binary
    if not binary.is_absolute():
        binary = pathlib.Path.cwd() / binary
    if not binary.exists():
        raise SystemExit(f"release binary not found: {binary} (run: cargo build --release)")
    receipt = measure(binary, args.input, args.out_dir)
    print(json.dumps(receipt["conditions"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
