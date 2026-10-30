//! [`Carousel`]: a horizontal list of items on the keyline model, scrolled by the user, a
//! controller or autoplay.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, DrivenController, MotionPolicy,
};
use flui_foundation::geometry::{Axis, Rect, Size};
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_interaction::{FocusManager, FocusNode};
use flui_objects::{
    CarouselConfig, CarouselGeometry, CarouselItemFrames, CarouselLayout, MAX_ITEM_EXTENT,
};
use flui_painting::Alignment;
use flui_painting::Canvas;
use flui_painting::paint::Paint;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_view::element::{CarouselSliverConfig, SliverCarousel};
use flui_view::prelude::*;
use flui_view::{
    LifecycleSubscription, PostFrameHandle, RebuildHandle, RebuildReason, WriterSource,
};

use super::autoplay::CarouselAutoplay;
use super::controller::{Attachment, CarouselController};
use super::snap::{CarouselSnap, KeylineSnap};
use crate::animated::VsyncScope;
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
    axis_direction_from_axis_reverse_and_directionality,
};
use crate::scroll::scroll_owner::SharedSnapPolicy;
use crate::scroll::{ScrollController, Scrollable, Viewport};
use crate::{
    Align, Column, CustomPaint, CustomPainter, Directionality, Expanded, Focus, Listener,
    MediaQuery, MouseRegion, RawButton, Semantics, SizedBox, TickerMode,
};

/// What an item's content reads of the item while it scrolls: its index, the part of it the
/// carousel shows, and how far it has travelled between keylines.
///
/// The values are those of the last layout, so a painter reads them in paint: a parallax
/// image shifts by [`progress`](Self::progress), a caption fades with the
/// [`mask`](Self::mask). [`listenable`](Self::listenable) notifies whenever the carousel
/// scrolls, the repaint signal for such a painter.
#[derive(Clone)]
pub struct CarouselItemScope {
    index: usize,
    frames: CarouselItemFrames,
    scrolled: Rc<ChangeNotifier>,
}

impl fmt::Debug for CarouselItemScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselItemScope")
            .field("index", &self.index)
            .field("mask", &self.mask())
            .field("progress", &self.progress())
            .finish_non_exhaustive()
    }
}

impl CarouselItemScope {
    /// The item's index.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The part of the item the carousel shows, in the item's own coordinates; `None` before
    /// the item is laid out.
    #[must_use]
    pub fn mask(&self) -> Option<Rect> {
        self.frames.mask(self.index)
    }

    /// How far the item is from the keyline before it to the one after, in `[0, 1]`; `0`
    /// before it is laid out.
    #[must_use]
    pub fn progress(&self) -> f64 {
        self.frames.progress(self.index).unwrap_or(0.0)
    }

    /// Notifies whenever the carousel scrolls.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.scrolled) as Rc<dyn Listenable>
    }
}

/// Builds item `index` with its scope.
type ItemBuilder = Rc<dyn Fn(&dyn BuildContext, &CarouselItemScope, usize) -> BoxedView>;

/// The default gap between items, logical pixels.
const DEFAULT_SPACING: f64 = 8.0;

/// The side of the autoplay pause control, logical pixels: a 48 dp target.
const PAUSE_CONTROL: f64 = 48.0;

/// A horizontal carousel: items laid out at the layout's large extent and masked down to the
/// keylines they pass as they scroll, so a large item shrinks into a medium and a small one
/// without its content reflowing.
///
/// Items are built lazily, with the [`CarouselItemScope`] their content reads. The carousel is
/// a [`Scrollable`]: every scroll, the user's, a [`CarouselController`] move or an autoplay
/// step, goes through its position, the latter two as programmatic scrolls that any user input
/// takes over. A [`KeylineSnap`] settles each release on an item; without one it scrolls
/// freely. It follows the reading direction: under right-to-left text the first item is at the
/// right.
///
/// Assistive technology sees a scroll view whose items are numbered in their set; its scroll
/// actions offer only a direction that has an item to go to, since only autoplay wraps from
/// the last item to the first. A tap or a touch exploration finds an item only where its mask
/// shows it.
///
/// # Examples
///
/// ```rust,ignore
/// Carousel::builder(photos.len(), move |_ctx, _scope, index| Image::new(photos[index].clone()))
///     .layout(CarouselLayout::multi_browse(40.0, 56.0)?)
///     .snap(KeylineSnap::SingleAdvance)
///     .autoplay(CarouselAutoplay::every(Duration::from_secs(6))?)
/// ```
#[derive(Clone, StatefulView)]
pub struct Carousel {
    count: usize,
    builder: ItemBuilder,
    layout: CarouselLayout,
    snap: Option<KeylineSnap>,
    controller: Option<CarouselController>,
    autoplay: Option<CarouselAutoplay>,
    spacing: f64,
    semantic_label: Option<String>,
}

