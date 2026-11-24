//! [`Timeline`]: events over time, exported as a Chrome / Perfetto trace.
//!
//! An application records its own work with [`Timeline::record_event`] (a guard that measures
//! until it drops), [`Timeline::record_instant`] and [`Timeline::record_completed_event`]; each
//! is a [`TimelineEventKind::Custom`] event. A timeline [over](Timeline::over) a
//! [`Capture`](crate::Capture) shows the framework's frames, presentation segments and phases
//! beside them on one time base, and [`Timeline::record_frame_snapshots`] adds a presentation's
//! frame snapshots. Every event says what it is through its [`TimelineEventKind`].
//!
//! # Example
//!
//! ```rust
//! use flui_devtools::{Timeline, TimelineEventKind};
//!
//! let timeline = Timeline::new();
//! {
//!     let _guard = timeline.record_event("load assets");
//!     // the work being measured
//! } // the event's duration is recorded here
//!
//! for event in timeline.events_of(TimelineEventKind::Custom) {
//!     println!("{}: {:.2} ms", event.name, event.duration_ms());
//! }
//!
//! // Open in chrome://tracing or ui.perfetto.dev.
//! let json = timeline.export_chrome_trace();
//! # drop(json);
//! ```

use std::collections::VecDeque;
use std::sync::Arc;

use flui_sdk::foundation::{PresentationAddress, PresentationId, UiRuntimeId};
use flui_sdk::hooks::FrameSnapshot;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use web_time::{Duration, Instant};

use crate::phase::Phase;

/// What a [`TimelineEvent`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TimelineEventKind {
    /// A whole frame: a scheduler frame from a [`Capture`](crate::Capture), or a frame snapshot
    /// from [`Timeline::record_frame_snapshots`].
    Frame,
    /// One presentation's segment of a captured frame.
    Segment,
    /// A phase of a captured frame.
    Phase(Phase),
    /// An event the application recorded.
    Custom,
}

impl TimelineEventKind {
    /// The kind as a trace viewer shows it: the event's `cat` in the Chrome trace. A phase is
    /// its [label](Phase::label).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Frame => "Frame",
            Self::Segment => "Segment",
            Self::Phase(phase) => phase.label(),
            Self::Custom => "Custom",
        }
    }
}

/// Returns the current thread's [`std::thread::ThreadId`].
///
/// Used as a serde `default` function for `TimelineEvent::thread_id`, which is
/// skipped during serialization and reconstructed on deserialization.
fn current_thread_id() -> std::thread::ThreadId {
    std::thread::current().id()
}

/// `duration.as_micros()` (`u128`) as a `u64`, saturating rather than
/// wrapping if it ever exceeds `u64::MAX` microseconds (~584,942 years —
/// never realistic for a real frame latency, but a silent wraparound would
/// be a far worse failure mode than an implausible ceiling: it could make
/// an enormous, clearly-bogus latency look like a small, plausible one in
/// an exported trace).
fn saturating_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// An id a [`TimelineEvent`] carries, written to JSON as its packed `u64` value.
trait PackedId: Copy {
    /// What the id names, for the error a value no id has reads back as.
    const NAMES: &'static str;

    fn pack(self) -> u64;

    fn unpack(packed: u64) -> Option<Self>;
}

impl PackedId for UiRuntimeId {
    const NAMES: &'static str = "UI runtime";

    fn pack(self) -> u64 {
        self.as_u64()
    }

    fn unpack(packed: u64) -> Option<Self> {
        Self::from_u64(packed)
    }
}

impl PackedId for PresentationId {
    const NAMES: &'static str = "presentation";

    fn pack(self) -> u64 {
        self.as_u64()
    }

    fn unpack(packed: u64) -> Option<Self> {
        Self::from_u64(packed)
    }
}

/// `#[serde(with)]` for an optional [`PackedId`]: the packed number, or `null`. The SDK builds
/// the ids without their `serde` feature, so the timeline writes the number they trace as.
mod packed_id {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::PackedId;

    #[expect(
        clippy::ref_option,
        reason = "serde's `with` hands the serializer a reference to the field"
    )]
    pub(super) fn serialize<I: PackedId, S: Serializer>(
        id: &Option<I>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        id.map(PackedId::pack).serialize(serializer)
    }

    pub(super) fn deserialize<'de, I: PackedId, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<I>, D::Error> {
        Option::<u64>::deserialize(deserializer)?
            .map(|packed| {
                I::unpack(packed).ok_or_else(|| {
                    serde::de::Error::custom(format!("{packed} is no packed {} id", I::NAMES))
                })
            })
            .transpose()
    }
}

