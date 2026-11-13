//! The input a window was delivered, as an agent reads it (`input.journal`, the `input` event;
//! ADR-0288 §9).
//!
//! Once an agent has been handed the window ([`UiRuntime::dev_agent_window`]), every input the
//! window takes is journaled where it is routed, the platform's and the agent's alike, the
//! agent's marked `injected`. Each entry takes the next sequence number, from 1, and goes out to
//! the window's subscribers as an `input` event before the app handles it, so a handler that
//! panics does not keep its input out of the journal. The journal holds the last
//! [`JOURNAL_ENTRIES`] and counts, for a reader, the ones it no longer holds.
//!
//! Input that is not a step a driver plays (a pointer entering or leaving, a device added, a
//! drag from another app, an input method turning on) is routed without an entry.

use std::collections::VecDeque;

use flui_interaction::PointerEventExt;
use flui_platform::keyboard::{KeyEvent, KeyState};
use flui_platform::pointer::{ButtonChange, PointerEvent, PointerKind};
use flui_platform::{EventTime, ImeEvent, PlatformInput};
use flui_protocol::{
    DeviceClass, Event, InputEvent, InputKind, Journal, JournalEntry, JournalParams, LogicalPoint,
};

use super::UiRuntime;
use crate::presentation::PresentationState;

/// The most entries a window's journal holds.
pub(crate) const JOURNAL_ENTRIES: usize = 4096;

/// A window's input journal: the entries it still holds, oldest first.
#[derive(Debug, Default)]
pub(crate) struct InputJournal {
    entries: VecDeque<JournalEntry>,
    /// The sequence number the last entry took; 0 before the first.
    last: u64,
}

impl InputJournal {
    /// Journals `step` as the next entry.
    fn note(&mut self, step: Step, injected: bool) -> JournalEntry {
        self.last = self
            .last
            .checked_add(1)
            .expect("BUG: a window journals fewer than 2^64 inputs");
        let mut entry = JournalEntry::new(self.last, step.device, step.kind, step.time_us)
            .with_injected(injected);
        if let Some(at) = step.at {
            entry = entry.with_position(at);
        }
        if self.entries.len() == JOURNAL_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry.clone());
        entry
    }

    /// The entries after `since`, at most `max_entries`, oldest first, with how many after
    /// `since` it no longer holds.
    fn read(&self, since: Option<u64>, max_entries: Option<u32>) -> Journal {
        let since = since.unwrap_or(0);
        let first_held = self
            .entries
            .front()
            .map_or(self.last.saturating_add(1), |entry| entry.seq);
        let dropped = first_held.saturating_sub(since.saturating_add(1));
        let max = max_entries.map_or(usize::MAX, |max| usize::try_from(max).unwrap_or(usize::MAX));
        // Sequence numbers are consecutive, so the first entry after `since` is found by its
        // offset from the first one held.
        let skip = usize::try_from(since.saturating_add(1).saturating_sub(first_held))
            .unwrap_or(usize::MAX);
        Journal::new(
            self.entries.iter().skip(skip).take(max).cloned().collect(),
            dropped,
        )
    }

    /// The sequence number the last entry took; 0 before the first.
    pub(super) const fn last_seq(&self) -> u64 {
        self.last
    }
}

/// One entry, before it takes its sequence number.
#[derive(Debug, Clone, Copy)]
struct Step {
    device: DeviceClass,
    kind: InputKind,
    at: Option<LogicalPoint>,
    time_us: u64,
}

impl Step {
    fn new(device: DeviceClass, kind: InputKind, time: EventTime) -> Self {
        Self {
            device,
            kind,
            at: None,
            time_us: time.as_nanos() / 1_000,
        }
    }
}

