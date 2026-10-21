//! A paragraph whose links a handler activates.
//!
//! Each link gets a tap recognizer of its own, built against the enclosing
//! arena scope, so two contacts on two links recognize independently and a
//! link competes with the gestures around it like any other tap. The
//! recognizers and the handler are owner-local; the render object holds only
//! the lane identities that reach them ([`TextLinkRoute`]): one pointer
//! target per link, and one activation ticket per link that assistive
//! technology clicks through.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_interaction::{
    GestureRecognizer, InteractionDispatchError, LocalPayloadTarget, PointerDispatch,
    PointerTarget, RecognizerSet, TapGestureRecognizer, cancel_all, resolve_local_payload,
};
use flui_objects::{SemanticsActionRoute, TextLinkRoute};
use flui_painting::typography::{InlineSpan, TextLink};
use flui_rendering::hit_testing::PointerEvent;
use flui_rendering::semantics::{ActionCall, SemanticsAction, SemanticsActionHandler};
use flui_view::element::ElementKind;
use flui_view::{
    BuildContext, EventCx, IntoView, LifecycleContext, RenderObjectContext,
    RenderObjectContextError, StatefulView, View, ViewState, WriterSource,
};

use super::rich_text::ResolvedParagraph;
use crate::interaction::GesturePolicy;
use crate::support::RefCallback;

/// What a link's tap and activation read when they fire: the latest handler
/// and links, and whether the paragraph is still mounted.
struct LiveLinks {
    on_link: RefCell<RefCallback<TextLink>>,
    links: RefCell<Vec<TextLink>>,
    mounted: Cell<bool>,
}

impl LiveLinks {
    fn refresh(&self, view: &LinkedParagraph) {
        *self.on_link.borrow_mut() = Rc::clone(&view.on_link);
        *self.links.borrow_mut() = links_of(&view.paragraph.authored.text);
    }

    /// Run the handler for the link at `index` inside `cx`, with no borrow
    /// held while it runs.
    fn activate(&self, cx: &mut EventCx<'_>, index: usize) {
        if !self.mounted.get() {
            return;
        }
        let handler = Rc::clone(&self.on_link.borrow());
        let Some(link) = self.links.borrow().get(index).cloned() else {
            return;
        };
        handler(cx, &link);
    }
}

fn links_of(text: &InlineSpan) -> Vec<TextLink> {
    text.links().into_iter().cloned().collect()
}

/// [`ResolvedParagraph`] mounted under a state that owns its links' tap
/// recognizers.
#[derive(Clone)]
pub(super) struct LinkedParagraph {
    paragraph: ResolvedParagraph,
    on_link: RefCallback<TextLink>,
}

impl LinkedParagraph {
    pub(super) fn new(paragraph: ResolvedParagraph, on_link: RefCallback<TextLink>) -> Self {
        Self { paragraph, on_link }
    }

    fn link_count(&self) -> usize {
        self.paragraph.authored.text.links().len()
    }
}

impl View for LinkedParagraph {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for LinkedParagraph {
    type State = LinkedParagraphState;

    fn create_state(&self) -> Self::State {
        let live = Rc::new(LiveLinks {
            on_link: RefCell::new(Rc::clone(&self.on_link)),
            links: RefCell::new(links_of(&self.paragraph.authored.text)),
            mounted: Cell::new(true),
        });
        LinkedParagraphState {
            live,
            policy: None,
            taps: Vec::new(),
        }
    }
}

/// The links' recognizers, one per link in [`InlineSpan::links`] order.
pub(super) struct LinkedParagraphState {
    live: Rc<LiveLinks>,
    policy: Option<(GesturePolicy, WriterSource)>,
    taps: Vec<Rc<TapGestureRecognizer>>,
}

impl LinkedParagraphState {
    fn tap(
        &self,
        policy: &GesturePolicy,
        writer: &WriterSource,
        index: usize,
    ) -> Rc<TapGestureRecognizer> {
        let (live, writer) = (Rc::clone(&self.live), writer.clone());
        TapGestureRecognizer::builder(policy.arena.clone())
            .settings(policy.settings.clone())
            .on_tap(move |_| writer.write(|cx| live.activate(cx, index)))
            .build()
    }

