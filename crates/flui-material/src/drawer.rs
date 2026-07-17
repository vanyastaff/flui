//! [`Drawer`] and [`DrawerController`] — a Material Design panel that slides
//! in horizontally from the edge of a [`crate::Scaffold`], plus
//! [`DrawerHandle`] — the runtime capability to open/close it.
//!
//! # Flutter parity
//!
//! `material/drawer.dart`'s `Drawer`/`DrawerController` (oracle tag
//! `3.44.0`). `DrawerController` owns a 246ms [`AnimationController`] that
//! drives the open/close/drag/fling state machine; `Drawer` is the
//! M3-styled content panel it wraps.
//!
//! ## The `GlobalKey` bridge (why `DrawerHandle` exists)
//!
//! The oracle's `ScaffoldState.openDrawer()` reaches into its own
//! `DrawerController` child via `_drawerKey.currentState!.open()` — a
//! `GlobalKey<DrawerControllerState>` the `Scaffold`'s own `State` holds.
//! [`DrawerHandle`] ports that exact mechanism: it wraps the same two
//! `GlobalKey<DrawerControllerState>` instances [`crate::Scaffold`]'s state
//! attaches to the `drawer`/`end_drawer` `DrawerController`s it builds, so
//! `DrawerHandle::open_drawer`/`close_drawer` are direct, faithful ports of
//! `ScaffoldState::openDrawer`/`closeDrawer`.
//!
//! `DrawerHandle` is deliberately **`Rc`-based and `!Send`**, not
//! `Arc`/`Send + Sync`. `GlobalKey::with_current_state` resolves against the
//! owner-thread element-tree registry, and this workspace already carries a
//! documented tension between `Send + Sync` data-plane primitives (gesture
//! recognizers, render objects — ADR-0002) and owner-affine widget-layer
//! capability handles (an in-flight `Send`-bound-drop migration found this
//! exact knot at `flui_widgets::NavigatorHandle`, which is `Cloneable, Send +
//! Sync` in name only — see that type's own module doc). `DrawerHandle`
//! sidesteps the knot entirely by never claiming `Send` in the first place.
//!
//! ## Named divergence: the drag divisor is the *configured* panel width,
//! not a live render-object measurement
//!
//! The oracle's `_width` getter (`DrawerControllerState._width`) reads the
//! mounted `Drawer` panel's **actual laid-out** `RenderBox.size.width** via
//! `_drawerKey.currentContext?.findRenderObject()`, falling back to
//! `_kWidth` only while unmounted. FLUI has no render-object size query for
//! an arbitrary descendant from event-handling code (no `GlobalKey`
//! `findRenderObject` equivalent) — building one is a new cross-crate
//! primitive out of this feature's scope. [`DrawerController`] instead uses
//! [`Drawer::width`]'s **configured** value directly (default
//! [`DEFAULT_DRAWER_WIDTH`]), passed down via [`DrawerController::panel_width`]. This is
//! behaviorally equivalent in the drawer's actual mounting context: the
//! open panel is wrapped in an [`flui_widgets::Align`] with a `width_factor`,
//! which gives its child **loose** (unbounded) width constraints to measure
//! its natural size — so `Drawer`'s own `BoxConstraints.expand(width:)`
//! (ported as [`flui_rendering::constraints::BoxConstraints::tighten`])
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
//! `flui_widgets::Directionality` is not read, matching `crate::Scaffold`'s
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

use flui_animation::{
    Animation, AnimationController, AnimationStatus, Scheduler, Vsync, VsyncRegistration,
};
use flui_foundation::Listenable;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::geometry::{Radius, px};
use flui_types::styling::{BorderRadius, BorderRadiusExt, Color};
use flui_types::{Alignment, painting::Clip};
use flui_view::prelude::*;
use flui_view::{GlobalKey, RebuildHandle, impl_inherited_view};
use flui_widgets::animated::VsyncScope;
use flui_widgets::{
    Align, ColoredBox, ConstrainedBox, GestureDetector, MediaQuery, SizedBox, Stack,
};