/// The Chrome-trace `args` object of `event`: every key of `event.args` when it is a JSON
/// object (a frame snapshot's shape, see [`Timeline::record_frame_snapshots`]), then
/// `category`, the [label](TimelineEventKind::label) of the event's kind, and `presentation`,
/// the packed value of the presentation the event ran for, when it ran for one. An event the
/// application recorded carries `args: Value::Null`, which adds nothing.
///
/// `category` and `presentation` are inserted after the event's own keys, so they win over a
/// key of the same name: they always mean the event's kind and presentation. A frame
/// snapshot's own `presentation`, the id as text, is therefore the packed number here, as it is
/// on every other event of the presentation.
fn begin_event_args(event: &TimelineEvent) -> serde_json::Value {
    let mut args = serde_json::Map::new();
    if let serde_json::Value::Object(extra) = &event.args {
        for (key, value) in extra {
            args.insert(key.clone(), value.clone());
        }
    }
    args.insert("category".to_string(), json!(event.kind.label()));
    if let Some(presentation) = event.presentation {
        args.insert("presentation".to_string(), json!(presentation.as_u64()));
    }
    serde_json::Value::Object(args)
}

/// A single timeline event
///
/// `#[non_exhaustive]`: a future field is additive — this slice already
/// added one (`args`), a semver break for any external constructor; marking
/// it now stops the next addition from repeating that break.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TimelineEvent {
    /// Event name/description
    pub name: String,
    /// Start time (microseconds since timeline creation)
    pub start_micros: u128,
    /// Duration (microseconds)
    pub duration_micros: u128,
    /// What the event is.
    pub kind: TimelineEventKind,
    /// Extra structured data carried alongside this event — e.g. a
    /// frame-telemetry event's coalesced input ids and latencies (see
    /// [`Timeline::record_frame_snapshots`]). `Value::Null` for an event
    /// recorded through [`Timeline::record_event`]/[`Timeline::record_instant`],
    /// which carry no extra args of their own.
    #[serde(default)]
    pub args: serde_json::Value,
    /// Thread ID (for multi-threaded applications)
    ///
    /// Not serialized; restored to the deserializing thread's ID on load.
    #[serde(skip, default = "current_thread_id")]
    pub thread_id: std::thread::ThreadId,
    /// The UI runtime that drove the frame this event is part of: a span from a
    /// [`Capture`](crate::Capture), or a frame snapshot's. Application events carry none. The
    /// JSON export writes it as its packed value.
    #[serde(default, with = "packed_id")]
    pub runtime: Option<UiRuntimeId>,
    /// The presentation the event ran for, when it ran for one. The JSON export writes it as its
    /// packed value.
    #[serde(default, with = "packed_id")]
    pub presentation: Option<PresentationId>,
}

impl TimelineEvent {
    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> f64 {
        self.duration_micros as f64 / 1000.0
    }

    /// Get start time in milliseconds
    pub fn start_ms(&self) -> f64 {
        self.start_micros as f64 / 1000.0
    }

    /// Get duration as Duration, saturating values beyond its representable range.
    pub fn duration(&self) -> Duration {
        let Ok(seconds) = u64::try_from(self.duration_micros / 1_000_000) else {
            return Duration::MAX;
        };
        let nanos = (self.duration_micros % 1_000_000) as u32 * 1_000;
        Duration::new(seconds, nanos)
    }
}

/// Names one recorded event by mint order, which a trim of the oldest events does not change:
/// [`TimelineInner::index_of`] turns it into a position, or into `None` once the event was
/// trimmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EventHandle(u64);

/// RAII guard for recording an event
///
/// Automatically records the event duration when dropped.
#[must_use = "EventGuard does nothing if not held"]
#[derive(Debug)]
pub struct EventGuard {
    timeline: Arc<Mutex<TimelineInner>>,
    event_handle: EventHandle,
    start: Instant,
}

impl Drop for EventGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let mut inner = self.timeline.lock();
        inner.end_event(self.event_handle, duration);
    }
}

