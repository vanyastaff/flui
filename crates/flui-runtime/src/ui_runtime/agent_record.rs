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
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_protocol::input_recording::{self as rec, Row};
use flui_protocol::{RecordLayer, RecordParams};
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

#[derive(Default)]
struct QueueState {
    rows: VecDeque<Row>,
    /// Rows lost since the last `gap` row was queued.
    missed: u64,
    /// Rows the queue lost in all.
    lost: u64,
    /// Nothing is queued after what is there: the `end` row went in, or the tap went without it.
    ended: bool,
}

impl QueueState {
    /// Queue the `gap` row still owed, past the bound.
    fn owe_gap(&mut self) {
        if self.missed > 0 {
            let lost_rows = std::mem::take(&mut self.missed);
            self.rows.push_back(Row::Gap { rows: lost_rows });
        }
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
    /// Queue `row`, or count it lost when there is no room; whether the reader is to be woken.
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
        state.rows.push_back(row);
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

    /// The tap's `end` row, which counts the `lost_rows` the tap lost: queued past the bound
    /// after the gap still owed, counting the queue's losses too.
    fn end(&self, lost_rows: u64) {
        let mut state = self.state.lock();
        if state.ended {
            return;
        }
        state.owe_gap();
        let lost_rows = lost_rows.saturating_add(state.lost);
        state.rows.push_back(Row::End { lost_rows });
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
        state.owe_gap();
        state.ended = true;
        drop(state);
        self.arrived.notify_all();
    }

    fn is_ended(&self) -> bool {
        self.state.lock().ended
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
    fn try_next(&mut self) -> Option<Row> {
        self.queue.state.lock().rows.pop_front()
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Option<Row> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.queue.state.lock();
        loop {
            if let Some(row) = state.rows.pop_front() {
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
                        return state.rows.pop_front();
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
        if params
            .layers
            .iter()
            .any(|layer| *layer != RecordLayer::Input)
        {
            return Err(AgentError::NotSupported {
                capability: "record",
                reason: "this build records the `input` layer only",
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
