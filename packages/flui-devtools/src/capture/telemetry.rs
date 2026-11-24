//! The frames a presentation produced, read from the runtime's frame telemetry.
//!
//! Every frame a UI runtime records for a presentation is traced as one `frame_telemetry` event
//! on target `flui.frame` at `TRACE`, and the frame's GPU time, which arrives later, as one
//! `frame_gpu_time` event naming the same frame. Their ids are primitives (ADR-0242 §7): the
//! runtime (`ui_runtime_id`) and the presentation (`presentation`) as their packed `as_u64`, the
//! frame (`frame_id`) as its number within the presentation. A number that is no valid id is
//! refused: the event is skipped rather than filed under an id nobody minted.

use std::num::NonZeroU64;
use std::thread::ThreadId;

use flui_sdk::foundation::{PresentationId, UiRuntimeId};
use tracing::field::{Field, Visit};
use tracing::{Level, Metadata};
use web_time::Duration;

use super::{FRAME_TARGET, Partition};

/// The fields every frame-telemetry event carries, which no other `flui.frame` callsite has
/// together.
const TELEMETRY_FIELDS: [&str; 4] = ["event", "ui_runtime_id", "presentation", "frame_id"];

/// A `frame_telemetry` or `frame_gpu_time` event's callsite.
pub(super) fn is_telemetry(metadata: &Metadata<'_>) -> bool {
    metadata.is_event()
        && metadata.target() == FRAME_TARGET
        && *metadata.level() == Level::TRACE
        && TELEMETRY_FIELDS
            .iter()
            .all(|name| metadata.fields().field(name).is_some())
}

/// The `enabled!` hint the runtime asks before it builds a frame's telemetry: a callsite that is
/// neither a span nor an event, on `flui.frame` at `TRACE`. Refusing it would make the runtime
/// skip the event the capture wants.
pub(super) fn is_telemetry_gate(metadata: &Metadata<'_>) -> bool {
    !metadata.is_span()
        && !metadata.is_event()
        && metadata.target() == FRAME_TARGET
        && *metadata.level() == Level::TRACE
}

/// How a produced frame's presentation went, as its telemetry's `present_outcome` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameOutcome {
    /// The sink showed the frame.
    Presented,
    /// The frame failed on its way to the screen.
    Errored,
    /// The runtime named an outcome this crate does not know.
    Unknown,
}

impl FrameOutcome {
    /// The outcome as the telemetry spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Presented => "presented",
            Self::Errored => "errored",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "presented" => Self::Presented,
            "errored" => Self::Errored,
            _ => Self::Unknown,
        }
    }
}

/// One frame a presentation produced, as its runtime's `frame_telemetry` event reported it, with
/// the GPU time a later `frame_gpu_time` event joined to it.
///
/// The durations are the runtime's, measured on the presentation's frame clock. That clock may be
/// a virtual one (a headless host's manual clock), which the capture cannot read, so the frame
/// is placed in time by when the capture received its telemetry: [`Self::received`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProducedFrame {
    pub(super) runtime: UiRuntimeId,
    pub(super) presentation: PresentationId,
    pub(super) number: NonZeroU64,
    pub(super) outcome: FrameOutcome,
    pub(super) received: Duration,
    pub(super) thread: ThreadId,
    pub(super) segment: Duration,
    pub(super) build: Duration,
    pub(super) layout: Duration,
    pub(super) paint: Duration,
    pub(super) composite: Duration,
    pub(super) produce_to_present: Duration,
    pub(super) input_count: u64,
    pub(super) input_latency_max: Duration,
    pub(super) inputs_overflowed: bool,
    pub(super) gpu_time: Option<Duration>,
}

impl ProducedFrame {
    /// The UI runtime that produced the frame.
    #[must_use]
    pub const fn runtime(&self) -> UiRuntimeId {
        self.runtime
    }

    /// The presentation the frame was produced for.
    #[must_use]
    pub const fn presentation(&self) -> PresentationId {
        self.presentation
    }

    /// The runtime and presentation, as the partition the capture's spans use.
    #[must_use]
    pub const fn partition(&self) -> Partition {
        Partition::new(Some(self.runtime), Some(self.presentation))
    }

    /// The frame's number within its presentation, counting from 1. Two presentations, or two
    /// runtimes, number their frames apart, so a frame is named by its partition and this.
    #[must_use]
    pub const fn number(&self) -> NonZeroU64 {
        self.number
    }

    /// How its presentation went.
    #[must_use]
    pub const fn outcome(&self) -> FrameOutcome {
        self.outcome
    }

    /// When the capture received the frame's telemetry, from the capture's epoch.
    #[must_use]
    pub const fn received(&self) -> Duration {
        self.received
    }

    /// The thread the telemetry was traced on.
    #[must_use]
    pub const fn thread(&self) -> ThreadId {
        self.thread
    }

    /// How long the presentation's segment of the frame took.
    #[must_use]
    pub const fn segment(&self) -> Duration {
        self.segment
    }

    /// The segment's build.
    #[must_use]
    pub const fn build(&self) -> Duration {
        self.build
    }

