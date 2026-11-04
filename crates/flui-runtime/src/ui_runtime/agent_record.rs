//! The recordings agents ask of a UI runtime (`record.start`, ADR-0288 §11).
//!
//! Each recording is a tap beside the application's own (ADR-0215 §1): it turns every row the
//! runtime delivers into the recording format and queues it for the agent's reader. The queue is
//! bounded, so a reader that falls behind costs the runtime nothing but its own rows: the rows
//! that do not fit are counted, and the next row that fits is preceded by a `gap` row naming how
//! many were lost. The `end` row always fits, and counts every row the recording lost, the tap's
//! and the queue's.
//!
//! A recording ends with its `end` row when its reader asks it to stop, when the window it was
//! started on closes, and when the runtime goes; the owner ends it at its next drain, the close
//! or its drop. Its reader going removes the tap with no row sent. A tap removed before its `end`
//! row (its registry already gone) ends the queue without one, so a reader never waits on a
//! recording nothing fills.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_protocol::input_recording::{self as rec, Row};
use flui_protocol::{RecordLayer, RecordParams, RecordRow, native_recording};
use flui_scheduler::ClockSource;
use flui_view::__runtime::{RecordSource, Reply, WakeSlot};
use flui_view::dev_agent::AgentRecording;
use parking_lot::{Condvar, Mutex};

use super::agent::{AgentError, window_of};
use super::{UiCommandSender, UiRuntime};
use crate::input_record::InputRecord;
use crate::input_tap::{InputTap, InputTapHandle};
use crate::recording_wire::Privacy;

/// The most recordings one UI runtime holds at once. One past it is refused with
/// [`AgentError::TooManyRecordings`]; one whose reader went no longer counts.
pub(crate) const MAX_RECORDINGS: usize = 8;

/// The most rows one recording's queue holds before it counts the rest as lost.
const QUEUE_ROWS: usize = 1024;

/// The most native rows one recording's queue holds: the rest of it is the input's, so a
/// backend's readings never crowd out the rows a replay needs first.
const NATIVE_ROWS: usize = 768;

#[derive(Default)]
struct QueueState {
    rows: VecDeque<RecordRow>,
    /// Input rows lost since the last input `gap` row was queued.
    missed: u64,
    /// Input rows the queue lost in all.
    lost: u64,
    /// Native lines queued now.
    native: usize,
    /// Per native stream, the rows lost since its last `gap` line was queued.
    native_missed: BTreeMap<u32, u64>,
    /// Nothing is queued after what is there: the `end` row went in, or the tap went without it.
    ended: bool,
}

impl QueueState {
    /// Queue the input `gap` row still owed, past the bound.
    fn owe_gap(&mut self) {
        if self.missed > 0 {
            let lost_rows = std::mem::take(&mut self.missed);
            self.rows
                .push_back(RecordRow::Input(Row::Gap { rows: lost_rows }));
        }
    }

    /// Queue every native stream's `gap` line still owed, past the bound: the recording ends.
    fn owe_native_gaps(&mut self) {
        for (stream, rows) in std::mem::take(&mut self.native_missed) {
            self.queue_native(stream, native_recording::Line::Gap { rows });
        }
    }

    fn queue_native(&mut self, stream: u32, line: native_recording::Line) {
        self.rows.push_back(RecordRow::Native { stream, line });
        self.native += 1;
    }

    fn native_full(&self) -> bool {
        self.native >= NATIVE_ROWS || self.rows.len() >= QUEUE_ROWS
    }

    /// Count a native row of `stream` lost.
    fn miss_native(&mut self, stream: u32) {
        let missed = self.native_missed.entry(stream).or_default();
        *missed = missed.saturating_add(1);
    }

    /// The oldest row queued.
    fn pop(&mut self) -> Option<RecordRow> {
        let row = self.rows.pop_front()?;
        if matches!(row, RecordRow::Native { .. }) {
            self.native -= 1;
        }
        Some(row)
    }
}

/// One recording's queue, shared by its tap, its reader and the runtime's list.
#[derive(Default)]
struct RecordQueue {
    state: Mutex<QueueState>,
    arrived: Condvar,
    /// The reader's waker.
    wake: WakeSlot,
    /// The reader asked the recording to end.
    stop: AtomicBool,
    /// The reader went.
    abandoned: AtomicBool,
}

