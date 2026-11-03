//! [`Phase`]: the spans the framework opens that measure a part of a frame.
//!
//! One table, here, says which `(target, name)` pairs are phases. The frame profiler, the
//! timeline and the trace export all ask [`Phase::of`], so a span an application opens with
//! the name `paint` is not the framework's paint phase, and a phase added here reaches every
//! consumer at once.

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
        match self {
            Self::Build
            | Self::Layout
            | Self::Compositing
            | Self::Paint
            | Self::Semantics
            | Self::Commit
            | Self::Submit => "flui.frame",
            Self::AnimationTick | Self::AnimationSampling | Self::AnimationListeners => {
                "flui.animation"
            }
        }
    }

    /// The name of the span that measures this phase.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Layout => "layout",
            Self::Compositing => "compositing",
            Self::Paint => "paint",
            Self::Semantics => "semantics",
            Self::Commit => "commit",
            Self::Submit => "submit",
            Self::AnimationTick => "animation_tick",
            Self::AnimationSampling => "animation_sampling",
            Self::AnimationListeners => "animation_listeners",
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
        Self::ALL
            .into_iter()
            .find(|phase| phase.target() == metadata.target() && phase.name() == metadata.name())
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