    /// The segment's layout.
    #[must_use]
    pub const fn layout(&self) -> Duration {
        self.layout
    }

    /// The segment's paint.
    #[must_use]
    pub const fn paint(&self) -> Duration {
        self.paint
    }

    /// The segment's scene composition.
    #[must_use]
    pub const fn composite(&self) -> Duration {
        self.composite
    }

    /// From the frame's clock timestamp to its submit.
    #[must_use]
    pub const fn produce_to_present(&self) -> Duration {
        self.produce_to_present
    }

    /// How many routed inputs the frame coalesced.
    #[must_use]
    pub const fn input_count(&self) -> u64 {
        self.input_count
    }

    /// The longest of those inputs' latencies, from arrival to the frame's submit; zero without
    /// inputs.
    #[must_use]
    pub const fn input_latency_max(&self) -> Duration {
        self.input_latency_max
    }

    /// Whether the frame coalesced more inputs than its bounded record kept, so
    /// [`Self::input_latency_max`] covers only those kept.
    #[must_use]
    pub const fn inputs_overflowed(&self) -> bool {
        self.inputs_overflowed
    }

    /// The frame's GPU time, once its `frame_gpu_time` event arrived.
    #[must_use]
    pub const fn gpu_time(&self) -> Option<Duration> {
        self.gpu_time
    }
}

/// Which of the two telemetry events a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetryKind {
    Frame,
    GpuTime,
}

/// One telemetry event, read field by field.
#[derive(Default)]
pub(super) struct TelemetryFields {
    kind: Option<TelemetryKind>,
    runtime: Option<u64>,
    presentation: Option<u64>,
    frame_id: Option<u64>,
    outcome: Option<FrameOutcome>,
    segment_us: Option<u64>,
    build_us: Option<u64>,
    layout_us: Option<u64>,
    paint_us: Option<u64>,
    composite_us: Option<u64>,
    produce_to_present_us: Option<u64>,
    input_count: Option<u64>,
    input_latency_max_us: Option<u64>,
    inputs_overflowed: Option<bool>,
    gpu_us: Option<u64>,
}

/// What a telemetry event says, once its ids are known to be valid.
pub(super) enum Telemetry {
    /// A produced frame.
    Frame(ProducedFrame),
    /// A GPU time for the frame `number` of `runtime`'s `presentation`.
    GpuTime {
        runtime: UiRuntimeId,
        presentation: PresentationId,
        number: NonZeroU64,
        gpu: Duration,
    },
}

impl TelemetryFields {
    /// The event these fields describe, received at `received` on `thread`; `None` for an event
    /// that is neither telemetry event, names an id no runtime mints, or lacks a field the
    /// runtime always writes.
    pub(super) fn into_telemetry(self, received: Duration, thread: ThreadId) -> Option<Telemetry> {
        let runtime = UiRuntimeId::from_u64(self.runtime?)?;
        let presentation = PresentationId::from_u64(self.presentation?)?;
        let number = NonZeroU64::new(self.frame_id?)?;
        match self.kind? {
            TelemetryKind::Frame => Some(Telemetry::Frame(ProducedFrame {
                runtime,
                presentation,
                number,
                outcome: self.outcome?,
                received,
                thread,
                segment: Duration::from_micros(self.segment_us?),
                build: Duration::from_micros(self.build_us?),
                layout: Duration::from_micros(self.layout_us?),
                paint: Duration::from_micros(self.paint_us?),
                composite: Duration::from_micros(self.composite_us?),
                produce_to_present: Duration::from_micros(self.produce_to_present_us?),
                input_count: self.input_count?,
                input_latency_max: Duration::from_micros(self.input_latency_max_us?),
                inputs_overflowed: self.inputs_overflowed?,
                gpu_time: None,
            })),
            TelemetryKind::GpuTime => Some(Telemetry::GpuTime {
                runtime,
                presentation,
                number,
                gpu: Duration::from_micros(self.gpu_us?),
            }),
        }
    }
}

impl Visit for TelemetryFields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        let slot = match field.name() {
            "ui_runtime_id" => &mut self.runtime,
            "presentation" => &mut self.presentation,
            "frame_id" => &mut self.frame_id,
            "segment_us" => &mut self.segment_us,
            "build_us" => &mut self.build_us,
            "layout_us" => &mut self.layout_us,
            "paint_us" => &mut self.paint_us,
            "composite_us" => &mut self.composite_us,
            "produce_to_present_us" => &mut self.produce_to_present_us,
            "input_count" => &mut self.input_count,
            "input_latency_max_us" => &mut self.input_latency_max_us,
            "gpu_us" => &mut self.gpu_us,
            _ => return,
        };
        *slot = Some(value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "input_epochs_overflowed" {
            self.inputs_overflowed = Some(value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "event" => {
                self.kind = match value {
                    "frame_telemetry" => Some(TelemetryKind::Frame),
                    "frame_gpu_time" => Some(TelemetryKind::GpuTime),
                    _ => None,
                };
            }
            "present_outcome" => self.outcome = Some(FrameOutcome::parse(value)),
            _ => {}
        }
    }

    // A field written as display or debug text (an id traced with `%`) is no number to read.
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