impl RecordQueue {
    /// Queue input `row`, or count it lost when there is no room; whether the reader is to be
    /// woken.
    #[must_use]
    fn push(&self, row: Row) -> bool {
        let mut state = self.state.lock();
        if state.ended {
            return false;
        }
        if state.rows.len() >= QUEUE_ROWS {
            state.missed = state.missed.saturating_add(1);
            state.lost = state.lost.saturating_add(1);
            return false;
        }
        if state.missed > 0 {
            state.owe_gap();
            if state.rows.len() >= QUEUE_ROWS {
                // The gap took the last place: this row is lost too, and the next gap says so.
                state.missed = 1;
                state.lost = state.lost.saturating_add(1);
                drop(state);
                self.arrived.notify_one();
                return true;
            }
        }
        state.rows.push_back(RecordRow::Input(row));
        drop(state);
        self.arrived.notify_one();
        true
    }

    /// Queue `line` of native `stream`, or count it lost when the native layer's share of the
    /// queue is full; whether the reader is to be woken.
    #[must_use]
    fn push_native(&self, stream: u32, line: native_recording::Line) -> bool {
        let mut state = self.state.lock();
        if state.ended {
            return false;
        }
        if state.native_full() {
            state.miss_native(stream);
            return false;
        }
        if let Some(rows) = state.native_missed.remove(&stream) {
            state.queue_native(stream, native_recording::Line::Gap { rows });
            if state.native_full() {
                // The gap took the last place: this line is lost too.
                state.miss_native(stream);
                drop(state);
                self.arrived.notify_one();
                return true;
            }
        }
        state.queue_native(stream, line);
        drop(state);
        self.arrived.notify_one();
        true
    }

    /// Count a row the tap could not put in the recording's format as lost.
    fn lose(&self) {
        let mut state = self.state.lock();
        if !state.ended {
            state.missed = state.missed.saturating_add(1);
            state.lost = state.lost.saturating_add(1);
        }
    }

    /// Count a row of native `stream` the backend could not put in the format as lost.
    fn lose_native(&self, stream: u32) {
        let mut state = self.state.lock();
        if !state.ended {
            state.miss_native(stream);
        }
    }

    /// The tap's `end` row, which counts the `lost_rows` the tap lost: queued past the bound
    /// after the gaps still owed, counting the queue's losses too.
    fn end(&self, lost_rows: u64) {
        let mut state = self.state.lock();
        if state.ended {
            return;
        }
        state.owe_native_gaps();
        state.owe_gap();
        let lost_rows = lost_rows.saturating_add(state.lost);
        state
            .rows
            .push_back(RecordRow::Input(Row::End { lost_rows }));
        state.ended = true;
        drop(state);
        self.arrived.notify_all();
    }

    /// The tap went: a recording that has no `end` row by now gets none, and still says what it
    /// lost.
    fn close(&self) {
        let mut state = self.state.lock();
        if state.ended {
            return;
        }
        state.owe_native_gaps();
        state.owe_gap();
        state.ended = true;
        drop(state);
        self.arrived.notify_all();
    }

    fn is_ended(&self) -> bool {
        self.state.lock().ended
    }
}

/// The most setup lines kept per native stream for a recording that starts later.
const PREFIX_ROWS: usize = 64;

/// The forms of a native line a host makes for its recordings (ADR-0215 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeForms {
    /// With its text kept: a recording asked for `keep_text`.
    pub kept: bool,
    /// With its text redacted.
    pub redacted: bool,
}

/// Where a host's native journal sends its lines for the recordings of the `native` layer
/// (ADR-0288 §11): a host makes one for its backend (`NativeLines::default()`) and attaches it to
/// each runtime it hosts with [`UiRuntime::attach_native_journal`].
///
/// It hands each line to every recording of the layer, and keeps each stream's header and its
/// last [`PREFIX_ROWS`] setup lines, so a recording started later begins each stream with them,
/// after a `gap` when the bound let some go. The journal calls it on its own thread, so it is
/// shared behind a lock held only to queue or keep a line, never across a waker.
#[derive(Clone, Default)]
pub struct NativeLines(Arc<Mutex<NativeFanOut>>);

impl std::fmt::Debug for NativeLines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fan_out = self.0.lock();
        f.debug_struct("NativeLines")
            .field("streams", &fan_out.streams.len())
            .field("readers", &fan_out.readers.len())
            .finish()
    }
}

#[derive(Default)]
struct NativeFanOut {
    streams: BTreeMap<u32, Prefix>,
    readers: Vec<NativeReader>,
}

/// One recording of the `native` layer, as the fan-out sees it.
struct NativeReader {
    queue: Weak<RecordQueue>,
    privacy: Privacy,
}