impl fmt::Debug for Carousel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Carousel")
            .field("count", &self.count)
            .field("layout", &self.layout)
            .field("snap", &self.snap)
            .field("autoplay", &self.autoplay)
            .field("spacing", &self.spacing)
            .finish_non_exhaustive()
    }
}

impl Carousel {
    /// A carousel of `count` items, item `index` built by `builder` with its scope.
    #[must_use]
    pub fn builder<F, V>(count: usize, builder: F) -> Self
    where
        F: Fn(&dyn BuildContext, &CarouselItemScope, usize) -> V + 'static,
        V: IntoView,
    {
        Self {
            count,
            builder: Rc::new(move |ctx, scope, index| {
                builder(ctx, scope, index).into_view().boxed()
            }),
            layout: CarouselLayout::default(),
            snap: None,
            controller: None,
            autoplay: None,
            spacing: DEFAULT_SPACING,
            semantic_label: None,
        }
    }

    /// How items are sized; Material's multi-browse layout by default.
    #[must_use]
    pub fn layout(mut self, layout: CarouselLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Settles each release on an item.
    #[must_use]
    pub fn snap(mut self, snap: KeylineSnap) -> Self {
        self.snap = Some(snap);
        self
    }

    /// The controller that reads and moves the active item; the carousel keeps one of its own
    /// otherwise.
    #[must_use]
    pub fn controller(mut self, controller: CarouselController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Moves on by itself; see [`CarouselAutoplay`] for the limits it keeps.
    #[must_use]
    pub fn autoplay(mut self, autoplay: CarouselAutoplay) -> Self {
        self.autoplay = Some(autoplay);
        self
    }

    /// The gap between items, logical pixels (default 8); a value that is not finite or is
    /// negative is no gap, and a gap is at most [`MAX_ITEM_EXTENT`].
    #[must_use]
    pub fn item_spacing(mut self, spacing: f64) -> Self {
        self.spacing = if spacing.is_finite() {
            spacing.clamp(0.0, MAX_ITEM_EXTENT)
        } else {
            0.0
        };
        self
    }

    /// What assistive technology announces for the carousel.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    fn snap_policy(&self) -> Option<CarouselSnap> {
        Some(CarouselSnap {
            kind: self.snap?,
            layout: self.layout.clone(),
            spacing: self.spacing,
            count: self.count,
        })
    }
}

/// Item `index`, built with its scope in its own context.
#[derive(Clone, StatelessView)]
struct CarouselItem {
    builder: ItemBuilder,
    scope: CarouselItemScope,
}

impl fmt::Debug for CarouselItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselItem")
            .field("index", &self.scope.index)
            .finish_non_exhaustive()
    }
}

impl StatelessView for CarouselItem {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.builder)(ctx, &self.scope, self.scope.index)
    }
}

/// What the scroll listener and the post-frame deliveries share with the state.
struct Shared {
    controller: RefCell<CarouselController>,
    token: Cell<Option<u64>>,
    attachment: RefCell<Option<Attachment>>,
    scrolled: Rc<ChangeNotifier>,
    post_frame: RefCell<Option<PostFrameHandle>>,
    writer: RefCell<Option<WriterSource>>,
    /// Set while the state lives: a delivery queued past its teardown finds it gone.
    alive: Cell<bool>,
    /// Whether the autoplay timer is bound to frames: a timer with none settles at once, and
    /// a step it ends must not restart it.
    autoplays: Cell<bool>,
}

impl Shared {
    fn geometry(&self) -> Option<CarouselGeometry> {
        let attachment = self.attachment.borrow();
        let attachment = attachment.as_ref()?;
        let position = attachment.scroll.position();
        if !position.has_applied_viewport_dimension() {
            return None;
        }
        CarouselGeometry::new(
            &attachment.layout,
            position.viewport_dimension(),
            attachment.spacing,
            attachment.count,
        )
    }

    /// The scroll moved: items repaint, and the active item follows.
    fn scrolled(self: &Rc<Self>) {
        self.scrolled.notify_listeners();
        self.sync();
    }

