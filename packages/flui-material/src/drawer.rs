//! [`Drawer`] and [`DrawerController`] — a Material Design panel that slides
//! in horizontally from the edge of a [`crate::Scaffold`], plus
//! [`DrawerHandle`] — the runtime capability to open/close it.
//!
//! # Structure
//!
//! `DrawerController` owns a 246ms [`AnimationController`] that
//! drives the open/close/drag/fling state machine; `Drawer` is the
//! M3-styled content panel it wraps.
//!
//! ## The `GlobalKey` bridge (why `DrawerHandle` exists)
//!
//! The `Scaffold` opens a drawer by reaching into its own
//! `DrawerController` child through a
//! `GlobalKey<DrawerControllerState>` its `State` holds.
//! [`DrawerHandle`] wraps the same two
//! `GlobalKey<DrawerControllerState>` instances [`crate::Scaffold`]'s state
//! attaches to the `drawer`/`end_drawer` `DrawerController`s it builds, so
//! `DrawerHandle::open_drawer`/`close_drawer` drive them directly.
//!
//! `DrawerHandle` is deliberately **`Rc`-based and `!Send`**, not
//! `Arc`/`Send + Sync`. `GlobalKey::with_current_state` resolves against the
//! owner-thread element-tree registry, and this workspace already carries a
//! documented tension between `Send + Sync` data-plane primitives (gesture
//! recognizers, render objects — ADR-0027) and owner-affine widget-layer
//! capability handles (an in-flight `Send`-bound-drop migration found this
//! exact knot at `flui_sdk::widgets::NavigatorHandle`, which is `Cloneable, Send +
//! Sync` in name only — see that type's own module doc). `DrawerHandle`
//! sidesteps the knot entirely by never claiming `Send` in the first place.
//!
//! ## Named divergence: the drag divisor is the *configured* panel width,
//! not a live render-object measurement
//!
//! The oracle's `_width` getter (`DrawerControllerState._width`) reads the
//! mounted `Drawer` panel's **actual laid-out** `RenderBox.size.width` via
//! `_drawerKey.currentContext?.findRenderObject()`, falling back to
//! `_kWidth` only while unmounted. FLUI has no render-object size query for
//! an arbitrary descendant from event-handling code (no `GlobalKey`
//! `findRenderObject` equivalent) — building one is a new cross-crate
//! primitive out of this feature's scope. [`DrawerController`] instead uses
//! [`Drawer::width`]'s **configured** value directly (default
//! [`DEFAULT_DRAWER_WIDTH`]), passed down via [`DrawerController::panel_width`]. This is
//! behaviorally equivalent in the drawer's actual mounting context: the
//! open panel is wrapped in an [`flui_sdk::widgets::Align`] with a `width_factor`,
//! which gives its child **loose** (unbounded) width constraints to measure
//! its natural size — so `Drawer`'s own `BoxConstraints.expand(width:)`
//! (ported as [`flui_sdk::rendering::BoxConstraints::tighten`])
//! renders at exactly its configured width, unclamped. The divergence is
//! bounded to the case the oracle's own comment calls out — the drawer
//! genuinely being unmounted, where both approaches already agree on
//! [`DEFAULT_DRAWER_WIDTH`] — plus an exotic ambient-constraint scenario the oracle's
//! live measurement would catch and this substrate would not.
//!
//! ## Deferred, and named
//!
//! `DrawerTheme` (no such theme-extension slot exists yet in this crate — see
//! `theme_data.rs`'s scope note), the `AppBar` auto-hamburger, `RTL`
//! (`DrawerAlignment`'s outer/inner `Alignment` mapping is LTR-only —
//! `flui_sdk::widgets::Directionality` is not read, matching `crate::Scaffold`'s
//! own documented RTL gap), `BlockSemantics`/`ExcludeSemantics`/modal-barrier
//! semantics labeling, `FocusScope` (no focus trap inside an open drawer
//! yet), and local-history back-dismissal (`LocalHistoryEntry` — this
//! substrate has no `ModalRoute`-integrated history-entry mechanism to hang
//! it on). `RepaintBoundary` is also skipped — a paint-layer optimization
//! hint with no observable behavior difference for this substrate's tests.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::{
    Animation, AnimationController, AnimationStatus, UpdateScheduler, Vsync, VsyncRegistration,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::Radius;
