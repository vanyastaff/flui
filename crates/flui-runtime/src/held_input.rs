//! Bounded pointer input retained while a presentation has no committed tree.

use std::cell::RefCell;
use std::collections::VecDeque;

use flui_foundation::PresentationId;
use flui_interaction::{PointerEvent, PointerId};

/// Most pointer events one presentation retains while it has no committed
/// tree, counting events handed out for an in-flight replay.
pub const HELD_POINTER_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionClass {
    Contact,
    Hover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaySupersession {
    DispatchedOpen(PointerId),
    FinalUndispatched(PointerId),
}

impl ReplaySupersession {
    fn pointer_id(self) -> PointerId {
        match self {
            Self::DispatchedOpen(pointer_id) | Self::FinalUndispatched(pointer_id) => pointer_id,
        }
    }
}

#[derive(Debug, Default)]
struct QueueCounters {
    coalesced_events: usize,
    dropped_events: usize,
    dropped_sequences: usize,
}

/// Presentation-local pointer events waiting for a committed render tree.
///
/// Appends are amortized O(1). Coalescing, supersession, and safe eviction
/// are O(n) average and O(n²) worst-case; `n` is always bounded by
/// [`HELD_POINTER_CAPACITY`], so neither work nor storage can grow with an
/// uncommitted-frame storm.
pub struct HeldPointerQueue {
    presentation_id: PresentationId,
    events: VecDeque<PointerEvent>,
    replay_in_flight: bool,
    replay_reserved: usize,
    replay_tail_open_pointers: Vec<PointerId>,
    replay_dispatched_open_pointers: Vec<PointerId>,
    replay_supersessions: Vec<ReplaySupersession>,
    active_route_terminal_pointers: Vec<PointerId>,
    replay_active_route_terminal_pointers: Vec<PointerId>,
    replay_discard_remaining: bool,
    saturation_warned: bool,
    saturation_episodes: usize,
    counters: QueueCounters,
}

impl std::fmt::Debug for HeldPointerQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldPointerQueue")
            .field("presentation_id", &self.presentation_id)
            .field("held", &self.total_len())
            .field("replay_in_flight", &self.replay_in_flight)
            .finish_non_exhaustive()
    }
}

impl HeldPointerQueue {
    /// An empty queue for the presentation `presentation_id`, which names it
    /// in traces.
    #[must_use]
    pub fn new(presentation_id: PresentationId) -> Self {
        Self {
            presentation_id,
            events: VecDeque::new(),
            replay_in_flight: false,
            replay_reserved: 0,
            replay_tail_open_pointers: Vec::new(),
            replay_dispatched_open_pointers: Vec::new(),
            replay_supersessions: Vec::new(),
            active_route_terminal_pointers: Vec::new(),
            replay_active_route_terminal_pointers: Vec::new(),
            replay_discard_remaining: false,
            saturation_warned: false,
            saturation_episodes: 0,
            counters: QueueCounters::default(),
        }
    }

