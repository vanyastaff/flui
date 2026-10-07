//! [`Listener`] — the lowest-level pointer-event widget: routes the raw
//! pointer events landing on its child to callbacks, each one carrying both
//! the listener's own space and the root's (`PointerDispatch`).

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use flui_interaction::events::ScrollEventData;
use flui_interaction::routing::{EventPropagation, PanZoomTarget, ScrollTarget};
use flui_interaction::{
    GestureRecognizer, PanZoomEvent, PanZoomPhase, PointerDispatch, PointerTarget, RecognizerSet,
};
use flui_objects::RenderListener;
use flui_rendering::hit_testing::{HitTestBehavior, PointerEvent};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    Child, EventCx, EventOutcome, IntoView, RenderObjectContext, RenderView, WriterSource,
    impl_render_view,
};

use crate::support::{RefCallback, ref_callback};

/// A pointer-event callback: receives the dispatch's [`EventCx`] and the event
/// that landed on the [`Listener`] in both spaces — see [`PointerDispatch`].
/// Stored already adapted to report its outcome.
type PointerCallback = Rc<dyn Fn(&mut EventCx<'_>, PointerDispatch<'_>)>;

/// A trackpad pan/zoom callback routed from a [`PointerEvent::PanZoom`] update.
type PointerPanZoomCallback = RefCallback<PanZoomEvent>;

/// Store a pointer callback, adapted to report its outcome.
fn pointer_callback<F, R>(callback: F) -> PointerCallback
where
    F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, dispatch: PointerDispatch<'_>| {
        callback(cx, dispatch).report();
    })
}

/// An arbitrated scroll-signal handler: returns
/// [`EventPropagation::Stop`] to claim the tick, ending the leaf-first walk.
type ScrollClaimCallback = Rc<dyn Fn(&ScrollEventData) -> EventPropagation>;

/// An arbitrated trackpad pan-zoom handler: returns
/// [`EventPropagation::Stop`] to claim the tick, ending the leaf-first walk.
type PanZoomClaimCallback = Rc<dyn Fn(&PanZoomEvent) -> EventPropagation>;

