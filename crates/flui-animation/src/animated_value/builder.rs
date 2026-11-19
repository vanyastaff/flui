//! Configuration of an [`AnimatedValue`] before it takes its registry seat.

use std::borrow::Cow;

use super::AnimatedValue;
use crate::{AnimationError, MotionSpec, MotionValue, Vsync};

/// Configures an [`AnimatedValue`], the way
/// [`AnimationControllerBuilder`](crate::AnimationControllerBuilder) configures
/// a controller.
///
/// ```
/// use flui_animation::{AnimatedValue, MotionSpec};
///
/// let opacity = AnimatedValue::builder(1.0, MotionSpec::default())
///     .debug_label("fade")
///     .build_on(None)?;
/// assert_eq!(opacity.value(), 1.0);
/// # Ok::<(), flui_animation::AnimationError>(())
/// ```
#[must_use = "a builder does nothing until `build_on` is called"]
pub struct AnimatedValueBuilder<T: MotionValue + 'static> {
    initial: T,
    motion: MotionSpec,
    debug_label: Option<Cow<'static, str>>,
}

impl<T: MotionValue + 'static> AnimatedValueBuilder<T> {
    pub(super) fn new(initial: T, motion: MotionSpec) -> Self {
        Self {
            initial,
            motion,
            debug_label: None,
        }
    }

    /// Name the value's controller `label` in diagnostics (see
    /// [`AnimationControllerBuilder::debug_label`](crate::AnimationControllerBuilder::debug_label)).
    pub fn debug_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.debug_label = Some(label.into());
        self
    }

    /// Build a resting value on `vsync`, or unbound when it is `None`. An
    /// unbound value settles newly admitted motion synchronously. The caller
    /// is recorded as its controller's creation site.
    ///
    /// # Errors
    /// Refuses a non-finite initial component before constructing the owner.
    #[track_caller]
    pub fn build_on(self, vsync: Option<&Vsync>) -> Result<AnimatedValue<T>, AnimationError> {
        AnimatedValue::construct(
            "AnimatedValueBuilder::build_on",
            self.debug_label,
            self.initial,
            self.motion,
            vsync,
        )
    }
}

impl<T: MotionValue + 'static> std::fmt::Debug for AnimatedValueBuilder<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedValueBuilder")
            .field("debug_label", &self.debug_label)
            .finish_non_exhaustive()
    }
}