/// Internal timeline state
#[derive(Debug)]
struct TimelineInner {
    /// Timeline start time (for relative timestamps)
    start_time: Instant,
    /// The recorded events, oldest first; the oldest is dropped when `max_events` is exceeded.
    events: VecDeque<TimelineEvent>,
    /// Maximum number of events to keep
    max_events: usize,
    /// The [`EventHandle`] the next `push_event` mints.
    next_event_id: u64,
    /// The [`EventHandle`] of `events[0]`: events leave only from the front, so `events[i]` has
    /// the handle `base_event_id + i`, and [`TimelineEvent`] stores none.
    base_event_id: u64,
    /// Whether `start_time` is a capture's epoch, which [`TimelineInner::clear`] must keep: the
    /// capture's spans are measured from it.
    shared_epoch: bool,
}

impl TimelineInner {
    fn new(max_events: usize) -> Self {
        Self::starting_at(max_events, Instant::now(), false)
    }

    fn starting_at(max_events: usize, start_time: Instant, shared_epoch: bool) -> Self {
        Self {
            start_time,
            events: VecDeque::new(),
            max_events,
            next_event_id: 0,
            base_event_id: 0,
            shared_epoch,
        }
    }

    fn start_event(&mut self, name: String) -> EventHandle {
        let now = Instant::now();
        let start_micros = (now - self.start_time).as_micros();

        let event = TimelineEvent {
            name,
            start_micros,
            duration_micros: 0, // Will be filled in when event ends
            kind: TimelineEventKind::Custom,
            args: serde_json::Value::Null,
            thread_id: std::thread::current().id(),
            runtime: None,
            presentation: None,
        };

        self.push_event(event)
    }

    /// Push a fully formed event and drop the oldest ones beyond `max_events`, advancing
    /// `base_event_id` by the number dropped so every surviving handle still resolves.
    fn push_event(&mut self, event: TimelineEvent) -> EventHandle {
        let handle = EventHandle(self.next_event_id);
        self.next_event_id += 1;
        self.events.push_back(event);
        while self.events.len() > self.max_events {
            self.events.pop_front();
            self.base_event_id += 1;
        }
        handle
    }

    /// The position of `handle`'s event, or `None` when it is not (or no longer) recorded.
    fn index_of(&self, handle: EventHandle) -> Option<usize> {
        let offset = handle.0.checked_sub(self.base_event_id)?;
        usize::try_from(offset).ok()
    }

    fn end_event(&mut self, handle: EventHandle, duration: Duration) {
        let Some(index) = self.index_of(handle) else {
            return;
        };
        if let Some(event) = self.events.get_mut(index) {
            event.duration_micros = duration.as_micros();
        }
    }

    fn clear(&mut self) {
        self.events.clear();
        self.base_event_id = self.next_event_id;
        if !self.shared_epoch {
            self.start_time = Instant::now();
        }
    }
}

/// Timeline for recording and visualizing events
///
/// Thread-safe timeline that records events with precise timing.
/// Events can be exported to Chrome DevTools trace format for visualization.
///
/// Every event says what it is with its [`TimelineEventKind`]: what the application records
/// here is [`Custom`](TimelineEventKind::Custom), and a timeline [over](Self::over) a capture
/// adds the framework's frames, presentation segments and phases.
#[derive(Clone)]
pub struct Timeline {
    inner: Arc<Mutex<TimelineInner>>,
    /// The capture of framework spans this timeline is a view over, if any.
    #[cfg(feature = "profiling")]
    capture: Option<crate::Capture>,
}

impl Timeline {
    /// Create a new timeline
    ///
    /// Events will be kept in memory up to a default limit (10,000 events).
    pub fn new() -> Self {
        Self::bounded(10_000)
    }