    /// Commits the item nearest the offset as active, and queues its delivery when it changed
    /// from a known one.
    fn sync(self: &Rc<Self>) {
        let Some(token) = self.token.get() else {
            return;
        };
        let Some(geometry) = self.geometry() else {
            return;
        };
        let pixels = self
            .attachment
            .borrow()
            .as_ref()
            .map_or(0.0, |attachment| attachment.scroll.pixels());
        let Some(index) = geometry.nearest_item(pixels) else {
            return;
        };
        let controller = self.controller.borrow().clone();
        let known = controller.index().is_some();
        if !controller.commit(token, index) || !known {
            return;
        }
        let post_frame = self.post_frame.borrow().clone();
        let Some(post_frame) = post_frame else {
            tracing::warn!(
                index,
                "a carousel's index change has no frame to be delivered after"
            );
            return;
        };
        let shared: Weak<Self> = Rc::downgrade(self);
        if let Err(error) = post_frame.schedule(move |_timing| {
            if let Some(shared) = shared.upgrade() {
                shared.deliver(index);
            }
        }) {
            tracing::warn!(
                ?error,
                index,
                "a carousel's index change was dropped: its runtime is gone"
            );
        }
    }

    fn deliver(&self, index: usize) {
        if !self.alive.get() {
            return;
        }
        let callback = self.controller.borrow().callback();
        let writer = self.writer.borrow().clone();
        if let (Some(callback), Some(writer)) = (callback, writer) {
            writer.write(|cx| callback(cx, index));
        }
    }

    /// Moves to the next item in reading order, or from the last back to the first; nothing
    /// when every item fits.
    fn step(&self) {
        let Some(geometry) = self.geometry() else {
            return;
        };
        if geometry.max_offset() <= 0.0 {
            return;
        }
        let controller = self.controller.borrow().clone();
        let pixels = self
            .attachment
            .borrow()
            .as_ref()
            .map_or(0.0, |attachment| attachment.scroll.pixels());
        let current = controller
            .index()
            .or_else(|| geometry.nearest_item(pixels))
            .unwrap_or(0);
        let here = geometry.rest_offset(current);
        let next = (current + 1..geometry.count())
            .find(|&index| geometry.rest_offset(index) > here)
            .unwrap_or(0);
        // Autoplay advances from its timer, never from a build.
        let _ = controller.animate_to_item(next);
    }
}

/// Persistent state for [`Carousel`].
pub struct CarouselState {
    own: CarouselController,
    scroll: ScrollController,
    frames: CarouselItemFrames,
    shared: Rc<Shared>,
    snap: Option<SharedSnapPolicy>,
    scroll_listener: Option<(Rc<dyn Listenable>, ListenerId)>,
    pause_listener: Option<(CarouselController, u64)>,
    focus: Rc<FocusNode>,
    focus_manager: Option<(Rc<FocusManager>, ListenerId)>,
    lifecycle: Option<LifecycleSubscription>,
    rebuild: Option<RebuildHandle>,
    timer: DrivenController,
    timer_status: Option<flui_animation::StatusSubscription>,
    vsync: Option<flui_animation::Vsync>,
    moves: bool,
    autoplay: Option<CarouselAutoplay>,
    reduced_motion: bool,
    /// The view as last built, read by the lifecycle hooks.
    view: Carousel,
}

impl fmt::Debug for CarouselState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselState")
            .field("controller", &*self.shared.controller.borrow())
            .field("autoplay", &self.autoplay)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Carousel {
    type State = CarouselState;

    fn create_state(&self) -> Self::State {
        let own = CarouselController::new();
        let controller = self.controller.clone().unwrap_or_else(|| own.clone());
        let period = self
            .autoplay
            .map_or(super::autoplay::MIN_AUTOPLAY_PERIOD, |autoplay| {
                autoplay.period()
            });
        CarouselState {
            shared: Rc::new(Shared {
                controller: RefCell::new(controller),
                token: Cell::new(None),
                attachment: RefCell::new(None),
                scrolled: Rc::new(ChangeNotifier::new()),
                post_frame: RefCell::new(None),
                writer: RefCell::new(None),
                alive: Cell::new(true),
                autoplays: Cell::new(false),
            }),
            own,
            scroll: ScrollController::new(),
            frames: CarouselItemFrames::new(),
            snap: self
                .snap_policy()
                .map(|policy| Arc::new(policy) as SharedSnapPolicy),
            scroll_listener: None,
            pause_listener: None,
            focus: FocusNode::new(),
            focus_manager: None,
            lifecycle: None,
            rebuild: None,
            timer: AnimationController::builder(period)
                .debug_label("Carousel.autoplay")
                .build_on(None),
            timer_status: None,
            vsync: None,
            moves: false,
            autoplay: self.autoplay,
            reduced_motion: false,
            view: self.clone(),
        }
    }
}

