//! [`Listener`] — the lowest-level pointer-event widget: routes raw
//! `PointerEvent`s landing on its child to callbacks.

use std::rc::Rc;

use flui_interaction::events::ScrollEventData;
use flui_interaction::routing::{EventPropagation, PanZoomTarget, ScrollTarget};
use flui_interaction::{PointerPanZoomEvent, PointerTarget, from_w3c_event};
use flui_objects::RenderListener;
use flui_rendering::hit_testing::{HitTestBehavior, PointerEvent};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderObjectContext, RenderView, View, impl_render_view};

/// A pointer-event callback: receives the (locally-transformed) [`PointerEvent`]
/// that landed on the [`Listener`].
type PointerCallback = Rc<dyn Fn(&PointerEvent)>;

/// A trackpad pan/zoom callback routed from a [`PointerEvent::Gesture`] update.
type PointerPanZoomCallback = Rc<dyn Fn(&PointerPanZoomEvent)>;

/// An arbitrated scroll-signal handler: returns
/// [`EventPropagation::Stop`] to claim the tick, ending the leaf-first walk.
type ScrollClaimCallback = Rc<dyn Fn(&ScrollEventData) -> EventPropagation>;

/// An arbitrated trackpad pan-zoom handler: returns
/// [`EventPropagation::Stop`] to claim the tick, ending the leaf-first walk.
type PanZoomClaimCallback = Rc<dyn Fn(&PointerPanZoomEvent) -> EventPropagation>;