use flui_sdk::interaction::GestureEndReason;
use flui_sdk::painting::{Alignment, Clip};
use flui_sdk::painting::{BorderRadius, BorderRadiusExt, Color};
use flui_sdk::rendering::{BoxConstraints, HitTestBehavior};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{GlobalKey, RebuildHandle, impl_inherited_view};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Align, ColoredBox, ConstrainedBox, GestureDetector, MediaQuery, SizedBox, Stack,
};

use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// Default width of a [`Drawer`].
pub const DEFAULT_DRAWER_WIDTH: f64 = 304.0;
/// Default width of the closed-state edge-drag detection zone — `_kEdgeDragWidth`.
const EDGE_DRAG_WIDTH: f64 = 20.0;
/// Fling-velocity threshold, in normalized (value/second) units —
/// `_kMinFlingVelocity`.
const MIN_FLING_VELOCITY: f64 = 365.0;
/// The drawer's settle-animation duration — `_kBaseSettleDuration`.
const BASE_SETTLE_DURATION: Duration = Duration::from_millis(246);
/// M3 default elevation.
const ELEVATION: f64 = 1.0;
/// M3 default corner radius on the drawer's end-facing edge.
const CORNER_RADIUS: f64 = 16.0;
/// `Colors.black54` (`material/colors.dart`) — the default drawer scrim.
const BLACK54: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0x8A,
};

/// Which edge of the [`crate::Scaffold`] a drawer slides in from.
///
/// RTL mirroring is a named deferral — see the module docs — so
/// `Start`/`End` map directly to left/right rather than resolving against
/// `Directionality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerAlignment {
    /// The start (left, under the LTR-only mapping this substrate uses) edge.
    Start,
    /// The end (right, under the LTR-only mapping this substrate uses) edge.
    End,
}

/// Publishes the enclosing [`DrawerController`]'s [`DrawerAlignment`] to its
/// mounted content, so a [`Drawer`] can pick the correctly-mirrored rounded
/// corner. Private: `DrawerController` is the only publisher, `Drawer` the
/// only reader.
#[derive(Clone)]
struct DrawerAlignmentScope {
    alignment: DrawerAlignment,
    child: BoxedView,
}

impl InheritedView for DrawerAlignmentScope {
    type Data = DrawerAlignment;

    fn data(&self) -> &Self::Data {
        &self.alignment
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.alignment != old.alignment
    }
}

impl_inherited_view!(DrawerAlignmentScope);

/// The end-rounded [`MaterialShape`] for `alignment` — 16dp on the edge
/// facing the scaffold's interior, sharp on the edge flush with the screen.
/// LTR-only, matching [`DrawerAlignment`]'s own documented scope.
fn end_rounded_shape(alignment: DrawerAlignment) -> MaterialShape {
    let rounded = Radius::circular(CORNER_RADIUS);
    let square = Radius::ZERO;
    match alignment {
        // top_left, top_right, bottom_right, bottom_left.
        DrawerAlignment::Start => {
            MaterialShape::RoundedRect(BorderRadius::only(square, rounded, rounded, square))
        }
        DrawerAlignment::End => {
            MaterialShape::RoundedRect(BorderRadius::only(rounded, square, square, rounded))
        }
    }
}

/// A Material Design panel that slides in horizontally to show navigation
/// links, set on [`crate::Scaffold::drawer`]/[`crate::Scaffold::end_drawer`].
///
/// M3 styling:
/// [`ColorScheme::surface_container_low`](crate::ColorScheme::surface_container_low)
/// background, elevation `1.0`, a 16dp end-rounded shape (mirrored for
/// [`DrawerAlignment::End`]), width [`DEFAULT_DRAWER_WIDTH`] (304.0) by
/// default. [`crate::theme_data::ThemeData`] has no `DrawerTheme` extension
/// slot yet — see the module docs.
///
/// # Examples
///
/// ```rust
/// use flui_material::Drawer;
/// use flui_sdk::widgets::Text;
///
/// let _drawer = Drawer::new().child(Text::new("Navigation"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Drawer {
    background_color: Option<Color>,
    elevation: f64,
    width: f64,
    child: Option<BoxedView>,
}

