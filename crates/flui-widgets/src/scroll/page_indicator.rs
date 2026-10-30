//! Dots that show which page of a [`PageView`](super::PageView) is in view.

use std::any::Any;
use std::cell::Cell;
use std::num::NonZeroUsize;
use std::rc::Rc;

use flui_animation::{Animation, MotionPolicy, SharedCurve, curves};
use flui_foundation::geometry::{Point, Size};
use flui_foundation::{Listenable, ListenerId};
use flui_painting::{Canvas, Paint, styling::Color, typography::TextDirection};
use flui_rendering::{delegates::CustomPainter, semantics::SemanticsRole};
use flui_view::prelude::*;
use flui_view::{EventCx, RebuildHandle, RebuildReason};

use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::scroll::PageController;
use crate::{
    CustomPaint, Directionality, GestureDetector, MediaQuery, RepaintBoundary, Semantics, SizedBox,
};

/// How long a tap or an assistive-technology step takes to turn the page, the
/// same as a [`PageView`](super::PageView)'s own page keys.
const TURN: std::time::Duration = std::time::Duration::from_millis(300);
/// How small a dot at the edge of the window gets while pages lie beyond it.
const EDGE_SCALE: f64 = 0.5;

/// Why [`PageIndicatorStyle::try_new`] refused its sizes.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum PageIndicatorStyleError {
    /// The dot's diameter is not finite or not positive.
    #[error("a page indicator's dot must be finite and positive, not {0}")]
    Dot(f64),
    /// The gap between dots is not finite or is negative.
    #[error("a page indicator's gap must be finite and not negative, not {0}")]
    Gap(f64),
    /// The window of dots is wider than a finite length.
    #[error("a page indicator's window of dots overflows")]
    Overflow,
}

/// How a [`PageIndicator`] draws its dots.
///
/// The default is an 8 dp dot, an 8 dp gap and a window of five dots, the
/// current page dark grey and the others at 30 % of it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageIndicatorStyle {
    dot: f64,
    gap: f64,
    window: NonZeroUsize,
    active: Color,
    inactive: Color,
}

const DEFAULT_ACTIVE: Color = Color::rgb(0x1d, 0x1b, 0x20);

impl Default for PageIndicatorStyle {
    fn default() -> Self {
        Self {
            dot: 8.0,
            gap: 8.0,
            window: NonZeroUsize::new(5).expect("BUG: five is not zero"),
            active: DEFAULT_ACTIVE,
            inactive: DEFAULT_ACTIVE.with_opacity(0.3),
        }
    }
}

impl PageIndicatorStyle {
    /// Dots of diameter `dot`, `gap` apart, at most `window` of them shown at
    /// once, in the default colours.
    ///
    /// # Errors
    ///
    /// [`PageIndicatorStyleError`] when `dot` is not finite and positive, when
    /// `gap` is not finite and non-negative, or when the window's width
    /// overflows.
    pub fn try_new(
        dot: f64,
        gap: f64,
        window: NonZeroUsize,
    ) -> Result<Self, PageIndicatorStyleError> {
        if !dot.is_finite() || dot <= 0.0 {
            return Err(PageIndicatorStyleError::Dot(dot));
        }
        if !gap.is_finite() || gap < 0.0 {
            return Err(PageIndicatorStyleError::Gap(gap));
        }
        let style = Self {
            dot,
            gap,
            window,
            ..Self::default()
        };
        if !style.extent(window.get()).width.is_finite() {
            return Err(PageIndicatorStyleError::Overflow);
        }
        Ok(style)
    }

    /// The colour of the current page's dot.
    #[must_use]
    pub fn active_color(mut self, color: Color) -> Self {
        self.active = color;
        self
    }

    /// The colour of the other dots.
    #[must_use]
    pub fn inactive_color(mut self, color: Color) -> Self {
        self.inactive = color;
        self
    }