    /// Recognizers for links `from..to` against the current policy.
    fn taps(&self, from: usize, to: usize) -> Vec<Rc<TapGestureRecognizer>> {
        let Some((policy, writer)) = &self.policy else {
            return Vec::new();
        };
        (from..to)
            .map(|index| self.tap(policy, writer, index))
            .collect()
    }
}

fn cancel(taps: &[Rc<TapGestureRecognizer>]) {
    cancel_all(taps.iter().map(|tap| &**tap as &dyn GestureRecognizer));
}

impl ViewState<LinkedParagraph> for LinkedParagraphState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.policy = Some((GesturePolicy::of(ctx), ctx.writer_source()));
        self.taps = self.taps(0, self.live.links.borrow().len());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let policy = GesturePolicy::of(ctx);
        let changed = self
            .policy
            .as_ref()
            .is_some_and(|(current, _)| *current != policy);
        self.policy = Some((policy, ctx.writer_source()));
        if changed {
            // The replacements are committed before the outgoing recognizers'
            // cancellation runs their callbacks.
            let replacements = self.taps(0, self.taps.len());
            let outgoing = std::mem::replace(&mut self.taps, replacements);
            cancel(&outgoing);
        }
    }

    fn did_update_view(&mut self, _old_view: &LinkedParagraph, new_view: &LinkedParagraph) {
        self.live.refresh(new_view);
        let count = new_view.link_count();
        if count > self.taps.len() {
            let added = self.taps(self.taps.len(), count);
            self.taps.extend(added);
        } else {
            let retired = self.taps.split_off(count);
            cancel(&retired);
        }
    }

    fn build(&self, view: &LinkedParagraph, _ctx: &dyn BuildContext) -> impl IntoView {
        let links = self.policy.as_ref().map(|(_, writer)| LinkRouting {
            live: Rc::clone(&self.live),
            writer: writer.clone(),
            taps: self.taps.clone(),
        });
        ResolvedParagraph {
            links,
            ..view.paragraph.clone()
        }
    }

    fn dispose(&mut self) {
        // Refuse taps and activations before cancellation can run callbacks.
        self.live.mounted.set(false);
        let outgoing = std::mem::take(&mut self.taps);
        cancel(&outgoing);
    }
}

/// What the paragraph's render view registers in the owner lane for each
/// link: the link's recognizer behind a pointer target, and its activation
/// behind a payload ticket.
#[derive(Clone)]
pub(super) struct LinkRouting {
    live: Rc<LiveLinks>,
    writer: WriterSource,
    taps: Vec<Rc<TapGestureRecognizer>>,
}

impl std::fmt::Debug for LinkRouting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkRouting")
            .field("links", &self.taps.len())
            .finish_non_exhaustive()
    }
}

/// The lane payload one link's activation resolves.
struct LinkActivation {
    live: Rc<LiveLinks>,
    writer: WriterSource,
    index: usize,
}

/// Run the activation `target` names, on the owner thread inside its UI
/// runtime. Every failure is a dropped action, never a panic: the platform
/// asked for something the tree can no longer do.
fn activate(target: LocalPayloadTarget, call: &ActionCall) {
    if call.action() != SemanticsAction::Click {
        return;
    }
    let payload = match resolve_local_payload(target) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                ?error,
                "a link activation was dropped: its handler is owner-local and runs only \
                 inside its ui_runtime"
            );
            return;
        }
    };
    let Ok(activation) = payload.downcast::<LinkActivation>() else {
        tracing::error!("BUG: a link activation ticket resolved to a payload of another type");
        return;
    };
    activation
        .writer
        .write(|cx| activation.live.activate(cx, activation.index));
}

/// Hand a link's contacts to its recognizer. A revision corrects a reading
/// recognizers already decided on, so it never reaches one (ADR-0143).
fn pointer_handler(tap: &Rc<TapGestureRecognizer>) -> impl Fn(PointerDispatch<'_>) + 'static {
    let mut recognizers = RecognizerSet::default();
    recognizers.attach(tap);
    move |dispatch: PointerDispatch<'_>| {
        if matches!(
            dispatch.local,
            PointerEvent::Down(_)
                | PointerEvent::Move(_)
                | PointerEvent::ButtonChange(_)
                | PointerEvent::Up(_)
                | PointerEvent::Cancel(_)
        ) {
            recognizers.dispatch(dispatch);
        }
    }
}