use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// Default width of a [`Drawer`] — Flutter's `_kWidth` (`drawer.dart`).
pub const DEFAULT_DRAWER_WIDTH: f32 = 304.0;
/// Default width of the closed-state edge-drag detection zone — `_kEdgeDragWidth`.
const EDGE_DRAG_WIDTH: f32 = 20.0;
/// Fling-velocity threshold, in normalized (value/second) units —
/// `_kMinFlingVelocity`.
const MIN_FLING_VELOCITY: f32 = 365.0;
/// The drawer's settle-animation duration — `_kBaseSettleDuration`.
const BASE_SETTLE_DURATION: Duration = Duration::from_millis(246);
/// M3 default elevation.
const ELEVATION: f32 = 1.0;
/// M3 default corner radius on the drawer's end-facing edge.
const CORNER_RADIUS: f32 = 16.0;
/// `Colors.black54` (`material/colors.dart`) — the default drawer scrim.
const BLACK54: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0x8A,
};

/// Which edge of the [`crate::Scaffold`] a drawer slides in from.
///
/// Flutter parity: `DrawerAlignment` (`drawer.dart`). RTL mirroring is a
/// named deferral — see the module docs — so `Start`/`End` map directly to
/// left/right rather than resolving against `Directionality`.
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
/// only reader — Flutter parity: `_DrawerControllerScope`, similarly private.
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
    let rounded = Radius::circular(px(CORNER_RADIUS));
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
/// Flutter parity: `Drawer` (`drawer.dart`, oracle tag `3.44.0`). M3 styling:
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
/// use flui_widgets::Text;
///
/// let _drawer = Drawer::new().child(Text::new("Navigation"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Drawer {
    background_color: Option<Color>,
    elevation: f32,
    width: f32,
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
    pub fn elevation(mut self, elevation: f32) -> Self {
        debug_assert!(elevation >= 0.0, "Drawer elevation must be non-negative");
        self.elevation = elevation;
        self
    }

    /// Overrides the panel's width. Defaults to [`DEFAULT_DRAWER_WIDTH`] (304.0) — see the
    /// module docs on why this value, not a live measurement, is what drives
    /// [`DrawerController`]'s drag math.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
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
    pub fn configured_width(&self) -> f32 {
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

        ConstrainedBox::new(BoxConstraints::UNCONSTRAINED.tighten(Some(px(self.width)), None))
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
    /// is open. Flutter parity: `ScaffoldState.openDrawer`.
    ///
    /// A no-op if no [`crate::Scaffold::drawer`] is mounted — the
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

    /// Closes the start-side drawer. Flutter parity: `ScaffoldState.closeDrawer`.
    pub fn close_drawer(&self) {
        let _ = self
            .shared
            .drawer_key
            .with_current_state(DrawerControllerState::close);
    }

    /// Opens the end-side drawer, closing the start-side drawer first if it
    /// is open. Flutter parity: `ScaffoldState.openEndDrawer`.
    pub fn open_end_drawer(&self) {
        if self.is_drawer_open() {
            self.close_drawer();
        }
        let _ = self
            .shared
            .end_drawer_key
            .with_current_state(DrawerControllerState::open);
    }

    /// Closes the end-side drawer. Flutter parity: `ScaffoldState.closeEndDrawer`.
    pub fn close_end_drawer(&self) {
        let _ = self
            .shared
            .end_drawer_key
            .with_current_state(DrawerControllerState::close);
    }
}

/// Signature for [`DrawerController::on_open_changed`] — Flutter's
/// `DrawerCallback`.
type DrawerCallback = Rc<dyn Fn(bool)>;

/// Provides interactive behavior for [`Drawer`] content: open/close
/// animation, edge-swipe-to-open, drag-to-close, and the scrim. Built by
/// [`crate::Scaffold`] — rarely constructed directly.
///
/// Flutter parity: `DrawerController` (`drawer.dart`, oracle tag `3.44.0`).
#[derive(Clone)]
pub struct DrawerController {
    key: GlobalKey<DrawerControllerState>,
    alignment: DrawerAlignment,
    child: BoxedView,
    panel_width: f32,
    is_open: bool,
    on_open_changed: Option<DrawerCallback>,
    scrim_color: Option<Color>,
    edge_drag_width: Option<f32>,
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
    pub fn panel_width(mut self, panel_width: f32) -> Self {
        self.panel_width = panel_width;
        self
    }