impl Drawer {
    /// A drawer with M3 defaults and no content.
    #[must_use]
    pub fn new() -> Self {
        Self {
            background_color: None,
            elevation: ELEVATION,
            width: DEFAULT_DRAWER_WIDTH,
            child: None,
        }
    }

    /// Overrides the panel's background color. Defaults to
    /// `ColorScheme.surfaceContainerLow`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the panel's elevation (must be non-negative). Defaults to
    /// `1.0`.
    #[must_use]
    pub fn elevation(mut self, elevation: f64) -> Self {
        debug_assert!(elevation >= 0.0, "Drawer elevation must be non-negative");
        self.elevation = elevation;
        self
    }

    /// Overrides the panel's width. Defaults to [`DEFAULT_DRAWER_WIDTH`] (304.0) — see the
    /// module docs on why this value, not a live measurement, is what drives
    /// [`DrawerController`]'s drag math.
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        debug_assert!(width > 0.0, "Drawer width must be positive");
        self.width = width;
        self
    }

    /// Sets the panel's content — typically a `ListView` of navigation items.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view().boxed());
        self
    }

    /// The configured width — what [`crate::Scaffold`] passes to
    /// [`DrawerController::panel_width`] when it builds the controller
    /// wrapping this drawer.
    #[must_use]
    pub fn configured_width(&self) -> f64 {
        self.width
    }
}

impl Default for Drawer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Drawer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drawer")
            .field("elevation", &self.elevation)
            .field("width", &self.width)
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl StatelessView for Drawer {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        // No dependency needed: alignment is fixed for the controller's
        // whole life, same reasoning `GestureArenaScope`'s ambient lookup
        // documents.
        let alignment = ctx
            .get::<DrawerAlignmentScope, _>(|scope| *scope.data())
            .unwrap_or(DrawerAlignment::Start);

        let background_color = self
            .background_color
            .unwrap_or(theme.color_scheme.surface_container_low);

        let mut material = Material::new(background_color)
            .elevation(self.elevation)
            .shape(end_rounded_shape(alignment))
            .clip_behavior(Clip::AntiAlias);
        if let Some(child) = &self.child {
            material = material.child(child.clone());
        }

        ConstrainedBox::new(BoxConstraints::UNCONSTRAINED.tighten(Some(self.width), None))
            .child(material)
    }
}

/// An owned, `Rc`-based (owner-affine, **not** `Send`/`Sync`) capability to
/// open/close a [`crate::Scaffold`]'s drawer/end-drawer from anywhere in its
/// subtree. Published via `ScaffoldScope` (`crate::ScaffoldScope::of`/
/// `maybe_of`). See the module docs for why this stays `!Send`.
#[derive(Clone, Debug)]
pub struct DrawerHandle {
    shared: Rc<DrawerHandleShared>,
}

#[derive(Debug)]
struct DrawerHandleShared {
    drawer_key: GlobalKey<DrawerControllerState>,
    end_drawer_key: GlobalKey<DrawerControllerState>,
    has_drawer: Cell<bool>,
    has_end_drawer: Cell<bool>,
    drawer_opened: Cell<bool>,
    end_drawer_opened: Cell<bool>,
}

