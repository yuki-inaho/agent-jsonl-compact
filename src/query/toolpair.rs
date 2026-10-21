//! Tool call/result correlation for `--with-pair`.
//!
//! Constant-pass design: after the primary selection we run at most **two** additional
//! streaming passes.
//! 1. Pass 1 collects counterpart identities for the selected call ids (bounded by
//!    `max_events`; overflow is reported with an exact total).
//! 2. Pass 2 renders the bounded counterpart events that fit the display budget.
//!
//! Pair states stay distinct: `paired`, `pending_result`, `orphan`/`unknown_pair`,
//! `not_rendered_budget`. Task completion is never inferred from rc0, and pairing is scoped to
//! provider + session + call id only.

use crate::query::model::{Kind, Provider, QEvent};
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
}

impl PairState {
    pub fn as_str(self) -> &'static str {
        match self {
            PairState::Paired => "paired",
            PairState::PendingResult => "pending_result",
            PairState::Orphan => "orphan",
            PairState::NotRenderedBudget => "not_rendered_budget",
            PairState::UnknownPair => "unknown_pair",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PairInfo {
    pub state: Option<PairState>,
    /// Exact number of other events sharing the namespaced call id (even beyond the bounded IDs).
    pub exact_counterparts: u64,
    /// Whether the stored id list is truncated.
    pub ids_truncated: bool,
    /// Raw pointer of the first counterpart whose id was not stored (reentry).
    pub reentry: Option<(u64, u64)>,
    /// Stable ids of every other event sharing the namespaced call id (always complete here).
    pub counterpart_event_ids: Vec<String>,
    pub counterparts: Vec<QEvent>,
    /// Counterpart records found in the snapshot (exact, even when IDs are truncated).
    pub total_counterparts: u64,
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
}

struct ScanContext<'a> {
    provider: Provider,
    /// Starting session id; the normalizer updates it per record when the record states one.
    session_id: &'a str,
    source_id: &'a str,
}

fn scan_tool_events(context: &ScanContext<'_>, span: RecordSpan, bytes: &[u8]) -> Vec<QEvent> {
    let mut normalizer =
        crate::query::normalize::Normalizer::new(context.provider, context.session_id.to_string());
    normalizer
        .normalize(span, bytes, context.source_id)
        .into_iter()
        .filter(|e| e.kind.is_tool())
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

    // Namespace is provider + session + call id: the same call id in another session must never
    // pair. Keys are `session\u{1f}call_id`.
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for event in selected.iter().filter(|e| e.kind.is_tool()) {
        if let Some(call_id) = &event.call_id {
            let key = format!("{}\u{1f}{}", event.session_id, call_id);
            wanted.entry(key).or_default().push(event.event_id.clone());
        }
    }
    if wanted.is_empty() {
        return Ok(report);
    }

    // Pass 1: counterpart identities.
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let mut reentry: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut open_calls: BTreeSet<String> = BTreeSet::new();
    {
        let mut pass = SourceReader::open(reader_path, max_record_bytes)?;
        // One snapshot for the whole query: never mix data appended after it.
        pass.constrain_snapshot(snapshot_end);
        let result = pass.scan_from(start_from, |item| {
            match &item {
                LineItem::Uninspected { span } => {
                    report.uninspected += 1;
                    if report.gaps.len() < 8 {
                        report
                            .gaps
                            .push((span.raw_line, span.byte_start, span.byte_end));
                    }
                    return Ok(true);
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
                for event in scan_tool_events(&context, span, &bytes) {
                    let Some(call_id) = event.call_id.clone() else {
                        continue;
                    };
                    let key = format!("{}\u{1f}{}", event.session_id, call_id);
                    if !wanted.contains_key(&key) {
                        continue;
                    }
                    if event.kind == Kind::ToolCall {
                        open_calls.insert(key.clone());
                    }
                    *totals.entry(key.clone()).or_insert(0) += 1;
                    let ids = found.entry(key.clone()).or_default();
                    // Exact total is counted for every record; only the id list is bounded.
                    if ids.len() <= max_events {
                        ids.push(event.event_id.clone());
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
            // A result may exist inside the unread range: pending would be a false claim.
            report.incomplete = true;
        }
    }

    for (key, event_ids) in &wanted {
        let ids = found.get(key).cloned().unwrap_or_default();
        let has_call = open_calls.contains(key) || ids.len() > event_ids.len();
        for event_id in event_ids {
            // Counterparts are every *other* event sharing the namespaced call id, including
            // ones that are themselves selected (e.g. `--scope tools` renders outputs directly).
            let counterpart_ids: Vec<String> =
                ids.iter().filter(|id| *id != event_id).cloned().collect();
            let is_call = selected
                .iter()
                .find(|e| &e.event_id == event_id)
                .map(|e| e.kind == Kind::ToolCall)
                .unwrap_or(true);
            let state = if report.incomplete {
                PairState::UnknownPair
            } else if is_call {
                if counterpart_ids.is_empty() {
                    PairState::PendingResult
                } else {
                    PairState::Paired
                }
            } else if has_call {
                PairState::Paired
            } else {
                PairState::Orphan
            };
            report.pairs.insert(
                event_id.clone(),
                PairInfo {
                    state: Some(state),
                    exact_counterparts: totals.get(key).copied().unwrap_or(0).saturating_sub(1),
                    ids_truncated: totals.get(key).copied().unwrap_or(0).saturating_sub(1)
                        > counterpart_ids.len() as u64,
                    reentry: reentry.get(key).copied(),
                    counterpart_event_ids: counterpart_ids.clone(),
                    counterparts: Vec::new(),
                    total_counterparts: totals.get(key).copied().unwrap_or(0).saturating_sub(1),
                },
            );
        }
    }

    // Pass 2: render bounded counterparts into QEvents. Pass 1 recorded only identities and
    // exact counts, so the render pass runs whenever at least one pair is `paired`.
    let has_paired = report
        .pairs
        .values()
        .any(|info| info.state == Some(PairState::Paired));
    if has_paired {
        // Counterparts are identified by (call_id, counterpart event id) from pass 1.
        let mut targets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (call_id, event_ids) in &wanted {
            let ids = found.get(call_id).cloned().unwrap_or_default();
            let selected_ids: BTreeSet<&String> = event_ids.iter().collect();
            targets.insert(
                call_id.clone(),
                ids.into_iter()
                    .filter(|id| !selected_ids.contains(id))
                    .collect(),
            );
        }
        let mut rendered = 0usize;
        let cap = max_events.max(1);
        let mut pass = SourceReader::open(reader_path, max_record_bytes)?;
        // Pass 2 must read exactly the same frozen snapshot as pass 1; otherwise a row appended
        // after the query snapshot could be consumed even when the output ids would not mix.
        pass.constrain_snapshot(snapshot_end);
        let result = pass.scan_from(start_from, |item| {
            if let LineItem::Line { span, bytes } = item {
                for event in scan_tool_events(&context, span, &bytes) {
                    let Some(call_id) = &event.call_id else {
                        continue;
                    };
                    let key = format!("{}\u{1f}{}", event.session_id, call_id);
                    let Some(target_ids) = targets.get_mut(&key) else {
                        continue;
                    };
                    let Some(pos) = target_ids.iter().position(|id| id == &event.event_id) else {
                        continue;
                    };
                    target_ids.remove(pos);
                    if rendered >= cap {
                        report.not_rendered += 1;
                        continue;
                    }
                    // Attach to every selected event of this call id.
                    let selected_ids = wanted.get(&key).cloned().unwrap_or_default();
                    for selected_id in selected_ids {
                        if let Some(info) = report.pairs.get_mut(&selected_id) {
                            if info.state == Some(PairState::Paired)
                                && info
                                    .counterparts
                                    .iter()
                                    .all(|e| e.event_id != event.event_id)
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
        // A counterpart that was already selected/rendered by the main window is still paired;
        // only genuinely unrendered counterparts downgrade the state.
        let selected_ids: BTreeSet<String> = selected.iter().map(|e| e.event_id.clone()).collect();
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

    #[test]
    fn both_pair_passes_stay_inside_frozen_snapshot() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frozen.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        for line in [
            r#"{"type":"session_meta","payload":{"id":"S","cwd":"/r"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{}"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"inside result"}}"#,
        ] {
            writeln!(file, "{line}").unwrap();
        }
        file.flush().unwrap();
        let snapshot_end = std::fs::metadata(&path).unwrap().len();

        // Append a same-call result AFTER the frozen snapshot. Without the pass-2 constraint the
        // scan would read (and count) past the boundary even if ids filter the output.
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
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    if span.byte_start >= snapshot_end {
                        return Ok(false);
                    }
                    for event in scan_tool_events(
                        &ScanContext {
                            provider: Provider::Codex,
                            session_id: "S",
                            source_id: "src",
                        },
                        span,
                        &bytes,
                    ) {
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
            "both passes must stay within the frozen range: read={} snapshot={snapshot_end}",
            report.read_bytes
        );
        let info = &report.pairs[&selected[0].event_id];
        assert_eq!(
            info.exact_counterparts, 1,
            "the future row must not count as a counterpart"
        );
        assert_eq!(info.counterparts.len(), 1);
        assert!(
            info.counterparts[0].source.byte_start < snapshot_end,
            "counterpart must come from the frozen snapshot"
        );
        assert!(!info.ids_truncated);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn pairing_is_scoped_provider_session_and_call_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        for line in [
            r#"{"type":"session_meta","payload":{"id":"S","cwd":"/r"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{}"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c2","arguments":"{}"}}"#,
        ] {
            writeln!(file, "{line}").unwrap();
        }
        let mut reader = SourceReader::open(&path, 1 << 20).unwrap();
        let mut selected = Vec::new();
        reader
            .scan_from(0, |item| {
                if let LineItem::Line { span, bytes } = item {
                    for event in scan_tool_events(
                        &ScanContext {
                            provider: Provider::Codex,
                            session_id: "S",
                            source_id: "src",
                        },
                        span,
                        &bytes,
                    ) {
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
        assert!(report.passes <= 2);
    }
}
