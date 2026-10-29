//! [`AnimatedIcon`]: an [`AnimatedIconData`] drawn at an animation's progress.

use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use flui_animation::Animation;
use flui_foundation::Listenable;
use flui_foundation::geometry::{Matrix4, Size};
use flui_painting::Canvas;
use flui_painting::paint::Paint;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_view::prelude::*;

use super::{AnimatedIconData, IconTheme};
use crate::{CustomPaint, CustomPainter, Directionality, Semantics};

/// An icon whose vector paths move with an animation: a play button that turns into pause,
/// a menu that turns into an arrow.
///
/// It draws `data` at `progress`, clamped into `[0, 1]`; a progress that is not finite shows
/// the last finite frame (the first frame before any), so the icon never vanishes and no
/// geometry that is not finite is drawn. Its size and color come from the ambient
/// [`IconTheme`] unless set here, and data that [matches the text
/// direction](AnimatedIconData::match_text_direction) mirrors under right-to-left text.
///
/// The animation is the caller's, so its timing follows the caller's controller and the
/// motion policy: under reduced motion a controller's run lands at its target on the next
/// tick, and the icon shows that frame at once. Assistive technology sees an image with the
/// [`semantic_label`](Self::semantic_label); the animation itself is never announced.
///
/// # Examples
///
/// ```rust,ignore
/// AnimatedIcon::new(flui_material::AnimatedIcons::PLAY_PAUSE, controller.clone())
///     .semantic_label("Play");
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedIcon {
    data: AnimatedIconData,
    progress: Rc<dyn Animation<f64>>,
    size: Option<f64>,
    color: Option<Color>,
    semantic_label: Option<String>,
}

impl fmt::Debug for AnimatedIcon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimatedIcon")
            .field("paths", &self.data.path_count())
            .field("progress", &self.progress.value())
            .field("size", &self.size)
            .field("color", &self.color)
            .field("semantic_label", &self.semantic_label)
            .finish()
    }
}

impl AnimatedIcon {
    /// `data` drawn at `progress`'s value.
    #[must_use]
    pub fn new(data: AnimatedIconData, progress: impl Animation<f64> + 'static) -> Self {
        Self::shared(data, Rc::new(progress))
    }

    /// [`new`](Self::new) over an animation already shared.
    #[must_use]
    pub fn shared(data: AnimatedIconData, progress: Rc<dyn Animation<f64>>) -> Self {
        Self {
            data,
            progress,
            size: None,
            color: None,
            semantic_label: None,
        }
    }

    /// The icon's side in logical pixels; defaults to the ambient [`IconTheme`]'s size, or
    /// 24.
    #[must_use]
    pub fn size(mut self, size: f64) -> Self {
        self.size = Some(size);
        self
    }

    /// The icon's color; defaults to the ambient [`IconTheme`]'s color, or opaque black.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// What assistive technology announces for the icon. Without a label the icon is
    /// decorative.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }
}

/// Persistent state for [`AnimatedIcon`]: the last finite progress it drew.
#[derive(Debug)]
pub struct AnimatedIconState {
    last: Rc<Cell<f64>>,
}

impl StatefulView for AnimatedIcon {
    type State = AnimatedIconState;

    fn create_state(&self) -> Self::State {
        AnimatedIconState {
            last: Rc::new(Cell::new(0.0)),
        }
    }
}

impl ViewState<AnimatedIcon> for AnimatedIconState {
    fn build(&self, view: &AnimatedIcon, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = IconTheme::of(ctx);
        let size = view.size.or(theme.size).unwrap_or(24.0);
        let color = view.color.or(theme.color).unwrap_or(Color::BLACK);
        let mirrored = view.data.match_text_direction()
            && Directionality::maybe_of(ctx) == Some(TextDirection::Rtl);
        let painter = IconPainter {
            data: view.data.clone(),
            progress: Rc::clone(&view.progress),
            last: Rc::clone(&self.last),
            color,
            mirrored,
        };
        let paint = CustomPaint::new()
            .size(Size::new(size, size))
            .painter(Rc::new(painter));
        let mut semantics = Semantics::new();
        if let Some(label) = &view.semantic_label {
            semantics = semantics
                .label(label.clone())
                .role(crate::SemanticsRole::Image);
        }
        semantics.child(paint)
    }
}

/// Paints an icon's frame at the animation's progress.
struct IconPainter {
    data: AnimatedIconData,
    progress: Rc<dyn Animation<f64>>,
    /// The last finite progress drawn, kept by the icon's state across rebuilds.
    last: Rc<Cell<f64>>,
    color: Color,
    mirrored: bool,
}

impl fmt::Debug for IconPainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconPainter")
            .field("color", &self.color)
            .field("mirrored", &self.mirrored)
            .finish_non_exhaustive()
    }
}

impl CustomPainter for IconPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        if !side.is_finite() || side <= 0.0 {
            return;
        }
        let progress = self.progress.value();
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            self.last.get()
        };
        // A frame that does not interpolate to finite points keeps the last one that did.
        let frame = match self.data.frame(progress) {
            Some(frame) => {
                self.last.set(progress);
                frame
            }
            None => match self.data.frame(self.last.get()) {
                Some(frame) => frame,
                None => return,
            },
        };
        let scale = side / self.data.view_box();
        let mut placement = Matrix4::scaling(scale, scale, 1.0);
        if self.mirrored {
            placement =
                Matrix4::translation(side, 0.0, 0.0) * Matrix4::scaling(-1.0, 1.0, 1.0) * placement;
        }
        let opacity = f64::from(self.color.opacity());
        for (path, path_opacity) in frame {
            if path_opacity <= 0.0 {
                continue;
            }
            if let Some(path) = path.transform(&placement) {
                let color = self.color.with_opacity(opacity * path_opacity);
                canvas.draw_path(&path, &Paint::fill(color));
            }
        }
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::clone(&self.progress) as Rc<dyn Listenable>)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