    /// A timeline that keeps the newest `max_events` events recorded here: recording one more
    /// drops the oldest, and an [`EventGuard`] whose event was dropped closes nothing. Nothing
    /// is allocated up front.
    pub fn bounded(max_events: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TimelineInner::new(max_events))),
            #[cfg(feature = "profiling")]
            capture: None,
        }
    }

    /// A timeline that is a view over `capture`: the frames, presentation segments and phases the
    /// framework ran appear beside the events recorded here, on one time base, and
    /// [`Self::clear`] clears both. The capture is the only recorder of framework spans; this
    /// timeline keeps no copy of them.
    #[cfg(feature = "profiling")]
    #[must_use]
    pub fn over(capture: &crate::Capture) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TimelineInner::starting_at(
                10_000,
                capture.epoch(),
                true,
            ))),
            capture: Some(capture.clone()),
        }
    }
    /// Record an application event that lasts until the returned guard drops. The event is
    /// [`TimelineEventKind::Custom`].
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_devtools::Timeline;
    /// # let timeline = Timeline::new();
    /// {
    ///     let _guard = timeline.record_event("My Operation");
    ///     // Your code here
    /// } // Event duration recorded here
    /// ```
    pub fn record_event(&self, name: impl Into<String>) -> EventGuard {
        let name = name.into();
        let mut inner = self.inner.lock();
        let event_handle = inner.start_event(name);
        let start = Instant::now();

        EventGuard {
            timeline: self.inner.clone(),
            event_handle,
            start,
        }
    }

    /// Record an application event that happens at a point in time: a
    /// [`TimelineEventKind::Custom`] event whose duration is zero.
    pub fn record_instant(&self, name: impl Into<String>) {
        let name = name.into();
        let mut inner = self.inner.lock();
        let event_handle = inner.start_event(name);
        inner.end_event(event_handle, Duration::ZERO);
    }

    /// Record an application event whose timing is already known: a
    /// [`TimelineEventKind::Custom`] event. Unlike [`Self::record_event`],
    /// `start`/`duration` are supplied up front and this call never reads the
    /// wall clock itself. `start` is converted to this timeline's own
    /// relative-microseconds basis the same way `record_event` does.
    pub fn record_completed_event(
        &self,
        name: impl Into<String>,
        start: Instant,
        duration: Duration,
        args: serde_json::Value,
    ) {
        self.record_completed(
            name.into(),
            TimelineEventKind::Custom,
            start,
            duration,
            args,
            None,
        );
    }

    /// Record an event of `kind` whose timing is already known, for the presentation `address`
    /// names when given. `name` is converted by the caller, before the lock: a conversion may
    /// read or clear this timeline.
    fn record_completed(
        &self,
        name: String,
        kind: TimelineEventKind,
        start: Instant,
        duration: Duration,
        args: serde_json::Value,
        address: Option<PresentationAddress>,
    ) {
        let mut inner = self.inner.lock();
        let start_micros = start
            .saturating_duration_since(inner.start_time)
            .as_micros();
        inner.push_event(TimelineEvent {
            name,
            start_micros,
            duration_micros: duration.as_micros(),
            kind,
            args,
            thread_id: std::thread::current().id(),
            runtime: address.map(|address| address.ui_runtime_id),
            presentation: address.map(|address| address.presentation_id),
        });
    }

    /// Record every produced-frame [`FrameSnapshot`] in `snapshots` as one
    /// Chrome-trace-compatible timeline event per frame, each carrying its
    /// coalesced input ids and (present − arrival) latencies as trace args
    /// — issue #556's exportable, per-input-attributed frame telemetry.
    ///
    /// A presentation's frame clock produces the snapshots. That clock is
    /// internal to the app host, and no public API hands its snapshots out
    /// yet; the public source is the follow-up ADR-0088 names for move 4.
    /// Reuses this module's own [`Self::export_chrome_trace`] serializer;
    /// no second trace format exists anywhere in this crate.
    ///
    /// The event name embeds [`FrameSnapshot::address`], not just
    /// `frame_id`: a `frame_id` is scoped to the presentation whose clock
    /// minted it (each starts counting from 1), and a `PresentationId` is
    /// scoped to the UI runtime that minted it (ADR-0266 §3). Two
    /// presentations' snapshots recorded into the SAME `Timeline` — a UI
    /// runtime hosting more than one presentation, or two UI runtimes whose
    /// first presentations share an id — would otherwise collide in one
    /// exported trace file. The event's [`TimelineEvent::runtime`] and
    /// [`TimelineEvent::presentation`] are the address's, so the Chrome
    /// export puts the frame in its runtime's process, and its `args` carry
    /// both ids as text (`ui_runtime`, `presentation`) so a consumer of the
    /// JSON export can group by them without parsing the name back apart.
    pub fn record_frame_snapshots(&self, snapshots: &[FrameSnapshot]) {
        for snapshot in snapshots {
            let inputs: Vec<serde_json::Value> = snapshot
                .latencies()
                .map(|(id, latency)| {
                    json!({
                        "input_epoch_id": id.get(),
                        "latency_us": saturating_micros(latency),
                    })
                })
                .collect();
            let mut args = json!({
                "ui_runtime": snapshot.address.ui_runtime_id.to_string(),
                "presentation": snapshot.address.presentation_id.to_string(),
                "frame_id": snapshot.frame_id.to_string(),
                "present_outcome": format!("{:?}", snapshot.present_outcome),
                "inputs": inputs,
            });
            // A frame's GPU time arrives after its snapshot was recorded
            // (ADR-0388): a snapshot pulled before it carries none.
            if let Some(gpu) = snapshot.gpu_time {
                args["gpu_us"] = json!(saturating_micros(gpu));
            }
            self.record_completed(
                format!(
                    "Frame {} [{} {}]",
                    snapshot.frame_id,
                    snapshot.address.ui_runtime_id,
                    snapshot.address.presentation_id
                ),
                TimelineEventKind::Frame,
                snapshot.segment_start,
                snapshot.segment_span(),
                args,
                Some(snapshot.address),
            );
        }
    }

    /// Every recorded event, oldest first. A timeline [over](Self::over) a capture includes
    /// the framework spans the capture holds.
    pub fn events(&self) -> Vec<TimelineEvent> {
        self.collect_events(|_, _, _| true)
    }

    /// The events of `kind`, oldest first.
    pub fn events_of(&self, kind: TimelineEventKind) -> Vec<TimelineEvent> {
        self.collect_events(|event_kind, _, _| event_kind == kind)
    }

    /// The events that lie wholly inside `start..=end`, measured from the timeline's start,
    /// oldest first.
    pub fn events_between(&self, start: Duration, end: Duration) -> Vec<TimelineEvent> {
        let (start, end) = (start.as_micros(), end.as_micros());
        self.collect_events(|_, event_start, duration| {
            event_start >= start && event_start.saturating_add(duration) <= end
        })
    }

    /// The events `keep` accepts, given an event's kind, start and duration in microseconds,
    /// oldest first. A rejected event is never copied or built.
    fn collect_events(
        &self,
        keep: impl Fn(TimelineEventKind, u128, u128) -> bool,
    ) -> Vec<TimelineEvent> {
        #[cfg_attr(
            not(feature = "profiling"),
            expect(unused_mut, reason = "only a profiling build appends captured frames")
        )]
        let mut events: Vec<TimelineEvent> = self
            .inner
            .lock()
            .events
            .iter()
            .filter(|event| keep(event.kind, event.start_micros, event.duration_micros))
            .cloned()
            .collect();
        #[cfg(feature = "profiling")]
        if let Some(capture) = &self.capture {
            capture.visit_frames(|frame| {
                let span = frame.span();
                if keep(
                    captured_kind(span),
                    span.start().as_micros(),
                    span.duration().as_micros(),
                ) {
                    let mut event = captured_event(span);
                    if let Some(period) = frame.period() {
                        event.args["period_us"] = json!(saturating_micros(period));
                    }
                    events.push(event);
                }
                events.extend(
                    frame
                        .spans()
                        .iter()
                        .filter(|span| {
                            keep(
                                captured_kind(span),
                                span.start().as_micros(),
                                span.duration().as_micros(),
                            )
                        })
                        .map(captured_event),
                );
            });
            events.sort_by_key(|event| event.start_micros);
        }
        events
    }

    /// Clear all events
    pub fn clear(&self) {
        self.inner.lock().clear();
        #[cfg(feature = "profiling")]
        if let Some(capture) = &self.capture {
            capture.clear();
        }
    }

    /// The number of events [`Self::events`] returns, counted without copying any.
    pub fn event_count(&self) -> usize {
        #[cfg_attr(
            not(feature = "profiling"),
            expect(unused_mut, reason = "only a profiling build counts captured frames")
        )]
        let mut count = self.inner.lock().events.len();
        #[cfg(feature = "profiling")]
        if let Some(capture) = &self.capture {
            capture.visit_frames(|frame| count += 1 + frame.spans().len());
        }
        count
    }

    /// Export events as a Chrome / Perfetto trace.
    ///
    /// Returns a JSON string that opens in `chrome://tracing` and `ui.perfetto.dev`. Every
    /// event is one complete (`X`) event with its duration, so nesting is the events' own
    /// containment; a span the capture could not close never appears half-open. Each UI runtime
    /// is a process (named by metadata events) and each thread a thread of it. A timeline
    /// [over](Self::over) a capture adds, for every frame, the counters `frame_ms` and `jank`
    /// (`1` when the frame outlasted the display period it reported, 60 Hz when it reported
    /// none).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_devtools::Timeline;
    /// # let timeline = Timeline::new();
    /// let json = timeline.export_chrome_trace();
    /// // std::fs::write("trace.json", &json)?;
    /// # drop(json);
    /// // Then open chrome://tracing or ui.perfetto.dev and load trace.json
    /// ```
    pub fn export_chrome_trace(&self) -> String {
        let mut events = self.events();
        // Parents first among events that start together, so a viewer nests them.
        events.sort_by(|a, b| {
            a.start_micros
                .cmp(&b.start_micros)
                .then(b.duration_micros.cmp(&a.duration_micros))
        });

        let mut processes: Vec<Option<UiRuntimeId>> = Vec::new();
        let mut threads: Vec<(i64, std::thread::ThreadId)> = Vec::new();
        let mut trace_events: Vec<serde_json::Value> = Vec::new();
        let mut counters: Vec<serde_json::Value> = Vec::new();
        for event in &events {
            let process = processes
                .iter()
                .position(|runtime| *runtime == event.runtime)
                .unwrap_or_else(|| {
                    processes.push(event.runtime);
                    processes.len() - 1
                });
            let pid = process as i64 + 1;
            let thread = threads
                .iter()
                .position(|(owner, thread)| *owner == pid && *thread == event.thread_id)
                .unwrap_or_else(|| {
                    threads.push((pid, event.thread_id));
                    threads.len() - 1
                });
            let tid = thread as i64 + 1;
            trace_events.push(json!({
                "name": event.name,
                "cat": event.kind.label(),
                "ph": "X",
                "ts": saturating_micros_u128(event.start_micros),
                "dur": saturating_micros_u128(event.duration_micros),
                "pid": pid,
                "tid": tid,
                "args": begin_event_args(event),
            }));
            if event.kind == TimelineEventKind::Frame {
                // A frame that reported no display period is judged against 60 Hz, as the
                // profiler judges it.
                let period_us = event
                    .args
                    .get("period_us")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let frame_us = saturating_micros_u128(event.duration_micros);
                let end = saturating_micros_u128(event.start_micros + event.duration_micros);
                let line_us = if period_us == 0 { 16_667 } else { period_us };
                counters.push(json!({
                    "name": "frame_ms", "ph": "C", "ts": end, "pid": pid,
                    "args": { "frame_ms": frame_us as f64 / 1000.0 },
                }));
                counters.push(json!({
                    "name": "jank", "ph": "C", "ts": end, "pid": pid,
                    "args": { "jank": u8::from(frame_us > line_us) },
                }));
            }
        }

        let mut metadata: Vec<serde_json::Value> = Vec::new();
        for (index, runtime) in processes.iter().enumerate() {
            let name = match runtime {
                Some(runtime) => format!("FLUI runtime {:#x}", runtime.as_u64()),
                None => "FLUI".to_string(),
            };
            metadata.push(json!({
                "name": "process_name", "ph": "M", "pid": index as i64 + 1, "args": { "name": name },
            }));
        }
        for (index, (pid, thread)) in threads.iter().enumerate() {
            metadata.push(json!({
                "name": "thread_name", "ph": "M", "pid": pid, "tid": index as i64 + 1,
                "args": { "name": format!("{thread:?}") },
            }));
        }
        metadata.extend(trace_events);
        metadata.extend(counters);

        json!({
            "traceEvents": metadata,
            "displayTimeUnit": "ms",
            "otherData": {
                "version": "FLUI DevTools Timeline"
            }
        })
        .to_string()
    }

    /// Export events to a simple JSON format
    ///
    /// This is a simpler format than Chrome trace, useful for custom
    /// visualization.
    pub fn export_json(&self) -> String {
        let events = self.events();
        serde_json::to_string_pretty(&events).unwrap_or_default()
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Timeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeline")
            .field("event_count", &self.event_count())
            .finish_non_exhaustive()
    }
}

