//! One capture of the framework's frame spans, shared by every consumer.
//!
//! # One stream
//!
//! `flui-devtools` is a package: nothing in the framework may depend on it, so the framework
//! emits `tracing` spans and the tools read them. [`CaptureLayer`] is the one reader. It
//! recognizes a frame, the spans of its presentations and its [`Phase`]s, measures each from its
//! outermost entered visit, and keeps them in a [`Capture`]. The
//! [`Profiler`](crate::Profiler), the [`Timeline`](crate::Timeline) and the Chrome/Perfetto
//! export read that capture; none of them has a recorder of its own for framework spans.
//!
//! # Partitions
//!
//! A frame belongs to the UI runtime that drove it, and a span inside it to the presentation it
//! ran for. Neither is guessed: the scheduler's `frame` span carries a `runtime` field, and the
//! runtime opens a `presentation` span with a `presentation` field around each presentation's
//! segment. A span inherits its [`Partition`] from its ancestors, so two runtimes rendering at
//! once under one subscriber are two partitions rather than one blended measurement, and a
//! span with no known owner is kept apart as [`Partition::UNKNOWN`]'s runtime.
//!
//! # What is captured
//!
//! Only spans inside a `frame` span count. A phase span outside any frame, such as a headless
//! bootstrap or a one-off layout pass, is not a frame, and attributing it to one would be a
//! fabricated measurement. A phase is recognized by [`Phase::of`], so an application span named
//! `paint` is not captured. A phase reentered inside the same phase is covered by the outer
//! extent, and a span retained after it closed cannot restart a measurement.
//!
//! # Produced frames
//!
//! Beside the spans, the layer reads the two `TRACE` events the runtime traces for each frame a
//! presentation produces: `frame_telemetry` (its outcome, phase durations, produce-to-present
//! span and inputs) and `frame_gpu_time` (its GPU time, which arrives later and is joined to the
//! frame by runtime, presentation and frame number). Each is a [`ProducedFrame`], kept in its
//! runtime's ring. They are the only events the layer admits.

mod telemetry;

pub use telemetry::{FrameOutcome, ProducedFrame};

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use flui_sdk::foundation::panic::{FirstPanic, payload_text};
use flui_sdk::foundation::{AtomicIdMint, PresentationId, UiRuntimeId};
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::filter::{FilterFn, Filtered, filter_fn};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::{LookupSpan, SpanRef};
use web_time::{Duration, Instant};

use crate::phase::Phase;

/// The target of the framework's frame and segment spans.
const FRAME_TARGET: &str = "flui.frame";
/// The span the scheduler opens around every frame.
const FRAME_SPAN: &str = "frame";
/// The span the runtime opens around one presentation's segment of a frame.
const SEGMENT_SPAN: &str = "presentation";

fn is_frame(metadata: &Metadata<'_>) -> bool {
    metadata.is_span() && metadata.target() == FRAME_TARGET && metadata.name() == FRAME_SPAN
}

fn is_segment(metadata: &Metadata<'_>) -> bool {
    metadata.is_span() && metadata.target() == FRAME_TARGET && metadata.name() == SEGMENT_SPAN
}

/// What a capture layer admits: frames, segments, phases, and the animation controller
/// identities that keep an animation span's ancestry visible to the layer; and of the events
/// only the frame telemetry, with the `enabled!` gate the runtime asks before tracing it.
fn is_captured(metadata: &Metadata<'_>) -> bool {
    let span = metadata.is_span()
        && (is_frame(metadata)
            || is_segment(metadata)
            || Phase::of(metadata).is_some()
            || (metadata.target() == "flui.animation.diagnostics"
                && metadata.name() == "animation_controller"));
    span || telemetry::is_telemetry(metadata) || telemetry::is_telemetry_gate(metadata)
}

/// The filter a capture layer carries: the framework's frame spans and its frame telemetry.
pub type FrameSpanFilter = FilterFn<fn(&Metadata<'_>) -> bool>;

/// Whose spans these are: the UI runtime that drove the frame and, for a span that ran for one
/// presentation, that presentation.
///
/// A partition with no presentation covers every presentation of its runtime when used to query
/// a [`Capture`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Partition {
    runtime: Option<UiRuntimeId>,
    presentation: Option<PresentationId>,
}

impl Partition {
    /// Spans whose frame did not say which runtime drove it.
    pub const UNKNOWN: Self = Self {
        runtime: None,
        presentation: None,
    };

    /// A partition of `runtime`, narrowed to `presentation` when given.
    #[must_use]
    pub const fn new(runtime: Option<UiRuntimeId>, presentation: Option<PresentationId>) -> Self {
        Self {
            runtime,
            presentation,
        }
    }

    /// The runtime that drove the frame, if the frame said.
    #[must_use]
    pub const fn runtime(&self) -> Option<UiRuntimeId> {
        self.runtime
    }

    /// The presentation the span ran for, if it ran for one.
    #[must_use]
    pub const fn presentation(&self) -> Option<PresentationId> {
        self.presentation
    }

    /// Whether `other` is this partition or lies inside it.
    #[must_use]
    pub fn covers(&self, other: &Self) -> bool {
        self.runtime == other.runtime
            && (self.presentation.is_none() || self.presentation == other.presentation)
    }
}

/// One closed span: a frame, one presentation's segment of it, or a [`Phase`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CapturedSpan {
    target: &'static str,
    name: &'static str,
    phase: Option<Phase>,
    partition: Partition,
    start: Duration,
    duration: Duration,
    thread: ThreadId,
}

impl CapturedSpan {
    /// The span's tracing target.
    #[must_use]
    pub const fn target(&self) -> &'static str {
        self.target
    }

    /// The span's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The phase the span measures; `None` for a frame or a segment.
    #[must_use]
    pub const fn phase(&self) -> Option<Phase> {
        self.phase
    }

    /// Whose span this is.
    #[must_use]
    pub const fn partition(&self) -> Partition {
        self.partition
    }

    /// When the span's outermost visit began, from the capture's epoch.
    #[must_use]
    pub const fn start(&self) -> Duration {
        self.start
    }

    /// How long the outermost visit lasted.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// The thread that entered the span.
    #[must_use]
    pub const fn thread(&self) -> ThreadId {
        self.thread
    }

    /// Whether this is a frame span.
    #[must_use]
    pub fn is_frame(&self) -> bool {
        self.target == FRAME_TARGET && self.name == FRAME_SPAN
    }
}

/// One closed frame and the spans that ran inside it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CapturedFrame {
    span: CapturedSpan,
    period: Option<Duration>,
    spans: Vec<CapturedSpan>,
    outermost: bool,
}

impl CapturedFrame {
    /// The frame span itself.
    #[must_use]
    pub const fn span(&self) -> &CapturedSpan {
        &self.span
    }

    /// The display period the frame reported, which the jank line follows.
    #[must_use]
    pub const fn period(&self) -> Option<Duration> {
        self.period
    }

    /// The segments and phases that ran inside the frame, in the order they ended.
    #[must_use]
    pub fn spans(&self) -> &[CapturedSpan] {
        &self.spans
    }