    /// The size of a strip of `slots` dots, with half a gap of padding above
    /// and below for the tap target.
    fn extent(&self, slots: usize) -> Size {
        let slots = slots as f64;
        Size::new(
            slots * self.dot + (slots - 1.0).max(0.0) * self.gap,
            self.dot + self.gap,
        )
    }
}

/// Where the indicator reads its position.
#[derive(Clone)]
enum Source {
    /// A pager the indicator follows and turns.
    Controlled(PageController),
    /// A position the indicator only shows.
    Observed(Rc<dyn Animation<f64>>),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Controlled(_) => f.write_str("Controlled"),
            Self::Observed(_) => f.write_str("Observed"),
        }
    }
}

impl Source {
    fn listenable(&self) -> Rc<dyn Listenable> {
        match self {
            Self::Controlled(controller) => controller.listenable(),
            Self::Observed(position) => Rc::clone(position) as Rc<dyn Listenable>,
        }
    }

    /// The fractional page in `[0, count − 1]`. A non-finite sample keeps the
    /// last finite one, which `last` holds.
    fn position(&self, last: &Cell<f64>, count: usize) -> f64 {
        let raw = match self {
            Self::Controlled(controller) => controller
                .page()
                .unwrap_or(controller.initial_page() as f64),
            Self::Observed(position) => position.value(),
        };
        if raw.is_finite() {
            last.set(raw);
        }
        let last_page = count.saturating_sub(1) as f64;
        last.get().clamp(0.0, last_page)
    }
}

/// The page nearest `position`; exactly between two pages it is the later one.
fn nearest(position: f64) -> usize {
    // `position` is clamped to `[0, count − 1]`, so this never truncates.
    position.round() as usize
}

/// Dots for the pages of a [`PageView`](super::PageView), one per page, the
/// current one highlighted.
///
/// [`controlled`](Self::controlled) follows a [`PageController`] and turns its
/// pages: a tap on either side of the dots, or assistive technology's increase
/// and decrease, moves one page, and only when a page lies that way. It
/// is announced as an adjustable control. [`observed`](Self::observed) only
/// shows a position; it takes no tap and offers no actions.
///
/// Between two pages, the two neighbouring dots share the highlight. With more
/// pages than the style's window, a window of dots follows the position and
/// the dots at its edges shrink while pages lie beyond them. The dots follow
/// [`Directionality`]: right to left, the first page is on the right, and a
/// tap at the reading end turns forward.
///
/// The announced value is "page n of N", with n the nearest page (1.5 reads as
/// page 3, 1.49 as page 2), and it changes only when n does. With one page or
/// none the indicator is hidden; with none it also announces nothing.
///
/// The dots follow the position without a transition of their own, so reduced
/// motion changes nothing about them; under reduced motion a tap or a step jumps
/// to the page instead of animating the pager there.
///
/// It lives in `scroll` beside [`PageView`](super::PageView), not with the
/// other controls: `controls` sits below `scroll` in the crate's module graph
/// and cannot name [`PageController`].
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_widgets::{PageController, PageIndicator};
/// let pager = PageController::new();
/// let _ = PageIndicator::controlled(&pager, 5);
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct PageIndicator {
    source: Source,
    count: usize,
    style: PageIndicatorStyle,
}

impl PageIndicator {
    /// Dots that follow `controller`'s fractional page and turn its pages.
    ///
    /// Before the controller's [`PageView`](super::PageView) is first laid out,
    /// the dots show its [`initial_page`](PageController::initial_page).
    #[must_use]
    pub fn controlled(controller: &PageController, count: usize) -> Self {
        Self {
            source: Source::Controlled(controller.clone()),
            count,
            style: PageIndicatorStyle::default(),
        }
    }

    /// Dots that show `position`, a fractional page, and nothing more.
    #[must_use]
    pub fn observed(position: impl Animation<f64> + 'static, count: usize) -> Self {
        Self::observed_shared(Rc::new(position), count)
    }

    /// [`observed`](Self::observed) for a position that is already shared.
    #[must_use]
    pub fn observed_shared(position: Rc<dyn Animation<f64>>, count: usize) -> Self {
        Self {
            source: Source::Observed(position),
            count,
            style: PageIndicatorStyle::default(),
        }
    }