/// What a recording that starts later is given of one stream: its header, the setup lines kept,
/// in both forms, and how many the bound let go.
#[derive(Default)]
struct Prefix {
    header: Option<Box<Forms>>,
    setup: VecDeque<Forms>,
    dropped: u64,
}

/// A line in both its forms.
struct Forms {
    kept: native_recording::Line,
    redacted: native_recording::Line,
}

impl Forms {
    fn of(&self, privacy: Privacy) -> &native_recording::Line {
        match privacy {
            Privacy::KeepText => &self.kept,
            Privacy::Redact => &self.redacted,
        }
    }
}

impl NativeLines {
    /// The forms of the next row that is not a setup row the recordings want now; a host makes
    /// no other.
    #[must_use]
    pub fn wants(&self) -> NativeForms {
        let mut fan_out = self.0.lock();
        fan_out.prune();
        let mut forms = NativeForms::default();
        for reader in &fan_out.readers {
            match reader.privacy {
                Privacy::KeepText => forms.kept = true,
                Privacy::Redact => forms.redacted = true,
            }
        }
        forms
    }

    /// The next line of native `stream`, in the forms made. A header or a `setup` line comes in
    /// both forms, and is kept for a recording that starts later.
    pub fn line(
        &self,
        stream: u32,
        setup: bool,
        kept: Option<&native_recording::Line>,
        redacted: Option<&native_recording::Line>,
    ) {
        let woken = {
            let mut fan_out = self.0.lock();
            fan_out.prune();
            if setup && let (Some(kept), Some(redacted)) = (kept, redacted) {
                fan_out.keep(stream, kept, redacted);
            }
            let mut woken = Vec::new();
            for reader in &fan_out.readers {
                let line = match reader.privacy {
                    Privacy::KeepText => kept,
                    Privacy::Redact => redacted,
                };
                if let (Some(queue), Some(line)) = (reader.queue.upgrade(), line)
                    && queue.push_native(stream, line.clone())
                {
                    woken.push(queue);
                }
            }
            woken
        };
        // Woken with the fan-out let go: a waker is the reader's code.
        for queue in woken {
            queue.wake.wake();
        }
    }

    /// A row of native `stream` the backend could not put in the format was lost: each
    /// recording of the layer has a gap there.
    pub fn lost(&self, stream: u32) {
        let mut fan_out = self.0.lock();
        fan_out.prune();
        for reader in &fan_out.readers {
            if let Some(queue) = reader.queue.upgrade() {
                queue.lose_native(stream);
            }
        }
    }

    /// Adds `queue` to the readers, after queuing it every stream's prefix.
    fn join(&self, queue: &Arc<RecordQueue>, privacy: Privacy) {
        let mut fan_out = self.0.lock();
        fan_out.prune();
        for (&stream, prefix) in &fan_out.streams {
            let Some(header) = &prefix.header else {
                continue;
            };
            let _ = queue.push_native(stream, header.of(privacy).clone());
            if prefix.dropped > 0 {
                let _ = queue.push_native(
                    stream,
                    native_recording::Line::Gap {
                        rows: prefix.dropped,
                    },
                );
            }
            for line in &prefix.setup {
                let _ = queue.push_native(stream, line.of(privacy).clone());
            }
        }
        fan_out.readers.push(NativeReader {
            queue: Arc::downgrade(queue),
            privacy,
        });
    }
}

impl NativeFanOut {
    /// Lets go of the readers whose recording ended.
    fn prune(&mut self) {
        self.readers.retain(|reader| {
            reader
                .queue
                .upgrade()
                .is_some_and(|queue| !queue.is_ended())
        });
    }

    /// Keeps a header or setup line of `stream` for a recording that starts later.
    fn keep(
        &mut self,
        stream: u32,
        kept: &native_recording::Line,
        redacted: &native_recording::Line,
    ) {
        let prefix = self.streams.entry(stream).or_default();
        let forms = Forms {
            kept: kept.clone(),
            redacted: redacted.clone(),
        };
        if matches!(kept, native_recording::Line::Header(_)) {
            prefix.header = Some(Box::new(forms));
            return;
        }
        prefix.setup.push_back(forms);
        if prefix.setup.len() > PREFIX_ROWS {
            prefix.setup.pop_front();
            prefix.dropped = prefix.dropped.saturating_add(1);
        }
    }
}

/// The tap a recording registers: each row in the recording's format, queued for its reader.
struct RecordingTap {
    queue: Arc<RecordQueue>,
    privacy: Privacy,
}