impl DrawerHandle {
    /// A handle to an as-yet-unconfigured scaffold: no drawer, no end
    /// drawer, both closed. [`crate::Scaffold`]'s own state creates one of
    /// these once and keeps it for its whole life.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            shared: Rc::new(DrawerHandleShared {
                drawer_key: GlobalKey::new(),
                end_drawer_key: GlobalKey::new(),
                has_drawer: Cell::new(false),
                has_end_drawer: Cell::new(false),
                drawer_opened: Cell::new(false),
                end_drawer_opened: Cell::new(false),
            }),
        }
    }

    /// The [`GlobalKey`] `crate::Scaffold` must attach to the `DrawerController`
    /// it builds for `Scaffold.drawer` — the other half of the `open_drawer`/
    /// `close_drawer` bridge.
    pub(crate) fn drawer_key(&self) -> GlobalKey<DrawerControllerState> {
        self.shared.drawer_key.clone()
    }

    /// The end-drawer counterpart of [`Self::drawer_key`].
    pub(crate) fn end_drawer_key(&self) -> GlobalKey<DrawerControllerState> {
        self.shared.end_drawer_key.clone()
    }

    /// Records whether `Scaffold.drawer` is currently configured. Called
    /// once per `Scaffold` build.
    pub(crate) fn set_has_drawer(&self, has_drawer: bool) {
        self.shared.has_drawer.set(has_drawer);
    }

    /// Records whether `Scaffold.end_drawer` is currently configured.
    pub(crate) fn set_has_end_drawer(&self, has_end_drawer: bool) {
        self.shared.has_end_drawer.set(has_end_drawer);
    }

    /// Records the drawer's current opened state — the single source of
    /// truth `crate::Scaffold`'s dynamic slot order and `on_drawer_changed`
    /// relay both read.
    pub(crate) fn set_drawer_opened(&self, opened: bool) {
        self.shared.drawer_opened.set(opened);
    }

    /// The end-drawer counterpart of [`Self::set_drawer_opened`].
    pub(crate) fn set_end_drawer_opened(&self, opened: bool) {
        self.shared.end_drawer_opened.set(opened);
    }

    /// Whether [`crate::Scaffold::drawer`] is currently configured.
    #[must_use]
    pub fn has_drawer(&self) -> bool {
        self.shared.has_drawer.get()
    }

    /// Whether [`crate::Scaffold::end_drawer`] is currently configured.
    #[must_use]
    pub fn has_end_drawer(&self) -> bool {
        self.shared.has_end_drawer.get()
    }

    /// Whether the start-side drawer is currently open.
    #[must_use]
    pub fn is_drawer_open(&self) -> bool {
        self.shared.drawer_opened.get()
    }

    /// Whether the end-side drawer is currently open.
    #[must_use]
    pub fn is_end_drawer_open(&self) -> bool {
        self.shared.end_drawer_opened.get()
    }

    /// Opens the start-side drawer, closing the end-side drawer first if it
    /// is open.
    ///
    /// A no-op if no [`crate::Scaffold::drawer`] is mounted, or when called
    /// from inside the frame of the presentation that hosts it — the
    /// [`GlobalKey`] simply resolves to nothing.
    pub fn open_drawer(&self) {
        if self.is_end_drawer_open() {
            self.close_end_drawer();
        }
        let _ = self
            .shared
            .drawer_key
            .with_current_state(DrawerControllerState::open);
    }

    /// Closes the start-side drawer.
    pub fn close_drawer(&self) {
        let _ = self
            .shared
            .drawer_key
            .with_current_state(DrawerControllerState::close);
    }

    /// Opens the end-side drawer, closing the start-side drawer first if it
    /// is open.
    pub fn open_end_drawer(&self) {
        if self.is_drawer_open() {
            self.close_drawer();
        }
        let _ = self
            .shared
            .end_drawer_key
            .with_current_state(DrawerControllerState::open);
    }

    /// Closes the end-side drawer.
    pub fn close_end_drawer(&self) {
        let _ = self
            .shared
            .end_drawer_key
            .with_current_state(DrawerControllerState::close);
    }
}

