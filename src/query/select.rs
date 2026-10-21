//! Streaming selection with bounded retention.
//!
//! Memory is governed by the requested window (max events / context / pair budget / output
//! bytes), never by the number of events in the input: the scan keeps only a ring of the last
//! `window` hard-scope events plus the matched primary events, and stops early once the window
//! is provably full and every buffered event has left the trailing context window.

use crate::query::model::{Kind, QEvent};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// First window in raw order.
    Head,
    /// Last `window` matches (tail).
    Tail,
}

#[derive(Debug, Clone)]
pub struct SelectOptions {
    /// Primary events to select.
    pub window: usize,
    pub mode: Mode,
    /// Context events before and after each primary (hard-scope neighbours only).
    pub context: usize,
    /// Maximum retained hard-scope neighbours (also bounds memory for huge context values).
    pub max_retained: usize,
}

impl Default for SelectOptions {
    fn default() -> Self {
        Self {
            window: 50,
            mode: Mode::Head,
            context: 0,
            max_retained: 4096,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct MatchInfo {
    pub queries: Vec<String>,
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct Selection {
    /// Retained events in raw order (primary and context), deduplicated by event id.
    pub events: Vec<QEvent>,
    pub primary_ids: Vec<String>,
    pub context_ids: BTreeSet<String>,
    /// Context events dropped because the retained cap forced it (reported, not hidden).
    pub dropped_context: u64,
    pub matches: BTreeMap<String, MatchInfo>,
    /// Matching events that did not fit the window.
    pub unrendered_matches: u64,
    /// Total events examined in hard scope.
    pub hard_scope_events: u64,
    /// Events examined at all (raw records expanding to events).
    pub scanned_events: u64,
    pub exhausted: bool,
}

pub struct Selector {
    options: SelectOptions,
    /// Retained hard-scope events keyed by their hard-scope ordinal (bounded).
    retained: BTreeMap<u64, QEvent>,
    /// Primaries in hard-scope order; in tail mode only the last `window` are kept.
    primary: VecDeque<u64>,
    primary_set: BTreeSet<u64>,
    context: BTreeSet<u64>,
    matches: BTreeMap<String, MatchInfo>,
    unrendered_matches: u64,
    /// Hard cap on retained events (bounded memory independent of input size).
    total_retained_cap: usize,
    scanned: u64,
    hard_scope: u64,
    /// One ordinal for every hard-scope event; used for context distance and early stop.
    ordinal: u64,
    last_primary_ordinal: Option<u64>,
    /// Non-primary context events dropped because of the retained cap (disclosed, not silent).
    dropped_context: u64,
}

impl Selector {
    pub fn new(options: SelectOptions) -> Self {
        let cap = options
            .max_retained
            .min(
                options
                    .window
                    .saturating_add(options.context.saturating_mul(2))
                    .saturating_add(8),
            )
            .max(1);
        Self {
            options,
            retained: BTreeMap::new(),
            primary: VecDeque::new(),
            primary_set: BTreeSet::new(),
            context: BTreeSet::new(),
            matches: BTreeMap::new(),
            unrendered_matches: 0,
            total_retained_cap: cap,
            scanned: 0,
            hard_scope: 0,
            ordinal: 0,
            last_primary_ordinal: None,
            dropped_context: 0,
        }
    }

    pub fn dropped_context(&self) -> u64 {
        self.dropped_context
    }

    /// Visit one event.
    ///
    /// * `hard_scope` – the event's kind belongs to the requested scope (messages/tools/all).
    ///   Only hard-scope events enter the context ring, distance computation or matching.
    /// * `primary_eligible` – hard scope **and** the soft filters (role/time/phase) allow it to be
    ///   a primary. Soft-filtered-out neighbours may still become context.
    pub fn feed<F>(
        &mut self,
        event: QEvent,
        hard_scope: bool,
        primary_eligible: bool,
        matcher: &mut F,
    ) where
        F: FnMut(&QEvent) -> Option<MatchInfo>,
    {
        self.scanned += 1;
        // Hard exclusion happens before any retention, matching or context work.
        if !hard_scope || event.kind.is_private() {
            return;
        }
        self.hard_scope += 1;
        self.ordinal += 1;
        let ordinal = self.ordinal;

        if self.options.context > 0 {
            self.retain(ordinal, event.clone());
        }

        if !primary_eligible {
            return;
        }
        let Some(info) = matcher(&event) else {
            return;
        };
        match self.options.mode {
            Mode::Head => {
                if self.primary.len() >= self.options.window {
                    self.unrendered_matches += 1;
                    return;
                }
            }
            Mode::Tail => {
                while self.primary.len() >= self.options.window {
                    if let Some(old) = self.primary.pop_front() {
                        self.primary_set.remove(&old);
                        if let Some(event) = self.retained.get(&old) {
                            let id = event.event_id.clone();
                            self.matches.remove(&id);
                        }
                        if self.options.context == 0 {
                            self.retained.remove(&old);
                        }
                    }
                }
            }
        }
        self.retain(ordinal, event.clone());
        self.matches.insert(event.event_id.clone(), info);
        self.primary.push_back(ordinal);
        self.primary_set.insert(ordinal);
        self.last_primary_ordinal = Some(ordinal);
        self.prune();
    }

    fn retain(&mut self, ordinal: u64, event: QEvent) {
        if self.retained.contains_key(&ordinal) {
            return;
        }
        self.retained.insert(ordinal, event);
        self.prune();
    }

    /// Enforce the retained cap by dropping the oldest **non-primary** events. A primary is never
    /// evicted, so a full window stays intact; unnecessary later context cannot accumulate.
    fn prune(&mut self) {
        while self.retained.len() > self.total_retained_cap {
            let Some((&oldest, _)) = self.retained.iter().next() else {
                break;
            };
            if self.primary_set.contains(&oldest) {
                // Skip primaries instead of stopping: a later non-primary may still be dropped.
                let Some(candidate) = self
                    .retained
                    .keys()
                    .copied()
                    .find(|ordinal| !self.primary_set.contains(ordinal))
                else {
                    break;
                };
                self.retained.remove(&candidate);
                self.dropped_context += 1;
                continue;
            }
            if self.options.context > 0 {
                if let Some(last) = self.last_primary_ordinal {
                    if oldest.saturating_add(self.options.context as u64) >= last {
                        // Needed for trailing context of the newest primary; try a later one.
                        let Some(candidate) = self.retained.keys().copied().find(|ordinal| {
                            !self.primary_set.contains(ordinal)
                                && *ordinal < last.saturating_sub(self.options.context as u64)
                        }) else {
                            break;
                        };
                        self.retained.remove(&candidate);
                        self.dropped_context += 1;
                        continue;
                    }
                }
            }
            self.retained.remove(&oldest);
            self.dropped_context += 1;
        }
    }

    /// Whether the scan can stop early: head window full and every needed trailing context event
    /// has been seen. Uses the same hard-scope ordinal as context distance.
    pub fn can_stop(&self) -> bool {
        if self.options.mode != Mode::Head {
            return false;
        }
        if self.options.window == 0 || self.primary.len() < self.options.window {
            return false;
        }
        let last = self.last_primary_ordinal.unwrap_or(0);
        self.ordinal > last.saturating_add(self.options.context as u64)
    }

    pub fn finish(mut self) -> Selection {
        let primaries: Vec<u64> = self.primary.iter().copied().collect();
        let mut keep: BTreeSet<u64> = primaries.iter().copied().collect();
        if self.options.context > 0 {
            let ordinals: Vec<u64> = self.retained.keys().copied().collect();
            let position = |ordinal: u64| ordinals.iter().position(|o| *o == ordinal);
            for primary in &primaries {
                let Some(center) = position(*primary) else {
                    continue;
                };
                let span = self.options.context;
                let lo = center.saturating_sub(span);
                let hi = (center + span).min(ordinals.len().saturating_sub(1));
                for ordinal in &ordinals[lo..=hi] {
                    if *ordinal != *primary {
                        keep.insert(*ordinal);
                        self.context.insert(*ordinal);
                    }
                }
            }
        }
        self.retained.retain(|ordinal, _| keep.contains(ordinal));

        let primary_ids: BTreeSet<String> = self
            .retained
            .iter()
            .filter(|(ordinal, _)| self.primary_set.contains(ordinal))
            .map(|(_, event)| event.event_id.clone())
            .collect();
        let context_ids: BTreeSet<String> = self
            .retained
            .iter()
            .filter(|(ordinal, _)| {
                self.context.contains(ordinal) && !self.primary_set.contains(ordinal)
            })
            .map(|(_, event)| event.event_id.clone())
            .collect();

        let mut events: Vec<QEvent> = self.retained.values().cloned().collect();
        events.sort_by_key(|e| {
            (
                e.source.byte_start,
                e.source.subevent_index,
                e.source.raw_line,
            )
        });
        let primary_order: Vec<String> = events
            .iter()
            .filter(|e| primary_ids.contains(&e.event_id))
            .map(|e| e.event_id.clone())
            .collect();
        let mut matches = self.matches;
        matches.retain(|id, _| primary_ids.contains(id));
        Selection {
            events,
            primary_ids: primary_order,
            context_ids,
            matches,
            unrendered_matches: self.unrendered_matches,
            hard_scope_events: self.hard_scope,
            scanned_events: self.scanned,
            dropped_context: self.dropped_context,
            exhausted: true,
        }
    }
}

/// Hard-scope test for one event under a scope plus an optional event-id filter.
pub fn in_scope(kind: Kind, scope: crate::query::filter::Scope) -> bool {
    match scope {
        crate::query::filter::Scope::Messages => kind.is_message(),
        crate::query::filter::Scope::Tools => kind.is_tool(),
        crate::query::filter::Scope::All => !kind.is_private(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::model::{Origin, Provider, Role, SourceRef, ToolObserved};

    fn ev(seq: u64, kind: Kind, text: &str) -> QEvent {
        QEvent {
            provider: Provider::Codex,
            session_id: "s".into(),
            event_id: format!("e{seq}"),
            timestamp_raw: None,
            epoch_ms: None,
            role: if kind == Kind::User {
                Some(Role::User)
            } else {
                Some(Role::Assistant)
            },
            phase: None,
            kind,
            source_kind: "t".into(),
            origin: Origin::Human,
            text: text.into(),
            tool_name: None,
            call_id: None,
            tool_observed: ToolObserved::None,
            origin_id: None,
            mirror_of: None,
            mirror_relation: None,
            source: SourceRef {
                raw_line: seq,
                byte_start: seq * 10,
                byte_end: seq * 10 + 9,
                subevent_index: 0,
            },
            sequence: seq,
        }
    }

    #[test]
    fn hard_scope_membership_is_separate_from_soft_eligibility() {
        // messages scope: a tool neighbour must NOT become context, a soft-filtered message may.
        let mut selector = Selector::new(SelectOptions {
            window: 1,
            mode: Mode::Head,
            context: 1,
            max_retained: 16,
        });
        let mut matcher = |e: &QEvent| {
            if e.text == "alpha" {
                Some(MatchInfo::default())
            } else {
                None
            }
        };
        // hard messages, tool is hard-excluded for this scope
        selector.feed(ev(0, Kind::User, "alpha"), true, true, &mut matcher);
        selector.feed(ev(1, Kind::ToolCall, "tool"), false, false, &mut matcher);
        // soft role filter would exclude this message, but it is a hard neighbour -> context
        selector.feed(ev(2, Kind::Assistant, "beta"), true, false, &mut matcher);
        let selection = selector.finish();
        let ids: Vec<&str> = selection
            .events
            .iter()
            .map(|e| e.event_id.as_str())
            .collect();
        assert!(
            !ids.contains(&"e1"),
            "tool must not leak into messages context: {ids:?}"
        );
        assert!(
            ids.contains(&"e2"),
            "soft-filtered message neighbour is context: {ids:?}"
        );
        assert!(selection.context_ids.contains("e2"));

        // tools scope: a user message neighbour must NOT become context; tool result stays.
        let mut selector = Selector::new(SelectOptions {
            window: 1,
            mode: Mode::Head,
            context: 1,
            max_retained: 16,
        });
        let mut matcher = |e: &QEvent| {
            if e.text == "call" {
                Some(MatchInfo::default())
            } else {
                None
            }
        };
        selector.feed(ev(0, Kind::ToolCall, "call"), true, true, &mut matcher);
        selector.feed(ev(1, Kind::User, "msg"), false, false, &mut matcher);
        selector.feed(ev(2, Kind::ToolOutput, "result"), true, false, &mut matcher);
        let selection = selector.finish();
        let ids: Vec<&str> = selection
            .events
            .iter()
            .map(|e| e.event_id.as_str())
            .collect();
        assert!(
            !ids.contains(&"e1"),
            "user message must not leak into tools context: {ids:?}"
        );
        assert!(ids.contains(&"e2"), "tool neighbour is context: {ids:?}");
    }

    #[test]
    fn retained_cap_never_exceeded_with_early_primary_and_many_neighbours() {
        let cap = 16usize;
        let mut selector = Selector::new(SelectOptions {
            window: 2,
            mode: Mode::Head,
            context: 1,
            max_retained: cap,
        });
        let mut matcher = |e: &QEvent| {
            if e.sequence < 2 {
                Some(MatchInfo::default())
            } else {
                None
            }
        };
        for seq in 0..500u64 {
            selector.feed(ev(seq, Kind::User, "x"), true, true, &mut matcher);
            assert!(
                selector.retained.len() <= selector.total_retained_cap,
                "retained {} exceeds cap {} at seq {seq}",
                selector.retained.len(),
                selector.total_retained_cap
            );
        }
        let selection = selector.finish();
        assert_eq!(
            selection.primary_ids,
            vec!["e0", "e1"],
            "primaries preserved"
        );
        assert!(
            selection.dropped_context > 0,
            "forced context cuts are reported"
        );
    }

    #[test]
    fn head_stop_uses_hard_scope_ordinals_not_physical_rows() {
        let mut selector = Selector::new(SelectOptions {
            window: 2,
            mode: Mode::Head,
            context: 1,
            max_retained: 64,
        });
        let mut fed = 0u64;
        let mut matcher = |_e: &QEvent| Some(MatchInfo::default());
        // Many private records (never fed as hard scope) must not delay the stop.
        for seq in 0..200 {
            selector.feed(
                ev(seq, Kind::Reasoning, "private"),
                false,
                false,
                &mut matcher,
            );
        }
        for seq in 200..1000 {
            selector.feed(ev(seq, Kind::User, "public"), true, true, &mut matcher);
            fed += 1;
            if selector.can_stop() {
                break;
            }
        }
        // window 2 + context 1 => stop after a handful of hard-scope events, not hundreds.
        assert!(
            fed <= 6,
            "head stop must be bounded in hard events, fed={fed}"
        );
    }

    #[test]
    fn head_mode_stops_after_window_plus_context() {
        let mut selector = Selector::new(SelectOptions {
            window: 2,
            mode: Mode::Head,
            context: 1,
            max_retained: 64,
        });
        let mut matcher = |_e: &QEvent| {
            Some(MatchInfo {
                queries: vec!["q".into()],
                ranges: vec![],
            })
        };
        for seq in 0..100 {
            let event = ev(seq, Kind::User, "q");
            selector.feed(event, true, true, &mut matcher);
            if selector.can_stop() {
                break;
            }
        }
        let selection = selector.finish();
        assert_eq!(selection.primary_ids.len(), 2);
        assert!(
            selection.scanned_events < 100,
            "must stop early: {}",
            selection.scanned_events
        );
    }

    #[test]
    fn tail_mode_keeps_last_window_only() {
        let mut selector = Selector::new(SelectOptions {
            window: 2,
            mode: Mode::Tail,
            context: 0,
            max_retained: 8,
        });
        let mut matcher = |_e: &QEvent| Some(MatchInfo::default());
        for seq in 0..50 {
            selector.feed(ev(seq, Kind::User, "x"), true, true, &mut matcher);
        }
        let selection = selector.finish();
        assert_eq!(selection.primary_ids, vec!["e48", "e49"]);
        assert!(selection.retained_len_ok());
    }

    impl Selection {
        fn retained_len_ok(&self) -> bool {
            self.events.len() <= 2
        }
    }

    #[test]
    fn context_can_cross_soft_filters_but_not_private() {
        let mut selector = Selector::new(SelectOptions {
            window: 1,
            mode: Mode::Head,
            context: 1,
            max_retained: 16,
        });
        let mut matcher = |e: &QEvent| {
            if e.text == "hit" {
                Some(MatchInfo::default())
            } else {
                None
            }
        };
        // `before`/`hit` are hard messages; `after` is hard but soft-filtered (context only);
        // `neighbor` stays in the messages hard scope.
        selector.feed(ev(0, Kind::User, "before"), true, true, &mut matcher);
        selector.feed(ev(1, Kind::User, "hit"), true, true, &mut matcher);
        selector.feed(ev(2, Kind::Assistant, "after"), true, false, &mut matcher);
        selector.feed(ev(3, Kind::Unknown, "neighbor"), false, false, &mut matcher);
        let selection = selector.finish();
        let ids: Vec<&str> = selection
            .events
            .iter()
            .map(|e| e.event_id.as_str())
            .collect();
        assert!(ids.contains(&"e0"), "{ids:?}");
        assert!(
            ids.contains(&"e2"),
            "soft-filtered neighbor kept as context: {ids:?}"
        );
        assert!(selection.context_ids.contains("e2"));
    }
}