/// Calls callbacks in response to raw pointer events on its child.
///
/// Flutter parity: `widgets/basic.dart` `Listener` over `RenderPointerListener`
/// — the foundation the higher-level gesture widgets build on. Layout and paint
/// pass through; the listener registers itself in the hit-test path per its
/// [`HitTestBehavior`] (default [`DeferToChild`](HitTestBehavior::DeferToChild):
/// fires only for pointers that land on a descendant), so the matching callback
/// receives the event.
#[derive(Clone)]
pub struct Listener {
    on_pointer_down: Option<PointerCallback>,
    on_pointer_up: Option<PointerCallback>,
    on_pointer_move: Option<PointerCallback>,
    on_pointer_hover: Option<PointerCallback>,
    on_pointer_cancel: Option<PointerCallback>,
    on_pointer_signal: Option<PointerCallback>,
    on_scroll_claim: Option<ScrollClaimCallback>,
    on_pointer_pan_zoom_update: Option<PointerPanZoomCallback>,
    on_pointer_pan_zoom_claim: Option<PanZoomClaimCallback>,
    behavior: HitTestBehavior,
    child: Child,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_pointer_hover: None,
            on_pointer_cancel: None,
            on_pointer_signal: None,
            on_scroll_claim: None,
            on_pointer_pan_zoom_update: None,
            on_pointer_pan_zoom_claim: None,
            // Flutter's `Listener` default.
            behavior: HitTestBehavior::DeferToChild,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("on_pointer_down", &self.on_pointer_down.is_some())
            .field("on_pointer_up", &self.on_pointer_up.is_some())
            .field("on_pointer_move", &self.on_pointer_move.is_some())
            .field("on_pointer_hover", &self.on_pointer_hover.is_some())
            .field("on_pointer_cancel", &self.on_pointer_cancel.is_some())
            .field("on_pointer_signal", &self.on_pointer_signal.is_some())
            .field("on_scroll_claim", &self.on_scroll_claim.is_some())
            .field(
                "on_pointer_pan_zoom_update",
                &self.on_pointer_pan_zoom_update.is_some(),
            )
            .field(
                "on_pointer_pan_zoom_claim",
                &self.on_pointer_pan_zoom_claim.is_some(),
            )
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl Listener {
    /// A listener with no callbacks (a transparent pass-through until one is
    /// set), defaulting to [`HitTestBehavior::DeferToChild`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Set how the listener participates in hit-testing (default
    /// [`DeferToChild`](HitTestBehavior::DeferToChild)).
    #[must_use]
    pub fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Called when a pointer makes contact within the child's bounds.
    #[must_use]
    pub fn on_pointer_down(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_down = Some(Rc::new(callback));
        self
    }

    /// Called when a pointer that was in contact lifts.
    #[must_use]
    pub fn on_pointer_up(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_up = Some(Rc::new(callback));
        self
    }

    /// Called when a pointer moves while in contact.
    #[must_use]
    pub fn on_pointer_move(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_move = Some(Rc::new(callback));
        self
    }

    /// Called when a pointer moves without active buttons over the listener.
    ///
    /// FLUI models Flutter's distinct `PointerHoverEvent` as
    /// [`PointerEvent::Move`] whose current button mask is empty.
    #[must_use]
    pub fn on_pointer_hover(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_hover = Some(Rc::new(callback));
        self
    }

    /// Called when contact is interrupted (the platform cancels the pointer, or
    /// it leaves the surface) — a gesture must abandon any in-flight tracking.
    #[must_use]
    pub fn on_pointer_cancel(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_cancel = Some(Rc::new(callback));
        self
    }

    /// Called when a pointer signal occurs over the listener.
    ///
    /// FLUI currently models pointer signals as [`PointerEvent::Scroll`].
    ///
    /// This channel *observes*: every listener on the hit path sees the
    /// signal (Flutter parity — `Listener.onPointerSignal` fires for the whole
    /// path). A widget that should *act* on the tick only when it is the
    /// leaf-most interested party registers with
    /// [`on_scroll_claim`](Self::on_scroll_claim) instead.
    #[must_use]
    pub fn on_pointer_signal(mut self, callback: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_signal = Some(Rc::new(callback));
        self
    }

    /// Register this listener in the arbitrated scroll-signal walk — the FLUI
    /// port of Flutter's `PointerSignalResolver` pair to
    /// `Listener.onPointerSignal`.
    ///
    /// After the whole hit path has observed a scroll signal, the leaf-first
    /// claim walk invokes each registered handler until one returns
    /// [`EventPropagation::Stop`]; later (outer) handlers then never act.
    /// Return `Stop` only when this widget will actually consume the tick
    /// (a scrollable that can still move, a viewer that will zoom) and
    /// [`EventPropagation::Continue`] otherwise, so an inner scrollable at its
    /// extent hands the wheel to the outer one — the oracle's
    /// `_receivedPointerSignal` registers only when the target offset differs
    /// from the current pixels (`widgets/scrollable.dart`).
    #[must_use]
    pub fn on_scroll_claim(
        mut self,
        callback: impl Fn(&ScrollEventData) -> EventPropagation + 'static,
    ) -> Self {
        self.on_scroll_claim = Some(Rc::new(callback));
        self
    }

    /// Called when a trackpad pan/zoom update reaches the listener.
    ///
    /// Current FLUI routing converts upstream [`PointerEvent::Gesture`] into a
    /// Flutter-shaped [`PointerPanZoomEvent::Update`]. Start/end callbacks are
    /// intentionally not exposed until the platform layer can provide reliable
    /// gesture-boundary events.
    ///
    /// This channel *observes*: every listener on the hit path sees the tick.
    /// A widget that should *act* on it only when it is the leaf-most
    /// interested party registers with
    /// [`on_pointer_pan_zoom_claim`](Self::on_pointer_pan_zoom_claim)
    /// instead.
    #[must_use]
    pub fn on_pointer_pan_zoom_update(
        mut self,
        callback: impl Fn(&PointerPanZoomEvent) + 'static,
    ) -> Self {
        self.on_pointer_pan_zoom_update = Some(Rc::new(callback));
        self
    }

    /// Register this listener in the arbitrated trackpad pan-zoom walk.
    ///
    /// The pan-zoom counterpart of
    /// [`on_scroll_claim`](Self::on_scroll_claim). After the whole hit path
    /// has observed a gesture tick, the leaf-first claim walk invokes each
    /// registered handler until one returns [`EventPropagation::Stop`]; later
    /// (outer) handlers then never act. Return `Stop` only when this widget
    /// will actually consume the tick — a viewer that will really zoom — and
    /// [`EventPropagation::Continue`] otherwise, so a viewer pinned at its
    /// scale extent hands the pinch to the one enclosing it.
    ///
    /// Flutter arbitrates the same contention through the scale gesture
    /// arena (`gestures/scale.dart`) rather than a signal-style claim; this
    /// surface is the interim arbitration until that recognizer lands.
    #[must_use]
    pub fn on_pointer_pan_zoom_claim(
        mut self,
        callback: impl Fn(&PointerPanZoomEvent) -> EventPropagation + 'static,
    ) -> Self {
        self.on_pointer_pan_zoom_claim = Some(Rc::new(callback));
        self
    }

    /// Set the child whose pointer events are observed.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Merge the per-kind callbacks into the single owner-local handler the
    /// interaction lane invokes: route each event to the matching callback. A
    /// raw `Listener` never claims an event — ordinary pointer delivery has no
    /// propagation result (ADR-0027).
    fn handler(&self) -> impl Fn(&PointerEvent) + 'static {
        let on_down = self.on_pointer_down.clone();
        let on_up = self.on_pointer_up.clone();
        let on_move = self.on_pointer_move.clone();
        let on_hover = self.on_pointer_hover.clone();
        let on_cancel = self.on_pointer_cancel.clone();
        let on_signal = self.on_pointer_signal.clone();
        let on_pan_zoom_update = self.on_pointer_pan_zoom_update.clone();
        move |event: &PointerEvent| match event {
            PointerEvent::Down(_) => {
                if let Some(callback) = &on_down {
                    callback(event);
                }
            }
            PointerEvent::Up(_) => {
                if let Some(callback) = &on_up {
                    callback(event);
                }
            }
            PointerEvent::Move(update) => {
                if update.current.buttons.is_empty() {
                    if let Some(callback) = &on_hover {
                        callback(event);
                    }
                } else if let Some(callback) = &on_move {
                    callback(event);
                }
            }
            PointerEvent::Cancel(_) => {
                if let Some(callback) = &on_cancel {
                    callback(event);
                }
            }
            PointerEvent::Scroll(_) => {
                if let Some(callback) = &on_signal {
                    callback(event);
                }
            }
            PointerEvent::Gesture(_) => {
                if let Some(callback) = &on_pan_zoom_update
                    && let Some(pan_zoom) = from_w3c_event(event)
                    && pan_zoom.is_update()
                {
                    callback(&pan_zoom);
                }
            }
            _ => {}
        }
    }