/// Signature for [`DrawerController::on_open_changed`].
type DrawerCallback = Rc<dyn Fn(&mut EventCx<'_>, bool)>;
type BoundDrawerCallback = Rc<dyn Fn(bool)>;

/// Provides interactive behavior for [`Drawer`] content: open/close
/// animation, edge-swipe-to-open, drag-to-close, and the scrim. Built by
/// [`crate::Scaffold`] — rarely constructed directly.
#[derive(Clone)]
pub struct DrawerController {
    key: GlobalKey<DrawerControllerState>,
    alignment: DrawerAlignment,
    child: BoxedView,
    panel_width: f64,
    is_open: bool,
    on_open_changed: Option<DrawerCallback>,
    scrim_color: Option<Color>,
    edge_drag_width: Option<f64>,
    enable_open_drag_gesture: bool,
    barrier_dismissible: bool,
}

impl DrawerController {
    /// Creates a controller for `child` (typically a [`Drawer`]), keyed by
    /// `key` — `crate::Scaffold` keeps one long-lived key per slot so
    /// [`DrawerHandle`] can reach the mounted state later.
    #[must_use]
    pub fn new(
        key: GlobalKey<DrawerControllerState>,
        alignment: DrawerAlignment,
        child: impl IntoView,
    ) -> Self {
        Self {
            key,
            alignment,
            child: child.into_view().boxed(),
            panel_width: DEFAULT_DRAWER_WIDTH,
            is_open: false,
            on_open_changed: None,
            scrim_color: None,
            edge_drag_width: None,
            enable_open_drag_gesture: true,
            barrier_dismissible: true,
        }
    }

    /// The drag divisor — see the module docs' named-divergence note.
    /// Defaults to [`DEFAULT_DRAWER_WIDTH`].
    #[must_use]
    pub fn panel_width(mut self, panel_width: f64) -> Self {
        self.panel_width = panel_width;
        self
    }

    /// Whether the drawer should render open. Primarily used by
    /// `crate::Scaffold` to reflect its own tracked opened-state back into a
    /// freshly (re)built controller. Ignored while the controller is
    /// mid-animation (an `is_animating` status guard).
    #[must_use]
    pub fn is_open(mut self, is_open: bool) -> Self {
        self.is_open = is_open;
        self
    }

    /// Called whenever the drawer opens or closes — via drag, fling,
    /// `open()`/`close()`, or the scrim tap.
    #[must_use]
    pub fn on_open_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, bool) -> R + 'static,
        R: EventOutcome,
    {
        self.on_open_changed = Some(crate::event_callback::value_callback(callback));
        self
    }

    /// Overrides the scrim color. Defaults to `Colors.black54`.
    #[must_use]
    pub fn scrim_color(mut self, color: Color) -> Self {
        self.scrim_color = Some(color);
        self
    }

    /// Overrides the closed-state edge-drag detection width. Defaults to
    /// `20.0` plus the ambient safe-area inset on the drawer's edge.
    #[must_use]
    pub fn edge_drag_width(mut self, width: f64) -> Self {
        self.edge_drag_width = Some(width);
        self
    }

    /// Whether the drawer can be opened with an edge-swipe from closed.
    /// Defaults to `true` on every platform — a **named divergence** from
    /// the oracle, which disables this on desktop platforms
    /// (`_buildDrawer`'s `isDesktop` check). FLUI has desktop as a primary
    /// target, not an edge case, so this substrate exposes the choice
    /// directly instead of hardcoding a platform gate.
    #[must_use]
    pub fn enable_open_drag_gesture(mut self, enabled: bool) -> Self {
        self.enable_open_drag_gesture = enabled;
        self
    }

    /// Whether tapping the scrim closes the drawer. Defaults to `true`.
    #[must_use]
    pub fn barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.barrier_dismissible = dismissible;
        self
    }
}

impl std::fmt::Debug for DrawerController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrawerController")
            .field("alignment", &self.alignment)
            .field("panel_width", &self.panel_width)
            .field("is_open", &self.is_open)
            .field("enable_open_drag_gesture", &self.enable_open_drag_gesture)
            .field("barrier_dismissible", &self.barrier_dismissible)
            .finish_non_exhaustive()
    }
}

/// The interior-mutable state a [`DrawerController`] drives — separated from
/// [`DrawerControllerState`] so it can be `Rc`-cloned into gesture closures
/// while [`DrawerControllerState`] itself stays the single `&self` the
/// [`GlobalKey`] bridge and `ViewState` lifecycle both operate on.
struct DrawerControllerCore {
    controller: AnimationController,
    vsync: RefCell<Option<Vsync>>,
    vsync_registration: RefCell<Option<VsyncRegistration>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    /// Starts `false` regardless of the controller's initial value (a
    /// drawer that starts open still fires one on-changed(true) the first
    /// time its value is nudged, since nothing has "previously" been
    /// recorded as opened yet).
    previously_opened: Cell<bool>,
    alignment: Cell<DrawerAlignment>,
    panel_width: Cell<f64>,
    on_open_changed: RefCell<Option<BoundDrawerCallback>>,
}

impl DrawerControllerCore {
    fn replace_callback(&self, callback: Option<BoundDrawerCallback>) {
        let previous = self.on_open_changed.replace(callback);
        drop(previous);
    }

    fn is_dismissed(&self) -> bool {
        self.controller.status() == AnimationStatus::Dismissed
    }

    fn notify_open_changed(&self, opened: bool) {
        let callback = self.on_open_changed.borrow().clone();
        if let Some(callback) = callback {
            callback(opened);
        }
    }

