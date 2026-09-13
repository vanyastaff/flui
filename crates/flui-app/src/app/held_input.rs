//! Bounded pointer input retained while a presentation has no committed tree.

use std::cell::RefCell;
use std::collections::VecDeque;

use flui_foundation::PresentationId;
use flui_interaction::{PointerEvent, PointerId};

pub(crate) const HELD_POINTER_CAPACITY: usize = 256;

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
pub(crate) struct HeldPointerQueue {
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

impl HeldPointerQueue {
    pub(crate) fn new(presentation_id: PresentationId) -> Self {
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
    #[cfg(test)]
    pub(crate) fn append(&mut self, event: PointerEvent) {
        self.append_with_active_contact(event, false);
    }

    /// Admit one event whose pointer may already have a live gesture route.
    pub(crate) fn append_with_active_contact(
        &mut self,
        event: PointerEvent,
        has_active_contact_sequence: bool,
    ) {
        let pointer_id = flui_interaction::events::extract_pointer_id(&event);
        let motion_class = Self::motion_class(&event);
        let has_queued_open_epoch = self.has_open_epoch(pointer_id);
        let has_active_route_open = has_active_contact_sequence
            && !self.has_active_route_terminal_after_latest_down(pointer_id);
        let has_open_epoch = has_queued_open_epoch || has_active_route_open;

        if matches!(motion_class, Some(MotionClass::Contact)) && !has_open_epoch {
            self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
            return;
        }
        if matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) && !has_open_epoch {
            self.counters.dropped_events = self.counters.dropped_events.saturating_add(1);
            return;
        }

        if matches!(event, PointerEvent::Down(_)) {
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

        if let Some(class) = motion_class
            && let Some(index) = self.coalescible_motion_index(pointer_id, class)
        {
            let _ = self.events.remove(index);
            self.events.push_back(event);
            self.counters.coalesced_events = self.counters.coalesced_events.saturating_add(1);
            return;
        }

        let is_terminal = matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_));
        let needs_active_terminal_room = is_terminal && has_active_route_open;
        let has_room = if needs_active_terminal_room {
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
        if is_terminal && has_active_route_open {
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
    pub(crate) fn drop_hovers(&mut self) {
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

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.total_len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.total_len() == 0
    }

    pub(crate) fn clear(&mut self) {
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

    #[cfg(test)]
    fn saturation_episodes(&self) -> usize {
        self.saturation_episodes
    }

    #[cfg(test)]
    fn is_replay_in_flight(&self) -> bool {
        self.replay_in_flight
    }

    fn total_len(&self) -> usize {
        self.events.len().saturating_add(self.replay_reserved)
    }

    fn motion_class(event: &PointerEvent) -> Option<MotionClass> {
        match event {
            PointerEvent::Move(update) if update.current.buttons.is_empty() => {
                Some(MotionClass::Hover)
            }
            PointerEvent::Move(_) => Some(MotionClass::Contact),
            _ => None,
        }
    }

    fn has_open_epoch(&self, pointer_id: PointerId) -> bool {
        let mut is_open = self.replay_tail_open_pointers.contains(&pointer_id)
            || self.replay_dispatched_open_pointers.contains(&pointer_id);
        for event in &self.events {
            if flui_interaction::events::extract_pointer_id(event) != pointer_id {
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
            flui_interaction::events::extract_pointer_id(queued) == pointer_id
        })?;
        if Self::motion_class(&self.events[index]) != Some(class) {
            return None;
        }
        Some(index)
    }

    fn remove_open_epoch(&mut self, pointer_id: PointerId) {
        let mut open_start = None;
        for (index, event) in self.events.iter().enumerate() {
            if flui_interaction::events::extract_pointer_id(event) != pointer_id {
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
                    && flui_interaction::events::extract_pointer_id(&event) == pointer_id;
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
            let pointer_id = flui_interaction::events::extract_pointer_id(event);
            for (end, candidate) in self.events.iter().enumerate().skip(start + 1) {
                if flui_interaction::events::extract_pointer_id(candidate) != pointer_id {
                    continue;
                }
                match candidate {
                    PointerEvent::Down(_) => break,
                    PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                        if self.is_protected_terminal_at(pointer_id, end) {
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
                    && flui_interaction::events::extract_pointer_id(&event) == pointer_id;
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
            flui_interaction::events::extract_pointer_id(event) == pointer_id
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
                flui_interaction::events::extract_pointer_id(event) == pointer_id
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        let mut terminal_count = 0usize;
        for (index, event) in self.events.iter().enumerate() {
            if flui_interaction::events::extract_pointer_id(event) == pointer_id
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

    fn has_active_route_terminal_after_latest_down(&self, pointer_id: PointerId) -> bool {
        let protected_terminal_count = self.protected_terminal_count(pointer_id);
        let total_terminal_count = self
            .events
            .iter()
            .filter(|event| {
                flui_interaction::events::extract_pointer_id(event) == pointer_id
                    && matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_))
            })
            .count();
        let mut terminal_count = 0usize;
        let mut has_protected_terminal_after_latest_down = self
            .replay_active_route_terminal_pointers
            .contains(&pointer_id);
        for event in &self.events {
            if flui_interaction::events::extract_pointer_id(event) != pointer_id {
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
                    flui_interaction::events::extract_pointer_id(event) == *pointer_id
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
                    flui_interaction::events::extract_pointer_id(event) == *pointer_id
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
                flui_interaction::events::extract_pointer_id(event) == pointer_id
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
            let pointer_id = flui_interaction::events::extract_pointer_id(event);
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
                    && flui_interaction::events::extract_pointer_id(&event) == pointer_id;
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
pub(crate) struct HeldPointerReplay<'a> {
    queue: &'a RefCell<HeldPointerQueue>,
    remaining: VecDeque<PointerEvent>,
    snapshot_events: usize,
    completed: bool,
}

impl<'a> HeldPointerReplay<'a> {
    pub(crate) fn begin(queue: &'a RefCell<HeldPointerQueue>) -> Option<Self> {
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
                let pointer_id = flui_interaction::events::extract_pointer_id(event);
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

    pub(crate) fn complete(mut self) {
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
                            if flui_interaction::events::extract_pointer_id(&event) != pointer_id {
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
                        if flui_interaction::events::extract_pointer_id(event) != pointer_id {
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
                                    && flui_interaction::events::extract_pointer_id(&event)
                                        == pointer_id;
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
            let pointer_id = flui_interaction::events::extract_pointer_id(event);
            let final_open_down_remains = matches!(event, PointerEvent::Down(_))
                && self.remaining.iter().any(|remaining| {
                    matches!(remaining, PointerEvent::Down(_))
                        && flui_interaction::events::extract_pointer_id(remaining) == pointer_id
                });
            let mut queue = self.queue.borrow_mut();
            queue.replay_reserved = queue.replay_reserved.saturating_sub(1);
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
    use flui_interaction::events::pointer::{PointerButtons, PointerType};
    use flui_interaction::events::{
        PointerEventExt as _, make_cancel_event_for_id, make_down_event_for_id,
        make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{PointerEvent, PointerId};
    use flui_types::geometry::{Offset, Pixels};

    use super::{HELD_POINTER_CAPACITY, HeldPointerQueue, HeldPointerReplay};

    fn pointer(raw: u64) -> PointerId {
        PointerId::new(raw).expect("test pointer ids are nonzero")
    }

    fn position(x: f32) -> Offset<Pixels> {
        Offset::new(Pixels(x), Pixels(0.0))
    }

    fn down(pointer_id: PointerId) -> PointerEvent {
        make_down_event_for_id(pointer_id, position(0.0), PointerType::Touch)
    }

    fn contact_move(pointer_id: PointerId, x: f32) -> PointerEvent {
        make_move_event_for_id(pointer_id, position(x), PointerType::Touch)
    }

    fn hover(pointer_id: PointerId, x: f32) -> PointerEvent {
        let mut event = make_move_event_for_id(pointer_id, position(x), PointerType::Mouse);
        let PointerEvent::Move(update) = &mut event else {
            unreachable!("the move helper always constructs PointerEvent::Move")
        };
        update.current.buttons = PointerButtons::new();
        event
    }

    fn up(pointer_id: PointerId) -> PointerEvent {
        make_up_event_for_id(pointer_id, position(9.0), PointerType::Touch)
    }

    fn cancel(pointer_id: PointerId) -> PointerEvent {
        make_cancel_event_for_id(pointer_id, PointerType::Touch)
    }

    fn enter(pointer_id: PointerId) -> PointerEvent {
        let PointerEvent::Cancel(info) = cancel(pointer_id) else {
            unreachable!("the cancel helper always constructs PointerEvent::Cancel")
        };
        PointerEvent::Enter(info)
    }

    fn positions(events: &[PointerEvent]) -> Vec<f32> {
        events
            .iter()
            .map(|event| event.position().dx.get())
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

    #[test]
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

    #[test]
    fn hover_and_contact_motion_do_not_cross_class_or_discrete_barriers() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(hover(id, 1.0));
        queue.borrow_mut().append(hover(id, 2.0));
        queue.borrow_mut().append(enter(id));
        queue.borrow_mut().append(hover(id, 3.0));
        queue.borrow_mut().append(hover(id, 4.0));
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 5.0));
        queue.borrow_mut().append(contact_move(id, 6.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(hover(id, 7.0));

        let events = drain(&queue);
        assert_eq!(positions(&events), vec![2.0, 0.0, 4.0, 0.0, 6.0, 9.0, 7.0]);
    }

    #[test]
    fn oldest_complete_sequence_is_evicted_without_reordering_other_pointers() {
        let queue = queue();
        let victim = pointer(2);
        queue.borrow_mut().append(down(victim));
        for raw in 3..=HELD_POINTER_CAPACITY as u64 + 1 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        queue.borrow_mut().append(up(victim));

        let events = drain(&queue);
        assert_eq!(events.len(), HELD_POINTER_CAPACITY - 1);
        assert!(
            events
                .iter()
                .all(|event| { flui_interaction::events::extract_pointer_id(event) != victim })
        );
        let ids: Vec<_> = events
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn cancel_completes_an_evictable_sequence() {
        let queue = queue();
        let victim = pointer(2);
        queue.borrow_mut().append(down(victim));
        for raw in 3..=HELD_POINTER_CAPACITY as u64 + 1 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        queue.borrow_mut().append(cancel(victim));

        assert_eq!(drain(&queue).len(), HELD_POINTER_CAPACITY - 1);
    }

    #[test]
    fn hard_cap_rejects_unique_incomplete_downs_at_sizes_1_32_and_256() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 + 32 {
            queue.borrow_mut().append(down(pointer(raw)));
            assert!(queue.borrow().len() <= HELD_POINTER_CAPACITY);
            if raw == 1 || raw == 32 || raw == HELD_POINTER_CAPACITY as u64 {
                assert_eq!(queue.borrow().len(), raw as usize);
            }
        }
        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        assert_eq!(queue.borrow().saturation_episodes(), 1);
    }

    #[test]
    fn saturated_incomplete_epochs_wholly_reject_hover_and_discrete_events() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        let hover_id = pointer(400);
        let discrete_id = pointer(401);
        queue.borrow_mut().append(hover(hover_id, 4.0));
        queue.borrow_mut().append(enter(discrete_id));

        let events = drain(&queue);
        assert_eq!(events.len(), HELD_POINTER_CAPACITY);
        assert!(events.iter().all(|event| {
            let id = flui_interaction::events::extract_pointer_id(event);
            id != hover_id && id != discrete_id
        }));
    }

    #[test]
    fn repeated_down_supersedes_the_whole_prior_open_epoch() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 1.0));
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 2.0));

        let events = drain(&queue);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], PointerEvent::Down(_)));
        assert_eq!(events[1].position().dx.get(), 2.0);
    }

    #[test]
    fn orphan_contact_move_up_and_cancel_are_dropped() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(contact_move(id, 1.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(cancel(id));
        assert!(drain(&queue).is_empty());
    }

    #[test]
    fn active_contact_followups_without_a_queued_down_are_preserved() {
        let queue = queue();
        let id = pointer(2);
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(id, 1.0), true);
        queue.borrow_mut().append_with_active_contact(up(id), true);

        let events = drain(&queue);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], PointerEvent::Move(_)));
        assert!(matches!(events[1], PointerEvent::Up(_)));
    }

    #[test]
    fn active_route_terminal_survives_when_incomplete_epochs_fill_capacity() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        let active_route = pointer(HELD_POINTER_CAPACITY as u64 + 1);
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        assert!(events.iter().any(|event| {
            matches!(event, PointerEvent::Up(_))
                && flui_interaction::events::extract_pointer_id(event) == active_route
        }));
        assert!(
            !events
                .iter()
                .any(|event| flui_interaction::events::extract_pointer_id(event) == pointer(1))
        );
    }

    #[test]
    fn active_route_terminal_survives_after_queued_repeated_down_at_capacity() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));
        for raw in 2..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        assert!(events.iter().any(|event| {
            matches!(event, PointerEvent::Up(_))
                && flui_interaction::events::extract_pointer_id(event) == active_route
        }));
        assert!(
            !events
                .iter()
                .any(
                    |event| flui_interaction::events::extract_pointer_id(event) == active_route
                        && matches!(event, PointerEvent::Down(_))
                )
        );
    }