    /// Admit one event without ever exposing more than the fixed capacity.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn append(&mut self, event: PointerEvent) {
        self.append_with_active_contact(event, false);
    }

    /// Admit one event whose pointer may already have a live gesture route.
    pub fn append_with_active_contact(
        &mut self,
        mut event: PointerEvent,
        has_active_contact_sequence: bool,
    ) {
        let pointer_id = flui_interaction::PointerEventExt::pointer_id(&event);
        let motion_class = Self::motion_class(&event);
        let has_queued_open_epoch = pointer_id.is_some_and(|id| self.has_open_epoch(id));
        let has_active_route_open = has_active_contact_sequence
            && pointer_id.is_some_and(|id| !self.has_active_route_terminal_after_latest_down(id));
        let has_open_epoch = has_queued_open_epoch || has_active_route_open;

        if matches!(motion_class, Some(MotionClass::Contact)) && !has_open_epoch {
            self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
            return;
        }
        if matches!(
            event,
            PointerEvent::ButtonChange(_) | PointerEvent::Up(_) | PointerEvent::Cancel(_)
        ) && !has_open_epoch
        {
            self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
            return;
        }

        if let Some(pointer_id) = pointer_id
            && matches!(event, PointerEvent::Down(_))
        {
            let replay_supersession =
                if self.replay_in_flight && self.replay_tail_open_pointers.contains(&pointer_id) {
                    Some(ReplaySupersession::FinalUndispatched(pointer_id))
                } else if self.replay_in_flight
                    && self.replay_dispatched_open_pointers.contains(&pointer_id)
                {
                    Some(ReplaySupersession::DispatchedOpen(pointer_id))
                } else {
                    None
                };
            if replay_supersession.is_some() || self.has_open_epoch(pointer_id) {
                self.remove_open_epoch(pointer_id);
                if let Some(replay_supersession) = replay_supersession
                    && !self
                        .replay_supersessions
                        .iter()
                        .any(|queued| queued.pointer_id() == pointer_id)
                {
                    self.replay_supersessions.push(replay_supersession);
                }
            }
        }

        if let Some(pointer_id) = pointer_id
            && let Some(class) = motion_class
            && let Some(index) = self.coalescible_motion_index(pointer_id, class)
            && let PointerEvent::Move(newer) = &mut event
            && let PointerEvent::Move(older) = &self.events[index]
            && newer.try_coalesce(older).is_ok()
        {
            let _ = self.events.remove(index);
            self.events.push_back(event);
            self.counters.coalesced_events = self.counters.coalesced_events.saturating_add(1);
            return;
        }

        let is_terminal = matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_));
        let needs_active_terminal_room = is_terminal && has_active_route_open;
        let has_room = if let Some(pointer_id) = pointer_id
            && needs_active_terminal_room
        {
            self.make_room_for_active_route_terminal(pointer_id)
        } else if is_terminal {
            true
        } else {
            self.make_room_for_one()
        };
        if !has_room {
            self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
            self.note_saturation(
                self.total_len()
                    .saturating_add(1)
                    .saturating_sub(HELD_POINTER_CAPACITY),
            );
            return;
        }

        self.events.push_back(event);
        if let Some(pointer_id) = pointer_id
            && is_terminal
            && has_active_route_open
        {
            self.record_active_route_terminal(pointer_id);
        }
        while self.total_len() > HELD_POINTER_CAPACITY {
            let over_capacity = self.total_len().saturating_sub(HELD_POINTER_CAPACITY);
            let removed = self.evict_oldest_complete_sequence();
            debug_assert!(
                removed,
                "a terminal admitted above capacity completes an evictable pointer epoch"
            );
            if !removed {
                let _ = self.events.pop_back();
                self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
                break;
            }
            self.note_saturation(over_capacity);
        }
        debug_assert!(self.total_len() <= HELD_POINTER_CAPACITY);
    }

    /// Remove hover motion only, retaining every contact epoch intact.
    pub fn drop_hovers(&mut self) {
        let before = self.events.len();
        self.events
            .retain(|event| Self::motion_class(event) != Some(MotionClass::Hover));
        let dropped = before.saturating_sub(self.events.len());
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(dropped);
        if dropped != 0 {
            self.trace_counts("dropped held pointer hovers", dropped);
        }
        debug_assert!(self.total_len() <= HELD_POINTER_CAPACITY);
    }

    /// Drop every held contact sequence that is still open: a Down with no
    /// Up or Cancel after it, together with everything its pointer queued
    /// since. Complete sequences, hover and other pointers' input stay.
    ///
    /// Called when the presentation's pointer sequences are cancelled (focus
    /// loss, hidden, paused). An open epoch's terminal is no longer coming
    /// to this window, so replaying its Down at the next commit would open a
    /// route nothing closes. Its Down never reached a widget, so dropping it
    /// delivers nothing rather than a Down and Cancel pair for a gesture the
    /// user already abandoned. During a replay, an epoch the batch began is
    /// discarded from the batch the same way a superseding Down discards it.
    pub fn drop_open_sequences(&mut self) {
        let mut candidates: Vec<PointerId> = Vec::new();
        let queued_downs = self
            .events
            .iter()
            .filter(|event| matches!(event, PointerEvent::Down(_)))
            .filter_map(flui_interaction::PointerEventExt::pointer_id);
        for pointer_id in self
            .replay_tail_open_pointers
            .iter()
            .chain(&self.replay_dispatched_open_pointers)
            .copied()
            .chain(queued_downs)
        {
            if !candidates.contains(&pointer_id) {
                candidates.push(pointer_id);
            }
        }
        let mut dropped = 0usize;
        let mut sequences = 0usize;
        for pointer_id in candidates {
            if !self.has_open_epoch(pointer_id) {
                continue;
            }
            let queued_start = self.events.iter().rposition(|event| {
                matches!(event, PointerEvent::Down(_))
                    && flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
            });
            let start = queued_start.unwrap_or(0);
            let before = self.events.len();
            self.events = std::mem::take(&mut self.events)
                .into_iter()
                .enumerate()
                .filter_map(|(index, event)| {
                    let belongs_to_open_epoch = index >= start
                        && flui_interaction::PointerEventExt::pointer_id(&event) == Some(pointer_id);
                    (!belongs_to_open_epoch).then_some(event)
                })
                .collect();
            dropped = dropped.saturating_add(before.saturating_sub(self.events.len()));
            sequences = sequences.saturating_add(1);
            // An epoch that began inside the detached batch is discarded
            // from it before the replay exposes another event.
            if self.replay_in_flight && queued_start.is_none() {
                let supersession = if self.replay_tail_open_pointers.contains(&pointer_id) {
                    ReplaySupersession::FinalUndispatched(pointer_id)
                } else {
                    ReplaySupersession::DispatchedOpen(pointer_id)
                };
                if !self
                    .replay_supersessions
                    .iter()
                    .any(|queued| queued.pointer_id() == pointer_id)
                {
                    self.replay_supersessions.push(supersession);
                }
            }
            // The epoch is closed from here on: contact input arriving for
            // it before the next Down is refused at admission.
            self.replay_tail_open_pointers
                .retain(|open| *open != pointer_id);
            self.replay_dispatched_open_pointers
                .retain(|open| *open != pointer_id);
        }
        self.retain_recorded_active_route_terminals();
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(dropped);
        self.counters.dropped_sequences = self.counters.dropped_sequences.saturating_add(sequences);
        if sequences != 0 {
            self.trace_counts("dropped open held pointer sequences", sequences);
        }
    }

    /// Events held, counting those handed out for an in-flight replay.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.total_len()
    }

    /// Whether no event is held, counting those handed out for an in-flight
    /// replay.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_len() == 0
    }

    /// Drop every held event. A replay in flight discards what it has not
    /// yet dispatched.
    pub fn clear(&mut self) {
        let dropped = self.total_len();
        self.events.clear();
        self.replay_reserved = 0;
        self.replay_tail_open_pointers.clear();
        self.replay_dispatched_open_pointers.clear();
        self.replay_supersessions.clear();
        self.active_route_terminal_pointers.clear();
        self.replay_active_route_terminal_pointers.clear();
        self.replay_discard_remaining = self.replay_in_flight;
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(dropped);
        if dropped != 0 {
            self.trace_counts("dropped held pointer input", dropped);
        }
    }

    fn total_len(&self) -> usize {
        self.events.len().saturating_add(self.replay_reserved)
    }

    fn motion_class(event: &PointerEvent) -> Option<MotionClass> {
        match event {
            PointerEvent::Move(update) if update.buttons.is_empty() => Some(MotionClass::Hover),
            PointerEvent::Move(_) => Some(MotionClass::Contact),
            _ => None,
        }
    }

    fn has_open_epoch(&self, pointer_id: PointerId) -> bool {
        let mut is_open = self.replay_tail_open_pointers.contains(&pointer_id)
            || self.replay_dispatched_open_pointers.contains(&pointer_id);
        for event in &self.events {
            if flui_interaction::PointerEventExt::pointer_id(event) != Some(pointer_id) {
                continue;
            }
            match event {
                PointerEvent::Down(_) => is_open = true,
                PointerEvent::Up(_) | PointerEvent::Cancel(_) => is_open = false,
                _ => {}
            }
        }
        is_open
    }

    fn coalescible_motion_index(&self, pointer_id: PointerId, class: MotionClass) -> Option<usize> {
        let index = self.events.iter().rposition(|queued| {
            flui_interaction::PointerEventExt::pointer_id(queued) == Some(pointer_id)
        })?;
        if Self::motion_class(&self.events[index]) != Some(class) {
            return None;
        }
        Some(index)
    }

    fn remove_open_epoch(&mut self, pointer_id: PointerId) {
        let mut open_start = None;
        for (index, event) in self.events.iter().enumerate() {
            if flui_interaction::PointerEventExt::pointer_id(event) != Some(pointer_id) {
                continue;
            }
            match event {
                PointerEvent::Down(_) => open_start = Some(index),
                PointerEvent::Up(_) | PointerEvent::Cancel(_) => open_start = None,
                _ => {}
            }
        }
        let Some(open_start) = open_start else {
            return;
        };
        let before = self.events.len();
        self.events = std::mem::take(&mut self.events)
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let belongs_to_superseded_epoch = index >= open_start
                    && flui_interaction::PointerEventExt::pointer_id(&event) == Some(pointer_id);
                (!belongs_to_superseded_epoch).then_some(event)
            })
            .collect();
        let removed = before.saturating_sub(self.events.len());
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(removed);
        self.counters.dropped_sequences = self.counters.dropped_sequences.saturating_add(1);
        self.retain_recorded_active_route_terminals();
    }

    fn make_room_for_one(&mut self) -> bool {
        while self.total_len() >= HELD_POINTER_CAPACITY {
            if !self.evict_oldest_complete_sequence() {
                return false;
            }
            self.note_saturation(1);
        }
        true
    }

    fn make_room_for_active_route_terminal(&mut self, pointer_id: PointerId) -> bool {
        while self.total_len() >= HELD_POINTER_CAPACITY {
            let evicted = self.evict_oldest_complete_sequence()
                || self.evict_oldest_hover()
                || self.evict_oldest_contact_motion(pointer_id)
                || self.evict_oldest_discrete()
                || self.evict_oldest_incomplete_sequence();
            if !evicted {
                return false;
            }
            self.note_saturation(1);
        }
        true
    }

    fn evict_oldest_complete_sequence(&mut self) -> bool {
        let mut victim = None;
        for (start, event) in self.events.iter().enumerate() {
            if !matches!(event, PointerEvent::Down(_)) {
                continue;
            }
            let Some(pointer_id) = flui_interaction::PointerEventExt::pointer_id(event) else {
                continue;
            };
            for (end, candidate) in self.events.iter().enumerate().skip(start + 1) {
                if flui_interaction::PointerEventExt::pointer_id(candidate) != Some(pointer_id) {
                    continue;
                }
                match candidate {
                    PointerEvent::Down(_) => break,
                    PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                        if self.must_keep_protected_terminal_at(pointer_id, end) {
                            break;
                        }
                        victim = Some((pointer_id, start, end));
                        break;
                    }
                    _ => {}
                }
            }
            if victim.is_some() {
                break;
            }
        }
        let Some((pointer_id, start, end)) = victim else {
            return false;
        };
        let before = self.events.len();
        self.events = std::mem::take(&mut self.events)
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let belongs_to_victim = (start..=end).contains(&index)
                    && flui_interaction::PointerEventExt::pointer_id(&event) == Some(pointer_id);
                (!belongs_to_victim).then_some(event)
            })
            .collect();
        let removed = before.saturating_sub(self.events.len());
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(removed);
        self.counters.dropped_sequences = self.counters.dropped_sequences.saturating_add(1);
        self.retain_recorded_active_route_terminals();
        true
    }

    fn evict_oldest_discrete(&mut self) -> bool {
        let Some(index) = self.events.iter().position(|event| {
            !matches!(
                event,
                PointerEvent::Down(_)
                    | PointerEvent::ButtonChange(_)
                    | PointerEvent::Move(_)
                    | PointerEvent::Up(_)
                    | PointerEvent::Cancel(_)
            )
        }) else {
            return false;
        };
        let _ = self.events.remove(index);
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
        true
    }

    fn evict_oldest_hover(&mut self) -> bool {
        let Some(index) = self
            .events
            .iter()
            .position(|event| Self::motion_class(event) == Some(MotionClass::Hover))
        else {
            return false;
        };
        let _ = self.events.remove(index);
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
        true
    }

    fn evict_oldest_contact_motion(&mut self, pointer_id: PointerId) -> bool {
        let Some(index) = self.events.iter().position(|event| {
            flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                && Self::motion_class(event) == Some(MotionClass::Contact)
        }) else {
            return false;
        };
        let _ = self.events.remove(index);
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
        true
    }

    fn protected_terminal_count(&self, pointer_id: PointerId) -> usize {
        self.active_route_terminal_pointers
            .iter()
            .filter(|protected| **protected == pointer_id)
            .count()
    }

    fn is_protected_terminal_at(&self, pointer_id: PointerId, terminal_index: usize) -> bool {
        let protected_terminal_count = self.protected_terminal_count(pointer_id);
        let total_terminal_count = self
            .events
            .iter()
            .filter(|event| {
                flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        let mut terminal_count = 0usize;
        for (index, event) in self.events.iter().enumerate() {
            if flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            {
                terminal_count = terminal_count.saturating_add(1);
            }
            if index == terminal_index {
                return total_terminal_count.saturating_sub(terminal_count)
                    < protected_terminal_count;
            }
        }
        false
    }

    fn must_keep_protected_terminal_at(
        &self,
        pointer_id: PointerId,
        terminal_index: usize,
    ) -> bool {
        if !self.is_protected_terminal_at(pointer_id, terminal_index) {
            return false;
        }
        self.protected_terminal_suffix_count(pointer_id, terminal_index) == 1
    }

    fn protected_terminal_suffix_count(
        &self,
        pointer_id: PointerId,
        terminal_index: usize,
    ) -> usize {
        let protected_terminal_count = self.protected_terminal_count(pointer_id);
        let total_terminal_count = self
            .events
            .iter()
            .filter(|event| {
                flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        let mut terminal_count = 0usize;
        let mut protected_suffix_count = 0usize;
        for (index, event) in self.events.iter().enumerate() {
            if flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            {
                terminal_count = terminal_count.saturating_add(1);
                if index >= terminal_index
                    && total_terminal_count.saturating_sub(terminal_count)
                        < protected_terminal_count
                {
                    protected_suffix_count = protected_suffix_count.saturating_add(1);
                }
            }
        }
        protected_suffix_count
    }

    fn has_active_route_terminal_after_latest_down(&self, pointer_id: PointerId) -> bool {
        let protected_terminal_count = self.protected_terminal_count(pointer_id);
        let total_terminal_count = self
            .events
            .iter()
            .filter(|event| {
                flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        let mut terminal_count = 0usize;
        let mut has_protected_terminal_after_latest_down = self
            .replay_active_route_terminal_pointers
            .contains(&pointer_id);
        for event in &self.events {
            if flui_interaction::PointerEventExt::pointer_id(event) != Some(pointer_id) {
                continue;
            }
            match event {
                PointerEvent::Down(_) => has_protected_terminal_after_latest_down = false,
                PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                    terminal_count = terminal_count.saturating_add(1);
                    has_protected_terminal_after_latest_down = total_terminal_count
                        .saturating_sub(terminal_count)
                        < protected_terminal_count;
                }
                _ => {}
            }
        }
        has_protected_terminal_after_latest_down
    }

    fn record_active_route_terminal(&mut self, pointer_id: PointerId) {
        self.active_route_terminal_pointers.push(pointer_id);
    }

    fn retain_recorded_active_route_terminals(&mut self) {
        let mut retained_pointers = Vec::new();
        for pointer_id in &self.active_route_terminal_pointers {
            let retained_count = retained_pointers
                .iter()
                .filter(|retained| *retained == pointer_id)
                .count();
            let terminal_count = self
                .events
                .iter()
                .filter(|event| {
                    flui_interaction::PointerEventExt::pointer_id(event) == Some(*pointer_id)
                        && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
                })
                .count();
            if retained_count < terminal_count {
                retained_pointers.push(*pointer_id);
            }
        }
        self.active_route_terminal_pointers = retained_pointers;
    }

    fn retain_replay_active_route_terminals(&mut self, replay_events: &VecDeque<PointerEvent>) {
        let mut retained_pointers = Vec::new();
        for pointer_id in &self.replay_active_route_terminal_pointers {
            let retained_count = retained_pointers
                .iter()
                .filter(|retained| *retained == pointer_id)
                .count();
            let terminal_count = replay_events
                .iter()
                .filter(|event| {
                    flui_interaction::PointerEventExt::pointer_id(event) == Some(*pointer_id)
                        && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
                })
                .count();
            if retained_count < terminal_count {
                retained_pointers.push(*pointer_id);
            }
        }
        self.replay_active_route_terminal_pointers = retained_pointers;
    }

    fn remove_replay_active_route_terminal(
        &mut self,
        pointer_id: PointerId,
        replay_events_after_dispatch: &VecDeque<PointerEvent>,
    ) {
        let protected_terminal_count = self
            .replay_active_route_terminal_pointers
            .iter()
            .filter(|protected| **protected == pointer_id)
            .count();
        let remaining_terminal_count = replay_events_after_dispatch
            .iter()
            .filter(|event| {
                flui_interaction::PointerEventExt::pointer_id(event) == Some(pointer_id)
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        if remaining_terminal_count >= protected_terminal_count {
            return;
        }
        if let Some(index) = self
            .replay_active_route_terminal_pointers
            .iter()
            .position(|protected| *protected == pointer_id)
        {
            let _ = self.replay_active_route_terminal_pointers.remove(index);
        }
    }

    fn evict_oldest_incomplete_sequence(&mut self) -> bool {
        let mut open_epochs = Vec::new();
        for (index, event) in self.events.iter().enumerate() {
            let Some(pointer_id) = flui_interaction::PointerEventExt::pointer_id(event) else {
                continue;
            };
            match event {
                PointerEvent::Down(_) => {
                    open_epochs.retain(|(open_pointer, _)| *open_pointer != pointer_id);
                    open_epochs.push((pointer_id, index));
                }
                PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                    open_epochs.retain(|(open_pointer, _)| *open_pointer != pointer_id);
                }
                _ => {}
            }
        }
        let Some((pointer_id, start)) = open_epochs.into_iter().min_by_key(|(_, start)| *start)
        else {
            return false;
        };
        let before = self.events.len();
        self.events = std::mem::take(&mut self.events)
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let belongs_to_victim = index >= start
                    && flui_interaction::PointerEventExt::pointer_id(&event) == Some(pointer_id);
                (!belongs_to_victim).then_some(event)
            })
            .collect();
        let removed = before.saturating_sub(self.events.len());
        self.counters.dropped_events = self.counters.dropped_events.saturating_add(removed);
        self.counters.dropped_sequences = self.counters.dropped_sequences.saturating_add(1);
        self.retain_recorded_active_route_terminals();
        true
    }

    fn note_saturation(&mut self, over_capacity: usize) {
        if self.saturation_warned {
            return;
        }
        self.saturation_warned = true;
        self.saturation_episodes = self.saturation_episodes.saturating_add(1);
        tracing::warn!(
            presentation_id = self.presentation_id.as_u64(),
            capacity = HELD_POINTER_CAPACITY,
            queued = self.total_len(),
            over_capacity,
            dropped_events = self.counters.dropped_events,
            dropped_sequences = self.counters.dropped_sequences,
            "held pointer input reached its defensive capacity"
        );
    }

    fn trace_counts(&self, operation: &'static str, affected: usize) {
        tracing::trace!(
            presentation_id = self.presentation_id.as_u64(),
            capacity = HELD_POINTER_CAPACITY,
            queued = self.total_len(),
            coalesced_events = self.counters.coalesced_events,
            dropped_events = self.counters.dropped_events,
            dropped_sequences = self.counters.dropped_sequences,
            remaining = self.total_len(),
            affected,
            operation,
            "held pointer queue operation"
        );
    }

    fn finish_replay(&mut self, completed: bool, restored: usize) {
        self.replay_in_flight = false;
        self.replay_reserved = 0;
        self.replay_tail_open_pointers.clear();
        self.replay_dispatched_open_pointers.clear();
        self.replay_supersessions.clear();
        self.replay_discard_remaining = false;
        self.trace_counts(
            if completed {
                "drained held pointer input"
            } else {
                "restored aborted held pointer replay"
            },
            restored,
        );
        if completed {
            self.replay_active_route_terminal_pointers.clear();
            self.retain_recorded_active_route_terminals();
            self.saturation_warned = false;
            self.counters = QueueCounters::default();
        } else {
            self.active_route_terminal_pointers
                .append(&mut self.replay_active_route_terminal_pointers);
            self.retain_recorded_active_route_terminals();
        }
        debug_assert!(self.total_len() <= HELD_POINTER_CAPACITY);
    }

    fn finish_cleared_replay(&mut self, discarded: usize) {
        self.replay_in_flight = false;
        self.replay_reserved = 0;
        self.replay_tail_open_pointers.clear();
        self.replay_dispatched_open_pointers.clear();
        self.replay_supersessions.clear();
        self.active_route_terminal_pointers.clear();
        self.replay_active_route_terminal_pointers.clear();
        self.replay_discard_remaining = false;
        self.trace_counts("discarded cleared held pointer replay", discarded);
        debug_assert!(self.total_len() <= HELD_POINTER_CAPACITY);
    }
}

/// Detached replay batch that restores its undispatched suffix on unwind.
///
/// No `RefMut` survives `next`, so a dispatch callback may enqueue input
/// reentrantly. Dropping an unfinished batch places its suffix ahead of that
/// newly queued input and clears the in-flight state.
#[must_use = "dropping an unconsumed replay batch restores its remaining input"]
pub struct HeldPointerReplay<'a> {
    queue: &'a RefCell<HeldPointerQueue>,
    remaining: VecDeque<PointerEvent>,
    snapshot_events: usize,
    completed: bool,
}

impl std::fmt::Debug for HeldPointerReplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldPointerReplay")
            .field("remaining", &self.remaining.len())
            .field("snapshot_events", &self.snapshot_events)
            .field("completed", &self.completed)
            .finish_non_exhaustive()
    }
}

