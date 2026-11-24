//! Runtime developer tooling for FLUI: the half that runs *inside* the
//! application. Small, feature-gated modules, each an adapter over a seam the
//! framework already exposes, so nothing here reaches into a widget, element
//! or render tree:
//!
//! - **`profiling`** — [`Capture`], the one recorder of the framework's frame
//!   spans and frame telemetry (its [`layer`](Capture::layer) subscribes to the `frame` span
//!   `UpdateScheduler::drive_frame` opens, the [`Phase`] spans the pipeline
//!   emits inside it, and the `frame_telemetry` events the runtime traces for each
//!   [`ProducedFrame`]), and [`Profiler`], per-frame statistics read from a capture.
//! - **`timeline`** — [`Timeline`], an event recorder with Chrome
//!   trace export; over a capture it shows the framework's frames, phases and produced frames
//!   beside the application's events.
//! - **`inspector`** — [`GestureInspector`], which reports
//!   what a gesture arena decided per contest through `GestureObserver`, and
//!   [`GestureInspectorOverlay`], a debug overlay that draws
//!   those contests over the app, and [`InspectorCounters`], a counting
//!   `TreeObserver` over the ADR-0040 observation seam: mounts, moves,
//!   rebuilds per cause, unmounts.
//!
//! # What this crate is not
//!
//! Its one on-screen tool is the gesture overlay; it has no widget-tree
//! inspector UI and is not a hot-reload tool or a driver agent (that is
//! `flui-app`'s, ADR-0239): it opens no endpoint, walks no
//! tree itself, and watches no files (the source watcher lives in the `flui`
//! CLI). It has no network monitor, memory profiler or remote-debug protocol.
//!
//! # Profiling a running app
//!
//! ```no_run
//! use flui_devtools::{Capture, Profiler, ProfilerConfig};
//! use flui_log::config::LogConfig;
//! use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let capture = Capture::new();
//! // FLUI's own log subscriber, with the capture layer beside it. The layer
//! // carries its own filter, so the log filter's level does not affect it.
//! let subscriber = LogConfig::default().subscriber()?.with(capture.layer());
//! install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
//! let profiler = Profiler::over(&capture, ProfilerConfig::default());
//! // ...run the app; `flui-app` inherits an installed subscriber...
//! if let Some(frame) = profiler.frame_stats() {
//!     println!("{frame}");
//! }
//! if let Some(rate) = profiler.frame_rate() {
//!     println!("{rate:.1} frames per second");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # What a capture reads
//!
//! The framework emits a `frame` span from `UpdateScheduler::drive_frame` and `build`, `layout`,
//! `paint`, `compositing`, `semantics`, `commit` and `submit` spans from the pipeline and the
//! runtime. The `profiling` feature also turns on animation instrumentation through the SDK:
//! registry walks, source sampling and listener or future delivery become
//! [`Phase::AnimationTick`], [`Phase::AnimationSampling`] and [`Phase::AnimationListeners`].
//! The framework never calls this crate; [`Capture::layer`] subscribes to what it emits.
//!
//! The layer carries its own per-layer filter. It admits the frame and phase spans, the
//! animation controller span identities, and of the events only `frame_telemetry` and
//! `frame_gpu_time` (`TRACE`, target `flui.frame`) with the `enabled!` gate the runtime asks
//! before tracing them. FLUI's default `INFO` log filter therefore does not starve it, and
//! attaching it does not turn on `DEBUG` or `TRACE` logging for the whole process.
//!
//! Transition events can be logged separately with `animation-diagnostics` on `flui` or
//! `flui-sdk`, or `diagnostics` on `flui-animation`, and a `DEBUG` filter for
//! `flui.animation.diagnostics`. Animation instrumentation is off without those features or
//! devtools `profiling`; an interested subscriber is still required.
//!
//! # Frame telemetry and build levels
//!
//! The spans a capture reads are `DEBUG`; the `frame_telemetry` and `frame_gpu_time` events
//! behind each [`ProducedFrame`] are `TRACE`. Today the framework sets no `max_level_*` feature
//! (ADR-0242 §7), so a release build still traces them for a capture that asks. A build that
//! caps `tracing` below `TRACE` (a `max_level_*` or `release_max_level_*` feature) compiles the
//! two events out, and its capture then records no produced frames; one capped below `DEBUG`
//! loses the spans too.
//!
//! # Runtimes and animation timing
//!
//! A frame belongs to the runtime that drove it (the `runtime` field of its `frame` span) and a
//! span inside it to the presentation whose segment it ran in, so two runtimes rendering under
//! one subscriber are two [`Partition`]s of the capture. A [`Profiler`] follows one runtime:
//! the first that names itself, or the one [`ProfilerConfig::runtime`] names. Which spans are
//! phases is [`Phase::of`], the one `(target, name)` table, so an application span named
//! `paint` is not one; the profiler records framework phases only.
//!
//! Animation timings are inclusive: tick includes sampling and listeners, so adding the three
//! is not a frame CPU total. Reentrant work inside one phase is measured once. A phase belongs
//! to its nearest scheduler frame, recognized by its producer target as well as its name;
//! standalone controller ticks and frames nested from another runtime are not blended into the
//! owning frame. Phases and frames end at their outermost span exit, so a span clone retained
//! by application code cannot extend their timing into another frame. CPU spans do not measure
//! GPU execution.
//!
//! # Jank and frame rate
//!
//! [`ProfilerConfig`] sets the jank line, the history depth and the runtime followed. The line
//! is [`JankLine::FramePeriod`] by default: a frame is jank when it outlasts the period of the
//! display it was produced for, which the `frame` span reports as `period_us` (8.3 ms on a
//! 120 Hz display; 16.7 ms when a frame reports none). [`JankLine::Fixed`] keeps one threshold
//! for every display. [`Profiler::frame_rate`] counts the frames the scheduler produced per
//! second, from the starts of the retained frames, not a second divided by one frame's work; an
//! idle app produces no frames, so its rate stays at the cadence of its last frames.
//!
//! # Timeline export
//!
//! Every [`TimelineEvent`] says what it is with its [`TimelineEventKind`]: a scheduler `Frame`,
//! a [`ProducedFrame`] of a presentation, a presentation `Segment`, a `Phase` of a captured
//! frame, or a `Custom` event the application recorded with [`Timeline::record_event`].
//! `Timeline::export_chrome_trace` writes one complete (`X`) event per span with its kind's
//! label as `cat`, a process per runtime, a thread per thread, and, for a timeline over a
//! capture, `frame_ms` and `jank` counters per scheduler frame; it opens in `ui.perfetto.dev`
//! and `chrome://tracing`. A produced frame is named `Frame N [runtime presentation]`, carries
//! its values in `args`, and sits on a `Produced frames` track of its runtime's process, placed
//! by when the capture received its telemetry, since the presentation's clock may be virtual
//! (a headless host's).
//!
//! # Inspector counters
//!
//! An application installs an [`InspectorCounters`] through `flui-app`'s
//! `AppConfig::dev_tree_observer`. Its factory is called once by every UI runtime the runner
//! builds for a window, with the runtime's id, before the first root is mounted. The runner
//! installs the returned observer with the seeded install, which replays what is already
//! mounted, and the runtime's retirement ends the observer's stream
//! ([`InspectorSnapshot`] reports it). The observer sees the runtime's primary presentation; a
//! presentation opened into an existing runtime is not observed. A factory that panics is
//! logged and its runtime runs without an observer. One `InspectorCounters` shared by several
//! runtimes adds their counts and ends at the first retirement, so an application with several
//! windows hands each runtime counters of its own.
//!
//! # Feature flags
//!
//! The first three are on by default; disable what you do not need.
//!
//! - `profiling`: [`Capture`] and [`Profiler`] (`tracing` + `tracing-subscriber`).
//! - `timeline`: [`Timeline`], [`TimelineEvent`] and [`TimelineEventKind`]
//!   (`serde` for the exporters); with `profiling`, [`Timeline::over`] reads a capture.
//! - `inspector`: [`InspectorCounters`] (`flui_sdk::foundation::observe` for the
//!   seam), [`GestureInspectorOverlay`] (`flui_sdk::widgets`) and [`GestureInspector`] (`flui_sdk::interaction`'s arena
//!   observation).
//! - `full`: all of the above, as one name for feature-matrix runs.
//!
//! [`Phase`] comes with `profiling` or `timeline`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
// A table keyed by ids the process mints hashes with FxHash, not SipHash.
#![warn(clippy::disallowed_types)]