impl InputTap for RecordingTap {
    fn record(&self, record: &InputRecord) {
        let woken = match record {
            InputRecord::End { lost_rows } => {
                self.queue.end(*lost_rows);
                true
            }
            record => match record.to_wire(self.privacy) {
                Ok(row) => self.queue.push(row),
                Err(error) => {
                    self.queue.lose();
                    tracing::debug!(%error, "an agent's recording lost a row it cannot hold");
                    false
                }
            },
        };
        if woken {
            self.queue.wake.wake();
        }
    }
}

impl Drop for RecordingTap {
    fn drop(&mut self) {
        self.queue.close();
        self.queue.wake.wake();
    }
}

/// The reading half of a recording, behind the [`AgentRecording`] a window's `record` answers.
/// Dropping it removes the recording's tap at the owner's next drain.
struct RecordStream {
    queue: Arc<RecordQueue>,
    owner: UiCommandSender,
}

impl RecordSource for RecordStream {
    fn try_next(&mut self) -> Option<RecordRow> {
        self.queue.state.lock().pop()
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Option<RecordRow> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.queue.state.lock();
        loop {
            if let Some(row) = state.pop() {
                return Some(row);
            }
            if state.ended {
                return None;
            }
            match deadline {
                Some(deadline) => {
                    if self
                        .queue
                        .arrived
                        .wait_until(&mut state, deadline)
                        .timed_out()
                    {
                        return state.pop();
                    }
                }
                None => self.queue.arrived.wait(&mut state),
            }
        }
    }

    fn is_finished(&self) -> bool {
        let state = self.queue.state.lock();
        state.ended && state.rows.is_empty()
    }

    fn register(&mut self, waker: &std::task::Waker) {
        self.queue.wake.register(waker);
    }

    fn stop(&mut self) {
        if self.queue.stop.swap(true, Ordering::AcqRel) || self.queue.is_ended() {
            return;
        }
        // Recorded first: the wake is the platform's, and a stop it fails to deliver is taken at
        // the owner's next drain all the same.
        self.owner.wake_owner();
    }
}

impl Drop for RecordStream {
    fn drop(&mut self) {
        self.queue.abandoned.store(true, Ordering::Release);
        if !self.queue.is_ended() {
            self.owner.wake_owner();
        }
    }
}

/// One recording a UI runtime holds.
struct Held {
    /// The window it was started on: it ends when that window closes.
    presentation: PresentationId,
    queue: Arc<RecordQueue>,
    tap: InputTapHandle,
}

impl Held {
    /// Deliver what the runtime admitted for it, then its `end` row, and remove its tap.
    fn end(self) {
        if let Err(incomplete) = self.tap.finish() {
            tracing::debug!(
                lost_rows = incomplete.lost_rows,
                "an agent's recording ended without its end row"
            );
        }
    }
}

/// The recordings a UI runtime holds for agents. Owner thread only.
#[derive(Default)]
pub(crate) struct AgentRecordings(RefCell<Vec<Held>>);

impl AgentRecordings {
    /// Takes out the recordings `picked` selects, with no borrow held after.
    fn take(&self, mut picked: impl FnMut(&Held) -> bool) -> Vec<Held> {
        self.0
            .borrow_mut()
            .extract_if(.., |held| picked(held))
            .collect()
    }

    /// Ends every recording: the runtime goes. A recording whose reader went is removed with no
    /// row. The first panic is returned once every one has ended.
    pub(crate) fn end_all(&self) -> Option<Box<dyn std::any::Any + Send>> {
        let mut first = FirstPanic::new();
        for held in self.take(|_| true) {
            if held.queue.abandoned.load(Ordering::Acquire) {
                first.contain(|| drop(held));
            } else {
                first.contain(|| held.end());
            }
        }
        first.into_payload()
    }
}

impl UiRuntime {
    /// The owner half of `AgentWindow::record`: starts a recording of this runtime and answers
    /// with it.
    pub(super) fn start_agent_recording(
        &self,
        presentation: PresentationId,
        params: &RecordParams,
        reply: &Reply<AgentRecording>,
    ) {
        // Answered with the list let go: the answer wakes its asker, which is user code.
        let answer = self
            .agent_recording(presentation, params)
            .map_err(|error| error.into_error(window_of(presentation)));
        if !reply.send(answer) {
            // The recording, if one was made, dropped with the answer: its tap goes at the next
            // drain.
            tracing::trace!("dropping a development agent's recording nobody waits for");
        }
    }