impl<'a> HeldPointerReplay<'a> {
    /// Detach every held event into a replay batch, or `None` when a replay
    /// of this queue is already in flight.
    pub fn begin(queue: &'a RefCell<HeldPointerQueue>) -> Option<Self> {
        let remaining = {
            let mut queue = queue.borrow_mut();
            if queue.replay_in_flight {
                return None;
            }
            let remaining = std::mem::take(&mut queue.events);
            queue.replay_in_flight = true;
            queue.replay_reserved = remaining.len();
            queue.replay_active_route_terminal_pointers =
                std::mem::take(&mut queue.active_route_terminal_pointers);
            for event in &remaining {
                let Some(pointer_id) = flui_interaction::PointerEventExt::pointer_id(event) else {
                    continue;
                };
                match event {
                    PointerEvent::Down(_) => {
                        if !queue.replay_tail_open_pointers.contains(&pointer_id) {
                            queue.replay_tail_open_pointers.push(pointer_id);
                        }
                    }
                    PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                        queue
                            .replay_tail_open_pointers
                            .retain(|open| *open != pointer_id);
                    }
                    _ => {}
                }
            }
            remaining
        };
        Some(Self {
            queue,
            snapshot_events: remaining.len(),
            remaining,
            completed: false,
        })
    }

    /// Finish a fully dispatched batch. A batch with events left restores
    /// them to the queue instead, as dropping it would.
    pub fn complete(mut self) {
        if self.remaining.is_empty() {
            self.complete_inner();
        }
    }

    fn complete_inner(&mut self) {
        if self.completed {
            return;
        }
        self.queue
            .borrow_mut()
            .finish_replay(true, self.snapshot_events);
        self.completed = true;
    }

    fn discard_remaining_if_cleared(&mut self) -> bool {
        if !self.queue.borrow().replay_discard_remaining {
            return false;
        }
        let discarded = self.remaining.len();
        self.remaining.clear();
        self.queue.borrow_mut().finish_cleared_replay(discarded);
        self.completed = true;
        true
    }

    /// Apply replay-time repeated-Down requests before exposing another old
    /// event. The request list and every scan are bounded by the queue cap.
    fn discard_superseded_suffixes(&mut self) {
        let supersessions = {
            let mut queue = self.queue.borrow_mut();
            std::mem::take(&mut queue.replay_supersessions)
        };
        if supersessions.is_empty() {
            return;
        }

        let mut removed_events = 0usize;
        let mut removed_sequences = 0usize;
        for supersession in supersessions {
            let pointer_id = supersession.pointer_id();
            let before = self.remaining.len();
            let clears_tail_open = match supersession {
                ReplaySupersession::DispatchedOpen(_) => {
                    let mut removing_epoch = true;
                    self.remaining = std::mem::take(&mut self.remaining)
                        .into_iter()
                        .filter_map(|event| {
                            if flui_interaction::PointerEventExt::pointer_id(&event)
                                != Some(pointer_id)
                            {
                                return Some(event);
                            }
                            if removing_epoch {
                                if matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) {
                                    removing_epoch = false;
                                }
                                return None;
                            }
                            Some(event)
                        })
                        .collect();
                    removing_epoch
                }
                ReplaySupersession::FinalUndispatched(_) => {
                    let mut open_start = None;
                    for (index, event) in self.remaining.iter().enumerate() {
                        if flui_interaction::PointerEventExt::pointer_id(event) != Some(pointer_id) {
                            continue;
                        }
                        match event {
                            PointerEvent::Down(_) => open_start = Some(index),
                            PointerEvent::Up(_) | PointerEvent::Cancel(_) => open_start = None,
                            _ => {}
                        }
                    }
                    if let Some(open_start) = open_start {
                        self.remaining = std::mem::take(&mut self.remaining)
                            .into_iter()
                            .enumerate()
                            .filter_map(|(index, event)| {
                                let belongs_to_final_open_epoch = index >= open_start
                                    && flui_interaction::PointerEventExt::pointer_id(&event)
                                        == Some(pointer_id);
                                (!belongs_to_final_open_epoch).then_some(event)
                            })
                            .collect();
                    }
                    true
                }
            };
            let removed = before.saturating_sub(self.remaining.len());
            removed_events = removed_events.saturating_add(removed);
            removed_sequences = removed_sequences.saturating_add(usize::from(removed != 0));

            let mut queue = self.queue.borrow_mut();
            if matches!(supersession, ReplaySupersession::DispatchedOpen(_)) {
                queue
                    .replay_dispatched_open_pointers
                    .retain(|open| *open != pointer_id);
            }
            if clears_tail_open {
                queue
                    .replay_tail_open_pointers
                    .retain(|open| *open != pointer_id);
            }
        }

        let mut queue = self.queue.borrow_mut();
        queue.replay_reserved = queue.replay_reserved.saturating_sub(removed_events);
        queue.retain_replay_active_route_terminals(&self.remaining);
        queue.counters.dropped_events =
            queue.counters.dropped_events.saturating_add(removed_events);
        queue.counters.dropped_sequences = queue
            .counters
            .dropped_sequences
            .saturating_add(removed_sequences);
    }
}