// Reads the framework's own frame spans: the only seam layering permits, since nothing in the
// framework may depend on this crate.
#[cfg(feature = "profiling")]
mod capture;
#[cfg(feature = "inspector")]
mod gesture_inspector;
#[cfg(feature = "inspector")]
mod gesture_overlay;
#[cfg(feature = "inspector")]
mod inspector;
#[cfg(any(feature = "profiling", feature = "timeline"))]
mod phase;
#[cfg(feature = "profiling")]
mod profiler;
#[cfg(feature = "timeline")]
mod timeline;

#[cfg(feature = "profiling")]
pub use capture::{
    Capture, CaptureLayer, CaptureLimits, CaptureSubscription, CapturedFrame, CapturedSpan,
    FrameOutcome, FrameSpanFilter, Partition, ProducedFrame,
};
#[cfg(feature = "inspector")]
pub use gesture_inspector::{
    CompositionStep, ContestReport, DEFAULT_MAX_CONTESTS, DeadlineReport, GestureInspector,
    InspectedComposition, InspectedMember, MemberReport, Verdict,
};
#[cfg(feature = "inspector")]
pub use gesture_overlay::{
    CONTEST_FADE, GestureInspectorOverlay, GestureInspectorOverlayState, GestureOverlayController,
};
#[cfg(feature = "inspector")]
pub use inspector::{InspectorCounters, InspectorSnapshot};
#[cfg(any(feature = "profiling", feature = "timeline"))]
pub use phase::Phase;
#[cfg(feature = "profiling")]
pub use profiler::{FrameStats, JankLine, PhaseTiming, Profiler, ProfilerConfig};
#[cfg(feature = "timeline")]
pub use timeline::{EventGuard, Timeline, TimelineEvent, TimelineEventKind};