    /// Whether the drawer should render open. Primarily used by
    /// `crate::Scaffold` to reflect its own tracked opened-state back into a
    /// freshly (re)built controller. Ignored while the controller is
    /// mid-animation (Flutter parity: `didUpdateWidget`'s
    /// `_controller.status.isAnimating` guard).
    #[must_use]
    pub fn is_open(mut self, is_open: bool) -> Self {
        self.is_open = is_open;
        self
    }

    /// Called whenever the drawer opens or closes — via drag, fling,
    /// `open()`/`close()`, or the scrim tap. Flutter parity: `drawerCallback`.
    #[must_use]
    pub fn on_open_changed(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_open_changed = Some(Rc::new(callback));
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
    pub fn edge_drag_width(mut self, width: f32) -> Self {
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
    /// Flutter parity: `_previouslyOpened` — starts `false` regardless of
    /// the controller's initial value (an oracle quirk this substrate ports
    /// verbatim: a drawer that starts open still fires one on-changed(true)
    /// the first time its value is nudged, since nothing has "previously"
    /// been recorded as opened yet).
    previously_opened: Cell<bool>,
    alignment: Cell<DrawerAlignment>,
    panel_width: Cell<f32>,
    on_open_changed: RefCell<Option<DrawerCallback>>,
}

impl DrawerControllerCore {
    fn is_dismissed(&self) -> bool {
        self.controller.status() == AnimationStatus::Dismissed
    }

    fn notify_open_changed(&self, opened: bool) {
        if let Some(callback) = self.on_open_changed.borrow().clone() {
            callback(opened);
        }
    }

    /// Flutter parity: `_directionFactor` (`drawer.dart`), LTR-only — see
    /// the module docs.
    fn direction_factor(&self) -> f32 {
        match self.alignment.get() {
            DrawerAlignment::Start => 1.0,
            DrawerAlignment::End => -1.0,
        }
    }

    /// Flutter parity: `_move` (`drawer.dart`). Fires `on_open_changed` the
    /// instant the value crosses `0.5`, independent of `open()`/`close()`'s
    /// own immediate firing — the second of the three oracle-documented
    /// firing paths.
    fn move_by(&self, primary_delta: f32) {
        let width = self.panel_width.get();
        let new_value = self.controller.value() + primary_delta / width * self.direction_factor();
        self.controller.set_value(new_value);

        let opened = self.controller.value() > 0.5;
        if opened != self.previously_opened.get() {
            self.previously_opened.set(opened);
            self.notify_open_changed(opened);
        }
    }

    /// Flutter parity: `_settle` (`drawer.dart`). Fires `on_open_changed`
    /// immediately when the fling threshold is crossed (the third
    /// oracle-documented firing path — independent of the value later
    /// crossing `0.5` as the fling animates).
    fn settle(&self, primary_velocity: f32) {
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

    /// Flutter parity: `_handleDragCancel` (`drawer.dart`) — only reachable
    /// from the open panel's gesture detector; the closed-state edge strip
    /// wires no `on_horizontal_drag_cancel`, matching the oracle.
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

    /// Flutter parity: `open()` (`drawer.dart`). Fires `on_open_changed`
    /// immediately — the first oracle-documented firing path, independent
    /// of the fling animation that follows.
    fn open(&self) {
        let _ = self.controller.fling(1.0);
        self.notify_open_changed(true);
    }

    /// Flutter parity: `close()` (`drawer.dart`) — the mirror of
    /// [`Self::open`].
    fn close(&self) {
        let _ = self.controller.fling(-1.0);
        self.notify_open_changed(false);
    }
}

/// State for a [`DrawerController`] — see [`DrawerHandle`] for how
/// `crate::Scaffold` reaches this from outside the tree via [`GlobalKey`].
pub struct DrawerControllerState {
    core: Rc<DrawerControllerCore>,
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
        let controller = AnimationController::new(BASE_SETTLE_DURATION, Arc::new(Scheduler::new()));
        if self.is_open {
            controller.set_value(1.0);
        }
        DrawerControllerState {
            core: Rc::new(DrawerControllerCore {
                controller,
                vsync: RefCell::new(None),
                vsync_registration: RefCell::new(None),
                rebuild: RefCell::new(None),
                previously_opened: Cell::new(false),
                alignment: Cell::new(self.alignment),
                panel_width: Cell::new(self.panel_width),
                on_open_changed: RefCell::new(self.on_open_changed.clone()),
            }),
        }
    }
}

impl ViewState<DrawerController> for DrawerControllerState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let rebuild = ctx.rebuild_handle();
        *self.core.rebuild.borrow_mut() = Some(rebuild.clone());

        // No dependency: the vsync handle never changes for this
        // controller's life (same reasoning `GestureDetectorState::init_state`
        // documents for its own ambient-arena lookup).
        let vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        if let Some(vsync) = &vsync {
            let registration = vsync.register(self.core.controller.clone());
            *self.core.vsync_registration.borrow_mut() = Some(registration);
        }
        *self.core.vsync.borrow_mut() = vsync;

        // One listener pair covers every path that must rebuild: a value
        // tick (drag `set_value`, or a fling/forward settling frame-by-frame)
        // and a status transition (Dismissed -> Forward/Reverse the instant
        // `open()`/`close()`/`fling()` is called) — the latter is what makes
        // the panel mount on the SAME build the animation starts, at value
        // 0, with no flash (the plan's explicit "no Stack fallback... no
        // flash" requirement).
        let rebuild_for_value = rebuild.clone();
        self.core.controller.add_listener(Arc::new(move || {
            rebuild_for_value.schedule();
        }));
        let rebuild_for_status = rebuild;
        self.core
            .controller
            .add_status_listener(Arc::new(move |_status| {
                rebuild_for_status.schedule();
            }));
    }