    /// Register the merged handler in the active owner lane, returning its
    /// data-only identity for render storage.
    ///
    /// Returns `None` when no owner lane is active (a detached mount): the
    /// listener then participates in hit-testing without pointer delivery.
    fn register(&self, ctx: &RenderObjectContext<'_>) -> Option<PointerTarget> {
        match ctx.register_pointer(self.handler()) {
            Ok(target) => Some(target),
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "Listener mounted without an active interaction lane; \
                     pointer events will not be delivered"
                );
                None
            }
        }
    }

    /// Register the scroll-claim handler in the active owner lane, returning
    /// its data-only identity for render storage — `None` when no claim
    /// callback is configured or no owner lane is active.
    fn register_scroll_claim(&self, ctx: &RenderObjectContext<'_>) -> Option<ScrollTarget> {
        let claim = self.on_scroll_claim.clone()?;
        match ctx.register_scroll(move |event| claim(event)) {
            Ok(target) => Some(target),
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "Listener mounted without an active interaction lane; \
                     scroll signals will not be arbitrated"
                );
                None
            }
        }
    }

    /// Reconcile the render object's scroll-claim registration with this
    /// widget configuration: register, replace, or unregister so the lane
    /// mirrors whether `on_scroll_claim` is set.
    fn sync_scroll_claim(&self, ctx: &RenderObjectContext<'_>, render_object: &mut RenderListener) {
        match (render_object.claimed_scroll_target(), &self.on_scroll_claim) {
            (Some(target), Some(claim)) => {
                let claim = claim.clone();
                if let Err(error) = ctx.replace_scroll(target, move |event| claim(event)) {
                    tracing::warn!(?error, "Listener scroll-claim replacement failed");
                }
            }
            (Some(target), None) => {
                if let Err(error) = ctx.unregister_scroll(target) {
                    tracing::debug!(?error, "Listener scroll-claim unregistration failed");
                }
                render_object.set_scroll_target(None);
            }
            (None, Some(_)) => {
                render_object.set_scroll_target(self.register_scroll_claim(ctx));
            }
            (None, None) => {}
        }
    }

    /// Register the pan-zoom claim handler in the active owner lane,
    /// returning its data-only identity for render storage — `None` when no
    /// claim callback is configured or no owner lane is active.
    fn register_pan_zoom_claim(&self, ctx: &RenderObjectContext<'_>) -> Option<PanZoomTarget> {
        let claim = self.on_pointer_pan_zoom_claim.clone()?;
        match ctx.register_pan_zoom(move |event| claim(event)) {
            Ok(target) => Some(target),
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "Listener mounted without an active interaction lane; \
                     pan-zoom ticks will not be arbitrated"
                );
                None
            }
        }
    }

    /// Reconcile the render object's pan-zoom-claim registration with this
    /// widget configuration, the way
    /// [`sync_scroll_claim`](Self::sync_scroll_claim) does for scroll.
    fn sync_pan_zoom_claim(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut RenderListener,
    ) {
        match (
            render_object.claimed_pan_zoom_target(),
            &self.on_pointer_pan_zoom_claim,
        ) {
            (Some(target), Some(claim)) => {
                let claim = claim.clone();
                if let Err(error) = ctx.replace_pan_zoom(target, move |event| claim(event)) {
                    tracing::warn!(?error, "Listener pan-zoom-claim replacement failed");
                }
            }
            (Some(target), None) => {
                if let Err(error) = ctx.unregister_pan_zoom(target) {
                    tracing::debug!(?error, "Listener pan-zoom-claim unregistration failed");
                }
                render_object.set_pan_zoom_target(None);
            }
            (None, Some(_)) => {
                render_object.set_pan_zoom_target(self.register_pan_zoom_claim(ctx));
            }
            (None, None) => {}
        }
    }
}

