//! Tool call/result correlation for `--with-pair`.
//!
//! The correlation key is structured: provider + session id + call id. Delimiter-joined strings
//! are never used, and a call is only paired with a result (never with another call or another
//! result). Two additional streaming passes are bounded by the frozen snapshot.

use crate::query::model::{Kind, Provider, QEvent};
use crate::query::normalize::Normalizer;
use crate::query::reader::{LineItem, RecordSpan, SourceReader};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    Paired,
    PendingResult,
    Orphan,
    /// Present in the source but not rendered because of the display budget/total-event cap.
    NotRenderedBudget,
    /// The snapshot contains an unread (over-limit/invalid) range, so pairing cannot be proven.
    UnknownPair,
    /// Same call id appears more than once on the relevant side; the match is not unique.
    Ambiguous,
}

impl PairState {
    pub fn as_str(self) -> &'static str {
        match self {
            PairState::Paired => "paired",
            PairState::PendingResult => "pending_result",
            PairState::Orphan => "orphan",
            PairState::NotRenderedBudget => "not_rendered_budget",
            PairState::UnknownPair => "unknown_pair",
            PairState::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PairInfo {
    pub state: Option<PairState>,
    /// Exact number of opposite-side events sharing the structured call key.
    pub exact_counterparts: u64,
    /// Whether the stored id list is truncated.
    pub ids_truncated: bool,
    /// Raw pointer of the first counterpart whose id was not stored (reentry).
    pub reentry: Option<(u64, u64)>,
    /// Stable ids of opposite-side events sharing the structured call key.
    pub counterpart_event_ids: Vec<String>,
    pub counterparts: Vec<QEvent>,
    /// Exact opposite-side count, even when IDs are truncated.
    pub total_counterparts: u64,
    pub pair_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PairReport {
    /// Keyed by the selected event id.
    pub pairs: BTreeMap<String, PairInfo>,
    pub passes: u64,
    pub read_bytes: u64,
    /// Counterpart events found but not rendered (budget/cap).
    pub not_rendered: u64,
    /// Whether pairing can be proven over the whole snapshot (an unread range makes it false).
    pub incomplete: bool,
    /// Unread (over-limit) / invalid records encountered by the pair passes.
    pub uninspected: u64,
    pub invalid: u64,
    pub pending_tail: bool,
    /// Bounded raw pointers of the gaps that made completeness unprovable.
    pub gaps: Vec<(u64, u64, u64)>,
    pub duplicate_calls: u64,
    pub duplicate_results: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PairKey {
    provider: String,
    session_id: String,
    call_id: String,
}

impl PairKey {
    fn of(event: &QEvent, call_id: &str) -> Self {
        Self {
            provider: event.provider.as_str().to_string(),
            session_id: event.session_id.clone(),
            call_id: call_id.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct Found {
    event_id: String,
    kind: Kind,
}

struct ScanContext<'a> {
    provider: Provider,
    session_id: &'a str,
    source_id: &'a str,
}

fn scan_tool_events(
    normalizer: &mut Normalizer,
    span: RecordSpan,
    bytes: &[u8],
    source_id: &str,
) -> Vec<QEvent> {
    normalizer
        .normalize(span, bytes, source_id)
        .into_iter()
        .filter(|event| event.kind.is_tool())
        .collect()
}

/// Correlate the selected tool events against the rest of the snapshot.
#[allow(clippy::too_many_arguments)]
pub fn correlate(
    reader_path: &std::path::Path,
    provider: Provider,
    session_id: &str,
    source_id: &str,
    selected: &[QEvent],
    max_record_bytes: u64,
    start_from: u64,
    max_events: usize,
    snapshot_end: u64,
) -> Result<PairReport> {
    let context = ScanContext {
        provider,
        session_id,
        source_id,
    };
    let mut report = PairReport::default();

    let mut wanted: BTreeMap<PairKey, Vec<String>> = BTreeMap::new();
    for event in selected.iter().filter(|event| event.kind.is_tool()) {
        if let Some(call_id) = &event.call_id {
            wanted
                .entry(PairKey::of(event, call_id))
                .or_default()
                .push(event.event_id.clone());
        }
    }
    if wanted.is_empty() {
        return Ok(report);
    }

    // Pass 1: collect identities by structured key and by call/result side.
    let mut found: BTreeMap<PairKey, Vec<Found>> = BTreeMap::new();
    let mut totals: BTreeMap<PairKey, u64> = BTreeMap::new();
    let mut calls: BTreeMap<PairKey, u64> = BTreeMap::new();
    let mut results: BTreeMap<PairKey, u64> = BTreeMap::new();
    let mut reentry: BTreeMap<PairKey, (u64, u64)> = BTreeMap::new();
    {
        let mut pass = SourceReader::open(reader_path, max_record_bytes)?;
        pass.constrain_snapshot(snapshot_end);
        let mut normalizer = Normalizer::new(context.provider, context.session_id.to_string());
        let result = pass.scan_from(start_from, |item| {
            match &item {
                LineItem::Uninspected { span } => {
                    report.uninspected += 1;
                    if report.gaps.len() < 8 {
                        report
                            .gaps
                            .push((span.raw_line, span.byte_start, span.byte_end));
                    }
                }
                LineItem::PendingTail { .. } => {
                    report.pending_tail = true;
                }
                LineItem::Line { .. } => {}
            }
            if let LineItem::Line { span, bytes } = item {
                if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
                    report.invalid += 1;
                    if report.gaps.len() < 8 {
                        report
                            .gaps
                            .push((span.raw_line, span.byte_start, span.byte_end));
                    }
                }
                for event in scan_tool_events(&mut normalizer, span, &bytes, context.source_id) {
                    let Some(call_id) = event.call_id.clone() else {
                        continue;
                    };
                    let key = PairKey::of(&event, &call_id);
                    if !wanted.contains_key(&key) {
                        continue;
                    }
                    *totals.entry(key.clone()).or_insert(0) += 1;
                    match event.kind {
                        Kind::ToolCall => *calls.entry(key.clone()).or_insert(0) += 1,
                        Kind::ToolOutput => *results.entry(key.clone()).or_insert(0) += 1,
                        _ => {}
                    }
                    let ids = found.entry(key.clone()).or_default();
                    if ids.len() < max_events {
                        ids.push(Found {
                            event_id: event.event_id.clone(),
                            kind: event.kind,
                        });
                    } else {
                        reentry
                            .entry(key.clone())
                            .or_insert((event.source.raw_line, event.source.byte_start));
                    }
                }
            }
            Ok(true)
        });
        report.passes += 1;
        report.read_bytes += pass.read_stream_bytes();
        if result.is_err() || report.uninspected > 0 || report.invalid > 0 {
            report.incomplete = true;
        }
    }

    for (key, event_ids) in &wanted {
        let all = found.get(key).cloned().unwrap_or_default();
        let call_count = calls.get(key).copied().unwrap_or(0);
        let result_count = results.get(key).copied().unwrap_or(0);
        report.duplicate_calls += call_count.saturating_sub(1);
        report.duplicate_results += result_count.saturating_sub(1);
        for event_id in event_ids {
            let selected_kind = selected
                .iter()
                .find(|event| &event.event_id == event_id)
                .map(|event| event.kind)
                .unwrap_or(Kind::ToolCall);
            let (opposite_ids, opposite_count, duplicate) = if selected_kind == Kind::ToolCall {
                (
                    all.iter()
                        .filter(|found| found.kind == Kind::ToolOutput)
                        .map(|found| found.event_id.clone())
                        .filter(|id| id != event_id)
                        .collect::<Vec<_>>(),
                    result_count,
                    call_count > 1,
                )
            } else {
                (
                    all.iter()
                        .filter(|found| found.kind == Kind::ToolCall)
                        .map(|found| found.event_id.clone())
                        .filter(|id| id != event_id)
                        .collect::<Vec<_>>(),
                    call_count,
                    result_count > 1,
                )
            };
            let state = if report.incomplete {
                PairState::UnknownPair
            } else if duplicate {
                PairState::Ambiguous
            } else if opposite_count > 0 {
                PairState::Paired
            } else if selected_kind == Kind::ToolCall {
                PairState::PendingResult
            } else {
                PairState::Orphan
            };
            let pair_reason = match state {
                PairState::UnknownPair => Some("snapshot has uninspected or invalid records"),
                PairState::Ambiguous if selected_kind == Kind::ToolCall => {
                    Some("duplicate call id on the call side")
                }
                PairState::Ambiguous => Some("duplicate call id on the result side"),
                PairState::Orphan => Some("result has no matching call id on the call side"),
                PairState::PendingResult => Some("call has no result in the frozen snapshot"),
                PairState::Paired | PairState::NotRenderedBudget => None,
            }
            .map(str::to_string);
            report.pairs.insert(
                event_id.clone(),
                PairInfo {
                    state: Some(state),
                    exact_counterparts: opposite_count,
                    ids_truncated: opposite_count > opposite_ids.len() as u64,
                    reentry: reentry.get(key).copied(),
                    counterpart_event_ids: opposite_ids,
                    counterparts: Vec::new(),
                    total_counterparts: opposite_count,
                    pair_reason,
                },
            );
        }
    }

    // Pass 2: render bounded opposite-side counterparts into QEvents.
    let mut target_to_selected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (selected_id, info) in &report.pairs {
        for id in &info.counterpart_event_ids {
            target_to_selected
                .entry(id.clone())
                .or_default()
                .push(selected_id.clone());
        }
    }
    if !target_to_selected.is_empty() {
        let mut rendered = 0usize;
        let cap = max_events.max(1);
        let mut pass = SourceReader::open(reader_path, max_record_bytes)?;
        pass.constrain_snapshot(snapshot_end);
        let mut normalizer = Normalizer::new(context.provider, context.session_id.to_string());
        let result = pass.scan_from(start_from, |item| {
            if let LineItem::Line { span, bytes } = item {
                for event in scan_tool_events(&mut normalizer, span, &bytes, context.source_id) {
                    let Some(selected_ids) = target_to_selected.remove(&event.event_id) else {
                        continue;
                    };
                    if rendered >= cap {
                        report.not_rendered += 1;
                        continue;
                    }
                    for selected_id in selected_ids {
                        if let Some(info) = report.pairs.get_mut(&selected_id) {
                            if info.state == Some(PairState::Paired)
                                && info
                                    .counterparts
                                    .iter()
                                    .all(|existing| existing.event_id != event.event_id)
                            {
                                info.counterparts.push(event.clone());
                            }
                        }
                    }
                    rendered += 1;
                }
            }
            Ok(true)
        });
        report.passes += 1;
        report.read_bytes += pass.read_stream_bytes();
        if result.is_err() {
            report.incomplete = true;
        }
        let selected_ids: BTreeSet<String> = selected
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        for info in report.pairs.values_mut() {
            let already_shown = info
                .counterpart_event_ids
                .iter()
                .filter(|id| selected_ids.contains(*id))
                .count() as u64;
            if info.state == Some(PairState::Paired)
                && (info.counterparts.len() as u64) + already_shown < info.total_counterparts
            {
                info.state = Some(PairState::NotRenderedBudget);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_lines(path: &std::path::Path, lines: &[&str]) {
        let mut file = std::fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        file.flush().unwrap();
    }

    #[test]
    fn both_pair_passes_stay_inside_frozen_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frozen.jsonl");
        write_lines(
            &path,
            &[
                r#"{"type":"session_meta","payload":{"id":"S","cwd":"/r"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{}"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"inside result"}}"#,
            ],
        );
        let snapshot_end = std::fs::metadata(&path).unwrap().len();
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            f.write_all(
                br#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"future result"}}"#,
            )
            .unwrap();
            f.write_all(b"\n").unwrap();
            f.flush().unwrap();
        }

        let mut reader = SourceReader::open(&path, 1 << 20).unwrap();
        let mut selected = Vec::new();
        let mut normalizer = Normalizer::new(Provider::Codex, "S".to_string());
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    if span.byte_start >= snapshot_end {
                        return Ok(false);
                    }
                    for event in scan_tool_events(&mut normalizer, span, &bytes, "src") {
                        if event.call_id.as_deref() == Some("c1") && event.kind == Kind::ToolCall {
                            selected.push(event);
                        }
                    }
                }
                Ok(true)
            })
            .unwrap();

        let report = correlate(
            &path,
            Provider::Codex,
            "S",
            "src",
            &selected,
            1 << 20,
            0,
            16,
            snapshot_end,
        )
        .unwrap();
        assert!(report.passes <= 2, "at most two passes: {}", report.passes);
        assert!(
            report.read_bytes <= 2 * snapshot_end,
            "both passes must stay within the frozen range"
        );
        let info = &report.pairs[&selected[0].event_id];
        assert_eq!(info.exact_counterparts, 1);
        assert_eq!(info.counterparts.len(), 1);
        assert!(info.counterparts[0].source.byte_start < snapshot_end);
        assert!(!info.ids_truncated);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn pairing_is_scoped_provider_session_and_call_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        write_lines(
            &path,
            &[
                r#"{"type":"session_meta","payload":{"id":"S","cwd":"/r"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{}"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c2","arguments":"{}"}}"#,
            ],
        );
        let mut reader = SourceReader::open(&path, 1 << 20).unwrap();
        let mut selected = Vec::new();
        let mut normalizer = Normalizer::new(Provider::Codex, "S".to_string());
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    for event in scan_tool_events(&mut normalizer, span, &bytes, "src") {
                        if event.call_id.as_deref() == Some("c1") && event.kind == Kind::ToolCall {
                            selected.push(event);
                        }
                    }
                }
                Ok(true)
            })
            .unwrap();
        let report = correlate(
            &path,
            Provider::Codex,
            "S",
            "src",
            &selected,
            1 << 20,
            0,
            16,
            u64::MAX,
        )
        .unwrap();
        let info = &report.pairs[&selected[0].event_id];
        assert_eq!(info.state, Some(PairState::Paired));
        assert_eq!(info.counterparts.len(), 1);
        assert_eq!(info.counterpart_event_ids.len(), 1);
        assert!(report.passes <= 2);
    }

    #[test]
    fn call_and_result_sides_are_not_interchangeable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        write_lines(
            &path,
            &[
                r#"{"type":"session_meta","payload":{"id":"S","cwd":"/r"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"dup","arguments":"{}"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"dup","arguments":"{}"}}"#,
            ],
        );
        let mut reader = SourceReader::open(&path, 1 << 20).unwrap();
        let mut selected = Vec::new();
        let mut normalizer = Normalizer::new(Provider::Codex, "S".to_string());
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    for event in scan_tool_events(&mut normalizer, span, &bytes, "src") {
                        if event.call_id.as_deref() == Some("dup") && event.kind == Kind::ToolCall {
                            selected.push(event);
                        }
                    }
                }
                Ok(true)
            })
            .unwrap();
        let report = correlate(
            &path,
            Provider::Codex,
            "S",
            "src",
            &selected,
            1 << 20,
            0,
            16,
            u64::MAX,
        )
        .unwrap();
        let info = &report.pairs[&selected[0].event_id];
        assert_eq!(info.state, Some(PairState::Ambiguous));
        assert_eq!(info.counterpart_event_ids.len(), 0);
        assert_eq!(report.duplicate_calls, 1);
    }
}