    /// How the dots are drawn.
    #[must_use]
    pub fn style(mut self, style: PageIndicatorStyle) -> Self {
        self.style = style;
        self
    }
}

/// What the position listener reads and updates.
struct Live {
    source: std::cell::RefCell<Source>,
    count: Cell<usize>,
    /// The last finite position the source gave.
    last: Rc<Cell<f64>>,
    /// The page the semantics last announced.
    announced: Cell<usize>,
}

impl Live {
    fn position(&self) -> f64 {
        self.source.borrow().position(&self.last, self.count.get())
    }
}

/// Persistent state for [`PageIndicator`]: the subscription to its position.
pub struct PageIndicatorState {
    live: Rc<Live>,
    listener: Option<(Rc<dyn Listenable>, ListenerId)>,
    rebuild: Option<RebuildHandle>,
}

impl std::fmt::Debug for PageIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageIndicatorState")
            .field("announced", &self.live.announced.get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for PageIndicator {
    type State = PageIndicatorState;

    fn create_state(&self) -> Self::State {
        let initial = match &self.source {
            Source::Controlled(controller) => controller.initial_page() as f64,
            Source::Observed(_) => 0.0,
        };
        let live = Rc::new(Live {
            source: std::cell::RefCell::new(self.source.clone()),
            count: Cell::new(self.count),
            last: Rc::new(Cell::new(initial)),
            announced: Cell::new(0),
        });
        live.announced.set(nearest(live.position()));
        PageIndicatorState {
            live,
            listener: None,
            rebuild: None,
        }
    }
}

impl PageIndicatorState {
    fn subscribe(&mut self) {
        self.unsubscribe();
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state mints the rebuild handle before subscribing");
        let live = Rc::downgrade(&self.live);
        let listenable = self.live.source.borrow().listenable();
        let id = listenable.add_listener(Rc::new(move || {
            let Some(live) = live.upgrade() else { return };
            let page = nearest(live.position());
            if live.announced.replace(page) != page {
                rebuild.schedule(RebuildReason::StateChange);
            }
        }));
        self.listener = Some((listenable, id));
    }

    fn unsubscribe(&mut self) {
        if let Some((listenable, id)) = self.listener.take() {
            listenable.remove_listener(id);
        }
    }
}

impl ViewState<PageIndicator> for PageIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        self.subscribe();
    }

    fn did_update_view(&mut self, _old: &PageIndicator, view: &PageIndicator) {
        *self.live.source.borrow_mut() = view.source.clone();
        self.live.count.set(view.count);
        self.live.announced.set(nearest(self.live.position()));
        self.subscribe();
    }

    fn build(&self, view: &PageIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let count = view.count;
        if count <= 1 {
            return SizedBox::shrink().boxed();
        }
        let page = nearest(self.live.position());
        self.live.announced.set(page);
        let direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        let reduced = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        let style = view.style;
        let size = style.extent(count.min(style.window.get()));
        let paint = CustomPaint::new().size(size).painter(DotsPainter {
            live: Rc::clone(&self.live),
            source: Rc::clone(
                &self
                    .listener
                    .as_ref()
                    .expect("BUG: init_state subscribes before the first build")
                    .0,
            ),
            count,
            style,
            direction,
        });
        let label = match BoxedWidgetsLocalizations::maybe_of(ctx) {
            Some(localizations) => localizations.page_label(page + 1, count),
            None => DefaultWidgetsLocalizations.page_label(page + 1, count),
        };
        let semantics = Semantics::new().value(label);
        let Source::Controlled(controller) = &*self.live.source.borrow() else {
            return semantics.child(RepaintBoundary::new().child(paint)).boxed();
        };
        let mut semantics = semantics.role(SemanticsRole::Slider);
        if page + 1 < count {
            let controller = controller.clone();
            semantics = semantics.on_increment(move |_cx: &mut EventCx<'_>| {
                turn(&controller, page + 1, reduced);
            });
        }
        if page > 0 {
            let controller = controller.clone();
            semantics = semantics.on_decrement(move |_cx: &mut EventCx<'_>| {
                turn(&controller, page - 1, reduced);
            });
        }
        let tapped = Rc::clone(&self.live);
        let controller = controller.clone();
        let width = size.width;
        let detector = GestureDetector::new().on_tap_up(move |_cx: &mut EventCx<'_>, details| {
            let page = nearest(tapped.position());
            let count = tapped.count.get();
            let towards_end = details.local_position.dx > width / 2.0;
            let forward = towards_end == (direction == TextDirection::Ltr);
            let target = if forward {
                (page + 1 < count).then_some(page + 1)
            } else {
                page.checked_sub(1)
            };
            if let Some(target) = target {
                turn(&controller, target, reduced);
            }
        });
        semantics
            .child(detector.child(RepaintBoundary::new().child(paint)))
            .boxed()
    }

    fn dispose(&mut self) {
        self.unsubscribe();
    }
}