impl RenderView for Listener {
    type Protocol = BoxProtocol;
    type RenderObject = RenderListener;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderListener::new(self.register(ctx), self.behavior);
        render_object.set_scroll_target(self.register_scroll_claim(ctx));
        render_object.set_pan_zoom_target(self.register_pan_zoom_claim(ctx));
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // Rebuild replaces the handler INSIDE the existing cell, so an active
        // cached route observes the new configuration (ADR-0027 §rebuild).
        match render_object.target() {
            Some(target) => {
                if let Err(error) = ctx.replace_pointer(target, self.handler()) {
                    tracing::warn!(?error, "Listener handler replacement failed");
                }
            }
            None => render_object.set_target(self.register(ctx)),
        }
        self.sync_scroll_claim(ctx, render_object);
        self.sync_pan_zoom_claim(ctx, render_object);
        render_object.set_behavior(self.behavior);
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        // Unmount removes the target from NEW route resolution; an active
        // cached route keeps its strong handler cell through Up/Cancel.
        if let Some(target) = render_object.target() {
            if let Err(error) = ctx.unregister_pointer(target) {
                tracing::debug!(?error, "Listener target unregistration failed");
            }
            render_object.set_target(None);
        }
        if let Some(target) = render_object.claimed_pan_zoom_target() {
            if let Err(error) = ctx.unregister_pan_zoom(target) {
                tracing::debug!(?error, "Listener pan-zoom-claim unregistration failed");
            }
            render_object.set_pan_zoom_target(None);
        }
        if let Some(target) = render_object.claimed_scroll_target() {
            if let Err(error) = ctx.unregister_scroll(target) {
                tracing::debug!(?error, "Listener scroll-claim unregistration failed");
            }
            render_object.set_scroll_target(None);
        }
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(Listener);