    /// Whether no other frame was open around this one on its thread. A frame opened inside
    /// another, such as a nested runtime's, is captured but is not what a profiler follows.
    #[must_use]
    pub const fn is_outermost(&self) -> bool {
        self.outermost
    }
}

type FrameSink = Arc<dyn Fn(&Capture, &CapturedFrame) + Send + Sync>;

/// Names one sink among its capture's sinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SinkId(NonZeroU64);

/// A frame sink's registration on a [`Capture`], made by [`Capture::on_frame`]. The sink runs
/// on every frame the capture closes until this handle drops, and is removed then.
///
/// The handle holds its capture weakly: it never keeps a capture alive, and once the capture is
/// gone dropping it does nothing. A removal that lands while a frame is being delivered does
/// not change that delivery, which runs the sinks it started with; no later delivery calls the
/// removed sink.
///
/// It is not `flui_foundation`'s `Subscription`, which belongs to one owner thread (`Rc`). A
/// frame closes on whichever thread closes its tracing span, so a capture's sinks are shared by
/// every thread that emits, and this handle is `Send + Sync`.
#[must_use = "the sink is removed when this handle drops"]
pub struct CaptureSubscription {
    capture: Weak<Shared>,
    sink: SinkId,
}

impl Drop for CaptureSubscription {
    fn drop(&mut self) {
        let Some(shared) = self.capture.upgrade() else {
            return;
        };
        let removed = {
            let mut sinks = shared.sinks.lock();
            sinks
                .iter()
                .position(|(id, _)| *id == self.sink)
                .map(|at| sinks.remove(at))
        };
        // The sink's captures' `Drop` is user code: it runs after the lock is released, and
        // only here when no delivery still holds the sink.
        drop(removed);
    }
}