/// A `u128` count of microseconds as the `u64` a trace carries, saturating.
fn saturating_micros_u128(micros: u128) -> u64 {
    u64::try_from(micros).unwrap_or(u64::MAX)
}

/// What a captured span is as a timeline event.
#[cfg(feature = "profiling")]
fn captured_kind(span: &crate::capture::CapturedSpan) -> TimelineEventKind {
    match span.phase() {
        Some(phase) => TimelineEventKind::Phase(phase),
        None if span.is_frame() => TimelineEventKind::Frame,
        None => TimelineEventKind::Segment,
    }
}

/// A captured span as a timeline event.
#[cfg(feature = "profiling")]
fn captured_event(span: &crate::capture::CapturedSpan) -> TimelineEvent {
    let kind = captured_kind(span);
    let name = match kind {
        TimelineEventKind::Phase(phase) => phase.label(),
        TimelineEventKind::Frame => "Frame",
        _ => "Presentation",
    };
    TimelineEvent {
        name: name.to_string(),
        start_micros: span.start().as_micros(),
        duration_micros: span.duration().as_micros(),
        kind,
        args: json!({ "target": span.target(), "span": span.name() }),
        thread_id: span.thread(),
        runtime: span.partition().runtime(),
        presentation: span.partition().presentation(),
    }
}