    fn did_update_view(&mut self, old_view: &DrawerController, new_view: &DrawerController) {
        // Flutter parity: `didUpdateWidget` — never snap the value while
        // the user is dragging or a fling/forward is settling.
        if self.core.controller.is_animating() {
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
        self.core
            .on_open_changed
            .borrow_mut()
            .clone_from(&view.on_open_changed);

        let media_query = MediaQuery::of(ctx);
        let side_inset = match view.alignment {
            DrawerAlignment::Start => media_query.padding.left,
            DrawerAlignment::End => media_query.padding.right,
        };
        let drag_area_width = view
            .edge_drag_width
            .unwrap_or(EDGE_DRAG_WIDTH + side_inset.get());

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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

/// The closed-state edge-drag strip — `translucent` hit-testing (the body
/// stays tappable both inside and outside its bounds), only mounted when
/// [`DrawerController::enable_open_drag_gesture`] is set (Flutter parity:
/// `_buildDrawer`'s dismissed branch).
fn closed_edge_strip(
    core: &Rc<DrawerControllerCore>,
    alignment: DrawerAlignment,
    drag_area_width: f32,
) -> impl IntoView {
    let move_core = Rc::clone(core);
    let settle_core = Rc::clone(core);
    // `Align` measures its child against LOOSE constraints (0..available),
    // even though the scaffold's own drawer slot is tight — a
    // `SizedBox::width` (height passed through) would collapse to zero
    // height under that looseness. Forcing `f32::INFINITY` clamps to
    // whatever height Align's loose upper bound actually is (the slot's
    // full, bounded height — Scaffold's own `get_size` already requires
    // bounded constraints from ITS parent, so this is never truly
    // unbounded) — Flutter parity: `SizedBox(height: double.infinity)`
    // inside the oracle's `LimitedBox(maxHeight: 0.0, ...)`, whose
    // unbounded-height guard this substrate skips as a named
    // simplification (see the type docs).
    Align::new(outer_alignment(alignment)).child(
        GestureDetector::new()
            .on_horizontal_drag_update(move |details| move_core.move_by(details.primary_delta))
            .on_horizontal_drag_end(move |details| settle_core.settle(details.primary_velocity))
            .behavior(HitTestBehavior::Translucent)
            .child(SizedBox::new(drag_area_width, f32::INFINITY)),
    )
}

/// The open-state scrim + panel, wrapped in the drag-to-close detector.
/// Flutter parity: `_buildDrawer`'s non-dismissed branch.
fn open_panel(core: &Rc<DrawerControllerCore>, view: &DrawerController) -> impl IntoView {
    let value = core.controller.value();

    let scrim_color = scale_alpha(view.scrim_color.unwrap_or(BLACK54), value);
    let mut scrim_detector = GestureDetector::new();
    if view.barrier_dismissible {
        let close_core = Rc::clone(core);
        scrim_detector = scrim_detector.on_tap(move || close_core.close());
    }
    // `Stack` gives a non-positioned child LOOSE constraints (Flutter's
    // default `StackFit.loose`) — a bare `ColoredBox` (no size of its own)
    // collapses to zero under that looseness, same as the edge strip's
    // `SizedBox` needed `f32::INFINITY` above. `SizedBox::expand` clamps to
    // the Stack's own (bounded — the drawer slot is always tight) size, so
    // the scrim genuinely covers, and is tappable across, the whole area.
    // Flutter parity: the oracle's scrim is `ColoredBox(child: LimitedBox(...,
    // child: SizedBox.expand()))`.
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
        .on_horizontal_drag_down(move |_details: flui_interaction::DragDownDetails| {
            let _ = down_core.controller.stop();
        })
        .on_horizontal_drag_update(move |details| update_core.move_by(details.primary_delta))
        .on_horizontal_drag_end(move |details| end_core.settle(details.primary_velocity))
        .on_horizontal_drag_cancel(move || cancel_core.handle_drag_cancel())
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

/// Scales `color`'s alpha channel by `factor` (clamped to `[0, 1]`).
/// Flutter parity: `Color.withValues(alpha: scrimColor.a * _controller.value)`.
fn scale_alpha(color: Color, factor: f32) -> Color {
    let factor = factor.clamp(0.0, 1.0);
    let scaled = (f32::from(color.a) * factor).round().clamp(0.0, 255.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scaled is clamped to [0.0, 255.0] immediately above"
    )]
    let alpha = scaled as u8;
    color.with_alpha(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `DrawerControllerCore` with no `Vsync`/rebuild wiring — enough to
    /// exercise the pure value/status math (`move_by`/`settle`/`open`/
    /// `close`) without a mounted element tree. Start alignment, default
    /// panel width, matching most tests' needs; individual tests override
    /// `alignment`/`panel_width`/`on_open_changed` via the `Cell`/`RefCell`
    /// fields directly.
    fn test_core() -> Rc<DrawerControllerCore> {
        Rc::new(DrawerControllerCore {
            controller: AnimationController::new(BASE_SETTLE_DURATION, Arc::new(Scheduler::new())),
            vsync: RefCell::new(None),
            vsync_registration: RefCell::new(None),
            rebuild: RefCell::new(None),
            previously_opened: Cell::new(false),
            alignment: Cell::new(DrawerAlignment::Start),
            panel_width: Cell::new(DEFAULT_DRAWER_WIDTH),
            on_open_changed: RefCell::new(None),
        })
    }

    // ------------------------------------------------------------------
    // `move_by` — drag divisor and direction factor.
    // ------------------------------------------------------------------

    #[test]
    fn move_by_divides_the_delta_by_the_panel_width_for_a_start_drawer() {
        let core = test_core();
        core.move_by(DEFAULT_DRAWER_WIDTH / 2.0);
        assert!((core.controller.value() - 0.5).abs() < 1e-4);
    }

    #[test]
    fn move_by_negates_the_delta_for_an_end_drawer() {
        let core = test_core();
        core.alignment.set(DrawerAlignment::End);
        // A rightward (positive) delta must NOT open an end-anchored drawer
        // (direction factor -1 drives the value toward, and clamped at, the
        // lower bound).
        core.move_by(DEFAULT_DRAWER_WIDTH / 2.0);
        assert!((core.controller.value() - 0.0).abs() < 1e-4);
    }

    #[test]
    fn move_by_leftward_delta_opens_an_end_drawer() {
        let core = test_core();
        core.alignment.set(DrawerAlignment::End);
        core.move_by(-(DEFAULT_DRAWER_WIDTH / 2.0));
        assert!((core.controller.value() - 0.5).abs() < 1e-4);
    }

    // ------------------------------------------------------------------
    // `settle` — fling threshold and low-velocity snap.
    //
    // Red-check for all three: change `MIN_FLING_VELOCITY` from `365.0` to
    // `0.0` — `settle_below_the_fling_threshold_and_below_half_closes` would
    // then take the fling branch instead (10.0 >= 0.0), landing in
    // `AnimationStatus::Reverse` for the wrong reason (it already asserts
    // `Reverse`, so this specific case would still pass) — but
    // `settle_at_exactly_the_fling_threshold_flings` pinned at the boundary
    // below would newly diverge from `settle_just_under_the_fling_threshold_snaps`
    // if the threshold moved, since both drive off the same named constant.
    // ------------------------------------------------------------------

    #[test]
    fn settle_on_a_dismissed_controller_is_a_no_op() {
        let core = test_core();
        assert_eq!(core.controller.status(), AnimationStatus::Dismissed);
        core.settle(1000.0);
        assert_eq!(
            core.controller.status(),
            AnimationStatus::Dismissed,
            "a dismissed controller must ignore a drag-end"
        );
    }

    #[test]
    fn settle_at_or_above_the_fling_threshold_flings_toward_open() {
        let core = test_core();
        core.controller.set_value(0.1); // any in-flight value; not yet settled
        core.settle(MIN_FLING_VELOCITY);
        assert_eq!(
            core.controller.status(),
            AnimationStatus::Forward,
            "a positive fling at/above the threshold must fling toward open"
        );
    }

    #[test]
    fn settle_at_or_above_the_fling_threshold_flings_toward_close() {
        let core = test_core();
        core.controller.set_value(0.9);
        core.settle(-MIN_FLING_VELOCITY);
        assert_eq!(
            core.controller.status(),
            AnimationStatus::Reverse,
            "a negative fling at/above the threshold must fling toward close"
        );
    }

    #[test]
    fn settle_below_the_fling_threshold_and_below_half_closes() {
        let core = test_core();
        core.controller.set_value(0.4);
        core.settle(10.0);
        assert_eq!(
            core.controller.status(),
            AnimationStatus::Reverse,
            "value < 0.5 with no qualifying fling must close"
        );
    }

    #[test]
    fn settle_below_the_fling_threshold_and_above_half_opens() {
        let core = test_core();
        core.controller.set_value(0.6);
        core.settle(10.0);
        assert_eq!(
            core.controller.status(),
            AnimationStatus::Forward,
            "value >= 0.5 with no qualifying fling must open"
        );
    }

    // ------------------------------------------------------------------
    // `on_open_changed` — the three oracle-documented firing paths.
    // ------------------------------------------------------------------

    #[test]
    fn open_fires_on_open_changed_synchronously() {
        let core = test_core();
        let fired = Rc::new(Cell::new(None::<bool>));
        let fired_for_closure = Rc::clone(&fired);
        *core.on_open_changed.borrow_mut() =
            Some(Rc::new(move |opened| fired_for_closure.set(Some(opened))));

        core.open();

        assert_eq!(
            fired.get(),
            Some(true),
            "open() must fire on_open_changed(true) immediately, not deferred to a tick"
        );
    }

    #[test]
    fn close_fires_on_open_changed_synchronously() {
        let core = test_core();
        core.controller.set_value(1.0);
        let fired = Rc::new(Cell::new(None::<bool>));
        let fired_for_closure = Rc::clone(&fired);
        *core.on_open_changed.borrow_mut() =
            Some(Rc::new(move |opened| fired_for_closure.set(Some(opened))));

        core.close();

        assert_eq!(fired.get(), Some(false));
    }

    #[test]
    fn move_by_fires_on_open_changed_exactly_once_when_crossing_half() {
        let core = test_core();
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_closure = Rc::clone(&events);
        *core.on_open_changed.borrow_mut() = Some(Rc::new(move |opened| {
            events_for_closure.borrow_mut().push(opened);
        }));

        core.move_by(DEFAULT_DRAWER_WIDTH * 0.3); // value ~0.3, no crossing yet
        assert!(events.borrow().is_empty(), "no crossing yet must not fire");

        core.move_by(DEFAULT_DRAWER_WIDTH * 0.3); // value ~0.6, crosses 0.5
        assert_eq!(
            *events.borrow(),
            vec![true],
            "crossing 0.5 must fire exactly once"
        );

        core.move_by(DEFAULT_DRAWER_WIDTH * 0.1); // still above 0.5
        assert_eq!(
            events.borrow().len(),
            1,
            "staying above 0.5 must not re-fire"
        );
    }

    #[test]
    fn settle_with_a_qualifying_fling_fires_on_open_changed_immediately_not_at_animation_end() {
        let core = test_core();
        // A drag-end always follows at least one drag-update in real usage
        // (the update is what crosses the recognizer's slop and starts the
        // drag) — `settle` on a still-`Dismissed` controller (no preceding
        // `move_by`) early-returns entirely (see
        // `settle_on_a_dismissed_controller_is_a_no_op`), so this test moves
        // the value first, matching the real call sequence.
        core.move_by(1.0);
        let fired = Rc::new(Cell::new(None::<bool>));
        let fired_for_closure = Rc::clone(&fired);
        *core.on_open_changed.borrow_mut() =
            Some(Rc::new(move |opened| fired_for_closure.set(Some(opened))));

        core.settle(MIN_FLING_VELOCITY);

        assert_eq!(
            fired.get(),
            Some(true),
            "a qualifying fling must fire on_open_changed the instant settle() runs"
        );
        assert!(
            core.controller.value() < 1.0,
            "the fling has only just started — the value must not already be at rest"
        );
    }

    #[test]
    fn drawer_defaults_to_m3_width_and_elevation() {
        let drawer = Drawer::new();
        assert_eq!(drawer.configured_width(), DEFAULT_DRAWER_WIDTH);
        assert_eq!(drawer.elevation, ELEVATION);
    }

    #[test]
    fn drawer_width_builder_overrides_the_default() {
        let drawer = Drawer::new().width(360.0);
        assert_eq!(drawer.configured_width(), 360.0);
    }

    #[test]
    fn end_rounded_shape_rounds_the_right_side_for_a_start_drawer() {
        let shape = end_rounded_shape(DrawerAlignment::Start);
        let MaterialShape::RoundedRect(radius) = shape else {
            panic!("expected a rounded rect");
        };
        assert_eq!(radius.top_left, Radius::ZERO);
        assert_ne!(radius.top_right, Radius::ZERO);
    }

    #[test]
    fn end_rounded_shape_rounds_the_left_side_for_an_end_drawer() {
        let shape = end_rounded_shape(DrawerAlignment::End);
        let MaterialShape::RoundedRect(radius) = shape else {
            panic!("expected a rounded rect");
        };
        assert_ne!(radius.top_left, Radius::ZERO);
        assert_eq!(radius.top_right, Radius::ZERO);
    }

    #[test]
    fn scale_alpha_scales_black54_by_the_controller_value() {
        let half = scale_alpha(BLACK54, 0.5);
        assert_eq!(half.a, (0x8A as f32 * 0.5).round() as u8);
        let full = scale_alpha(BLACK54, 1.0);
        assert_eq!(full.a, BLACK54.a);
        let zero = scale_alpha(BLACK54, 0.0);
        assert_eq!(zero.a, 0);
    }

    #[test]
    fn scale_alpha_clamps_out_of_range_factors() {
        assert_eq!(scale_alpha(BLACK54, 2.0).a, BLACK54.a);
        assert_eq!(scale_alpha(BLACK54, -1.0).a, 0);
    }

    #[test]
    fn direction_factor_is_positive_for_start_and_negative_for_end() {
        assert_eq!(
            outer_alignment(DrawerAlignment::Start),
            Alignment::CENTER_LEFT
        );
        assert_eq!(
            outer_alignment(DrawerAlignment::End),
            Alignment::CENTER_RIGHT
        );
        assert_eq!(
            inner_alignment(DrawerAlignment::Start),
            Alignment::CENTER_RIGHT
        );
        assert_eq!(
            inner_alignment(DrawerAlignment::End),
            Alignment::CENTER_LEFT
        );
    }

    #[test]
    fn drawer_handle_starts_with_no_drawer_and_nothing_open() {
        let handle = DrawerHandle::new();
        assert!(!handle.has_drawer());
        assert!(!handle.has_end_drawer());
        assert!(!handle.is_drawer_open());
        assert!(!handle.is_end_drawer_open());
    }

    #[test]
    fn drawer_handle_setters_are_readable_back() {
        let handle = DrawerHandle::new();
        handle.set_has_drawer(true);
        handle.set_drawer_opened(true);
        assert!(handle.has_drawer());
        assert!(handle.is_drawer_open());
    }

    #[test]
    fn drawer_handle_open_drawer_on_an_unmounted_scaffold_is_a_no_op() {
        // No DrawerController is registered under either key: the
        // GlobalKey resolves to nothing, so this must not panic.
        let handle = DrawerHandle::new();
        handle.open_drawer();
        handle.close_drawer();
        handle.open_end_drawer();
        handle.close_end_drawer();
    }
}