impl std::fmt::Debug for CaptureSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureSubscription")
            .field("live", &(self.capture.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

/// Calls every sink with `frame`, in the order they were registered.
///
/// A sink is user code, and so is the `Drop` of one removed while this delivery ran, whose last
/// reference this delivery holds. Each runs under one [`FirstPanic`], so a sink that panics
/// does not take the frame from the sinks after it. The delivery runs inside a tracing layer:
/// unwinding out of it would stop the layers after this one from seeing the span exit, so the
/// first panic is reported as an error event, itself contained because a subscriber is user
/// code too, and not raised. Every payload is destroyed under containment (ADR-0281 §3).
fn deliver(capture: &Capture, sinks: Vec<FrameSink>, frame: &CapturedFrame) {
    let mut failures = FirstPanic::new();
    for sink in sinks {
        failures.contain(|| sink(capture, frame));
        failures.retire(sink);
    }
    let Some(text) = failures.held().map(|payload| {
        payload_text(payload)
            .unwrap_or("a non-text panic payload")
            .to_owned()
    }) else {
        return;
    };
    failures.contain(|| {
        tracing::error!(target: "flui.devtools", "a capture frame sink panicked: {text}");
    });
    // Reported, not raised: dropping the collector destroys the held payload, and any the
    // report raised, with `discard_payload`.
    drop(failures);
}

/// What one runtime has captured.
#[derive(Default)]
struct Ring {
    spans: VecDeque<CapturedSpan>,
    frames: VecDeque<CapturedFrame>,
    produced: VecDeque<ProducedFrame>,
}

/// Which capture a value in a span's extensions belongs to: two captures on one registry see
/// the same spans, and a span can outlive the capture that filed state in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureId(NonZeroU64);

/// The identities of every capture the process creates. A private ID counter: it mints and
/// never wraps, so a capture created after another was dropped never reads the state the
/// dropped one left in a long-lived span.
static CAPTURE_IDS: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

struct Shared {
    id: CaptureId,
    epoch: Instant,
    max_frames: usize,
    max_spans: usize,
    rings: Mutex<FxHashMap<Option<UiRuntimeId>, Ring>>,
    sinks: Mutex<Vec<(SinkId, FrameSink)>>,
    sink_ids: AtomicIdMint<NonZeroU64>,
}

/// How much of each runtime a [`Capture`] keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    /// Frames kept per runtime, and as many of its [`ProducedFrame`]s.
    pub frames: usize,
    /// Spans kept per runtime, which also bound the child spans the retained frames hold.
    pub spans: usize,
}

impl Default for CaptureLimits {
    /// 600 frames and 20,000 spans.
    fn default() -> Self {
        Self {
            frames: 600,
            spans: 20_000,
        }
    }
}

/// The spans of every frame the framework has driven under a [`CaptureLayer`], bounded per
/// runtime.
///
/// A handle is cheap to clone and every clone reads the same capture.
#[derive(Clone)]
pub struct Capture {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("max_frames", &self.shared.max_frames)
            .field("max_spans", &self.shared.max_spans)
            .finish_non_exhaustive()
    }
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture {
    /// A capture keeping the last [`CaptureLimits::default`] frames and spans of each runtime.
    ///
    /// # Panics
    ///
    /// After 2^64 - 1 captures in one process, as [`Self::bounded`] says.
    #[must_use]
    pub fn new() -> Self {
        Self::bounded(CaptureLimits::default())
    }

    /// A capture keeping the last `limits.frames` frames and `limits.spans` spans of each
    /// runtime. A capture of one runtime never evicts another's.
    ///
    /// # Panics
    ///
    /// After 2^64 - 1 captures in one process, which a process creating one a nanosecond
    /// reaches in 584 years; the identity mint never wraps, so no two captures share one.
    #[must_use]
    pub fn bounded(limits: CaptureLimits) -> Self {
        let id = CAPTURE_IDS
            .next()
            .expect("BUG: a process creates fewer than 2^64 - 1 captures");
        Self {
            shared: Arc::new(Shared {
                id: CaptureId(id),
                epoch: Instant::now(),
                max_frames: limits.frames,
                max_spans: limits.spans,
                rings: Mutex::new(FxHashMap::default()),
                sinks: Mutex::new(Vec::new()),
                sink_ids: AtomicIdMint::new(),
            }),
        }
    }

    /// The instant every [`CapturedSpan::start`] is measured from.
    #[must_use]
    pub fn epoch(&self) -> Instant {
        self.shared.epoch
    }

    /// What identifies this capture in the extensions of the spans it watches.
    fn owner(&self) -> CaptureId {
        self.shared.id
    }

    /// The layer that fills this capture: the one `tracing` layer every devtools reader of
    /// framework spans is fed through.
    ///
    /// It reads these spans:
    ///
    /// | span | opened by | read as |
    /// |------|-----------|---------|
    /// | `frame` (target `flui.frame`) | `UpdateScheduler::drive_frame` (`flui-scheduler`) | the frame; its `runtime` field names the runtime that drove it, its `period_us` field the display period the jank line follows |
    /// | `presentation` (target `flui.frame`) | the UI runtime, around one presentation's segment | the [`Partition`] of the spans inside it, from its `presentation` field |
    /// | `build`, `layout`, `compositing`, `paint`, `semantics`, `commit`, `submit` (target `flui.frame`) | the build owner, the pipeline owner and the runtime | [`Phase::Build`] … [`Phase::Submit`] |
    /// | `animation_tick`, `animation_sampling`, `animation_listeners` (target `flui.animation`) | `flui-animation`, when the `profiling` feature turns its instrumentation on (ADR-0202) | [`Phase::AnimationTick`], [`Phase::AnimationSampling`], [`Phase::AnimationListeners`] |
    ///
    /// And these `TRACE` events:
    ///
    /// | event (target `flui.frame`) | traced by | read as |
    /// |------|-----------|---------|
    /// | `frame_telemetry` | the UI runtime, for each frame it records for a presentation | a [`ProducedFrame`] of the runtime its `ui_runtime_id` names |
    /// | `frame_gpu_time` | the UI runtime, once a frame's GPU time arrives (ADR-0388) | the [`ProducedFrame::gpu_time`] of the frame its `ui_runtime_id`, `presentation` and `frame_id` name |
    ///
    /// The layer carries its own per-layer filter, which admits those spans, the animation
    /// controller spans that keep an animation span's ancestry visible, and of the events only
    /// those two, with the `enabled!` gate at `flui.frame` `TRACE` the runtime asks before it
    /// builds them. A log layer with an `INFO` filter beside it (FLUI's default) therefore does
    /// not starve it: the registry records a `DEBUG` frame span or a `TRACE` telemetry event for
    /// this layer while the log layer never sees it. And installing it does not switch `DEBUG` on
    /// for the rest of the process: an unfiltered layer would declare interest in every callsite,
    /// and every event in the application would pay the dispatch cost once profiling started.
    ///
    /// The `tests/frame_profile_end_to_end.rs` suite drives real frames through a headless UI
    /// runtime and reads them back, and `tests/produced_frames.rs` reads their telemetry: they
    /// are the evidence that the framework still opens the spans and traces the events these
    /// tables name.
    #[must_use]
    pub fn layer<S>(&self) -> Filtered<CaptureLayer, FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        CaptureLayer {
            capture: self.clone(),
        }
        .with_filter(filter())
    }

    /// Call `sink` with every frame as it closes, until the returned handle drops. It runs on the
    /// thread that closed the frame, after the frame is stored and with no lock held.
    ///
    /// The sink is handed this capture, so it never needs a clone of it: a sink that closes over a
    /// clone of its own capture would keep the capture alive for as long as the sink exists.
    ///
    /// Every sink registered when a frame closes sees that frame, in the order the sinks were
    /// registered. A sink that panics does not stop the sinks after it, and the capture keeps
    /// working for the next frame. The first panic of a frame is reported as an error event on
    /// target `flui.devtools` and is not raised: the sinks run inside a tracing layer, and
    /// unwinding out of it would stop the layers after it from seeing the frame span's exit.
    ///
    /// # Panics
    ///
    /// After 2^64 - 1 sinks on one capture; the sink identities never wrap.
    pub fn on_frame(
        &self,
        sink: impl Fn(&Capture, &CapturedFrame) + Send + Sync + 'static,
    ) -> CaptureSubscription {
        let id = SinkId(
            self.shared
                .sink_ids
                .next()
                .expect("BUG: a capture registers fewer than 2^64 - 1 sinks"),
        );
        self.shared.sinks.lock().push((id, Arc::new(sink)));
        CaptureSubscription {
            capture: Arc::downgrade(&self.shared),
            sink: id,
        }
    }

    /// How many sinks are registered, for the tests that pin their removal.
    #[cfg(test)]
    pub(crate) fn sink_count(&self) -> usize {
        self.shared.sinks.lock().len()
    }

    /// The runtimes that have captured a frame or a produced frame; `None` is frames that did not
    /// say which runtime drove them.
    #[must_use]
    pub fn runtimes(&self) -> Vec<Option<UiRuntimeId>> {
        let rings = self.shared.rings.lock();
        let mut runtimes: Vec<_> = rings.keys().copied().collect();
        runtimes.sort_by_key(|runtime| runtime.map(UiRuntimeId::as_u64));
        runtimes
    }

    /// Every captured span, oldest first.
    #[must_use]
    pub fn spans(&self) -> Vec<CapturedSpan> {
        let mut spans: Vec<_> = self
            .shared
            .rings
            .lock()
            .values()
            .flat_map(|ring| ring.spans.iter().cloned())
            .collect();
        spans.sort_by_key(CapturedSpan::start);
        spans
    }

    /// The captured spans inside `partition`, oldest first.
    #[must_use]
    pub fn spans_in(&self, partition: &Partition) -> Vec<CapturedSpan> {
        let mut spans: Vec<_> = self
            .shared
            .rings
            .lock()
            .get(&partition.runtime)
            .map_or_default(|ring| {
                ring.spans
                    .iter()
                    .filter(|span| partition.covers(&span.partition))
                    .cloned()
                    .collect()
            });
        spans.sort_by_key(CapturedSpan::start);
        spans
    }

    /// Every captured frame, oldest first.
    #[must_use]
    pub fn frames(&self) -> Vec<CapturedFrame> {
        let mut frames: Vec<_> = self
            .shared
            .rings
            .lock()
            .values()
            .flat_map(|ring| ring.frames.iter().cloned())
            .collect();
        frames.sort_by_key(|frame| frame.span.start);
        frames
    }

    /// The captured frames driven by `runtime`, oldest first.
    #[must_use]
    pub fn frames_of(&self, runtime: Option<UiRuntimeId>) -> Vec<CapturedFrame> {
        self.shared
            .rings
            .lock()
            .get(&runtime)
            .map_or_default(|ring| ring.frames.iter().cloned().collect())
    }

    /// Every frame a presentation produced, from its runtime's frame telemetry, in the order the
    /// capture received them.
    #[must_use]
    pub fn produced_frames(&self) -> Vec<ProducedFrame> {
        let mut produced: Vec<_> = self
            .shared
            .rings
            .lock()
            .values()
            .flat_map(|ring| ring.produced.iter().cloned())
            .collect();
        produced.sort_by_key(ProducedFrame::received);
        produced
    }

    /// The frames `runtime`'s presentations produced, in the order the capture received them.
    #[must_use]
    pub fn produced_frames_of(&self, runtime: UiRuntimeId) -> Vec<ProducedFrame> {
        self.shared
            .rings
            .lock()
            .get(&Some(runtime))
            .map_or_default(|ring| ring.produced.iter().cloned().collect())
    }

    /// Calls `visit` with every retained frame, in no order and without copying it, while the
    /// capture's rings are locked: `visit` must not call back into this capture.
    #[cfg(feature = "timeline")]
    pub(crate) fn visit_frames(&self, mut visit: impl FnMut(&CapturedFrame)) {
        for ring in self.shared.rings.lock().values() {
            ring.frames.iter().for_each(&mut visit);
        }
    }

    /// Calls `visit` with every retained produced frame, in no order and without copying it,
    /// while the capture's rings are locked: `visit` must not call back into this capture.
    #[cfg(feature = "timeline")]
    pub(crate) fn visit_produced_frames(&self, mut visit: impl FnMut(&ProducedFrame)) {
        for ring in self.shared.rings.lock().values() {
            ring.produced.iter().for_each(&mut visit);
        }
    }

    /// Forget everything captured so far. Sinks stay registered.
    pub fn clear(&self) {
        self.shared.rings.lock().clear();
    }

    /// Files a telemetry event: a produced frame in its runtime's ring, bounded like its frames,
    /// or a GPU time on the frame it names. A GPU time whose frame the ring no longer holds (or
    /// never held, because the capture started after it) is dropped, and a frame keeps the
    /// first time it was given.
    fn record_telemetry(&self, telemetry: telemetry::Telemetry) {
        let mut rings = self.shared.rings.lock();
        match telemetry {
            telemetry::Telemetry::Frame(frame) => {
                let ring = rings.entry(Some(frame.runtime)).or_default();
                ring.produced.push_back(frame);
                while ring.produced.len() > self.shared.max_frames {
                    ring.produced.pop_front();
                }
            }
            telemetry::Telemetry::GpuTime {
                runtime,
                presentation,
                number,
                gpu,
            } => {
                let frame = rings.get_mut(&Some(runtime)).and_then(|ring| {
                    ring.produced
                        .iter_mut()
                        .rev()
                        .find(|frame| frame.presentation == presentation && frame.number == number)
                });
                if let Some(frame) = frame
                    && frame.gpu_time.is_none()
                {
                    frame.gpu_time = Some(gpu);
                }
            }
        }
    }

    fn publish(&self, frame: CapturedFrame) {
        {
            let mut rings = self.shared.rings.lock();
            let ring = rings.entry(frame.span.partition.runtime).or_default();
            ring.spans.push_back(frame.span.clone());
            ring.spans.extend(frame.spans.iter().cloned());
            while ring.spans.len() > self.shared.max_spans {
                ring.spans.pop_front();
            }
            ring.frames.push_back(frame.clone());
            while ring.frames.len() > self.shared.max_frames {
                ring.frames.pop_front();
            }
            // The span budget bounds the retained frames' contents too, oldest first; the frame
            // records themselves stay up to `max_frames`, and sinks below get the whole frame.
            let mut excess = ring
                .frames
                .iter()
                .map(|frame| frame.spans.len())
                .sum::<usize>()
                .saturating_sub(self.shared.max_spans);
            for retained in &mut ring.frames {
                if excess == 0 {
                    break;
                }
                let dropped = excess.min(retained.spans.len());
                retained.spans.drain(..dropped);
                excess -= dropped;
            }
        }
        // Cloned out so a sink that reads the capture, adds a sink or removes one does not
        // deadlock, and so a removal during this delivery leaves it as it started.
        let sinks: Vec<FrameSink> = self
            .shared
            .sinks
            .lock()
            .iter()
            .map(|(_, sink)| Arc::clone(sink))
            .collect();
        deliver(self, sinks, &frame);
    }
}