impl LinkRouting {
    fn activation(&self, index: usize) -> Rc<dyn std::any::Any> {
        Rc::new(LinkActivation {
            live: Rc::clone(&self.live),
            writer: self.writer.clone(),
            index,
        })
    }

    /// Register link `index` in the owner lane, or `None` when no lane is
    /// active (a detached mount): the link then stays plain text.
    fn register(&self, ctx: &RenderObjectContext<'_>, index: usize) -> Option<TextLinkRoute> {
        let pointer = ctx.register_pointer(pointer_handler(&self.taps[index]));
        let activation = ctx.register_local_payload(self.activation(index));
        match (pointer, activation) {
            (Ok(pointer), Ok(target)) => {
                let handler: SemanticsActionHandler = Arc::new(move |call| activate(target, &call));
                Some(TextLinkRoute::new(
                    pointer,
                    SemanticsActionRoute::new(target, handler),
                ))
            }
            (pointer, activation) => {
                tracing::debug!(
                    "a link is not routed: the paragraph is mounted without an owner lane"
                );
                if let Ok(pointer) = pointer {
                    unregister_pointer(ctx, pointer);
                }
                if let Ok(target) = activation {
                    unregister_activation(ctx, target);
                }
                None
            }
        }
    }

    /// Replace link `index`'s handlers under `route`'s registrations, which
    /// keeps the published activation handler. A registration the lane lost
    /// is made anew; one the lane cannot be reached for is kept as it is,
    /// so unmount still releases it.
    fn replace(
        &self,
        ctx: &RenderObjectContext<'_>,
        route: &TextLinkRoute,
        index: usize,
    ) -> Option<TextLinkRoute> {
        let replaced = ctx
            .replace_pointer(route.pointer_target(), pointer_handler(&self.taps[index]))
            .and_then(|()| {
                ctx.replace_local_payload(route.activation().target(), self.activation(index))
            });
        match replaced {
            Ok(()) => Some(route.clone()),
            Err(RenderObjectContextError::Interaction(InteractionDispatchError::TargetGone)) => {
                tracing::debug!("a link's lane registration was gone; registering anew");
                release_route(ctx, route);
                self.register(ctx, index)
            }
            Err(error) => {
                tracing::debug!(?error, "a link's handlers were not replaced");
                Some(route.clone())
            }
        }
    }

    /// The routes for `routing`'s links, reusing `existing` registrations in
    /// order and releasing the ones no link uses any more.
    pub(super) fn sync(
        routing: Option<&Self>,
        ctx: &RenderObjectContext<'_>,
        existing: &[TextLinkRoute],
    ) -> Vec<TextLinkRoute> {
        let count = routing.map_or(0, |routing| routing.taps.len());
        let mut routes = Vec::with_capacity(count);
        if let Some(routing) = routing {
            for index in 0..count {
                let route = match existing.get(index) {
                    Some(route) => routing.replace(ctx, route, index),
                    None => routing.register(ctx, index),
                };
                // Routes stay in link order: past a link that could not be
                // routed, the rest stay plain text too.
                let Some(route) = route else {
                    Self::release(ctx, existing.get(index + 1..).unwrap_or_default());
                    return routes;
                };
                routes.push(route);
            }
        }
        Self::release(ctx, existing.get(count..).unwrap_or_default());
        routes
    }

    /// Remove `routes`' registrations from the owner lane.
    pub(super) fn release(ctx: &RenderObjectContext<'_>, routes: &[TextLinkRoute]) {
        for route in routes {
            release_route(ctx, route);
        }
    }
}

fn release_route(ctx: &RenderObjectContext<'_>, route: &TextLinkRoute) {
    unregister_pointer(ctx, route.pointer_target());
    unregister_activation(ctx, route.activation().target());
}

fn unregister_pointer(ctx: &RenderObjectContext<'_>, target: PointerTarget) {
    if let Err(error) = ctx.unregister_pointer(target) {
        tracing::debug!(?error, "a link's pointer target was already unregistered");
    }
}

fn unregister_activation(ctx: &RenderObjectContext<'_>, target: LocalPayloadTarget) {
    if let Err(error) = ctx.unregister_local_payload(target) {
        tracing::debug!(?error, "a link's activation was already unregistered");
    }
}