impl CarouselState {
    fn attachment(&self, view: &Carousel, token: u64) -> Attachment {
        Attachment {
            token,
            scroll: self.scroll.clone(),
            frames: self.frames.clone(),
            layout: view.layout.clone(),
            spacing: view.spacing,
            count: view.count,
            reduced_motion: self.reduced_motion,
        }
    }

    /// Takes `view`'s controller, or the carousel's own when another carousel holds it.
    fn attach(&mut self, view: &Carousel) {
        let wanted = view.controller.clone().unwrap_or_else(|| self.own.clone());
        let mut token = wanted.attach(self.attachment(view, 0));
        let controller = if token.is_some() {
            wanted
        } else {
            tracing::warn!(
                "a carousel controller already drives a mounted carousel; this carousel runs on \
                 a controller of its own"
            );
            token = self.own.attach(self.attachment(view, 0));
            self.own.clone()
        };
        let token = token.expect("BUG: a carousel's own controller is held by no other carousel");
        *self.shared.attachment.borrow_mut() = Some(self.attachment(view, token));
        *self.shared.controller.borrow_mut() = controller.clone();
        self.shared.token.set(Some(token));
        self.listen_for_pauses(&controller);
    }

    fn detach(&mut self) {
        if let Some(token) = self.shared.token.take() {
            self.shared.controller.borrow().detach(token);
        }
        self.shared.attachment.borrow_mut().take();
        if let Some((controller, id)) = self.pause_listener.take() {
            controller.remove_pause_listener(id);
        }
    }

    fn listen_for_pauses(&mut self, controller: &CarouselController) {
        let rebuild = self.rebuild.clone();
        let shared = Rc::downgrade(&self.shared);
        let timer = self.timer.controller().clone();
        let id = controller.add_pause_listener(Rc::new(move || {
            if let Some(rebuild) = &rebuild {
                rebuild.schedule(RebuildReason::StateChange);
            }
            let Some(shared) = shared.upgrade() else {
                return;
            };
            if shared.controller.borrow().is_paused() {
                let _ = timer.stop();
            } else if shared.autoplays.get() && !timer.is_animating() {
                let _ = timer.forward_from(Some(0.0));
            }
        }));
        self.pause_listener = Some((controller.clone(), id));
    }

    /// Re-attaches after the view changed what the controller drives.
    fn refresh_attachment(&self, view: &Carousel) {
        let Some(token) = self.shared.token.get() else {
            return;
        };
        let attachment = self.attachment(view, token);
        self.shared.controller.borrow().update(attachment.clone());
        *self.shared.attachment.borrow_mut() = Some(attachment);
    }

    /// Whether the autoplay timer may be bound to frames now; a pause stops it without
    /// releasing it.
    fn autoplay_runs(&self) -> bool {
        self.autoplay.is_some() && self.moves && !self.reduced_motion
    }

    /// Binds the autoplay timer while autoplay may run, and releases it otherwise; a bound
    /// timer runs unless paused.
    fn run_autoplay(&mut self) {
        let vsync = self.autoplay_runs().then(|| self.vsync.clone()).flatten();
        self.shared.autoplays.set(false);
        if vsync.is_none() {
            let _ = self.timer.controller().stop();
        }
        if let Err(error) = self.timer.rebind(vsync.as_ref()) {
            tracing::error!(%error, "a carousel's autoplay lost its frame registry");
            return;
        }
        if vsync.is_none() {
            return;
        }
        self.shared.autoplays.set(true);
        if !self.shared.controller.borrow().is_paused() && !self.timer.controller().is_animating() {
            let _ = self.timer.controller().forward_from(Some(0.0));
        }
    }
}

