//! [`Phase`]: the spans the framework opens that measure a part of a frame.
//!
//! [`FrameSpan`] owns the one `(target, name)` wire table shared with producers. `Phase` selects
//! the profiler-facing subset and supplies display labels; the frame profiler, timeline and trace
//! export all ask [`Phase::of`], so an application span named `paint` is not framework paint, and
//! a phase added to the shared contract reaches every consumer through this mapping.

use flui_sdk::foundation::diagnostics::FrameSpan;
use tracing::Metadata;

/// A part of a frame the framework measures.
///
/// A phase span is recognized by its target and its name together ([`Phase::target`],
/// [`Phase::name`]). Animation phases are inclusive: a tick contains its sampling and listener
/// work, so their durations are not summed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Phase {
    /// The widget tree rebuilds its dirty elements.
    Build,
    /// Render objects are laid out.
    Layout,
    /// Compositing bits are updated.
    Compositing,
    /// Render objects paint into layers.
    Paint,
    /// The semantics tree is rebuilt.
    Semantics,
    /// The sink accepted the scene and the presentation committed its tree revision.
    Commit,
    /// The scene is handed to the sink.
    Submit,
    /// The vsync registry walks its running tickers.
    AnimationTick,
    /// An animation computes a value from time, a simulation or a repeat.
    AnimationSampling,
    /// Animation value and status listeners run.
    AnimationListeners,
}

impl Phase {
    /// Every phase, in the order a frame runs them.
    pub const ALL: [Self; 10] = [
        Self::Build,
        Self::Layout,
        Self::Compositing,
        Self::Paint,
        Self::Semantics,
        Self::Commit,
        Self::Submit,
        Self::AnimationTick,
        Self::AnimationSampling,
        Self::AnimationListeners,
    ];

    /// The tracing target of the span that measures this phase.
    #[must_use]
    pub const fn target(self) -> &'static str {
        self.span().target()
    }

    /// The name of the span that measures this phase.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.span().name()
    }

    const fn span(self) -> FrameSpan {
        match self {
            Self::Build => FrameSpan::Build,
            Self::Layout => FrameSpan::Layout,
            Self::Compositing => FrameSpan::Compositing,
            Self::Paint => FrameSpan::Paint,
            Self::Semantics => FrameSpan::Semantics,
            Self::Commit => FrameSpan::Commit,
            Self::Submit => FrameSpan::Submit,
            Self::AnimationTick => FrameSpan::AnimationTick,
            Self::AnimationSampling => FrameSpan::AnimationSampling,
            Self::AnimationListeners => FrameSpan::AnimationListeners,
        }
    }

    pub(crate) const fn of_span(span: FrameSpan) -> Option<Self> {
        match span {
            FrameSpan::Build => Some(Self::Build),
            FrameSpan::Layout => Some(Self::Layout),
            FrameSpan::Compositing => Some(Self::Compositing),
            FrameSpan::Paint => Some(Self::Paint),
            FrameSpan::Semantics => Some(Self::Semantics),
            FrameSpan::Commit => Some(Self::Commit),
            FrameSpan::Submit => Some(Self::Submit),
            FrameSpan::AnimationTick => Some(Self::AnimationTick),
            FrameSpan::AnimationSampling => Some(Self::AnimationSampling),
            FrameSpan::AnimationListeners => Some(Self::AnimationListeners),
            _ => None,
        }
    }

    /// The phase as a person reads it: `Build`, `Animation tick`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Layout => "Layout",
            Self::Compositing => "Compositing",
            Self::Paint => "Paint",
            Self::Semantics => "Semantics",
            Self::Commit => "Commit",
            Self::Submit => "Submit",
            Self::AnimationTick => "Animation tick",
            Self::AnimationSampling => "Animation sampling",
            Self::AnimationListeners => "Animation listeners",
        }
    }

    /// The phase a span measures, or `None` if it is not one of the framework's.
    #[must_use]
    pub fn of(metadata: &Metadata<'_>) -> Option<Self> {
        if !metadata.is_span() {
            return None;
        }
        FrameSpan::of(metadata.target(), metadata.name()).and_then(Self::of_span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_is_found_by_its_own_target_and_name_and_by_nothing_else() {
        for phase in Phase::ALL {
            let mut seen = Phase::ALL
                .into_iter()
                .filter(|other| other.target() == phase.target() && other.name() == phase.name());
            assert_eq!(seen.next(), Some(phase));
            assert_eq!(
                seen.next(),
                None,
                "{phase:?} shares its span with another phase"
            );
        }
    }
}