/// Turns to `page`, at once under reduced motion.
fn turn(controller: &PageController, page: usize, reduced: bool) {
    if reduced {
        controller.jump_to_page(page);
        return;
    }
    controller.animate_to_page(page, TURN, SharedCurve::new(curves::EASE_IN_OUT));
}

/// Paints the dots at the position the source gives at paint time.
struct DotsPainter {
    live: Rc<Live>,
    /// The subscription the state holds; a new one means a new source.
    source: Rc<dyn Listenable>,
    count: usize,
    style: PageIndicatorStyle,
    direction: TextDirection,
}

impl std::fmt::Debug for DotsPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DotsPainter")
            .field("count", &self.count)
            .field("style", &self.style)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("count", &self.count.get())
            .finish_non_exhaustive()
    }
}

impl CustomPainter for DotsPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let count = self.live.count.get();
        let style = &self.style;
        let window = style.window.get().min(count);
        if window <= 1 {
            return;
        }
        let position = self.live.position();
        let hidden = (count - window) as f64;
        // The first page the window shows, fractional while it slides.
        let start = (position - (window - 1) as f64 / 2.0).clamp(0.0, hidden);
        let before = start.min(1.0);
        let after = (hidden - start).min(1.0);
        let pitch = style.dot + style.gap;
        let cy = size.height / 2.0;
        let last_slot = (window - 1) as f64;
        let first = start.floor() as usize;
        let end = ((start + last_slot).ceil() as usize).min(count - 1);
        for page in first..=end {
            let slot = page as f64 - start;
            let fade = if slot < 0.0 {
                1.0 + slot
            } else if slot > last_slot {
                last_slot + 1.0 - slot
            } else {
                1.0
            };
            if fade <= 0.0 {
                continue;
            }
            let shrink_before = 1.0 - before * (1.0 - EDGE_SCALE) * (1.0 - slot.clamp(0.0, 1.0));
            let shrink_after =
                1.0 - after * (1.0 - EDGE_SCALE) * (1.0 - (last_slot - slot).clamp(0.0, 1.0));
            let radius = style.dot / 2.0 * shrink_before.min(shrink_after);
            let along = slot * pitch + style.dot / 2.0;
            let cx = match self.direction {
                TextDirection::Ltr => along,
                TextDirection::Rtl => size.width - along,
            };
            let active = (1.0 - (position - page as f64).abs()).clamp(0.0, 1.0);
            let color = Color::lerp(style.inactive, style.active, active);
            let color = color.with_opacity(f64::from(color.opacity()) * fade);
            canvas.draw_circle(Point::new(cx, cy), radius, &Paint::fill(color));
        }
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any().downcast_ref::<Self>().is_none_or(|old| {
            old.style != self.style
                || old.count != self.count
                || old.direction != self.direction
                || !Rc::ptr_eq(&old.live, &self.live)
                || !Rc::ptr_eq(&old.source, &self.source)
        })
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::clone(&self.source))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