impl Iterator for HeldPointerReplay<'_> {
    type Item = PointerEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.discard_remaining_if_cleared() {
            return None;
        }
        self.discard_superseded_suffixes();
        let event = self.remaining.pop_front();
        if let Some(event) = &event {
            let pointer_id = flui_interaction::PointerEventExt::pointer_id(event);
            let final_open_down_remains = matches!(event, PointerEvent::Down(_))
                && self.remaining.iter().any(|remaining| {
                    matches!(remaining, PointerEvent::Down(_))
                        && flui_interaction::PointerEventExt::pointer_id(remaining) == pointer_id
                });
            let mut queue = self.queue.borrow_mut();
            queue.replay_reserved = queue.replay_reserved.saturating_sub(1);
            if let Some(pointer_id) = pointer_id {
                match event {
                    PointerEvent::Down(_) => {
                        if !final_open_down_remains {
                            queue
                                .replay_tail_open_pointers
                                .retain(|open| *open != pointer_id);
                        }
                        if !queue.replay_dispatched_open_pointers.contains(&pointer_id) {
                            queue.replay_dispatched_open_pointers.push(pointer_id);
                        }
                    }
                    PointerEvent::Up(_) | PointerEvent::Cancel(_) => queue
                        .replay_dispatched_open_pointers
                        .retain(|open| *open != pointer_id),
                    _ => {}
                }
                if matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) {
                    queue.remove_replay_active_route_terminal(pointer_id, &self.remaining);
                }
            }
        } else {
            self.complete_inner();
        }
        event
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining.len();
        let lower_bound = match self.queue.try_borrow() {
            Ok(queue) if queue.replay_supersessions.is_empty() => remaining,
            Ok(_) | Err(_) => 0,
        };
        (lower_bound, Some(remaining))
    }
}

