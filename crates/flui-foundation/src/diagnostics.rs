//! Shared names for structured `tracing` fields.
//!
//! # The rule
//!
//! Instrument with an identifier that names the **real owner or operation** an
//! event belongs to — not the runtime's current internal topology. A field name
//! is read by log sinks, devtools, exporters, crash reports, and by filters
//! people write by hand, so it becomes a consumer-visible format far faster
//! than the type it was named after becomes stable. Naming a field for an
//! internal construct turns that construct's removal from a refactor into a
//! format migration for everyone downstream.
//!
//! A constant lives here when, and only when, the concept it names is durable
//! *and* something in the workspace already emits it. A name with no emitter is
//! a guess about a design that has not been made yet; the crate that eventually
//! owns the concept can add the constant once it has something real to name.
//!
//! There is deliberately no exported list of "all the correlation fields". A
//! closed enumeration of field names *is* the wire contract, and publishing one
//! freezes the set at whatever the runtime happened to look like on the day it
//! was written.
//!
//! # Why constants at all
//!
//! One spelling per concept. Without a shared constant, one crate writes
//! `presentation` where another writes `presentation_id`, and correlation
//! silently stops working across the seam without anything failing to build.
//! The constant is what makes the name a single fact.
//!
//! ```rust
//! use flui_foundation::diagnostics;
//!
//! # let presentation = 1_u64;
//! tracing::info!(
//!     { diagnostics::PRESENTATION_ID } = presentation,
//!     "presentation committed"
//! );
//! ```
//!
//! Installing a subscriber to *receive* these events is a composition-root
//! decision and lives in `flui-log`, not here.

/// The UI runtime that drove a frame span.
pub const RUNTIME: &str = "runtime";

/// The presentation whose segment a span measured.
pub const PRESENTATION: &str = "presentation";

/// The display period, in microseconds, that a frame was produced for.
pub const PERIOD_US: &str = "period_us";

/// A framework-owned frame telemetry span identity.
///
/// Producers use [`target`](Self::target) and [`name`](Self::name) when opening
/// spans. Consumers classify the same pair with [`of`](Self::of), so an
/// application span that merely shares a name is not framework telemetry.
/// `LayoutRoot` is diagnostic detail, not a profiler phase; consumers choose
/// their captured subset explicitly rather than capturing every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum FrameSpan {
    /// One scheduler-driven frame.
    Frame,
    /// One presentation's segment of a frame.
    Presentation,
    /// The widget tree rebuilds its dirty elements.
    Build,
    /// Render objects are laid out.
    Layout,
    /// One relayout root's layout walk.
    LayoutRoot,
    /// Compositing bits are updated.
    Compositing,
    /// Render objects paint into layers.
    Paint,
    /// The semantics tree is rebuilt.
    Semantics,
    /// The sink accepted the scene and the presentation committed its revision.
    Commit,
    /// The scene is handed to the sink.
    Submit,
    /// The vsync registry walks its running tickers.
    AnimationTick,
    /// An animation computes a value from time, a simulation or a repeat.
    AnimationSampling,
    /// Animation value and status listeners run.
    AnimationListeners,
    /// One animation controller's subscriber-local identity.
    AnimationController,
}

impl FrameSpan {
    /// Every currently defined identity, in frame execution order.
    pub const ALL: [Self; 14] = [
        Self::Frame,
        Self::Presentation,
        Self::Build,
        Self::Layout,
        Self::LayoutRoot,
        Self::Compositing,
        Self::Paint,
        Self::Semantics,
        Self::Commit,
        Self::Submit,
        Self::AnimationTick,
        Self::AnimationSampling,
        Self::AnimationListeners,
        Self::AnimationController,
    ];

    /// The tracing target emitted by this producer.
    #[must_use]
    pub const fn target(self) -> &'static str {
        match self {
            Self::Frame
            | Self::Presentation
            | Self::Build
            | Self::Layout
            | Self::LayoutRoot
            | Self::Compositing
            | Self::Paint
            | Self::Semantics
            | Self::Commit
            | Self::Submit => "flui.frame",
            Self::AnimationTick | Self::AnimationSampling | Self::AnimationListeners => {
                "flui.animation"
            }
            Self::AnimationController => "flui.animation.diagnostics",
        }
    }

    /// The tracing span name emitted by this producer.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Presentation => "presentation",
            Self::Build => "build",
            Self::Layout => "layout",
            Self::LayoutRoot => "layout_root",
            Self::Compositing => "compositing",
            Self::Paint => "paint",
            Self::Semantics => "semantics",
            Self::Commit => "commit",
            Self::Submit => "submit",
            Self::AnimationTick => "animation_tick",
            Self::AnimationSampling => "animation_sampling",
            Self::AnimationListeners => "animation_listeners",
            Self::AnimationController => "animation_controller",
        }
    }

    /// Classify a tracing `(target, name)` pair as a framework span identity.
    #[must_use]
    pub const fn of(target: &str, name: &str) -> Option<Self> {
        const fn eq(left: &str, right: &str) -> bool {
            let left = left.as_bytes();
            let right = right.as_bytes();
            if left.len() != right.len() {
                return false;
            }
            let mut index = 0;
            while index < left.len() {
                if left[index] != right[index] {
                    return false;
                }
                index += 1;
            }
            true
        }

        let mut index = 0;
        while index < Self::ALL.len() {
            let span = Self::ALL[index];
            if eq(target, span.target()) && eq(name, span.name()) {
                return Some(span);
            }
            index += 1;
        }
        None
    }
}

/// The logical presented root an event concerns.
///
/// A presentation may currently map to a native window, but the identifier does
/// not encode that topology: a future texture or external render target remains
/// a presentation without pretending to be a window or GPU surface.
pub const PRESENTATION_ID: &str = "presentation_id";

/// What a redacted value renders as, matching Apple's own placeholder so a
/// reader of either platform's log recognises it.
///
/// `flui-log`'s privacy layer substitutes it for a field it redacts, and a
/// framework type that withholds text it may not publish (a caught panic's
/// payload, for one) formats as it, so both read the same in a log. It is
/// here, not in `flui-log`, because only composition roots may depend on
/// `flui-log`; that crate re-exports it at `flui_log::REDACTED_VALUE`.
pub const REDACTED_VALUE: &str = "<private>";