impl ViewState<Carousel> for CarouselState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        *self.shared.post_frame.borrow_mut() = ctx.post_frame_handle();
        *self.shared.writer.borrow_mut() = Some(ctx.writer_source());
        self.reduced_motion = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        let view = self.view.clone();
        self.attach(&view);

        let listenable = self.scroll.listenable();
        let shared = Rc::downgrade(&self.shared);
        let id = listenable.add_listener(Rc::new(move || {
            if let Some(shared) = shared.upgrade() {
                shared.scrolled();
            }
        }));
        self.scroll_listener = Some((listenable, id));

        // The first layout commits the first active item.
        if let Some(post_frame) = ctx.post_frame_handle() {
            let shared = Rc::downgrade(&self.shared);
            let _ = post_frame.schedule(move |_timing| {
                if let Some(shared) = shared.upgrade() {
                    shared.sync();
                }
            });
        }

        let manager = ctx.focus_manager();
        let focus = Rc::clone(&self.focus);
        let shared = Rc::downgrade(&self.shared);
        let has_autoplay = self.autoplay.is_some();
        let id = manager.add_listener(Rc::new(move |_previous, _next| {
            if has_autoplay
                && focus.has_focus()
                && let Some(shared) = shared.upgrade()
            {
                shared.controller.borrow().set_paused(true);
            }
        }));
        self.focus_manager = Some((manager, id));

        if let Some(lifecycle) = ctx.lifecycle_handle() {
            let shared = Rc::downgrade(&self.shared);
            match lifecycle.subscribe(move |state| {
                if state != flui_view::AppLifecycleState::Resumed
                    && has_autoplay
                    && let Some(shared) = shared.upgrade()
                {
                    shared.controller.borrow().set_paused(true);
                }
            }) {
                Ok((_, subscription)) => self.lifecycle = Some(subscription),
                Err(error) => tracing::debug!(?error, "a carousel sees no app lifecycle"),
            }
        }

        let shared = Rc::downgrade(&self.shared);
        let post_frame = ctx.post_frame_handle();
        let timer = self.timer.controller().clone();
        self.timer_status = Some(self.timer.controller().subscribe_status(Rc::new(
            move |status| {
                if status != AnimationStatus::ForwardSettled {
                    return;
                }
                let Some(post_frame) = &post_frame else {
                    return;
                };
                let shared = shared.clone();
                let timer = timer.clone();
                // Out of the timer's own delivery: the step scrolls and the timer restarts.
                let _ = post_frame.schedule(move |_timing| {
                    let Some(shared) = shared.upgrade() else {
                        return;
                    };
                    if !shared.autoplays.get() || shared.controller.borrow().is_paused() {
                        return;
                    }
                    shared.step();
                    let _ = timer.forward_from(Some(0.0));
                });
            },
        )));
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.reduced_motion = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        self.moves = TickerMode::is_enabled(ctx);
        let view = self.view.clone();
        self.refresh_attachment(&view);
        self.run_autoplay();
    }

    fn did_update_view(&mut self, old_view: &Carousel, new_view: &Carousel) {
        self.view = new_view.clone();
        let swapped = match (&old_view.controller, &new_view.controller) {
            (Some(old), Some(new)) => !old.ptr_eq(new),
            (None, None) => false,
            _ => true,
        };
        if swapped {
            self.detach();
            self.attach(new_view);
        } else {
            self.refresh_attachment(new_view);
        }
        if old_view.snap != new_view.snap
            || old_view.layout != new_view.layout
            || old_view.spacing != new_view.spacing
            || old_view.count != new_view.count
        {
            self.snap = new_view
                .snap_policy()
                .map(|policy| Arc::new(policy) as SharedSnapPolicy);
        }
        if old_view.autoplay != new_view.autoplay {
            self.autoplay = new_view.autoplay;
            if let Some(autoplay) = new_view.autoplay {
                self.timer.controller().set_duration(autoplay.period());
            }
        }
        self.run_autoplay();
    }

    fn build(&self, view: &Carousel, ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, Axis::Horizontal, false);
        let builder = Rc::clone(&view.builder);
        let frames = self.frames.clone();
        let scrolled = Rc::clone(&self.shared.scrolled);
        let count = view.count;
        let items: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |index| {
            (index < count).then(|| {
                CarouselItem {
                    builder: Rc::clone(&builder),
                    scope: CarouselItemScope {
                        index,
                        frames: frames.clone(),
                        scrolled: Rc::clone(&scrolled),
                    },
                }
                .boxed()
            })
        });
        let sliver = SliverCarousel::with_config(
            CarouselSliverConfig::new(
                CarouselConfig::new(view.layout.clone(), view.spacing),
                self.frames.clone(),
            ),
            count,
            items,
        );
        let mut scrollable = Scrollable::new()
            .controller(self.scroll.clone())
            .scroll_direction(Axis::Horizontal)
            .axis_direction(axis_direction)
            .viewport_builder(Rc::new(move |position| {
                Viewport::new((sliver.clone(),))
                    .axis_direction(axis_direction)
                    .position(position)
                    .boxed()
            }));
        if let Some(snap) = &self.snap {
            scrollable = scrollable.snap_policy(Arc::clone(snap));
        }
        if let Some(label) = &view.semantic_label {
            scrollable = scrollable.semantic_label(label.clone());
        }
        let Some(_) = view.autoplay else {
            return Focus::with_external_node(Rc::clone(&self.focus), scrollable)
                .can_request_focus(false)
                .skip_traversal(true)
                .boxed();
        };

        let shared = Rc::downgrade(&self.shared);
        let pause = move || {
            if let Some(shared) = shared.upgrade() {
                shared.controller.borrow().set_paused(true);
            }
        };
        let on_hover = pause.clone();
        let on_read = pause.clone();
        let paused = self.shared.controller.borrow().is_paused();
        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let label = match (&localizations, paused) {
            (Some(localizations), false) => localizations.autoplay_pause_label(),
            (Some(localizations), true) => localizations.autoplay_resume_label(),
            (None, false) => DefaultWidgetsLocalizations.autoplay_pause_label(),
            (None, true) => DefaultWidgetsLocalizations.autoplay_resume_label(),
        };
        let controller = self.shared.controller.borrow().clone();
        let button = RawButton::new(
            Semantics::new().label(label).child(
                SizedBox::new(PAUSE_CONTROL, PAUSE_CONTROL).child(
                    CustomPaint::new()
                        .size(Size::new(PAUSE_CONTROL, PAUSE_CONTROL))
                        .painter(PauseGlyph { paused }),
                ),
            ),
        )
        .on_press(move |_cx| controller.set_paused(!controller.is_paused()));
        let end = match Directionality::maybe_of(ctx) {
            Some(TextDirection::Rtl) => Alignment::CENTER_LEFT,
            _ => Alignment::CENTER_RIGHT,
        };
        let carousel = Listener::new()
            .on_pointer_down(move |_cx, _pointer| pause())
            .child(
                MouseRegion::new()
                    .on_enter(move |_cx, _pointer, _position| on_hover())
                    // A screen reader reading an item stops the carousel, as
                    // keyboard focus entering it does; only a platform that
                    // reports its cursor says so (`assistive_focus_support`).
                    .child(
                        Semantics::new()
                            .container(true)
                            .on_assistive_focus_within(move |_cx, within| {
                                if within {
                                    on_read();
                                }
                            })
                            .child(scrollable),
                    ),
            );
        Focus::with_external_node(
            Rc::clone(&self.focus),
            Column::new((Align::new(end).child(button), Expanded::new(carousel))),
        )
        .can_request_focus(false)
        .skip_traversal(true)
        .boxed()
    }

    fn dispose(&mut self) {
        self.shared.alive.set(false);
        if let Some((listenable, id)) = self.scroll_listener.take() {
            listenable.remove_listener(id);
        }
        if let Some((manager, id)) = self.focus_manager.take() {
            manager.remove_listener(id);
        }
        self.lifecycle.take();
        self.timer_status.take();
        self.detach();
        let _ = self.timer.controller().stop();
        let _ = self.timer.rebind(None);
        self.timer.dispose();
    }
}

