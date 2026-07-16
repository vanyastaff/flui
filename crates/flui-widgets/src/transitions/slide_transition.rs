//! [`SlideTransition`] — animates its child's position as a fraction of its
//! own size from an [`Animation<TranslationFraction>`].

use std::sync::Arc;

use flui_animation::Animation;
use flui_foundation::Listenable;
use flui_objects::TranslationFraction;
use flui_types::typography::TextDirection;
use flui_view::prelude::BuildContext;
use flui_view::{
    AnimatedView, BoxedView, IntoView, StatefulView, ViewExt, ViewState, impl_animated_view,
};

use crate::FractionalTranslation;

/// Animates its child's position by a fraction of the child's own size, as a
/// [`TranslationFraction`] read off an [`Animation`].
///
/// Flutter parity: `widgets/transitions.dart` `SlideTransition` — an
/// `AnimatedWidget` wrapping `FractionalTranslation`. Each tick of `position`
/// rebuilds the transition and re-reads [`Animation::value`] into a
/// [`FractionalTranslation`].
///
/// **Rust-native improvement over the oracle's `Animation<Offset>`**: Flutter
/// overloads `Offset` (normally *pixels*) to carry a size-relative fraction
/// here — the same unit-mismatch `FractionalTranslation` itself already
/// documents (see `flui_objects::TranslationFraction`'s module doc). This
/// transition drives that same dedicated fraction newtype end to end instead
/// of re-introducing the ambiguity one layer up.
///
/// `position.value() == TranslationFraction { dx: 0.0, dy: 0.0 }` paints the
/// child at its normal location; `{ dx: 1.0, dy: 0.0 }` shifts it fully off
/// to the right, one child-width away.
///
/// # `text_direction`
///
/// Verified against the oracle at tag `3.44.0`: `SlideTransition` does
/// **not** read the ambient `Directionality` — `textDirection` is a plain,
/// caller-supplied, nullable constructor parameter (`build` reads
/// `this.textDirection` directly, no `Directionality.of(context)` call
/// anywhere in the type). This port matches that exactly: `text_direction`
/// defaults to `None` (canvas coordinates — positive `dx` moves the child
/// right), and `Some(TextDirection::Rtl)` flips `dx`'s sign so positive
/// values move the child toward the reading-direction start instead.
///
/// ```rust,ignore
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
/// let tween = Tween::new(TranslationFraction::new(-1.0, 0.0), TranslationFraction::ZERO);
/// let position = Arc::new(tween.animate(Arc::new(controller.clone()) as Arc<dyn Animation<f32>>));
/// let slide = SlideTransition::new(position, Text::new("hi"));
/// controller.forward(); // each frame re-reads the fractional offset into the child
/// ```
#[derive(Clone)]
pub struct SlideTransition {
    position: Arc<dyn Animation<TranslationFraction>>,
    transform_hit_tests: bool,
    text_direction: Option<TextDirection>,
    child: BoxedView,
}

impl SlideTransition {
    /// A slide driven by `position`, translating `child`.
    pub fn new(position: Arc<dyn Animation<TranslationFraction>>, child: impl IntoView) -> Self {
        Self {
            position,
            transform_hit_tests: true,
            text_direction: None,
            child: child.into_view().boxed(),
        }
    }

    /// Sets whether hit-testing follows the painted translation (default
    /// `true`). Flutter parity: `SlideTransition.transformHitTests`.
    #[must_use]
    pub fn transform_hit_tests(mut self, transform_hit_tests: bool) -> Self {
        self.transform_hit_tests = transform_hit_tests;
        self
    }

    /// Sets the reading direction `dx` is interpreted against — see the type
    /// doc's `text_direction` section. Flutter parity:
    /// `SlideTransition.textDirection`.
    #[must_use]
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.text_direction = Some(text_direction);
        self
    }

    /// The [`TranslationFraction`] this transition currently paints at,
    /// after applying [`Self::text_direction`]'s sign flip.
    fn resolved_offset(&self) -> TranslationFraction {
        let offset = self.position.value();
        match self.text_direction {
            Some(TextDirection::Rtl) => TranslationFraction::new(-offset.dx, offset.dy),
            Some(TextDirection::Ltr) | None => offset,
        }
    }
}

impl std::fmt::Debug for SlideTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlideTransition")
            .field("position", &self.position.value())
            .field("transform_hit_tests", &self.transform_hit_tests)
            .field("text_direction", &self.text_direction)
            .finish_non_exhaustive()
    }
}