    #[test]
    fn active_route_terminal_stays_protected_when_later_input_fills_capacity() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);
        for raw in 2..=HELD_POINTER_CAPACITY as u64 + 1 {
            queue.borrow_mut().append(down(pointer(raw)));
        }

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        assert!(events.iter().any(|event| {
            matches!(event, PointerEvent::Up(_))
                && flui_interaction::events::extract_pointer_id(event) == active_route
        }));
    }

    #[test]
    fn active_route_fallback_closes_after_terminal_is_queued() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(active_route, 1.0), true);

        let events = drain(&queue);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, PointerEvent::Move(_)))
        );
    }

    #[test]
    fn completed_replay_keeps_reentrant_active_terminal_protected() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);
        assert!(replay.next().is_none());
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(active_route, 1.0), true);

        let events = drain(&queue);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], PointerEvent::Up(_)));
        assert_eq!(
            flui_interaction::events::extract_pointer_id(&events[0]),
            active_route
        );
    }

    #[test]
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
            flui_interaction::events::extract_pointer_id(&events[0]),
            active_route
        );
    }

    #[test]
    fn reused_pointer_id_gets_a_fresh_protected_terminal_epoch() {
        let queue = queue();
        let reused = pointer(1);
        queue
            .borrow_mut()
            .append_with_active_contact(up(reused), true);
        queue.borrow_mut().append(down(reused));
        for raw in 2..HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(hover(pointer(raw), 1.0));
        }
        queue
            .borrow_mut()
            .append_with_active_contact(up(reused), true);

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        let reused_events: Vec<_> = events
            .iter()
            .filter(|event| flui_interaction::events::extract_pointer_id(event) == reused)
            .collect();
        assert_eq!(reused_events.len(), 3);
        assert!(matches!(reused_events[0], PointerEvent::Up(_)));
        assert!(matches!(reused_events[1], PointerEvent::Down(_)));
        assert!(matches!(reused_events[2], PointerEvent::Up(_)));
    }

    #[test]
    fn active_route_terminal_waiting_in_replay_closes_the_fallback() {
        let queue = queue();
        let unrelated = pointer(1);
        let active_route = pointer(2);
        queue.borrow_mut().append(hover(unrelated, 1.0));
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Move(_))));
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(active_route, 2.0), true);
        replay.for_each(drop);

        assert!(drain(&queue).is_empty());
    }

    #[test]
    fn older_unprotected_terminal_does_not_clear_replay_active_terminal_marker() {
        let queue = queue();
        let active_route = pointer(1);
        queue.borrow_mut().append(down(active_route));
        queue.borrow_mut().append(up(active_route));
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Up(_))));
        queue
            .borrow_mut()
            .append_with_active_contact(contact_move(active_route, 2.0), true);
        replay.for_each(drop);

        assert!(drain(&queue).is_empty());
    }

    #[test]
    fn active_route_terminal_evicts_hover_backlog_at_capacity() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(hover(pointer(raw), 1.0));
        }
        let active_route = pointer(HELD_POINTER_CAPACITY as u64 + 1);
        queue
            .borrow_mut()
            .append_with_active_contact(cancel(active_route), true);

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        assert!(events.iter().any(|event| {
            matches!(event, PointerEvent::Cancel(_))
                && flui_interaction::events::extract_pointer_id(event) == active_route
        }));
        assert!(
            !events
                .iter()
                .any(|event| flui_interaction::events::extract_pointer_id(event) == pointer(1))
        );
    }

    #[test]
    fn active_route_terminal_evicts_discrete_backlog_at_capacity() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(enter(pointer(raw)));
        }
        let active_route = pointer(HELD_POINTER_CAPACITY as u64 + 1);
        queue
            .borrow_mut()
            .append_with_active_contact(up(active_route), true);

        assert_eq!(queue.borrow().len(), HELD_POINTER_CAPACITY);
        let events = drain(&queue);
        assert!(events.iter().any(|event| {
            matches!(event, PointerEvent::Up(_))
                && flui_interaction::events::extract_pointer_id(event) == active_route
        }));
        assert!(
            !events
                .iter()
                .any(|event| flui_interaction::events::extract_pointer_id(event) == pointer(1))
        );
    }

    #[test]
    fn dropping_hovers_preserves_complete_and_incomplete_contacts() {
        let queue = queue();
        let complete = pointer(2);
        let incomplete = pointer(3);
        queue.borrow_mut().append(hover(pointer(4), 1.0));
        queue.borrow_mut().append(down(complete));
        queue.borrow_mut().append(contact_move(complete, 2.0));
        queue.borrow_mut().append(up(complete));
        queue.borrow_mut().append(down(incomplete));
        queue.borrow_mut().append(contact_move(incomplete, 3.0));
        queue.borrow_mut().drop_hovers();

        let events = drain(&queue);
        assert_eq!(events.len(), 5);
        assert!(events.iter().all(|event| !matches!(event, PointerEvent::Move(update) if update.current.buttons.is_empty())));
    }

    #[test]
    fn completed_replay_preserves_queued_behind_input_and_rearms_warning() {
        let queue = queue();
        for raw in 1..=HELD_POINTER_CAPACITY as u64 + 1 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(replay.next().is_some());
        queue.borrow_mut().append(hover(pointer(400), 4.0));
        replay.for_each(drop);
        assert_eq!(queue.borrow().len(), 1);
        assert!(!queue.borrow().is_replay_in_flight());

        for raw in 500..=500 + HELD_POINTER_CAPACITY as u64 {
            queue.borrow_mut().append(down(pointer(raw)));
        }
        assert_eq!(queue.borrow().saturation_episodes(), 2);
    }

    #[test]
    fn contact_followup_arriving_during_replay_queues_behind_its_down() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(down(id));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue.borrow_mut().append(contact_move(id, 5.0));
        assert!(replay.next().is_none());

        let events = drain(&queue);
        assert_eq!(positions(&events), vec![5.0]);
    }

    #[test]
    fn reentrant_down_discards_the_old_replay_epochs_undispatched_suffix() {
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
        queue.borrow_mut().append(up(superseded));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Move(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Up(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue.borrow_mut().append(down(superseded));
        let remainder: Vec<_> = replay.by_ref().collect();
        replay.complete();

        let replayed_ids: Vec<_> = remainder
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(replayed_ids, vec![interleaved, interleaved]);
        assert_eq!(positions(&remainder), vec![0.0, 2.0]);
        let queued_behind = drain(&queue);
        assert_eq!(queued_behind.len(), 1);
        assert!(matches!(queued_behind[0], PointerEvent::Down(_)));
        assert_eq!(
            flui_interaction::events::extract_pointer_id(&queued_behind[0]),
            superseded
        );
    }

    #[test]
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
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(restored_ids, vec![interleaved, interleaved, superseded]);
        assert_eq!(positions(&restored), vec![0.0, 2.0, 0.0]);
    }

    #[test]
    fn replay_supersession_stops_at_the_current_epochs_terminal() {
        let queue = queue();
        let id = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 1.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 2.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(down(interleaved));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue.borrow_mut().append(down(id));
        let remainder: Vec<_> = replay.by_ref().collect();
        replay.complete();

        let ids: Vec<_> = remainder
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(ids, vec![id, id, id, interleaved]);
        assert_eq!(positions(&remainder), vec![0.0, 2.0, 9.0, 0.0]);
    }

    #[test]
    fn abort_supersession_stops_at_the_current_epochs_terminal() {
        let queue = queue();
        let id = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(cancel(id));
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 2.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(down(interleaved));

        {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
            queue.borrow_mut().append(down(id));
        }

        let restored = drain(&queue);
        let ids: Vec<_> = restored
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(ids, vec![id, id, id, interleaved, id]);
        assert_eq!(positions(&restored), vec![0.0, 2.0, 9.0, 0.0, 0.0]);
    }

    #[test]
    fn reentrant_down_after_a_completed_only_epoch_discards_no_snapshot_event() {
        let queue = queue();
        let id = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 1.0));
        queue.borrow_mut().append(up(id));
        queue.borrow_mut().append(hover(id, 2.0));
        queue.borrow_mut().append(down(interleaved));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Move(_))));
        assert!(matches!(replay.next(), Some(PointerEvent::Up(_))));
        queue.borrow_mut().append(down(id));
        let remainder: Vec<_> = replay.by_ref().collect();
        replay.complete();

        let ids: Vec<_> = remainder
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(ids, vec![id, interleaved]);
        assert_eq!(positions(&remainder), vec![2.0, 0.0]);
    }

    #[test]
    fn reentrant_down_supersedes_a_wholly_undispatched_open_replay_epoch() {
        let queue = queue();
        let dispatched = pointer(2);
        let superseded = pointer(3);
        queue.borrow_mut().append(down(dispatched));
        queue.borrow_mut().append(down(superseded));
        queue.borrow_mut().append(contact_move(superseded, 4.0));
        queue.borrow_mut().append(up(dispatched));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert_eq!(
            replay
                .next()
                .as_ref()
                .map(flui_interaction::events::extract_pointer_id),
            Some(dispatched)
        );
        queue.borrow_mut().append(down(superseded));
        let remainder: Vec<_> = replay.by_ref().collect();
        replay.complete();

        assert_eq!(remainder.len(), 1);
        assert!(matches!(remainder[0], PointerEvent::Up(_)));
        assert_eq!(
            flui_interaction::events::extract_pointer_id(&remainder[0]),
            dispatched
        );
        let queued_behind = drain(&queue);
        assert_eq!(queued_behind.len(), 1);
        assert!(matches!(queued_behind[0], PointerEvent::Down(_)));
        assert_eq!(
            flui_interaction::events::extract_pointer_id(&queued_behind[0]),
            superseded
        );
    }

    #[test]
    fn abort_after_undispatched_epoch_supersession_restores_interleaving_before_new_down() {
        let queue = queue();
        let dispatched = pointer(2);
        let superseded = pointer(3);
        queue.borrow_mut().append(down(dispatched));
        queue.borrow_mut().append(down(superseded));
        queue.borrow_mut().append(contact_move(superseded, 4.0));
        queue.borrow_mut().append(cancel(dispatched));

        {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert_eq!(
                replay
                    .next()
                    .as_ref()
                    .map(flui_interaction::events::extract_pointer_id),
                Some(dispatched)
            );
            queue.borrow_mut().append(down(superseded));
        }

        let restored = drain(&queue);
        let restored_ids: Vec<_> = restored
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(restored_ids, vec![dispatched, superseded]);
        assert!(matches!(restored[0], PointerEvent::Cancel(_)));
        assert!(matches!(restored[1], PointerEvent::Down(_)));
    }

    #[test]
    fn reentrant_down_prefers_the_final_undispatched_epoch_over_a_dispatched_open_epoch() {
        let queue = queue();
        let repeated = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(repeated));
        queue.borrow_mut().append(down(interleaved));
        queue.borrow_mut().append(up(repeated));
        queue.borrow_mut().append(down(repeated));
        queue.borrow_mut().append(contact_move(repeated, 4.0));
        queue.borrow_mut().append(up(interleaved));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert_eq!(
            replay
                .next()
                .as_ref()
                .map(flui_interaction::events::extract_pointer_id),
            Some(repeated)
        );
        queue.borrow_mut().append(down(repeated));
        let remainder: Vec<_> = replay.by_ref().collect();
        replay.complete();

        let replayed_ids: Vec<_> = remainder
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(replayed_ids, vec![interleaved, repeated, interleaved]);
        assert_eq!(positions(&remainder), vec![0.0, 9.0, 9.0]);
        assert!(matches!(remainder[0], PointerEvent::Down(_)));
        assert!(matches!(remainder[1], PointerEvent::Up(_)));
        assert!(matches!(remainder[2], PointerEvent::Up(_)));

        let queued_behind = drain(&queue);
        assert_eq!(queued_behind.len(), 1);
        assert!(matches!(queued_behind[0], PointerEvent::Down(_)));
        assert_eq!(
            flui_interaction::events::extract_pointer_id(&queued_behind[0]),
            repeated
        );
    }

    #[test]
    fn abort_prefers_the_final_undispatched_epoch_and_restores_completed_input_first() {
        let queue = queue();
        let repeated = pointer(2);
        let interleaved = pointer(3);
        queue.borrow_mut().append(down(repeated));
        queue.borrow_mut().append(down(interleaved));
        queue.borrow_mut().append(cancel(repeated));
        queue.borrow_mut().append(down(repeated));
        queue.borrow_mut().append(contact_move(repeated, 4.0));
        queue.borrow_mut().append(cancel(interleaved));

        {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert_eq!(
                replay
                    .next()
                    .as_ref()
                    .map(flui_interaction::events::extract_pointer_id),
                Some(repeated)
            );
            queue.borrow_mut().append(down(repeated));
        }

        let restored = drain(&queue);
        let restored_ids: Vec<_> = restored
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(
            restored_ids,
            vec![interleaved, repeated, interleaved, repeated]
        );
        assert_eq!(positions(&restored), vec![0.0, 0.0, 0.0, 0.0]);
        assert!(matches!(restored[0], PointerEvent::Down(_)));
        assert!(matches!(restored[1], PointerEvent::Cancel(_)));
        assert!(matches!(restored[2], PointerEvent::Cancel(_)));
        assert!(matches!(restored[3], PointerEvent::Down(_)));
    }

    #[test]
    fn abort_replay_restores_undispatched_snapshot_before_reentrant_input() {
        let queue = queue();
        let first = pointer(2);
        let second = pointer(3);
        let reentrant = pointer(4);
        queue.borrow_mut().append(down(first));
        queue.borrow_mut().append(down(second));

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut replay =
                HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
            assert!(replay.next().is_some());
            queue.borrow_mut().append(down(reentrant));
            panic!("injected replay dispatch panic");
        }));
        assert!(panic.is_err());

        let events = drain(&queue);
        let ids: Vec<_> = events
            .iter()
            .map(flui_interaction::events::extract_pointer_id)
            .collect();
        assert_eq!(ids, vec![second, reentrant]);
        assert!(!queue.borrow().is_replay_in_flight());
    }

    #[test]
    fn replay_size_hint_is_conservative_while_reentrant_supersession_is_pending() {
        let queue = queue();
        let id = pointer(2);
        queue.borrow_mut().append(down(id));
        queue.borrow_mut().append(contact_move(id, 1.0));

        let mut replay = HeldPointerReplay::begin(&queue).expect("no replay is already in flight");
        assert!(matches!(replay.next(), Some(PointerEvent::Down(_))));
        queue.borrow_mut().append(down(id));

        assert_eq!(replay.size_hint(), (0, Some(1)));
        assert!(replay.next().is_none());
        replay.complete();
    }

    #[test]
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
}
