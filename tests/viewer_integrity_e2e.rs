//! Actual concurrent-read integrity E2E (A12 "observable mid-read mutation").
//!
//! Platform scope: **Linux only** (`/proc/<pid>/fd`, `/proc/<pid>/stat`, `SIGSTOP`/`SIGCONT`).
//! CI runs on ubuntu-latest, which satisfies this scope. There is no ignore attribute, no skip and
//! no xfail here: if the premise cannot be established the test FAILS with a reason, and if
//! the mechanism is unavailable the whole target is not built (`#![cfg(target_os = "linux")]`).
//!
//! Why this file exists separately from `viewer_e2e.rs`: the A12 case there mutates the file
//! *before* the query starts (`fs::write` with the same contents). That exercises cursor
//! generation/anchor validation, not an actual change *during* the read. This target adds the
//! missing "the source changed while the CLI was reading it" observation.
//!
//! Method: the CLI is started as a real subprocess from the built binary
//! (`CARGO_BIN_EXE_agent-jsonl-compact`), the parent **only** observes its own child:
//!   1. wait until `/proc/<child>/fd` exposes the input file (proves the file is open,
//!      i.e. the snapshot length was already captured — `run_query` takes `metadata.len()`
//!      before opening the reader),
//!   2. `SIGSTOP` the exact owned child and verify `/proc/<child>/stat` state == `T`,
//!   3. rewrite one payload byte in place (same length, new mtime),
//!   4. `SIGCONT` and collect the result.
//!
//! This makes the overlap deterministic instead of a sleep race. Only logical/behavioural
//! requirements are asserted: no OS physical-I/O promise, no full-prefix hash promise.

#![cfg(target_os = "linux")]

mod viewer_support;

use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use viewer_support::{line, Sandbox};

/// Marker that must be present in the tail; the mutation rewrites its first byte.
const TAIL_MARKER: &str = "TAIL_SENTINEL_MARKER_V1";