    /// The slide direction, LTR-only — see the module docs.
    fn direction_factor(&self) -> f64 {
        match self.alignment.get() {
            DrawerAlignment::Start => 1.0,
            DrawerAlignment::End => -1.0,
        }
    }

    /// Fires `on_open_changed` the instant the value crosses `0.5`,
    /// independent of `open()`/`close()`'s own immediate firing — the second
    /// of the three firing paths.
    fn move_by(&self, primary_delta: f64) {
        let width = self.panel_width.get();
        let new_value = self.controller.value() + primary_delta / width * self.direction_factor();
        self.controller.set_value(new_value);

        let opened = self.controller.value() > 0.5;
        if opened != self.previously_opened.get() {
            self.previously_opened.set(opened);
            self.notify_open_changed(opened);
        }
    }

    /// Fires `on_open_changed` immediately when the fling threshold is
    /// crossed (the third firing path — independent of the value later
    /// crossing `0.5` as the fling animates).
    fn settle(&self, primary_velocity: f64) {
        if self.is_dismissed() {
            return;
        }
        let width = self.panel_width.get();
        if primary_velocity.abs() >= MIN_FLING_VELOCITY {
            let visual_velocity = primary_velocity / width * self.direction_factor();
            let _ = self.controller.fling(visual_velocity);
            self.notify_open_changed(visual_velocity > 0.0);
        } else if self.controller.value() < 0.5 {
            self.close();
        } else {
            self.open();
        }
    }

    /// Only reachable from the open panel's gesture detector; the
    /// closed-state edge strip wires no `on_horizontal_drag_cancel`.
    fn handle_drag_cancel(&self) {
        if self.is_dismissed() || self.controller.is_animating() {
            return;
        }
        if self.controller.value() < 0.5 {
            self.close();
        } else {
            self.open();
        }
    }

    /// Fires `on_open_changed` immediately — the first firing path,
    /// independent of the fling animation that follows.
    fn open(&self) {
        let _ = self.controller.fling(1.0);
        self.notify_open_changed(true);
    }

    /// The mirror of [`Self::open`].
    fn close(&self) {
        let _ = self.controller.fling(-1.0);
        self.notify_open_changed(false);
    }
}

/// State for a [`DrawerController`] — see [`DrawerHandle`] for how
/// `crate::Scaffold` reaches this from outside the tree via [`GlobalKey`].
pub struct DrawerControllerState {
    core: Rc<DrawerControllerCore>,
    writer: Option<WriterSource>,
}

impl std::fmt::Debug for DrawerControllerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrawerControllerState")
            .field("is_dismissed", &self.core.is_dismissed())
            .field("value", &self.core.controller.value())
            .finish_non_exhaustive()
    }
}

impl DrawerControllerState {
    /// Starts an animation to open the drawer — the [`GlobalKey`] entry
    /// point [`DrawerHandle::open_drawer`]/[`DrawerHandle::open_end_drawer`]
    /// call.
    pub(crate) fn open(&self) {
        self.core.open();
    }

    /// Starts an animation to close the drawer — the [`GlobalKey`] entry
    /// point [`DrawerHandle::close_drawer`]/[`DrawerHandle::close_end_drawer`]
    /// call.
    pub(crate) fn close(&self) {
        self.core.close();
    }
}

impl StatefulView for DrawerController {
    type State = DrawerControllerState;

    fn create_state(&self) -> Self::State {
        let controller = AnimationController::new(BASE_SETTLE_DURATION, &UpdateScheduler::new());
        if self.is_open {
            controller.set_value(1.0);
        }
        DrawerControllerState {
            writer: None,
            core: Rc::new(DrawerControllerCore {
                controller,
                vsync: RefCell::new(None),
                vsync_registration: RefCell::new(None),
                rebuild: RefCell::new(None),
                previously_opened: Cell::new(false),
                alignment: Cell::new(self.alignment),
                panel_width: Cell::new(self.panel_width),
                on_open_changed: RefCell::new(None),
            }),
        }
    }
}

