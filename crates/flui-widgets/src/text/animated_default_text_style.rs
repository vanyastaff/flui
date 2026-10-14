//! [`AnimatedDefaultTextStyle`] — a [`DefaultTextStyle`] that animates to
//! each new style.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec};
use flui_foundation::geometry::Lerp;
use flui_painting::typography::{TextAlign, TextStyle};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::{ImplicitMotion, MotionBuilder};
use crate::{AnimatedBuilder, DefaultTextStyle};

/// The ambient text style for its subtree, animating to each new style.
///
/// Sizes, spacing, line height, weight, colours and matching shadows
/// interpolate; families and other discrete properties switch half way
/// (see `Lerp for TextStyle`). A change mid-flight starts from the style
/// shown. Text alignment and the line limit apply at once.
///
/// ```
/// use flui_painting::typography::TextStyle;
/// use flui_widgets::{AnimatedDefaultTextStyle, MotionBuilder, Text};
/// use std::time::Duration;
///
/// let title = AnimatedDefaultTextStyle::new(
///     TextStyle { font_size: Some(24.0), ..TextStyle::default() },
///     Text::new("Inbox"),
/// )
/// .duration(Duration::from_millis(200));
/// # let _ = title;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedDefaultTextStyle {
    style: TextStyle,
    text_align: Option<TextAlign>,
    max_lines: Option<u32>,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedDefaultTextStyle {
    /// Apply `style` to `child`'s text, animating later changes with the
    /// default motion.
    pub fn new(style: TextStyle, child: impl IntoView) -> Self {
        Self {
            style,
            text_align: None,
            max_lines: None,
            motion: MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }

    /// The alignment for the subtree's text.
    #[must_use]
    pub fn text_align(mut self, text_align: TextAlign) -> Self {
        self.text_align = Some(text_align);
        self
    }

    /// The line limit for the subtree's text.
    #[must_use]
    pub fn max_lines(mut self, max_lines: u32) -> Self {
        self.max_lines = Some(max_lines);
        self
    }
}

impl MotionBuilder for AnimatedDefaultTextStyle {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedDefaultTextStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedDefaultTextStyle")
            .field("style", &self.style)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// The style the current run starts from and moves to, and its progress.
#[derive(Debug)]
pub struct AnimatedDefaultTextStyleState {
    from: TextStyle,
    to: TextStyle,
    progress: ImplicitMotion<f64>,
    stream: Rc<AnimatedValueView<f64>>,
}

impl StatefulView for AnimatedDefaultTextStyle {
    type State = AnimatedDefaultTextStyleState;

    fn create_state(&self) -> Self::State {
        let progress = ImplicitMotion::new("AnimatedDefaultTextStyle", 1.0, 1.0, &self.motion);
        let stream = Rc::new(progress.stream());
        AnimatedDefaultTextStyleState {
            from: self.style.clone(),
            to: self.style.clone(),
            progress,
            stream,
        }
    }
}

impl ViewState<AnimatedDefaultTextStyle> for AnimatedDefaultTextStyleState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn build(&self, view: &AnimatedDefaultTextStyle, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let (from, to) = (self.from.clone(), self.to.clone());
        let (text_align, max_lines) = (view.text_align, view.max_lines);
        let child = view.child.clone();
        AnimatedBuilder::new(self.stream.clone(), move || {
            let mut styled =
                DefaultTextStyle::new(from.lerp_to(&to, stream.value()), child.clone());
            if let Some(text_align) = text_align {
                styled = styled.text_align(text_align);
            }
            if let Some(max_lines) = max_lines {
                styled = styled.max_lines(max_lines);
            }
            styled
        })
    }

    fn did_update_view(
        &mut self,
        _old_view: &AnimatedDefaultTextStyle,
        new_view: &AnimatedDefaultTextStyle,
    ) {
        if new_view.style == self.to {
            return;
        }
        let shown = self.from.lerp_to(&self.to, self.progress.value());
        if self.progress.restart(0.0, 1.0, &new_view.motion) {
            self.from = shown;
            self.to = new_view.style.clone();
        }
    }

    fn dispose(&mut self) {
        self.progress.dispose();
    }
}