/// The entries `input` journals as; `now` stamps the input that carries no time of its own.
fn steps_of(input: &PlatformInput, now: impl Fn() -> EventTime) -> Vec<Step> {
    match input {
        PlatformInput::Pointer(event) => pointer_step(event, &now).into_iter().collect(),
        PlatformInput::PointerFrame(frame) => frame
            .events()
            .iter()
            .filter_map(|event| pointer_step(event, &now))
            .collect(),
        PlatformInput::Keyboard(key) => vec![key_step(key)],
        PlatformInput::Ime(ImeEvent::Commit(_)) => {
            vec![Step::new(DeviceClass::Ime, InputKind::Text, now())]
        }
        PlatformInput::Ime(ImeEvent::Preedit { .. }) => {
            vec![Step::new(DeviceClass::Ime, InputKind::Composition, now())]
        }
        PlatformInput::Ime(_) | PlatformInput::DragDrop(_) | PlatformInput::Capabilities(_) => {
            Vec::new()
        }
    }
}

fn key_step(key: &KeyEvent) -> Step {
    let kind = match key.state() {
        KeyState::Down => InputKind::KeyDown,
        KeyState::Up => InputKind::KeyUp,
    };
    Step::new(DeviceClass::Keyboard, kind, key.time)
}

fn pointer_step(event: &PointerEvent, now: &impl Fn() -> EventTime) -> Option<Step> {
    let kind = match event {
        PointerEvent::Down(_) | PointerEvent::ButtonChange(ButtonChange::Pressed(_)) => {
            InputKind::PointerDown
        }
        PointerEvent::Up(_) | PointerEvent::ButtonChange(ButtonChange::Released(_)) => {
            InputKind::PointerUp
        }
        PointerEvent::Move(_) => InputKind::PointerMove,
        PointerEvent::Cancel(_) => InputKind::PointerCancel,
        PointerEvent::Scroll(_) => InputKind::Scroll,
        PointerEvent::PanZoom(_) | PointerEvent::SmartZoom(_) => InputKind::Gesture,
        // Entering, leaving, a revision, a squeeze, a device coming or going: not a step a
        // driver plays.
        _ => return None,
    };
    let device = match event.pointer_kind() {
        Some(PointerKind::Mouse) => DeviceClass::Mouse,
        Some(PointerKind::Touch) => DeviceClass::Touch,
        Some(PointerKind::Pen { .. }) => DeviceClass::Pen,
        Some(PointerKind::Trackpad) => DeviceClass::Trackpad,
        Some(_) | None => DeviceClass::Other,
    };
    let mut step = Step::new(device, kind, event.time().unwrap_or_else(now));
    step.at = event
        .position()
        .map(|offset| LogicalPoint::new(offset.dx, offset.dy));
    Some(step)
}

impl UiRuntime {
    /// Journals `input`, routed to `state`, once an agent has been handed the window, and sends
    /// each entry to the window's subscribers.
    pub(super) fn journal_input(
        &self,
        state: &PresentationState,
        input: &PlatformInput,
        injected: bool,
    ) {
        let entries = {
            let mut slot = state.dev_agent.borrow_mut();
            let Some(slot) = slot.as_mut() else {
                return;
            };
            let steps = steps_of(input, || {
                self.event_time_at(flui_foundation::MonotonicClock::now(&self.clock))
            });
            steps
                .into_iter()
                .map(|step| slot.journal.note(step, injected))
                .collect::<Vec<JournalEntry>>()
        };
        let window = super::agent::window_of(state.id());
        for entry in entries {
            state.emit_agent_event(Event::Input(InputEvent::new(window, entry)));
        }
    }
}

/// What `input.journal` answers for `state`.
pub(super) fn read(state: &PresentationState, params: &JournalParams) -> Journal {
    state
        .dev_agent
        .borrow()
        .as_ref()
        .map_or_default(|slot| slot.journal.read(params.since, params.max_entries))
}

/// The sequence number of `state`'s last journal entry; 0 before the first.
pub(super) fn last_seq(state: &PresentationState) -> u64 {
    state
        .dev_agent
        .borrow()
        .as_ref()
        .map_or(0, |slot| slot.journal.last_seq())
}