impl ViewState<DrawerController> for DrawerControllerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.writer = Some(ctx.writer_source());
        let rebuild = ctx.rebuild_handle();
        let _prev = self.core.rebuild.borrow_mut().replace(rebuild.clone());

        // No dependency: the vsync handle never changes for this
        // controller's life (same reasoning `GestureDetectorState::init_state`
        // documents for its own ambient-arena lookup).
        let vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        if let Some(vsync) = &vsync {
            let registration = vsync.register(self.core.controller.clone());
            *self.core.vsync_registration.borrow_mut() = Some(registration);
        }
        let _prev = std::mem::replace(&mut *self.core.vsync.borrow_mut(), vsync);

        // One listener pair covers every path that must rebuild: a value
        // tick (drag `set_value`, or a fling/forward settling frame-by-frame)
        // and a status transition (Dismissed -> Forward/Reverse the instant
        // `open()`/`close()`/`fling()` is called) — the latter is what makes
        // the panel mount on the SAME build the animation starts, at value
        // 0: the mount gates on `is_dismissed()` (status), not on the value
        // reaching some threshold, so there is no intermediate frame where
        // the panel is visible at a stale, already-open-looking position.
        let rebuild_for_value = rebuild.clone();
        self.core.controller.add_listener(Arc::new(move || {
            rebuild_for_value.schedule(flui_sdk::view::RebuildReason::AnimationTick);
        }));
        let rebuild_for_status = rebuild;
        self.core
            .controller
            .add_status_listener(Arc::new(move |_status| {
                rebuild_for_status.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }));
    }

    fn did_update_view(&mut self, old_view: &DrawerController, new_view: &DrawerController) {
        // Check the controller's status, not its ticker-based
        // `isAnimating`. Setting `AnimationController.value` during a drag
        // stops the ticker but intentionally leaves an interior value in a
        // directional status. That distinction prevents Scaffold's
        // threshold callback from feeding `is_open` back into this state and
        // snapping an in-progress drag to an endpoint.
        if self.core.controller.status().is_running() {
            return;
        }
        if new_view.is_open != old_view.is_open {
            self.core
                .controller
                .set_value(if new_view.is_open { 1.0 } else { 0.0 });
        }
    }

    fn build(&self, view: &DrawerController, ctx: &dyn BuildContext) -> impl IntoView {
        self.core.panel_width.set(view.panel_width);
        self.core.alignment.set(view.alignment);
        let writer = self
            .writer
            .clone()
            .expect("BUG: drawer lifecycle precedes build");
        let callback = view.on_open_changed.clone().map(|callback| {
            Rc::new(move |opened| writer.write(|cx| callback(cx, opened))) as Rc<dyn Fn(bool)>
        });
        self.core.replace_callback(callback);

        let media_query = MediaQuery::of(ctx);
        let side_inset = match view.alignment {
            DrawerAlignment::Start => media_query.padding.left,
            DrawerAlignment::End => media_query.padding.right,
        };
        let drag_area_width = view.edge_drag_width.unwrap_or(EDGE_DRAG_WIDTH + side_inset);

        if self.core.is_dismissed() {
            if view.enable_open_drag_gesture {
                closed_edge_strip(&self.core, view.alignment, drag_area_width)
                    .into_view()
                    .boxed()
            } else {
                SizedBox::shrink().into_view().boxed()
            }
        } else {
            open_panel(&self.core, view).into_view().boxed()
        }
    }

    fn dispose(&mut self) {
        self.core.replace_callback(None);
        self.writer = None;
        if let (Some(vsync), Some(registration)) = (
            self.core.vsync.borrow_mut().take(),
            self.core.vsync_registration.borrow_mut().take(),
        ) {
            vsync.unregister(registration);
        }
        self.core.controller.dispose();
    }
}

impl View for DrawerController {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn flui_sdk::foundation::ViewKey> {
        Some(&self.key)
    }
}