    fn agent_recording(
        &self,
        presentation: PresentationId,
        params: &RecordParams,
    ) -> Result<AgentRecording, AgentError> {
        if self.presentations.get(presentation).is_none() {
            return Err(AgentError::PresentationGone);
        }
        if params.layers.is_empty() {
            return Err(AgentError::InvalidArgument {
                reason: "a recording names at least one layer",
            });
        }
        let native = params.layers.contains(&RecordLayer::Native);
        if native && !params.layers.contains(&RecordLayer::Input) {
            return Err(AgentError::InvalidArgument {
                reason: "a recording of `native` records `input` too",
            });
        }
        // The fan-out the recording joins once its tap is in, when it holds the native layer.
        let native_lines = if native {
            let Some(lines) = self.native_lines.borrow().clone() else {
                return Err(AgentError::NotSupported {
                    capability: "record",
                    reason: "this app journals no native layer: a debug build serving an agent does",
                });
            };
            Some(lines)
        } else {
            None
        };
        if params
            .layers
            .iter()
            .any(|layer| !matches!(layer, RecordLayer::Input | RecordLayer::Native))
        {
            return Err(AgentError::NotSupported {
                capability: "record",
                reason: "this build records the `input` and `native` layers only",
            });
        }
        // A recording whose reader went no longer counts: the drain's sweep removes it.
        let held = self
            .agent_recordings
            .0
            .borrow()
            .iter()
            .filter(|held| !held.queue.abandoned.load(Ordering::Acquire))
            .count();
        if held >= MAX_RECORDINGS {
            return Err(AgentError::TooManyRecordings);
        }
        let privacy = if params.keep_text {
            Privacy::KeepText
        } else {
            Privacy::Redact
        };
        let clock = match &self.clock {
            ClockSource::Manual(_) => rec::Clock::Virtual,
            _ => rec::Clock::Monotonic,
        };
        let header = self
            .recording_start()
            .to_wire(clock, privacy)
            .map_err(|_| AgentError::RecordingRefused {
                reason: "the runtime's windows or preferences hold a value the recording \
                         format does not",
            })?;
        let queue = Arc::new(RecordQueue::default());
        let tap = self
            .input_taps
            .install(RecordingTap {
                queue: Arc::clone(&queue),
                privacy,
            })
            .map_err(|_| AgentError::RecordingRefused {
                reason: "the runtime has registered every input tap it can",
            })?;
        if let Some(lines) = &native_lines {
            lines.join(&queue, privacy);
        }
        self.agent_recordings.0.borrow_mut().push(Held {
            presentation,
            queue: Arc::clone(&queue),
            tap,
        });
        Ok(flui_view::__runtime::agent_recording(
            header,
            RecordStream {
                queue,
                owner: self.sender_prototype.clone(),
            },
        ))
    }

    /// Ends the recordings whose reader asked to stop, with their `end` rows, and removes the
    /// ones whose reader went. The first panic is returned once every one has run.
    pub(super) fn sweep_agent_recordings(&self) -> Option<Box<dyn std::any::Any + Send>> {
        let gone = self
            .agent_recordings
            .take(|held| held.queue.abandoned.load(Ordering::Acquire));
        let stopped = self
            .agent_recordings
            .take(|held| held.queue.stop.load(Ordering::Acquire));
        let mut first = FirstPanic::new();
        for held in gone {
            first.contain(|| drop(held));
        }
        for held in stopped {
            first.contain(|| held.end());
        }
        first.into_payload()
    }

    /// Ends the recordings started on `presentation`, which closed, with their `end` rows. The
    /// first panic is returned once every one has ended.
    pub(super) fn end_agent_recordings_of(
        &self,
        presentation: PresentationId,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        let mut first = FirstPanic::new();
        for held in self
            .agent_recordings
            .take(|held| held.presentation == presentation)
        {
            first.contain(|| held.end());
        }
        first.into_payload()
    }
}

impl UiRuntime {
    /// Hands this runtime's recordings of the `native` layer (ADR-0288 §11) the lines of the
    /// host's native journal, which `lines` fans out. A host makes one [`NativeLines`] for its
    /// backend, installs the journal with it before it creates the platform, and attaches it to
    /// each runtime it hosts; a runtime nobody attached refuses a recording of `native`
    /// `not_supported`. Attaching again replaces the fan-out for later recordings.
    pub fn attach_native_journal(&self, lines: &NativeLines) {
        self.native_lines.replace(Some(lines.clone()));
    }
}