impl Drop for HeldPointerReplay<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if self.remaining.is_empty() {
            self.complete_inner();
            return;
        }
        if self.discard_remaining_if_cleared() {
            return;
        }
        self.discard_superseded_suffixes();
        let restored = self.remaining.len();
        let mut queue = self.queue.borrow_mut();
        let mut merged = std::mem::take(&mut self.remaining);
        merged.append(&mut queue.events);
        queue.events = merged;
        queue.finish_replay(false, restored);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_foundation::PresentationId;
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::pointer::{PointerButtons, PointerKind};
    use flui_interaction::events::{
        PointerEventExt as _, make_cancel_event_for_id, make_down_event_for_id,
        make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{PointerEvent, PointerId};

    use super::{HELD_POINTER_CAPACITY, HeldPointerQueue, HeldPointerReplay};

    fn pointer(raw: u64) -> PointerId {
        PointerId::try_from(raw).expect("test pointer ids are nonzero")
    }

    fn position(x: f64) -> Offset<f64> {
        Offset::new(x, 0.0)
    }

    fn down(pointer_id: PointerId) -> PointerEvent {
        make_down_event_for_id(pointer_id, position(0.0), PointerKind::Touch)
            .expect("test pointer positions are finite")
    }

    fn contact_move(pointer_id: PointerId, x: f64) -> PointerEvent {
        make_move_event_for_id(pointer_id, position(x), PointerKind::Touch)
            .expect("test pointer positions are finite")
    }

    fn hover(pointer_id: PointerId, x: f64) -> PointerEvent {
        let mut event = make_move_event_for_id(pointer_id, position(x), PointerKind::Mouse)
            .expect("test pointer positions are finite");
        let PointerEvent::Move(update) = &mut event else {
            unreachable!("the move helper always constructs PointerEvent::Move")
        };
        update.buttons = PointerButtons::NONE;
        event
    }

    fn up(pointer_id: PointerId) -> PointerEvent {
        make_up_event_for_id(pointer_id, position(9.0), PointerKind::Touch)
            .expect("test pointer positions are finite")
    }

    fn cancel(pointer_id: PointerId) -> PointerEvent {
        make_cancel_event_for_id(pointer_id, PointerKind::Touch)
    }

    fn positions(events: &[PointerEvent]) -> Vec<f64> {
        events
            .iter()
            .map(|event| event.position().expect("test events carry positions").dx)
            .collect()
    }

    fn drain(queue: &RefCell<HeldPointerQueue>) -> Vec<PointerEvent> {
        let mut replay = HeldPointerReplay::begin(queue).expect("no replay is already in flight");
        let events = replay.by_ref().collect();
        replay.complete();
        events
    }

    fn queue() -> RefCell<HeldPointerQueue> {
        RefCell::new(HeldPointerQueue::new(PresentationId::new(1)))
    }

    fn latest_motion_moves_to_the_arrival_tail_across_interleaved_pointers() {
        let queue = queue();
        let first = pointer(2);
        let second = pointer(3);
        queue.borrow_mut().append(down(first));
        queue.borrow_mut().append(down(second));
        queue.borrow_mut().append(contact_move(first, 1.0));
        queue.borrow_mut().append(contact_move(second, 2.0));
        queue.borrow_mut().append(contact_move(first, 3.0));

        let events = drain(&queue);
        assert_eq!(positions(&events), vec![0.0, 0.0, 2.0, 3.0]);
    }

    fn reentrant_active_route_terminal_evicts_queued_contact_motion() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));
        for raw in 2..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(hover(pointer(raw), 1.0));
        }

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(active_route, 1.0), true);
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);
        replay.for_each(drop);

        let events = drain(&queue);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], PointerEvent::Up(_)));
        assert_eq!(
            flui_interaction::PointerEventExt::pointer_id(&events[0]),
            Some(active_route)
        );
    }

    fn abort_after_reentrant_down_restores_only_interleaved_old_input_ahead_of_new_down() {
        let queue = queue();
        let superseded = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(superseded));
        queue.borrow_mut().append(contact_move(superseded, 0.5));
        queue.borrow_mut().append(up(superseded));
        queue.borrow_mut().append(down(superseded));
        queue.borrow_mut().append(down(interleaved));
        queue.borrow_mut().append(contact_move(superseded, 1.0));
        queue.borrow_mut().append(contact_move(interleaved, 2.0));
        queue.borrow_mut().append(cancel(superseded));

        {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
            assert!(matches!(replay.next(), Some(PointerEvent::Move(_))));
            assert!(matches!(replay.next(), Some(PointerEvent::Up(_))));
            assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
            queue.borrow_mut().append(down(superseded));
        }

        let restored = drain(&queue);
        let restored_ids: Vec<_> = restored
            .iter()
            .map(flui_interaction::PointerEventExt::pointer_id)
            .collect();
        assert_eq!(
            restored_ids,
            vec![Some(interleaved), Some(interleaved), Some(superseded)]
        );
        assert_eq!(positions(&restored), vec![0.0, 2.0, 0.0]);
    }

    fn clear_during_replay_prevents_drop_from_restoring_the_detached_suffix() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 1.0));

        {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
            queue.borrow_mut().clear();
        }

        assert!(queue.borrow().is_empty());
        assert!(drain(&queue).is_empty());
    }

    fn dropping_open_sequences_mid_replay_keeps_only_complete_epochs() {
        let queue = queue();
        let dispatched = pointer(2);
        let complete = pointer(3);
        let undispatched = pointer(4);
        queue.borrow_mut().append(down(dispatched));
        queue.borrow_mut().append(contact_move(dispatched, 1.0));
        queue.borrow_mut().append(down(complete));
        queue.borrow_mut().append(up(complete));
        queue.borrow_mut().append(down(undispatched));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue.borrow_mut().drop_open_sequences();
        // A contact move for the abandoned epoch is refused from here on.
        queue.borrow_mut().append(contact_move(dispatched, 2.0));
        let rest: Vec<_> = replay.by_ref().collect();
        replay.complete();

        let ids: Vec<_> = rest
            .iter()
            .map(flui_interaction::PointerEventExt::pointer_id)
            .collect();
        assert_eq!(ids, vec![Some(complete), Some(complete)]);
        assert!(matches!(rest[0], PointerEvent::Down(_)));
        assert!(matches!(rest[1], PointerEvent::Up(_)));
        assert!(queue.borrow().is_empty());
    }

    #[test]
    fn held_input_reentry_matrix() {
        crate::table_test::run_table(
            "held_input_reentry_matrix",
            &[
                (
                    "latest_motion_moves_to_the_arrival_tail_across_interleaved_pointers",
                    latest_motion_moves_to_the_arrival_tail_across_interleaved_pointers as fn(),
                ),
                (
                    "reentrant_active_route_terminal_evicts_queued_contact_motion",
                    reentrant_active_route_terminal_evicts_queued_contact_motion as fn(),
                ),
                (
                    "abort_after_reentrant_down_restores_only_interleaved_old_input_ahead_of_new_down",
                    abort_after_reentrant_down_restores_only_interleaved_old_input_ahead_of_new_down
                        as fn(),
                ),
                (
                    "clear_during_replay_prevents_drop_from_restoring_the_detached_suffix",
                    clear_during_replay_prevents_drop_from_restoring_the_detached_suffix as fn(),
                ),
                (
                    "dropping_open_sequences_mid_replay_keeps_only_complete_epochs",
                    dropping_open_sequences_mid_replay_keeps_only_complete_epochs as fn(),
                ),
            ],
        );
    }
}