/// The closed-state edge-drag strip — `translucent` hit-testing (the body
/// stays tappable both inside and outside its bounds), only mounted when
/// [`DrawerController::enable_open_drag_gesture`] is set.
fn closed_edge_strip(
    core: &Rc<DrawerControllerCore>,
    alignment: DrawerAlignment,
    drag_area_width: f64,
) -> impl IntoView {
    let move_core = Rc::clone(core);
    let settle_core = Rc::clone(core);
    // `Align` measures its child against LOOSE constraints (0..available),
    // even though the scaffold's own drawer slot is tight — a
    // `SizedBox::width` (height passed through) would collapse to zero
    // height under that looseness. Forcing `f64::INFINITY` clamps to
    // whatever height Align's loose upper bound actually is (the slot's
    // full, bounded height — Scaffold's own `get_size` already requires
    // bounded constraints from ITS parent, so this is never truly
    // unbounded) — a `SizedBox(height: f64::INFINITY)`; the
    // unbounded-height guard is skipped as a named
    // simplification (see the type docs).
    Align::new(outer_alignment(alignment)).child(
        GestureDetector::new()
            .on_horizontal_drag_update(move |_cx, details| move_core.move_by(details.primary_delta))
            .on_horizontal_drag_end(move |_cx, details| {
                settle_core.settle(match details.reason {
                    GestureEndReason::Completed => details.primary_velocity,
                    GestureEndReason::Cancelled => 0.0,
                });
            })
            .behavior(HitTestBehavior::Translucent)
            .child(SizedBox::new(drag_area_width, f64::INFINITY)),
    )
}

/// The open-state scrim + panel, wrapped in the drag-to-close detector.
fn open_panel(core: &Rc<DrawerControllerCore>, view: &DrawerController) -> impl IntoView {
    let value = core.controller.value();

    let scrim_color = scale_alpha(view.scrim_color.unwrap_or(BLACK54), value);
    let mut scrim_detector = GestureDetector::new();
    if view.barrier_dismissible {
        let close_core = Rc::clone(core);
        scrim_detector = scrim_detector.on_tap(move |_cx| close_core.close());
    }
    // `Stack` gives a non-positioned child LOOSE constraints (the default
    // `StackFit.loose`) — a bare `ColoredBox` (no size of its own)
    // collapses to zero under that looseness, same as the edge strip's
    // `SizedBox` needed `f64::INFINITY` above. `SizedBox::expand` clamps to
    // the Stack's own (bounded — the drawer slot is always tight) size, so
    // the scrim genuinely covers, and is tappable across, the whole area.
    let scrim = scrim_detector.child(SizedBox::expand().child(ColoredBox::new(scrim_color)));

    let panel = Align::new(outer_alignment(view.alignment)).child(
        Align::new(inner_alignment(view.alignment))
            .width_factor(value)
            .child(view.child.clone()),
    );

    let scoped = DrawerAlignmentScope {
        alignment: view.alignment,
        child: Stack::new(vec![scrim.boxed(), panel.boxed()]).boxed(),
    };

    let down_core = Rc::clone(core);
    let update_core = Rc::clone(core);
    let end_core = Rc::clone(core);
    let cancel_core = Rc::clone(core);
    GestureDetector::new()
        .on_horizontal_drag_down(
            move |_cx, _details: flui_sdk::interaction::DragDownDetails| {
                let _ = down_core.controller.stop();
            },
        )
        .on_horizontal_drag_update(move |_cx, details| update_core.move_by(details.primary_delta))
        .on_horizontal_drag_end(move |_cx, details| {
            end_core.settle(match details.reason {
                GestureEndReason::Completed => details.primary_velocity,
                GestureEndReason::Cancelled => 0.0,
            });
        })
        .on_horizontal_drag_cancel(move |_cx| cancel_core.handle_drag_cancel())
        .child(scoped)
}

fn outer_alignment(alignment: DrawerAlignment) -> Alignment {
    match alignment {
        DrawerAlignment::Start => Alignment::CENTER_LEFT,
        DrawerAlignment::End => Alignment::CENTER_RIGHT,
    }
}

fn inner_alignment(alignment: DrawerAlignment) -> Alignment {
    match alignment {
        DrawerAlignment::Start => Alignment::CENTER_RIGHT,
        DrawerAlignment::End => Alignment::CENTER_LEFT,
    }
}

/// Scales `color`'s alpha channel by `factor` (clamped to `[0, 1]`); the
/// scrim's alpha is `scrim_color.a * controller.value`.
fn scale_alpha(color: Color, factor: f64) -> Color {
    let factor = factor.clamp(0.0, 1.0);
    let scaled = (f64::from(color.a) * factor).round().clamp(0.0, 255.0);
    let alpha = scaled as u8;
    color.with_alpha(alpha)
}