/// State for [`SlideTransition`]. Stateless beyond the listenable
/// subscription that [`AnimatedView`] manages — the offset lives on the
/// animation, not here.
#[derive(Debug)]
pub struct SlideTransitionState;

impl ViewState<SlideTransition> for SlideTransitionState {
    fn build(&self, view: &SlideTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        let offset = view.resolved_offset();
        FractionalTranslation::new(offset.dx, offset.dy)
            .transform_hit_tests(view.transform_hit_tests)
            .child(view.child.clone())
    }
}

impl StatefulView for SlideTransition {
    type State = SlideTransitionState;

    fn create_state(&self) -> Self::State {
        SlideTransitionState
    }
}

impl AnimatedView for SlideTransition {
    fn listenable(&self) -> Arc<dyn Listenable> {
        // `Animation<TranslationFraction>: Listenable`; upcast the trait
        // object so the element subscribes to the same notifier the
        // animation ticks.
        self.position.clone() as Arc<dyn Listenable>
    }
}

impl_animated_view!(SlideTransition);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_animation::Tween;
    use flui_animation::ext::AnimatableExt;
    use flui_animation::{AnimationController, Scheduler};

    use super::*;

    fn position_animation(
        begin: TranslationFraction,
        end: TranslationFraction,
    ) -> (AnimationController, Arc<dyn Animation<TranslationFraction>>) {
        let controller =
            AnimationController::new(Duration::from_millis(300), Scheduler::new().into());
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        let animation: Arc<dyn Animation<TranslationFraction>> =
            Arc::new(Tween::new(begin, end).animate(parent));
        (controller, animation)
    }

    #[test]
    fn resolved_offset_tracks_the_animation_value_at_t_zero_half_and_one() {
        let (controller, position) = position_animation(
            TranslationFraction::new(-1.0, 0.0),
            TranslationFraction::ZERO,
        );
        let slide = SlideTransition::new(position, crate::SizedBox::shrink());

        assert_eq!(slide.resolved_offset(), TranslationFraction::new(-1.0, 0.0));

        controller.set_value(0.5);
        assert_eq!(slide.resolved_offset(), TranslationFraction::new(-0.5, 0.0));

        controller.set_value(1.0);
        assert_eq!(slide.resolved_offset(), TranslationFraction::ZERO);

        controller.dispose();
    }

    #[test]
    fn text_direction_none_leaves_dx_untouched() {
        let (controller, position) = position_animation(
            TranslationFraction::ZERO,
            TranslationFraction::new(0.6, 0.2),
        );
        controller.set_value(1.0);
        let slide = SlideTransition::new(position, crate::SizedBox::shrink());

        assert_eq!(slide.resolved_offset(), TranslationFraction::new(0.6, 0.2));
        controller.dispose();
    }

    #[test]
    fn text_direction_ltr_leaves_dx_untouched() {
        let (controller, position) = position_animation(
            TranslationFraction::ZERO,
            TranslationFraction::new(0.6, 0.2),
        );
        controller.set_value(1.0);
        let slide = SlideTransition::new(position, crate::SizedBox::shrink())
            .text_direction(TextDirection::Ltr);

        assert_eq!(slide.resolved_offset(), TranslationFraction::new(0.6, 0.2));
        controller.dispose();
    }

    #[test]
    fn text_direction_rtl_flips_dx_only() {
        let (controller, position) = position_animation(
            TranslationFraction::ZERO,
            TranslationFraction::new(0.6, 0.2),
        );
        controller.set_value(1.0);
        let slide = SlideTransition::new(position, crate::SizedBox::shrink())
            .text_direction(TextDirection::Rtl);

        assert_eq!(slide.resolved_offset(), TranslationFraction::new(-0.6, 0.2));
        controller.dispose();
    }

    #[test]
    fn create_element_is_stateful_kind() {
        use flui_view::View;

        let (_controller, position) = position_animation(
            TranslationFraction::ZERO,
            TranslationFraction::new(1.0, 0.0),
        );
        let slide = SlideTransition::new(position, crate::SizedBox::shrink());
        let kind = slide.create_element();
        assert!(matches!(
            kind,
            flui_view::element::ElementKind::Stateful { .. }
        ));
    }

    #[test]
    fn debug_format_does_not_panic() {
        let (_controller, position) = position_animation(
            TranslationFraction::ZERO,
            TranslationFraction::new(1.0, 0.0),
        );
        let slide = SlideTransition::new(position, crate::SizedBox::shrink());
        let rendered = format!("{slide:?}");
        assert!(rendered.contains("SlideTransition"));
    }
}