/// A frame span's state, parked in its extensions from creation until its outermost exit.
struct FrameState {
    start: Option<Instant>,
    thread: ThreadId,
    entries: usize,
    runtime: Option<u64>,
    period: Option<Duration>,
    outermost: bool,
    ended: Vec<Ended>,
}

/// A phase or segment that ended inside a frame, until the frame closes and names its runtime.
struct Ended {
    target: &'static str,
    name: &'static str,
    phase: Option<Phase>,
    presentation: Option<u64>,
    start: Instant,
    duration: Duration,
    thread: ThreadId,
}

/// A phase or segment span's outermost entered visit, parked in its extensions.
struct SpanVisit {
    start: Instant,
    thread: ThreadId,
    entries: usize,
}

/// The presentation a segment span was opened for.
struct SegmentPresentation(Option<u64>);

/// A span whose measurement is over; entering a retained clone again cannot restart it.
struct Closed;

/// Reads the `u64` fields the layer cares about.
#[derive(Default)]
struct Fields {
    runtime: Option<u64>,
    presentation: Option<u64>,
    period_us: Option<u64>,
}

impl Fields {
    /// The display period `period_us` reported. A zero period is no period: no display
    /// refreshes in no time, and a zero jank line would make every frame jank.
    fn period(&self) -> Option<Duration> {
        self.period_us
            .map(Duration::from_micros)
            .filter(|period| !period.is_zero())
    }
}

impl Visit for Fields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "runtime" => self.runtime = Some(value),
            "presentation" => self.presentation = Some(value),
            "period_us" => self.period_us = Some(value),
            _ => {}
        }
    }

    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}

/// The subscriber layer that fills a [`Capture`]. Build it with [`Capture::layer`].
pub struct CaptureLayer {
    capture: Capture,
}

impl std::fmt::Debug for CaptureLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureLayer").finish_non_exhaustive()
    }
}

/// One value per capture, kept in a span's extensions.
///
/// A span's extensions hold one value per type, and two capture layers on one registry see the
/// same spans, so each layer's state is filed under the capture that owns it.
struct Slots<T>(Vec<(CaptureId, T)>);

fn slot_has<'a, T, S>(span: &SpanRef<'a, S>, owner: CaptureId) -> bool
where
    T: Send + Sync + 'static,
    S: LookupSpan<'a>,
{
    slot_read::<T, _, S>(span, owner, |_| ()).is_some()
}

fn slot_read<'a, T, R, S>(
    span: &SpanRef<'a, S>,
    owner: CaptureId,
    read: impl FnOnce(&T) -> R,
) -> Option<R>
where
    T: Send + Sync + 'static,
    S: LookupSpan<'a>,
{
    let extensions = span.extensions();
    let slots = extensions.get::<Slots<T>>()?;
    slots
        .0
        .iter()
        .find(|(slot_owner, _)| *slot_owner == owner)
        .map(|(_, value)| read(value))
}

fn slot_modify<'a, T, R, S>(
    span: &SpanRef<'a, S>,
    owner: CaptureId,
    modify: impl FnOnce(&mut T) -> R,
) -> Option<R>
where
    T: Send + Sync + 'static,
    S: LookupSpan<'a>,
{
    let mut extensions = span.extensions_mut();
    let slots = extensions.get_mut::<Slots<T>>()?;
    slots
        .0
        .iter_mut()
        .find(|(slot_owner, _)| *slot_owner == owner)
        .map(|(_, value)| modify(value))
}

/// Files `value` under `owner`, replacing what that owner had.
fn slot_put<'a, T, S>(span: &SpanRef<'a, S>, owner: CaptureId, value: T)
where
    T: Send + Sync + 'static,
    S: LookupSpan<'a>,
{
    let mut extensions = span.extensions_mut();
    if let Some(slots) = extensions.get_mut::<Slots<T>>() {
        slots.0.retain(|(slot_owner, _)| *slot_owner != owner);
        slots.0.push((owner, value));
    } else {
        extensions.insert(Slots(vec![(owner, value)]));
    }
}