/// Wait until the child has the given file open, reading `/proc/<pid>/fd`.
/// Returns the fd numbers pointing at `path` (may be several: metadata + reader).
fn wait_for_open_fd(pid: u32, path: &Path, deadline: Duration) -> Result<Vec<u32>, String> {
    let started = Instant::now();
    let fd_dir = PathBuf::from(format!("/proc/{pid}/fd"));
    let want = fs::canonicalize(path).map_err(|e| format!("canonicalize target: {e}"))?;
    let mut last_err = String::from("no fd observed");
    while started.elapsed() < deadline {
        match fs::read_dir(&fd_dir) {
            Ok(entries) => {
                let mut hits = Vec::new();
                for entry in entries.flatten() {
                    let fd = match entry
                        .file_name()
                        .to_str()
                        .and_then(|s| s.parse::<u32>().ok())
                    {
                        Some(fd) => fd,
                        None => continue,
                    };
                    let link = fd_dir.join(fd.to_string());
                    match fs::read_link(&link) {
                        Ok(target) => {
                            if target == want {
                                hits.push(fd);
                            }
                        }
                        Err(e) => last_err = format!("readlink {link:?}: {e}"),
                    }
                }
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
            Err(e) => last_err = format!("read_dir {fd_dir:?}: {e}"),
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(format!(
        "input fd for {want:?} not observed within {:?} ({last_err})",
        deadline
    ))
}

/// `/proc/<pid>/stat` state character (`R`, `S`, `T`, ...).
fn proc_state(pid: u32) -> Option<char> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Format: "pid (comm) S ppid ..."; comm may contain spaces/parens -> split after the last ')'.
    let close = stat.rfind(')')?;
    stat[close + 1..].trim_start().chars().next()
}

/// Send a signal to one exact PID without any external dependency.
fn signal(pid: u32, sig: &str) {
    let status = Command::new("/bin/kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn /bin/kill");
    assert!(
        status.success(),
        "kill -{sig} {pid} must succeed (exit {:?})",
        status.code()
    );
}

/// RAII guard: always SIGCONT (if we stopped it) + kill + wait, only for the owned child.
struct OwnedChild {
    child: Child,
    stopped: bool,
    reaped: bool,
}

impl OwnedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            stopped: false,
            reaped: false,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn stop(&mut self) {
        signal(self.pid(), "STOP");
        self.stopped = true;
    }

    fn cont(&mut self) {
        if self.stopped {
            signal(self.pid(), "CONT");
            self.stopped = false;
        }
    }

    fn wait_with_output(mut self) -> std::process::Output {
        self.cont();
        // `Child::wait_with_output` consumes the Child, which is impossible while `Drop` owns it.
        // Take the pipes, wait on the borrowed handle, then reassemble an Output.
        use std::io::Read;
        let mut stdout = self.child.stdout.take().expect("child stdout piped");
        let mut stderr = self.child.stderr.take().expect("child stderr piped");
        let status = self.child.wait().expect("wait owned child");
        self.reaped = true;
        let mut out_buf = Vec::new();
        let mut err_buf = Vec::new();
        let _ = stdout.read_to_end(&mut out_buf);
        let _ = stderr.read_to_end(&mut err_buf);
        std::process::Output {
            status,
            stdout: out_buf,
            stderr: err_buf,
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if self.stopped {
            // Never leave a stopped process behind, even on panic.
            let _ = Command::new("/bin/kill")
                .arg("-CONT")
                .arg(self.pid().to_string())
                .status();
            self.stopped = false;
        }
        // Only the owned child handle is touched; the PID cannot be reused before `wait`.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Stream a large but valid Codex JSONL file (header + many small messages + tail marker)
/// with a bounded buffer: no whole-file string is ever materialized in the test process.
fn write_large_codex_file(path: &Path, filler_bytes: u64) -> u64 {
    let file = fs::File::create(path).expect("create fixture");
    let mut out = std::io::BufWriter::with_capacity(64 * 1024, file);
    let mut written: u64 = 0;

    let header = line(&viewer_support::codex_session_meta(
        "integrity-e2e",
        "/repoIntegrity",
        "2026-10-01T00:00:00Z",
    ));
    out.write_all(header.as_bytes()).unwrap();
    written += header.len() as u64;

    // Small messages, one per second of a synthetic clock, until the filler target is reached.
    let mut i: u64 = 0;
    while written < filler_bytes {
        let ts = format!(
            "2026-10-01T{:02}:{:02}:{:02}Z",
            (i / 3600) % 24,
            (i / 60) % 60,
            i % 60
        );
        let row = line(&viewer_support::codex_message(
            "user",
            &format!("filler message {i:07}"),
            &ts,
            None,
        ));
        out.write_all(row.as_bytes()).unwrap();
        written += row.len() as u64;
        i += 1;
    }

    // Tail record holding the marker we will rewrite in place.
    let tail = line(&viewer_support::codex_message(
        "assistant",
        &format!("{TAIL_MARKER} end"),
        "2026-10-02T00:00:00Z",
        Some("final"),
    ));
    out.write_all(tail.as_bytes()).unwrap();
    written += tail.len() as u64;
    out.flush().unwrap();
    written
}

/// Replace the first byte of TAIL_MARKER **in place**, preserving the file length, and force a
/// visible mtime change. The test (not the reader) is what mutates the source.
fn mutate_marker_byte_in_place(path: &Path, sentinel: &str) -> u64 {
    let bytes = fs::read(path).expect("read fixture for mutation");
    let needle = sentinel.as_bytes();
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("tail marker must exist in the fixture");
    let old_len = fs::metadata(path).unwrap().len();
    // Rewrite the first marker byte ('T' -> 'X'); length preserved by construction.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for in-place mutation");
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(pos as u64)).unwrap();
    file.write_all(b"X").unwrap();
    file.sync_all().unwrap();
    // Make the mtime change observable even on coarse clocks. `File::set_modified` is newer
    // than the declared MSRV 1.74, so sleep before the in-place write instead and rely on the
    // naturally updated mtime (root INTEGRITY_TEST_RELEASED_1021 guidance).
    std::thread::sleep(Duration::from_millis(20));
    let new_len = fs::metadata(path).unwrap().len();
    assert_eq!(
        old_len, new_len,
        "in-place mutation must preserve the file length"
    );
    pos as u64
}

fn spawn_args(sandbox: &Sandbox, args: &[&str]) -> OwnedChild {
    let mut command = Command::new(viewer_support::BIN);
    command
        .env_clear()
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.path().join("xdg-config"))
        .env("XDG_DATA_HOME", sandbox.path().join("xdg-data"))
        .env("XDG_CACHE_HOME", sandbox.path().join("xdg-cache"))
        .env("XDG_STATE_HOME", sandbox.path().join("xdg-state"))
        .env("PATH", "/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .current_dir(sandbox.path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    OwnedChild::new(command.spawn().expect("spawn built CLI"))
}

/// Header + optional pending call + ~`filler` bytes of public messages + a tail marker.
fn write_snapshot_fixture(path: &Path, filler_bytes: u64, with_pending_call: bool) -> u64 {
    use std::io::Write;
    let file = fs::File::create(path).expect("create fixture");
    let mut out = std::io::BufWriter::new(file);
    let mut written = 0u64;
    let header = line(&viewer_support::codex_session_meta(
        "snapshot-e2e",
        "/repoA",
        "2026-10-01T00:00:00Z",
    ));
    write!(out, "{header}").unwrap();
    written += header.len() as u64;
    if with_pending_call {
        let call = line(&viewer_support::codex_function_call(
            "shell",
            "pending-1",
            "{\"cmd\":\"x\"}",
            "2026-10-01T00:00:01Z",
        ));
        write!(out, "{call}").unwrap();
        written += call.len() as u64;
    }
    let mut i = 0u64;
    while written < filler_bytes {
        let row = line(&viewer_support::codex_message(
            "user",
            &format!("filler {i:05}"),
            "2026-10-01T00:00:02Z",
            None,
        ));
        write!(out, "{row}").unwrap();
        written += row.len() as u64;
        i += 1;
    }
    let tail = line(&viewer_support::codex_message(
        "assistant",
        "old tail body",
        "2026-10-01T00:00:03Z",
        Some("final"),
    ));
    write!(out, "{tail}").unwrap();
    written += tail.len() as u64;
    out.flush().unwrap();
    written
}

fn parse_rows(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("json row"))
        .collect()
}

#[test]
fn f06_initial_snapshot_holds_while_append_arrives_then_append_reads_it() {
    let sandbox = Sandbox::new();
    let path = sandbox.path().join("snapshot.jsonl");
    write_snapshot_fixture(&path, 8 * 1024 * 1024, false);

    let mut child = spawn_args(
        &sandbox,
        &[
            "view",
            "-i",
            path.to_str().unwrap(),
            "--tail",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let fds = wait_for_open_fd(child.pid(), &path, Duration::from_secs(10))
        .expect("source fd must be observable");
    assert!(!fds.is_empty());
    child.stop();
    let deadline = Instant::now() + Duration::from_secs(10);
    while proc_state(child.pid()) != Some('T') && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(proc_state(child.pid()), Some('T'), "child must be stopped");

    // Append a complete row AFTER the snapshot was taken.
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let row = line(&viewer_support::codex_message(
            "user",
            "future appended row",
            "2026-10-01T00:10:00Z",
            None,
        ));
        f.write_all(row.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    let out = child.wait_with_output();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "unchanged snapshot must stay ok: {stdout}"
    );
    assert!(
        !stdout.contains("future appended row"),
        "the initial query must not consume data appended after its snapshot"
    );
    let rows = parse_rows(&out.stdout);
    let meta = rows
        .iter()
        .find(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("metadata"))
        .expect("metadata");
    let cursor = meta
        .get("append_cursor")
        .and_then(|v| v.as_str())
        .expect("old boundary yields an append cursor")
        .to_string();

    // The append cursor then returns exactly the new row.
    let appended = viewer_support::ok(
        &sandbox,
        &[
            "view",
            "-i",
            path.to_str().unwrap(),
            "--after-cursor",
            &cursor,
            "--output",
            "jsonl",
        ],
    );
    let texts: Vec<String> = appended
        .events()
        .iter()
        .map(|e| {
            e.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(texts, vec!["future appended row".to_string()]);
}

#[test]
fn f04_pending_call_at_snapshot_ignores_result_appended_later() {
    let sandbox = Sandbox::new();
    let path = sandbox.path().join("snapshot-pair.jsonl");
    write_snapshot_fixture(&path, 8 * 1024 * 1024, true);

    let mut child = spawn_args(
        &sandbox,
        &[
            "view",
            "-i",
            path.to_str().unwrap(),
            "--scope",
            "tools",
            "--with-pair",
            "--tail",
            "1",
            "--output",
            "jsonl",
        ],
    );
    let fds = wait_for_open_fd(child.pid(), &path, Duration::from_secs(10))
        .expect("source fd must be observable");
    assert!(!fds.is_empty());
    child.stop();
    let deadline = Instant::now() + Duration::from_secs(10);
    while proc_state(child.pid()) != Some('T') && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(proc_state(child.pid()), Some('T'), "child must be stopped");

    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let row = line(&viewer_support::codex_function_output(
            "pending-1",
            "late result body",
            "2026-10-01T00:11:00Z",
        ));
        f.write_all(row.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    let out = child.wait_with_output();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "old snapshot stays ok: {stdout}"
    );
    assert!(
        !stdout.contains("late result body"),
        "pair passes must not read past the frozen boundary"
    );
    let rows = parse_rows(&out.stdout);
    let call = rows
        .iter()
        .find(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("event"))
        .expect("pending call visible");
    assert_eq!(
        call.get("pair_state").and_then(|v| v.as_str()),
        Some("pending_result"),
        "no result exists inside the frozen snapshot"
    );
    let meta = rows
        .iter()
        .find(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("metadata"))
        .expect("metadata");
    let cursor = meta
        .get("append_cursor")
        .and_then(|v| v.as_str())
        .expect("append cursor")
        .to_string();

    let appended = viewer_support::ok(
        &sandbox,
        &[
            "view",
            "-i",
            path.to_str().unwrap(),
            "--after-cursor",
            &cursor,
            "--output",
            "jsonl",
        ],
    );
    assert!(
        appended.stdout.contains("late result body"),
        "the later append read observes the new result: {}",
        appended.stdout
    );
}

fn spawn_view(sandbox: &Sandbox, path: &Path, query: &str) -> OwnedChild {
    let mut command = Command::new(viewer_support::BIN);
    command
        .env_clear()
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.path().join("xdg-config"))
        .env("XDG_DATA_HOME", sandbox.path().join("xdg-data"))
        .env("XDG_CACHE_HOME", sandbox.path().join("xdg-cache"))
        .env("XDG_STATE_HOME", sandbox.path().join("xdg-state"))
        .env("PATH", "/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .current_dir(sandbox.path())
        .arg("search")
        .arg("-i")
        .arg(path)
        .arg("--format")
        .arg("codex")
        .arg("--query")
        .arg(query)
        .arg("--output")
        .arg("jsonl")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    OwnedChild::new(command.spawn().expect("spawn built CLI"))
}

fn parse_stdout_jsonl(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .unwrap_or_else(|e| panic!("valid JSONL line required: {e}: {l}"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. ACTUAL mid-read mutation
// ---------------------------------------------------------------------------

#[test]
fn a12_actual_mid_read_mutation_is_detected() {
    let sandbox = Sandbox::new();
    let path = sandbox.path().join("integrity-big.jsonl");

    // ~64 MiB of small messages: long enough that the scan cannot finish inside the few
    // milliseconds it takes the parent to observe the fd, short enough for CI.
    let filler: u64 = 64 * 1024 * 1024;
    let size = write_large_codex_file(&path, filler);
    assert!(
        size >= filler,
        "fixture must reach the requested size (got {size})"
    );
    let before_meta = fs::metadata(&path).unwrap();
    let before_mtime = (before_meta.mtime(), before_meta.mtime_nsec());

    let mut child = spawn_view(&sandbox, &path, "NO_MATCH_SENTINEL_XYZ");

    // Premise 1: the child has the source open (snapshot already captured at open/meta time).
    let fds = match wait_for_open_fd(child.pid(), &path, Duration::from_secs(10)) {
        Ok(fds) => fds,
        Err(reason) => panic!("PREMISE NOT OBSERVED (child never exposed the input fd): {reason}"),
    };
    assert!(
        !fds.is_empty(),
        "at least one fd must point at the input file"
    );

    // Premise 2: freeze the exact owned child, then verify it is really stopped.
    child.stop();
    let stop_deadline = Instant::now() + Duration::from_secs(10);
    let mut state = proc_state(child.pid());
    while state != Some('T') && Instant::now() < stop_deadline {
        std::thread::sleep(Duration::from_millis(2));
        state = proc_state(child.pid());
    }
    assert_eq!(
        state,
        Some('T'),
        "PREMISE NOT OBSERVED (child did not reach stopped state T): {state:?}"
    );

    // Mutate while stopped: same length, new mtime. This is the test's own change, never the
    // reader's (the reader itself remains read-only).
    let marker_pos = mutate_marker_byte_in_place(&path, TAIL_MARKER);
    let after_meta = fs::metadata(&path).unwrap();
    assert_eq!(
        before_meta.len(),
        after_meta.len(),
        "mutation must preserve length"
    );
    assert_ne!(
        before_mtime,
        (after_meta.mtime(), after_meta.mtime_nsec()),
        "mutation must change mtime (observable change)"
    );

    // Resume and collect. The mutation is a deliberate concurrent write by the test; the
    // required outcome is a typed failure or an explicit inconsistency, never a silent complete.
    let out = child.wait_with_output();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let code = out.status.code().unwrap_or(-1);

    // stdout/stderr separation must hold regardless of the verdict.
    if !stdout.trim().is_empty() {
        let _ = parse_stdout_jsonl(&out.stdout);
    }

    let combined = format!("{stdout}{stderr}");
    let typed = combined.contains("inconsistent")
        || combined.contains("source_changed")
        || combined.contains("cursor_invalid")
        || combined.contains("\"partial\"")
        || combined.contains("status=partial");
    let silent_complete = code == 0 && !typed;

    assert!(
        !silent_complete,
        "a concurrent observable mutation before EOF must never return a silent complete rc0 \
         (marker at {marker_pos}, size {size}); rc={code}\nstdout={stdout}\nstderr={stderr}"
    );
    if code == 0 {
        // Permitted only with an explicit inconsistency declaration in the structured output.
        let rows = parse_stdout_jsonl(&out.stdout);
        let meta = rows
            .iter()
            .find(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("metadata"))
            .unwrap_or_else(|| panic!("metadata record required on rc0: {stdout}"));
        let status = meta.get("status").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            status == "inconsistent" || status == "partial",
            "rc0 is only acceptable with an explicit inconsistency declaration, got status={status:?}: {meta:#?}"
        );
    } else {
        assert_eq!(
            code, 1,
            "concurrent mutation must be a typed rc1 failure; rc={code}\nstdout={stdout}\nstderr={stderr}"
        );
        assert!(
            typed,
            "rc1 must carry a typed inconsistency/partial reason; stdout={stdout}\nstderr={stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Unchanged control (positive counts, proves the fixture itself is scannable)
// ---------------------------------------------------------------------------

#[test]
fn a12_unchanged_source_scans_completely() {
    let sandbox = Sandbox::new();
    let path = sandbox.path().join("integrity-control.jsonl");

    // Smaller control fixture: same generator, no concurrency, must complete normally.
    let filler: u64 = 4 * 1024 * 1024;
    let size = write_large_codex_file(&path, filler);
    let before_meta = fs::metadata(&path).unwrap();
    let before_mtime = (before_meta.mtime(), before_meta.mtime_nsec());

    let out = Command::new(viewer_support::BIN)
        .env_clear()
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.path().join("xdg-config"))
        .env("XDG_DATA_HOME", sandbox.path().join("xdg-data"))
        .env("XDG_CACHE_HOME", sandbox.path().join("xdg-cache"))
        .env("XDG_STATE_HOME", sandbox.path().join("xdg-state"))
        .env("PATH", "/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .current_dir(sandbox.path())
        .args([
            "search",
            "-i",
            path.to_str().unwrap(),
            "--format",
            "codex",
            "--query",
            "NO_MATCH_SENTINEL_XYZ",
            "--output",
            "jsonl",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run built CLI");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "unchanged control must succeed: rc={:?}\nstderr={stderr}",
        out.status.code()
    );
    let rows = parse_stdout_jsonl(&out.stdout);
    let meta = rows
        .iter()
        .find(|r| r.get("record_type").and_then(|v| v.as_str()) == Some("metadata"))
        .unwrap_or_else(|| panic!("metadata record required: {stdout}"));
    assert_eq!(
        meta.get("status").and_then(|v| v.as_str()),
        Some("ok"),
        "{meta:#?}"
    );
    // Positive denominators: the scan must actually have happened.
    let scanned = meta
        .pointer("/scan/scanned_events")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let read_bytes = meta
        .pointer("/scan/read_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(scanned > 0, "control must scan events: {meta:#?}");
    // Bounded check only: the reader must have consumed source bytes, and never more than the
    // real file length (logical-consumption semantics, no OS physical-I/O claim).
    assert!(read_bytes > 0, "control must read source bytes: {meta:#?}");
    assert!(
        read_bytes <= before_meta.len(),
        "read_bytes ({read_bytes}) must not exceed the real file size ({})",
        before_meta.len()
    );
    assert!(
        read_bytes >= size,
        "a full sentinel search must consume at least one logical pass over the whole fixture \
         (read={read_bytes}, size={size})"
    );
    assert!(size > filler, "control fixture non-trivial: {size}");
    assert_eq!(
        meta.get("exhausted").and_then(|v| v.as_bool()),
        Some(true),
        "a fully consumed unchanged snapshot must report exhausted: {meta:#?}"
    );

    // The reader did not mutate the source (the control case is the read-only half of A12).
    let after_meta = fs::metadata(&path).unwrap();
    assert_eq!(before_meta.len(), after_meta.len());
    assert_eq!(
        before_mtime,
        (after_meta.mtime(), after_meta.mtime_nsec()),
        "unchanged control must not be modified by reading"
    );
}