/// The pause control's surface and glyph: the control paints both, as the theme-free gesture
/// menu does, so whatever is drawn behind the carousel cannot hide it.
const GLYPH_SURFACE: Color = Color::WHITE;
const GLYPH_INK: Color = Color::BLACK;

/// The autoplay control's glyph on its disc: two bars while it plays, a triangle while it is
/// paused.
#[derive(Debug)]
struct PauseGlyph {
    paused: bool,
}

impl CustomPainter for PauseGlyph {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        let unit = side / 24.0;
        let point = |x: f64, y: f64| flui_foundation::geometry::Point::new(x * unit, y * unit);
        canvas.draw_circle(point(12.0, 12.0), 10.0 * unit, &Paint::fill(GLYPH_SURFACE));
        let ink = Paint::fill(GLYPH_INK);
        if self.paused {
            let mut path = flui_painting::paint::Path::new();
            path.move_to(point(9.0, 7.0));
            path.line_to(point(17.0, 12.0));
            path.line_to(point(9.0, 17.0));
            path.close();
            canvas.draw_path(&path, &ink);
        } else {
            for left in [8.0, 13.0] {
                canvas.draw_rect(
                    Rect::from_ltrb(left * unit, 7.0 * unit, (left + 3.0) * unit, 17.0 * unit),
                    &ink,
                );
            }
        }
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.paused != self.paused)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