fn slot_take<'a, T, S>(span: &SpanRef<'a, S>, owner: CaptureId) -> Option<T>
where
    T: Send + Sync + 'static,
    S: LookupSpan<'a>,
{
    let mut extensions = span.extensions_mut();
    let slots = extensions.get_mut::<Slots<T>>()?;
    let at = slots
        .0
        .iter()
        .position(|(slot_owner, _)| *slot_owner == owner)?;
    Some(slots.0.swap_remove(at).1)
}

/// The nearest frame span around `span`, open or already closed: work inside a frame that has
/// finished is not an enclosing frame's, so the search never looks past it.
fn owning_frame<'a, S>(span: &SpanRef<'a, S>) -> Option<SpanRef<'a, S>>
where
    S: LookupSpan<'a>,
{
    span.scope()
        .skip(1)
        .find(|ancestor| is_frame(ancestor.metadata()))
}

/// The nearest enclosing frame that is still open, past any that has closed.
fn enclosing_open_frame<'a, S>(span: &SpanRef<'a, S>, owner: CaptureId) -> Option<SpanRef<'a, S>>
where
    S: LookupSpan<'a>,
{
    span.scope()
        .skip(1)
        .find(|ancestor| slot_has::<FrameState, S>(ancestor, owner))
}

/// The presentation segment a span runs in, looked for up to the frame that owns it: an enclosing
/// frame is another runtime's, and its presentation is not this span's.
fn presentation_of<'a, S>(span: &SpanRef<'a, S>, owner: CaptureId) -> Option<u64>
where
    S: LookupSpan<'a>,
{
    span.scope()
        .take_while(|ancestor| !is_frame(ancestor.metadata()))
        .find_map(|ancestor| slot_read::<SegmentPresentation, _, S>(&ancestor, owner, |s| s.0))?
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let metadata = attrs.metadata();
        let Some(span) = ctx.span(id) else { return };
        let owner = self.capture.owner();
        if is_frame(metadata) {
            let mut fields = Fields::default();
            attrs.record(&mut fields);
            let outermost = enclosing_open_frame(&span, owner).is_none();
            slot_put(
                &span,
                owner,
                FrameState {
                    start: None,
                    thread: std::thread::current().id(),
                    entries: 0,
                    runtime: fields.runtime,
                    period: fields.period(),
                    outermost,
                    ended: Vec::new(),
                },
            );
        } else if is_segment(metadata) {
            let mut fields = Fields::default();
            attrs.record(&mut fields);
            slot_put(&span, owner, SegmentPresentation(fields.presentation));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        values.record(&mut fields);
        slot_modify(&span, self.capture.owner(), |state: &mut FrameState| {
            if fields.runtime.is_some() {
                state.runtime = fields.runtime;
            }
            if fields.period_us.is_some() {
                state.period = fields.period();
            }
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let metadata = span.metadata();
        let owner = self.capture.owner();
        if is_frame(metadata) {
            slot_modify(&span, owner, |state: &mut FrameState| {
                state.entries += 1;
                if state.entries == 1 {
                    state.start = Some(Instant::now());
                    state.thread = std::thread::current().id();
                }
            });
            return;
        }
        if !is_segment(metadata) && Phase::of(metadata).is_none() {
            return;
        }
        if slot_has::<Closed, S>(&span, owner) {
            return;
        }
        // Only spans inside an entered frame count: a phase outside any frame is not a frame.
        let in_open_frame = owning_frame(&span).is_some_and(|frame| {
            slot_read(&frame, owner, |state: &FrameState| state.entries > 0).unwrap_or(false)
        });
        if !in_open_frame {
            return;
        }
        // A reentrant visit is covered by the same span kind's enclosing extent; counting both
        // would double the work.
        let key = (metadata.target(), metadata.name());
        // The search stops at the owning frame: a phase of an enclosing frame belongs to another
        // runtime's pump and cannot stand in for this frame's own measurement.
        let covered = span
            .scope()
            .skip(1)
            .take_while(|ancestor| !is_frame(ancestor.metadata()))
            .any(|ancestor| {
                (ancestor.metadata().target(), ancestor.metadata().name()) == key
                    && slot_has::<SpanVisit, S>(&ancestor, owner)
            });
        if covered {
            return;
        }
        // Re-entered without closing: the visit already running measures the full extent.
        let reentered = slot_modify(&span, owner, |visit: &mut SpanVisit| visit.entries += 1);
        if reentered.is_none() {
            slot_put(
                &span,
                owner,
                SpanVisit {
                    start: Instant::now(),
                    thread: std::thread::current().id(),
                    entries: 1,
                },
            );
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let metadata = span.metadata();
        let owner = self.capture.owner();
        if is_frame(metadata) {
            let last = slot_modify(&span, owner, |state: &mut FrameState| {
                state.entries = state.entries.saturating_sub(1);
                state.entries == 0
            })
            .unwrap_or(false);
            if last && let Some(state) = slot_take::<FrameState, S>(&span, owner) {
                self.finish_frame(state);
            }
            return;
        }
        let phase = Phase::of(metadata);
        if phase.is_none() && !is_segment(metadata) {
            return;
        }
        // The outermost exit ends the visit: a clone retained by user code cannot extend it.
        let last = slot_modify(&span, owner, |visit: &mut SpanVisit| {
            visit.entries = visit.entries.saturating_sub(1);
            visit.entries == 0
        });
        let visit = match last {
            Some(true) => {
                let visit = slot_take::<SpanVisit, S>(&span, owner);
                slot_put(&span, owner, Closed);
                visit
            }
            Some(false) => None,
            // Covered or unowned visits must not become new work later.
            None => {
                slot_put(&span, owner, Closed);
                None
            }
        };
        let Some(visit) = visit else { return };
        let ended = Ended {
            target: metadata.target(),
            name: metadata.name(),
            phase,
            presentation: presentation_of(&span, owner),
            start: visit.start,
            duration: visit.start.elapsed(),
            thread: visit.thread,
        };
        if let Some(frame) = owning_frame(&span) {
            slot_modify(&frame, owner, |state: &mut FrameState| {
                state.ended.push(ended);
            });
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // The filter admits no other event; checking again keeps a layer used without it (or
        // with a filter of the caller's own) from reading another event's fields as telemetry.
        if !telemetry::is_telemetry(event.metadata()) {
            return;
        }
        let received = Instant::now().saturating_duration_since(self.capture.shared.epoch);
        let mut fields = telemetry::TelemetryFields::default();
        event.record(&mut fields);
        if let Some(telemetry) = fields.into_telemetry(received, std::thread::current().id()) {
            self.capture.record_telemetry(telemetry);
        }
    }
}

impl CaptureLayer {
    fn finish_frame(&self, state: FrameState) {
        let Some(start) = state.start else { return };
        let epoch = self.capture.shared.epoch;
        let runtime = state.runtime.and_then(UiRuntimeId::from_u64);
        let span_of = |ended: &Ended| CapturedSpan {
            target: ended.target,
            name: ended.name,
            phase: ended.phase,
            partition: Partition::new(
                runtime,
                ended.presentation.and_then(PresentationId::from_u64),
            ),
            start: ended.start.saturating_duration_since(epoch),
            duration: ended.duration,
            thread: ended.thread,
        };
        let frame = CapturedFrame {
            span: CapturedSpan {
                target: FRAME_TARGET,
                name: FRAME_SPAN,
                phase: None,
                partition: Partition::new(runtime, None),
                start: start.saturating_duration_since(epoch),
                duration: start.elapsed(),
                thread: state.thread,
            },
            period: state.period,
            spans: state.ended.iter().map(span_of).collect(),
            outermost: state.outermost,
        };
        self.capture.publish(frame);
    }
}

/// The filter a capture layer carries.
fn filter() -> FrameSpanFilter {
    filter_fn(is_captured as fn(&Metadata<'_>) -> bool)
}

#[cfg(test)]
mod tests {

    use tracing::Dispatch;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    fn runtime(n: u32) -> UiRuntimeId {
        UiRuntimeId::new_gen(n, flui_sdk::foundation::generation::Generation::FIRST)
    }

    fn presentation(n: u32) -> PresentationId {
        PresentationId::new_gen(n, flui_sdk::foundation::generation::Generation::FIRST)
    }

    /// Runs `body` with a fresh capture installed on this thread.
    fn capturing(body: impl FnOnce()) -> Capture {
        let capture = Capture::new();
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        capture
    }

    /// One frame of `runtime` with a layout inside the segment of each presentation given.
    fn frame_of(runtime: UiRuntimeId, presentations: &[PresentationId]) {
        let frame = tracing::debug_span!(
            target: "flui.frame",
            "frame",
            runtime = tracing::field::Empty
        );
        let _frame = frame.enter();
        {
            let _build = tracing::debug_span!(target: "flui.frame", "build").entered();
        }
        frame.record("runtime", runtime.as_u64());
        for presentation in presentations {
            let _segment = tracing::debug_span!(
                target: "flui.frame",
                "presentation",
                presentation = presentation.as_u64()
            )
            .entered();
            let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
        }
    }

    fn phase_partitions(capture: &Capture, phase: Phase) -> Vec<Partition> {
        capture
            .spans()
            .into_iter()
            .filter(|span| span.phase() == Some(phase))
            .map(|span| span.partition())
            .collect()
    }

    /// A span belongs to the runtime that drove its frame, even when the frame learned it after
    /// the span ended, and to the presentation whose segment it ran in.
    #[test]
    fn spans_are_partitioned_by_the_runtime_and_presentation_they_ran_for() {
        let (r1, p1, p2) = (runtime(1), presentation(1), presentation(2));
        let capture = capturing(|| frame_of(r1, &[p1, p2]));

        assert_eq!(
            phase_partitions(&capture, Phase::Build),
            [Partition::new(Some(r1), None)],
            "the build ran outside every segment, and still belongs to the runtime"
        );
        assert_eq!(
            phase_partitions(&capture, Phase::Layout),
            [
                Partition::new(Some(r1), Some(p1)),
                Partition::new(Some(r1), Some(p2))
            ]
        );
        let one = capture.spans_in(&Partition::new(Some(r1), Some(p2)));
        assert!(
            one.iter()
                .all(|span| span.partition().presentation() == Some(p2)),
            "a presentation's query holds only its own spans: {one:?}"
        );
        assert_eq!(
            capture.spans_in(&Partition::new(Some(r1), None)).len(),
            capture.spans().len(),
            "a runtime's query covers all its presentations"
        );
    }

    /// Two runtimes rendering at once, each on its own thread under one subscriber, are two
    /// partitions: neither's frames or phases are blended into the other's.
    #[test]
    fn concurrent_runtimes_are_captured_apart() {
        let capture = Capture::new();
        let subscriber = Arc::new(Dispatch::new(
            tracing_subscriber::registry().with(capture.layer()),
        ));
        flui_testing::log_capture::disarm_interest_cache();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [runtime(1), runtime(2)]
            .into_iter()
            .map(|runtime| {
                let (subscriber, barrier) = (Arc::clone(&subscriber), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    tracing::dispatcher::with_default(&subscriber, || {
                        let frame = tracing::debug_span!(
                            target: "flui.frame",
                            "frame",
                            runtime = runtime.as_u64()
                        );
                        let _frame = frame.enter();
                        // Both frames are open at once before either ends.
                        barrier.wait();
                        let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
                        barrier.wait();
                    });
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("the frame thread finishes");
        }

        for r in [runtime(1), runtime(2)] {
            let frames = capture.frames_of(Some(r));
            assert_eq!(frames.len(), 1, "one frame for {r:?}");
            assert_eq!(frames[0].spans().len(), 1, "its own paint, and no other's");
            assert_eq!(frames[0].spans()[0].partition().runtime(), Some(r));
        }
        assert_eq!(capture.runtimes(), [Some(runtime(1)), Some(runtime(2))]);
    }

    /// A phase span created inside a nested frame but first entered after that frame has closed is
    /// no work of the enclosing frame.
    #[test]
    fn a_deferred_phase_of_a_closed_nested_frame_is_not_charged_to_the_enclosing_frame() {
        let capture = capturing(|| {
            let _outer = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let deferred = {
                let _inner = tracing::debug_span!(target: "flui.frame", "frame").entered();
                tracing::debug_span!(target: "flui.frame", "layout")
            };
            let _entered = deferred.entered();
        });

        let outer = capture
            .frames()
            .into_iter()
            .find(CapturedFrame::is_outermost)
            .expect("the enclosing frame");
        assert!(
            outer.spans().is_empty(),
            "the deferred layout is not the enclosing frame's: {:?}",
            outer.spans()
        );
    }
    /// The span budget bounds what the retained frames hold, not only the flat span list.
    #[test]
    fn the_span_capacity_bounds_the_retained_frames_contents() {
        let capture = Capture::bounded(CaptureLimits {
            frames: 10,
            spans: 2,
        });
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let _sink = {
            let delivered = Arc::clone(&delivered);
            capture.on_frame(move |_, frame| delivered.lock().push(frame.spans().len()))
        };
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            for _ in 0..4 {
                frame_of(
                    runtime(1),
                    &[presentation(1), presentation(2), presentation(3)],
                );
            }
        });

        let retained: usize = capture.frames().iter().map(|f| f.spans().len()).sum();
        assert!(retained <= 2, "retained {retained} child spans");
        assert_eq!(capture.frames().len(), 4, "the frame records are kept");
        assert!(
            delivered.lock().iter().all(|&count| count > 2),
            "a live consumer still sees every span of the frame: {:?}",
            delivered.lock()
        );
    }

    #[test]
    fn a_runtime_never_evicts_another_runtimes_frames() {
        let capture = Capture::bounded(CaptureLimits {
            frames: 2,
            spans: 100,
        });
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            frame_of(runtime(1), &[]);
            for _ in 0..5 {
                frame_of(runtime(2), &[]);
            }
        });

        assert_eq!(capture.frames_of(Some(runtime(1))).len(), 1);
        assert_eq!(
            capture.frames_of(Some(runtime(2))).len(),
            2,
            "its own ring is bounded"
        );
    }

    /// A frame that never said which runtime drove it is kept apart, not merged into one.
    #[test]
    fn a_frame_with_no_runtime_is_kept_apart() {
        let capture = capturing(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        });

        assert_eq!(capture.runtimes(), [None]);
        assert_eq!(capture.frames_of(None).len(), 1);
    }

    /// Fails when a phase is recognized by name alone, or a phase outside a frame is kept.
    #[test]
    fn only_the_frameworks_phases_inside_a_frame_are_captured() {
        let capture = capturing(|| {
            {
                // Not inside a frame: a headless bootstrap is not a frame.
                let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
            }
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _app = tracing::debug_span!(target: "my_app", "paint").entered();
            let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
        });

        let frames = capture.frames();
        assert_eq!(frames.len(), 1);
        let phases: Vec<_> = frames[0]
            .spans()
            .iter()
            .filter_map(CapturedSpan::phase)
            .collect();
        assert_eq!(phases, [Phase::Layout]);
    }

    /// A frame opened inside another is captured, and is not the one a profiler follows.
    #[test]
    fn a_nested_frame_is_not_outermost() {
        let capture = capturing(|| {
            let _outer = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _inner = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        });

        let frames = capture.frames();
        assert_eq!(frames.len(), 2);
        let outermost: Vec<_> = frames.iter().map(CapturedFrame::is_outermost).collect();
        assert_eq!(outermost.iter().filter(|o| **o).count(), 1);
        let inner = frames
            .iter()
            .find(|f| !f.is_outermost())
            .expect("the nested frame");
        assert_eq!(
            inner.spans().len(),
            1,
            "the paint belongs to the frame nearest it"
        );
    }

    /// A phase reentered inside itself is covered by the outer extent, once.
    #[test]
    fn a_reentrant_phase_is_counted_once() {
        let capture = capturing(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _outer = tracing::debug_span!(target: "flui.frame", "layout").entered();
            let _inner = tracing::debug_span!(target: "flui.frame", "layout").entered();
        });

        let frames = capture.frames();
        assert_eq!(frames[0].spans().len(), 1);
    }

    #[test]
    fn a_frame_sink_sees_each_frame_after_it_is_stored() {
        let capture = Capture::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let _sink = capture.on_frame(move |from, frame| {
            sink_seen
                .lock()
                .push((frame.spans().len(), from.frames().len()));
        });
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || frame_of(runtime(1), &[]));

        assert_eq!(
            *seen.lock(),
            [(1, 1)],
            "the sink may read the capture it was called from"
        );
    }

    /// A sink needs no capture of its own, and its handle holds the capture weakly, so dropping
    /// the last capture frees it while the sink is still registered.
    #[test]
    fn a_capture_with_sinks_is_freed_with_its_last_handle() {
        let capture = Capture::new();
        let sink = capture.on_frame(|from, _| {
            let _ = from.frames();
        });
        let weak = Arc::downgrade(&capture.shared);
        drop(capture);

        assert!(weak.upgrade().is_none());
        drop(sink);
    }

    /// The enclosing frame belongs to another runtime's pump: it cannot stand in for the inner
    /// frame's own phase.
    #[test]
    fn a_phase_of_a_nested_frame_is_not_covered_by_the_enclosing_frames_phase() {
        let capture = capturing(|| {
            let _outer = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _build = tracing::debug_span!(target: "flui.frame", "build").entered();
            let _inner = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _inner_build = tracing::debug_span!(target: "flui.frame", "build").entered();
        });

        let frames = capture.frames();
        let inner = frames
            .iter()
            .find(|frame| !frame.is_outermost())
            .expect("the nested frame");
        assert_eq!(
            inner
                .spans()
                .iter()
                .filter_map(CapturedSpan::phase)
                .collect::<Vec<_>>(),
            [Phase::Build],
            "the inner frame's build is its own"
        );
    }

    /// A subscriber that fails reporting the failure, with a payload whose destructor panics too,
    /// still does not unwind out of the capture layer.
    #[test]
    fn a_failing_failure_report_does_not_unwind_out_of_the_layer() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("payload destructor");
            }
        }
        struct BadReporter;
        impl<S: Subscriber> Layer<S> for BadReporter {
            fn on_event(&self, _: &tracing::Event<'_>, _: Context<'_, S>) {
                std::panic::panic_any(PanicOnDrop);
            }
        }

        let capture = Capture::new();
        let _sink = capture.on_frame(|_, _| panic!("a sink that fails"));
        let subscriber = tracing_subscriber::registry()
            .with(capture.layer())
            .with(BadReporter);
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            frame_of(runtime(1), &[]);
        });

        assert_eq!(capture.frames().len(), 1);
    }
    /// A failing sink of one capture must not stop the capture layers after it from closing the
    /// frame: nothing unwinds through the registry.
    #[test]
    fn a_panicking_sink_does_not_stop_another_capture_on_the_registry() {
        let (first, second) = (Capture::new(), Capture::new());
        let _sink = first.on_frame(|_, _| panic!("a sink that fails"));
        let subscriber = tracing_subscriber::registry()
            .with(first.layer())
            .with(second.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            frame_of(runtime(1), &[]);
        });

        assert_eq!(first.frames().len(), 1);
        assert_eq!(
            second.frames().len(),
            1,
            "the other capture closed the frame"
        );
    }

    /// Two captures on one registry see the same spans; neither may disturb the other's state.
    #[test]
    fn two_captures_on_one_registry_each_see_every_frame() {
        let (first, second) = (Capture::new(), Capture::new());
        let subscriber = tracing_subscriber::registry()
            .with(first.layer())
            .with(second.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            frame_of(runtime(1), &[presentation(1)]);
        });

        for capture in [&first, &second] {
            let frames = capture.frames();
            assert_eq!(frames.len(), 1);
            assert_eq!(
                phase_partitions(capture, Phase::Layout),
                [Partition::new(Some(runtime(1)), Some(presentation(1)))]
            );
        }
    }

    /// The enclosing frame's presentation is another runtime's: a nested frame's phase without a
    /// segment of its own has no presentation.
    #[test]
    fn a_nested_frames_phase_does_not_inherit_the_enclosing_presentation() {
        let capture = capturing(|| {
            let _outer = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _segment = tracing::debug_span!(
                target: "flui.frame",
                "presentation",
                presentation = presentation(1).as_u64()
            )
            .entered();
            let _inner = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
        });

        let inner = capture
            .frames()
            .into_iter()
            .find(|frame| !frame.is_outermost())
            .expect("the nested frame");
        let layout = inner
            .spans()
            .iter()
            .find(|span| span.phase() == Some(Phase::Layout))
            .expect("the inner layout");
        assert_eq!(layout.partition().presentation(), None);
    }

    /// A `frame_telemetry` event in the runtime's shape, with `frame` as its number.
    fn telemetry_of(runtime: u64, presentation: u64, frame: u64) {
        tracing::trace!(
            target: "flui.frame",
            event = "frame_telemetry",
            ui_runtime_id = runtime,
            presentation = presentation,
            frame_id = frame,
            present_outcome = "presented",
            segment_us = 900_u64,
            build_us = 100_u64,
            layout_us = 200_u64,
            paint_us = 300_u64,
            composite_us = 40_u64,
            produce_to_present_us = 1_500_u64,
            input_count = 2_u64,
            input_latency_max_us = 700_u64,
            input_epochs_overflowed = false,
            produces_deferred = 0_u64,
            frames_dropped = 0_u64,
            "frame telemetry"
        );
    }

    /// A `frame_gpu_time` event in the runtime's shape.
    fn gpu_time_of(runtime: u64, presentation: u64, frame: u64, gpu_us: u64) {
        tracing::trace!(
            target: "flui.frame",
            event = "frame_gpu_time",
            ui_runtime_id = runtime,
            presentation = presentation,
            frame_id = frame,
            epoch = 1_u64,
            gpu_us = gpu_us,
            "frame GPU time"
        );
    }

    /// The capture reads each telemetry event as the produced frame it names, and joins a GPU
    /// time to the frame of the same runtime, presentation and number, never to a frame of the
    /// same number elsewhere.
    #[test]
    fn telemetry_events_are_produced_frames_with_their_gpu_time() {
        let (r1, r2, p1) = (runtime(1), runtime(2), presentation(1));
        let capture = capturing(|| {
            telemetry_of(r1.as_u64(), p1.as_u64(), 1);
            telemetry_of(r2.as_u64(), p1.as_u64(), 1);
            gpu_time_of(r2.as_u64(), p1.as_u64(), 1, 420);
            // No frame has this number: nothing is invented for it.
            gpu_time_of(r1.as_u64(), p1.as_u64(), 9, 1);
        });

        let first = capture.produced_frames_of(r1);
        assert_eq!(first.len(), 1);
        let frame = &first[0];
        assert_eq!(frame.partition(), Partition::new(Some(r1), Some(p1)));
        assert_eq!(frame.number().get(), 1);
        assert_eq!(frame.outcome(), FrameOutcome::Presented);
        assert_eq!(frame.segment(), Duration::from_micros(900));
        assert_eq!(
            [
                frame.build(),
                frame.layout(),
                frame.paint(),
                frame.composite()
            ],
            [100, 200, 300, 40].map(Duration::from_micros)
        );
        assert_eq!(frame.produce_to_present(), Duration::from_micros(1_500));
        assert_eq!(frame.input_count(), 2);
        assert_eq!(frame.input_latency_max(), Duration::from_micros(700));
        assert_eq!(
            frame.gpu_time(),
            None,
            "the other runtime's time is not this frame's"
        );
        assert_eq!(
            capture.produced_frames_of(r2)[0].gpu_time(),
            Some(Duration::from_micros(420))
        );
        assert_eq!(capture.runtimes(), [Some(r1), Some(r2)]);
    }

    /// A number that is no valid id, or an id written as text, is refused: the event is skipped,
    /// not filed under an id nobody minted.
    #[test]
    fn telemetry_naming_no_valid_id_is_skipped() {
        let (r1, p1) = (runtime(1), presentation(1));
        let capture = capturing(|| {
            telemetry_of(0, p1.as_u64(), 1);
            telemetry_of(r1.as_u64(), 0, 1);
            telemetry_of(r1.as_u64(), p1.as_u64(), 0);
            tracing::trace!(
                target: "flui.frame",
                event = "frame_telemetry",
                ui_runtime_id = r1.as_u64(),
                presentation = %p1,
                frame_id = 1_u64,
                "frame telemetry"
            );
        });

        assert!(
            capture.produced_frames().is_empty(),
            "{:?}",
            capture.produced_frames()
        );
    }

    /// The runtime asks `enabled!(flui.frame, TRACE)` before it builds a frame's telemetry: the
    /// capture's filter must admit that hint, or a capture with no other layer beside it never
    /// gets the events it reads. It admits no other hint and no other `flui.frame` event.
    #[test]
    fn the_filter_admits_the_telemetry_gate_and_no_other_trace_callsite() {
        /// A callsite's target (`flui.frame` or another), whether it is an event, and whether the
        /// capture's filter admits it.
        type Verdict = (&'static str, bool, bool);

        /// Records the capture filter's verdict on each callsite this thread asks about: a callsite
        /// registered on another thread reaches every dispatcher there.
        struct Verdicts(ThreadId, Arc<Mutex<Vec<Verdict>>>);
        impl<S: Subscriber> Layer<S> for Verdicts {
            fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
                let target = match metadata.target() {
                    FRAME_TARGET => FRAME_TARGET,
                    _ => "other",
                };
                if std::thread::current().id() != self.0 {
                    return true;
                }
                self.1
                    .lock()
                    .push((target, metadata.is_event(), is_captured(metadata)));
                true
            }
        }

        let verdicts = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(Verdicts(std::thread::current().id(), Arc::clone(&verdicts)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            // Creating the dispatch asked it about every callsite registered so far.
            verdicts.lock().clear();
            let _ = tracing::enabled!(target: "flui.frame", tracing::Level::TRACE);
            let _ = tracing::enabled!(target: "my_app", tracing::Level::TRACE);
            let _ = tracing::event_enabled!(
                target: "flui.frame",
                tracing::Level::TRACE,
                event = "tree_revision_advanced"
            );
        });

        let mut seen = verdicts.lock().clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen,
            [
                (FRAME_TARGET, false, true),
                (FRAME_TARGET, true, false),
                ("other", false, false)
            ],
            "(target, is an event, admitted): only the hint at flui.frame TRACE"
        );
    }

    /// The produced frames of a runtime are bounded like its frames.
    #[test]
    fn produced_frames_are_bounded_per_runtime() {
        let capture = Capture::bounded(CaptureLimits {
            frames: 2,
            spans: 10,
        });
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            for frame in 1..=5 {
                telemetry_of(runtime(1).as_u64(), presentation(1).as_u64(), frame);
            }
        });

        let numbers: Vec<u64> = capture
            .produced_frames()
            .iter()
            .map(|frame| frame.number().get())
            .collect();
        assert_eq!(numbers, [4, 5]);
    }

    /// A frame whose task panics before the pipeline runs still says whose frame it was.
    #[test]
    fn a_frame_whose_task_panics_still_names_its_runtime() {
        use flui_scheduler::__runtime::DriveFrame as _;
        use flui_scheduler::{OwnerFrame, UpdateScheduler};

        let capture = Capture::new();
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        flui_testing::log_capture::disarm_interest_cache();
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::try_new(&scheduler).expect("a fresh scheduler has no owner frame");
        owner.set_trace_runtime(runtime(7).as_u64());
        let _task = owner
            .async_driver()
            .spawn_local(Box::pin(async { panic!("a task fails") }));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
                let _ = owner.drive_frame(Instant::now(), || {}, || {});
            });
        }));

        assert!(outcome.is_err(), "the task's panic surfaces");
        assert_eq!(capture.runtimes(), [Some(runtime(7))]);
    }
}