// The one test reads frames from a `Capture`, which only `profiling` builds.
#[cfg(all(test, feature = "profiling"))]
mod tests {
    use super::*;
    use crate::Capture;
    use tracing::dispatcher::Dispatch;
    use tracing_subscriber::layer::SubscriberExt as _;

    /// A frame that reported no display period is still a `frame_ms` and `jank` counter.
    #[test]
    fn a_frame_without_a_display_period_still_gets_counters() {
        let capture = Capture::new();
        let timeline = Timeline::over(&capture);
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
        });

        let trace: serde_json::Value =
            serde_json::from_str(&timeline.export_chrome_trace()).expect("the export is JSON");
        let events = trace["traceEvents"].as_array().expect("traceEvents");
        for counter in ["frame_ms", "jank"] {
            assert!(
                events
                    .iter()
                    .any(|event| event["ph"] == "C" && event["name"] == counter),
                "no {counter} counter in {events:?}"
            );
        }
    }

    /// The JSON export reads back as the events it wrote, each with its kind and the typed ids
    /// of the runtime and presentation it ran for, written as their packed numbers.
    #[test]
    fn the_json_export_reads_back_with_each_events_kind_and_ids() {
        let generation = flui_sdk::foundation::generation::Generation::FIRST;
        let runtime = UiRuntimeId::new_gen(4, generation);
        let presentation = PresentationId::new_gen(2, generation);
        let capture = Capture::new();
        let timeline = Timeline::over(&capture);
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            let _frame = tracing::debug_span!(
                target: "flui.frame",
                "frame",
                runtime = runtime.as_u64()
            )
            .entered();
            let _segment = tracing::debug_span!(
                target: "flui.frame",
                "presentation",
                presentation = presentation.as_u64()
            )
            .entered();
            let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
        });
        timeline.record_instant("app work");

        let json = timeline.export_json();
        let written: Vec<serde_json::Value> = serde_json::from_str(&json).expect("JSON");
        assert!(
            written
                .iter()
                .any(|event| event["runtime"] == json!(runtime.as_u64())),
            "a runtime is written as its packed number: {json}"
        );
        let read: Vec<TimelineEvent> =
            serde_json::from_str(&json).expect("the export is the events");
        let mut events: Vec<_> = read
            .iter()
            .map(|event| (event.kind, event.runtime, event.presentation))
            .collect();
        events.sort_by_key(|(kind, ..)| kind.label());
        assert_eq!(
            events,
            [
                (TimelineEventKind::Custom, None, None),
                (TimelineEventKind::Frame, Some(runtime), None),
                (
                    TimelineEventKind::Phase(Phase::Layout),
                    Some(runtime),
                    Some(presentation)
                ),
                (
                    TimelineEventKind::Segment,
                    Some(runtime),
                    Some(presentation)
                ),
            ]
        );
    }

    /// A packed value no id has is refused on read, not read back as no id.
    #[test]
    fn a_json_event_naming_no_valid_runtime_is_refused() {
        let event = json!({
            "name": "x", "start_micros": 0, "duration_micros": 0, "kind": "Custom",
            "runtime": 0,
        });
        let read = serde_json::from_value::<TimelineEvent>(event);
        assert!(read.is_err(), "{read:?}");
    }
}