/// Calls callbacks in response to raw pointer events on its child.
///
/// The foundation the higher-level gesture widgets build on. Layout and paint
/// pass through; the listener registers itself in the hit-test path per its
/// [`HitTestBehavior`] (default [`DeferToChild`](HitTestBehavior::DeferToChild):
/// fires only for pointers that land on a descendant), so the matching callback
/// receives the event.
///
/// # What a callback receives
///
/// Each `on_pointer_*` callback receives the dispatch's `&mut EventCx<'_>`
/// first, so it writes a signal directly (ADR-0086):
/// `.on_pointer_down(move |cx, _dispatch| presses.update(cx, |n| *n += 1))`.
/// The listener has no `init_state`; it takes the owner's [`WriterSource`]
/// from the [`RenderObjectContext`] that registers its handler, and opens one
/// write per event. The two claim callbacks decide during routing and receive
/// no `cx` (ADR-0086 §6).
///
/// Then it is handed a [`PointerDispatch`], which carries
/// the event in two spaces: `local` (this listener's own box, the value to
/// measure against its size or child offsets) and `global` (the root's space,
/// the value to compare against another widget's position or to hand to a
/// fresh hit test). FLUI's pointer events come from
/// FLUI's owned input vocabulary and hold one position each, so the pair is delivered
/// alongside the event instead of on it.
///
#[derive(Clone)]
pub struct Listener {
    recognizers: RecognizerSet,
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
            recognizers: RecognizerSet::default(),
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_pointer_hover: None,
            on_pointer_cancel: None,
            on_pointer_signal: None,
            on_scroll_claim: None,
            on_pointer_pan_zoom_update: None,
            on_pointer_pan_zoom_claim: None,
            behavior: HitTestBehavior::DeferToChild,
            child: Child::empty(),
        }
    }
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("has_recognizers", &!self.recognizers.is_empty())
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

    /// Attach a recognizer without extending its owner's lifetime.
    ///
    /// The owner retains its `Rc`; delivery follows attachment order, after
    /// this listener's raw observer and outside its event-context write.
    #[must_use]
    pub fn recognizer<R: GestureRecognizer + 'static>(mut self, recognizer: &Rc<R>) -> Self {
        self.recognizers.attach(recognizer);
        self
    }

    /// Admit new contacts only when `admit` accepts their Down event.
    ///
    /// Later events, including Up and Cancel, reach every live attachment
    /// regardless of the current predicate, so reconfiguration cannot strand
    /// an already admitted contact.
    #[must_use]
    pub fn recognizer_when<R: GestureRecognizer + 'static>(
        mut self,
        recognizer: &Rc<R>,
        admit: impl Fn(PointerDispatch<'_>) -> bool + 'static,
    ) -> Self {
        self.recognizers.attach_when(recognizer, admit);
        self
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
    pub fn on_pointer_down<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_down = Some(pointer_callback(callback));
        self
    }

    /// Called when a pointer that was in contact lifts.
    #[must_use]
    pub fn on_pointer_up<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_up = Some(pointer_callback(callback));
        self
    }

    /// Called when a pointer moves while in contact.
    #[must_use]
    pub fn on_pointer_move<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_move = Some(pointer_callback(callback));
        self
    }

    /// Called when a pointer moves without active buttons over the listener.
    ///
    /// Hover is modelled as a
    /// [`PointerEvent::Move`] whose current button mask is empty.
    #[must_use]
    pub fn on_pointer_hover<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_hover = Some(pointer_callback(callback));
        self
    }

    /// Called when contact is interrupted (the platform cancels the pointer, or
    /// it leaves the surface) — a gesture must abandon any in-flight tracking.
    #[must_use]
    pub fn on_pointer_cancel<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_cancel = Some(pointer_callback(callback));
        self
    }

    /// Called when a pointer signal occurs over the listener.
    ///
    /// FLUI currently models pointer signals as [`PointerEvent::Scroll`].
    ///
    /// This channel *observes*: every listener on the hit path sees the
    /// signal (this fires for the whole path). A widget that should *act* on the tick only when it is the
    /// leaf-most interested party registers with
    /// [`on_scroll_claim`](Self::on_scroll_claim) instead.
    #[must_use]
    pub fn on_pointer_signal<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, PointerDispatch<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_signal = Some(pointer_callback(callback));
        self
    }

    /// Register this listener in the arbitrated scroll-signal walk, the
    /// arbitrated counterpart of [`on_pointer_signal`](Self::on_pointer_signal).
    ///
    /// After the whole hit path has observed a scroll signal, the leaf-first
    /// claim walk invokes each registered handler until one returns
    /// [`EventPropagation::Stop`]; later (outer) handlers then never act.
    /// Return `Stop` only when this widget will actually consume the tick
    /// (a scrollable that can still move, a viewer that will zoom) and
    /// [`EventPropagation::Continue`] otherwise, so an inner scrollable at its
    /// extent hands the wheel to the outer one.
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
    /// FLUI routing delivers owned [`PointerEvent::PanZoom`] events.
    /// [`PanZoomPhase::Update`] invokes this callback. Start/end callbacks are
    /// intentionally not exposed until the platform layer can provide reliable
    /// gesture-boundary events.
    ///
    /// This channel *observes*: every listener on the hit path sees the tick.
    /// A widget that should *act* on it only when it is the leaf-most
    /// interested party registers with
    /// [`on_pointer_pan_zoom_claim`](Self::on_pointer_pan_zoom_claim)
    /// instead.
    #[must_use]
    pub fn on_pointer_pan_zoom_update<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &PanZoomEvent) -> R + 'static,
        R: EventOutcome,
    {
        self.on_pointer_pan_zoom_update = Some(ref_callback(callback));
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
    /// This surface is the interim arbitration until a scale recognizer in the
    /// gesture arena lands.
    #[must_use]
    pub fn on_pointer_pan_zoom_claim(
        mut self,
        callback: impl Fn(&PanZoomEvent) -> EventPropagation + 'static,
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
    /// interaction lane invokes: route each event to the matching callback,
    /// inside one write `writer` opens. A raw `Listener` never claims an
    /// event — ordinary pointer delivery has no propagation result
    /// (ADR-0027).
    fn handler(&self, writer: WriterSource) -> impl Fn(PointerDispatch<'_>) + 'static {
        let on_down = self.on_pointer_down.clone();
        let on_up = self.on_pointer_up.clone();
        let on_move = self.on_pointer_move.clone();
        let on_hover = self.on_pointer_hover.clone();
        let on_cancel = self.on_pointer_cancel.clone();
        let on_signal = self.on_pointer_signal.clone();
        let on_pan_zoom_update = self.on_pointer_pan_zoom_update.clone();
        let recognizers = self.recognizers.clone();
        // The event KIND is the same in both spaces, so the routing match
        // reads the local one and each callback receives the whole pair.
        move |dispatch: PointerDispatch<'_>| {
            let raw = catch_unwind(AssertUnwindSafe(|| {
                let callback = match dispatch.local {
                    PointerEvent::Down(_) => &on_down,
                    PointerEvent::Up(_) => &on_up,
                    PointerEvent::Move(update) if update.buttons.is_empty() => &on_hover,
                    PointerEvent::Move(_) => &on_move,
                    PointerEvent::ButtonChange(_) => &on_move,
                    PointerEvent::Cancel(_) => &on_cancel,
                    PointerEvent::Scroll(_) => &on_signal,
                    PointerEvent::PanZoom(pan_zoom) => {
                        if let Some(callback) = &on_pan_zoom_update
                            && matches!(pan_zoom.phase, PanZoomPhase::Update(_))
                        {
                            writer.write(|cx| callback(cx, pan_zoom));
                        }
                        return;
                    }
                    _ => return,
                };
                if let Some(callback) = callback {
                    writer.write(|cx| callback(cx, dispatch));
                }
            }));
            let recognized = catch_unwind(AssertUnwindSafe(|| recognizers.dispatch(dispatch)));
            match (raw, recognized) {
                (Err(first), Err(later)) => {
                    flui_foundation::panic::retain_opaque_payload(later);
                    resume_unwind(first);
                }
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => resume_unwind(payload),
                (Ok(()), Ok(())) => {}
            }
        }
    }

    /// The merged handler over the owner's writer source, or `None` when the
    /// context has none (a detached mount).
    fn owner_handler(
        &self,
        ctx: &RenderObjectContext<'_>,
    ) -> Option<impl Fn(PointerDispatch<'_>) + 'static> {
        let Some(writer) = ctx.writer_source() else {
            tracing::debug!(
                "Listener mounted without an owner graph; pointer events will not be delivered"
            );
            return None;
        };
        Some(self.handler(writer))
    }

    /// Register the merged handler in the active owner lane, returning its
    /// data-only identity for render storage.
    ///
    /// Returns `None` when no owner lane is active (a detached mount): the
    /// listener then participates in hit-testing without pointer delivery.
    fn register(&self, ctx: &RenderObjectContext<'_>) -> Option<PointerTarget> {
        let handler = self.owner_handler(ctx)?;
        match ctx.register_pointer(handler) {
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
                if let Some(handler) = self.owner_handler(ctx)
                    && let Err(error) = ctx.replace_pointer(target, handler)
                {
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

    flui_view::single_child_view_children!();
}

impl_render_view!(Listener);
