//! [`AnimatedDefaultTextStyle`] — a [`DefaultTextStyle`] that animates to
//! each new style.

use flui_animation::MotionSpec;
use flui_painting::typography::{TextAlign, TextStyle};
use flui_view::prelude::{BuildContext, StatelessView};
use flui_view::{BoxedView, IntoView, ViewExt};

use crate::animated::MotionBuilder;
use crate::{AnimatedLerp, DefaultTextStyle};

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
#[derive(Clone, StatelessView)]
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

impl StatelessView for AnimatedDefaultTextStyle {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let (text_align, max_lines) = (self.text_align, self.max_lines);
        let child = self.child.clone();
        AnimatedLerp::new(self.style.clone(), move |style: &TextStyle| {
            let mut styled = DefaultTextStyle::new(style.clone(), child.clone());
            if let Some(text_align) = text_align {
                styled = styled.text_align(text_align);
            }
            if let Some(max_lines) = max_lines {
                styled = styled.max_lines(max_lines);
            }
            styled
        })
        .motion(self.motion.clone())
    }
}
