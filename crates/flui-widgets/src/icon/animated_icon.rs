//! [`AnimatedIcon`]: an [`AnimatedIconData`] drawn at an animation's progress.

use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use flui_animation::{Animation, AnimationController, ConstantAnimation, DrivenController};
use flui_foundation::Listenable;
use flui_foundation::geometry::{Matrix4, Size};
use flui_painting::Canvas;
use flui_painting::paint::Paint;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_view::ViewExt;
use flui_view::prelude::*;

use super::symbol_effect::{ContentKey, SymbolEffectHost};
use super::{AnimatedIconData, IconTheme, PathEffect, SymbolEffect};
use crate::animated::VsyncScope;
use crate::{AnimatedBuilder, CustomPaint, CustomPainter, Directionality, Semantics, TickerMode};

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
/// On top of its own motion the icon takes a [`SymbolEffect`] and a [`PathEffect`] that
/// draws its paths on or off.
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
    effect: Option<SymbolEffect>,
    path_effect: Option<PathEffect>,
}

impl fmt::Debug for AnimatedIcon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimatedIcon")
            .field("paths", &self.data.path_count())
            .field("progress", &self.progress.value())
            .field("size", &self.size)
            .field("color", &self.color)
            .field("semantic_label", &self.semantic_label)
            .field("effect", &self.effect)
            .field("path_effect", &self.path_effect)
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
            effect: None,
            path_effect: None,
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

    /// Moves the icon with `effect`; see [`SymbolEffect`]. A replace cross-fades when the
    /// data changes to other data. It takes the place of the ambient
    /// [`IconThemeData::symbol_effect`](super::IconThemeData::symbol_effect).
    #[must_use]
    pub fn symbol_effect(mut self, effect: SymbolEffect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// Draws the icon's paths on or off with `effect`; see [`PathEffect`].
    #[must_use]
    pub fn path_effect(mut self, effect: PathEffect) -> Self {
        self.path_effect = Some(effect);
        self
    }
}

/// Persistent state for [`AnimatedIcon`]: the last finite progress it drew and the run of
/// its path effect.
pub struct AnimatedIconState {
    last: Rc<Cell<f64>>,
    /// Runs a path effect from 0 to 1; idle at 1 without one.
    draw: DrivenController,
}

impl fmt::Debug for AnimatedIconState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimatedIconState")
            .field("last", &self.last.get())
            .field("draw", &self.draw.controller().value())
            .finish()
    }
}

impl StatefulView for AnimatedIcon {
    type State = AnimatedIconState;

    fn create_state(&self) -> Self::State {
        let draw = AnimationController::builder(PathEffect::DRAW_DURATION)
            .debug_label("PathEffect")
            .build_on(None);
        // A path effect on mount waits at its start for the frame clock.
        match &self.path_effect {
            Some(effect) => {
                draw.controller().set_duration(effect.duration());
                let _ = draw.controller().set_value(0.0);
            }
            None => {
                let _ = draw.controller().set_value(1.0);
            }
        }
        AnimatedIconState {
            last: Rc::new(Cell::new(0.0)),
            draw,
        }
    }
}

impl AnimatedIconState {
    /// Plays `effect` from its start, as when it is first applied.
    fn play(&self, effect: Option<&PathEffect>) {
        let controller = self.draw.controller();
        let _ = controller.stop();
        match effect {
            Some(effect) => {
                controller.set_duration(effect.duration());
                let _ = controller.set_value(0.0);
                let _ = controller.forward();
            }
            None => {
                let _ = controller.set_value(1.0);
            }
        }
    }
}

impl ViewState<AnimatedIcon> for AnimatedIconState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let unbound = !self.draw.is_bound();
        if let Err(error) = self.draw.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "an animated icon lost its frame registry");
        }
        if unbound && self.draw.is_bound() && self.draw.controller().value() < 1.0 {
            // A path effect applied before a frame clock was found starts now.
            let _ = self.draw.controller().forward();
        }
    }

    fn did_update_view(&mut self, old: &AnimatedIcon, view: &AnimatedIcon) {
        match (old.path_effect, view.path_effect) {
            (Some(was), Some(now)) if was.on == now.on => {
                self.draw.controller().set_duration(now.duration());
            }
            (None, None) => {}
            (_, now) => self.play(now.as_ref()),
        }
    }

    fn build(&self, view: &AnimatedIcon, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = IconTheme::of(ctx);
        let size = view.size.or(theme.size).unwrap_or(24.0);
        let color = view.color.or(theme.color).unwrap_or(Color::BLACK);
        let mirrored = view.data.match_text_direction()
            && Directionality::maybe_of(ctx) == Some(TextDirection::Rtl);
        let built = match view.path_effect {
            None => self.paint(view, size, color, mirrored, None).boxed(),
            Some(effect) => {
                let live = self.draw.is_bound() && TickerMode::is_enabled(ctx);
                let draw: Rc<dyn Animation<f64>> = if live {
                    Rc::new(self.draw.controller().clone())
                } else {
                    Rc::new(ConstantAnimation::new(1.0))
                };
                let trim = Trim {
                    draw,
                    on: effect.on,
                };
                let state = (Rc::clone(&self.last), view.clone());
                AnimatedBuilder::new(Rc::clone(&trim.draw) as Rc<dyn Listenable>, move || {
                    let (last, view) = &state;
                    Self::paint_with(last, view, size, color, mirrored, Some(trim.fraction()))
                })
                .boxed()
            }
        };
        let mut semantics = Semantics::new();
        if let Some(label) = &view.semantic_label {
            semantics = semantics
                .label(label.clone())
                .role(crate::SemanticsRole::Image);
        }
        let built = semantics.child(built).boxed();
        match view.effect.as_ref().or(theme.symbol_effect.as_ref()) {
            Some(effect) => {
                SymbolEffectHost::new(effect.clone(), ContentKey::Vector(view.data.clone()), built)
                    .boxed()
            }
            None => built,
        }
    }

    fn dispose(&mut self) {
        self.draw.dispose();
    }
}

impl AnimatedIconState {
    fn paint(
        &self,
        view: &AnimatedIcon,
        size: f64,
        color: Color,
        mirrored: bool,
        trim: Option<f64>,
    ) -> CustomPaint {
        Self::paint_with(&self.last, view, size, color, mirrored, trim)
    }

    fn paint_with(
        last: &Rc<Cell<f64>>,
        view: &AnimatedIcon,
        size: f64,
        color: Color,
        mirrored: bool,
        trim: Option<f64>,
    ) -> CustomPaint {
        let painter = IconPainter {
            data: view.data.clone(),
            progress: Rc::clone(&view.progress),
            last: Rc::clone(last),
            color,
            mirrored,
            trim,
        };
        CustomPaint::new()
            .size(Size::new(size, size))
            .painter(painter)
    }
}

/// How much of the paths a path effect has drawn.
struct Trim {
    draw: Rc<dyn Animation<f64>>,
    on: bool,
}

impl Trim {
    /// The drawn fraction of each path's length, in `[0, 1]`; a run that is not finite reads
    /// as finished.
    fn fraction(&self) -> f64 {
        let t = self.draw.value();
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if self.on { t } else { 1.0 - t }
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
    /// The drawn fraction of each path under a path effect.
    trim: Option<f64>,
}

impl fmt::Debug for IconPainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconPainter")
            .field("color", &self.color)
            .field("mirrored", &self.mirrored)
            .field("trim", &self.trim)
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
                let path = match self.trim {
                    Some(drawn) if drawn < 1.0 => path.trim(0.0, drawn),
                    _ => path,
                };
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
