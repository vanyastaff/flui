//! A paragraph whose links a handler activates.
//!
//! Each link gets a tap recognizer of its own, built against the enclosing
//! arena scope, so two contacts on two links recognize independently and a
//! link competes with the gestures around it like any other tap. Each link
//! is also a focus stop of its own: a [`Focus`] per link hosts a node
//! measured by the link's glyphs, so Tab reaches links in reading order
//! among every other control, and Enter opens the focused one. Space does
//! not: a link is not a button.
//!
//! The recognizers, nodes and handler are owner-local; the render object
//! holds only the lane identities that reach them ([`TextLinkRoute`]): one
//! pointer target per link, and one activation ticket per link that
//! assistive technology clicks and focuses through.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_interaction::focus::{FocusCause, FocusNode, FocusSubscription, KeyEventResult};
use flui_interaction::{
    GestureRecognizer, InteractionDispatchError, LocalPayloadTarget, PointerDispatch,
    PointerTarget, RecognizerSet, RectProvider, TapGestureRecognizer, cancel_all,
    resolve_local_payload,
};
use flui_objects::{
    RenderParagraph, SemanticsActionRoute, SubtreeAnchor, TextLinkFocus, TextLinkRoute,
};
use flui_painting::typography::{InlineSpan, TextLink};
use flui_platform::keyboard::{KeyEvent, NamedKey};
use flui_rendering::hit_testing::PointerEvent;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::semantics::{ActionCall, SemanticsAction, SemanticsActionHandler};
use flui_view::element::ElementKind;
use flui_view::{
    BuildContext, EventCx, IntoView, LifecycleContext, RebuildHandle, RebuildReason,
    RenderObjectContext, RenderObjectContextError, StatefulView, View, ViewExt, ViewState,
    WriterSource,
};

use super::rich_text::ResolvedParagraph;
use crate::anchored_box::AnchoredBox;
use crate::interaction::GesturePolicy;
use crate::support::RefCallback;
use crate::{
    Action, ActionOutcome, Actions, ActivateIntent, ActivationPressIntent, DefaultFocusRingStyle,
    Focus, SingleActivator,
};

/// What a link's tap, key and assistive activation read when they fire: the
/// latest handler, links and focus nodes, and whether the paragraph is still
/// mounted.
struct LiveLinks {
    on_link: RefCell<RefCallback<TextLink>>,
    links: RefCell<Vec<TextLink>>,
    nodes: RefCell<Vec<Rc<FocusNode>>>,
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

    /// Move the focus to the link at `index`, as assistive technology asked.
    fn focus(&self, index: usize) {
        if !self.mounted.get() {
            return;
        }
        let Some(node) = self.nodes.borrow().get(index).cloned() else {
            return;
        };
        let _ = node.request_focus_with(FocusCause::Assistive);
    }
}

fn links_of(text: &InlineSpan) -> Vec<TextLink> {
    text.links().into_iter().cloned().collect()
}

/// [`ResolvedParagraph`] mounted under a state that owns its links' tap
/// recognizers and focus nodes.
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
            nodes: RefCell::new(Vec::new()),
            mounted: Cell::new(true),
        });
        LinkedParagraphState {
            live,
            mount: None,
            anchor: SubtreeAnchor::new(),
            slots: Vec::new(),
        }
    }
}

/// What the state acquired when it mounted, to build each link's parts.
struct Mount {
    policy: GesturePolicy,
    writer: WriterSource,
    pipeline: Option<PipelineCell>,
    rebuild: RebuildHandle,
}

/// One link's owner-local parts.
struct LinkSlot {
    tap: Rc<TapGestureRecognizer>,
    node: Rc<FocusNode>,
    rect: RectProvider,
    /// Rebuilds the paragraph when the node's focus or its visibility
    /// changes, so the ring and the link node follow.
    _subscription: FocusSubscription,
}

/// The links' recognizers and focus nodes, one slot per link in
/// [`InlineSpan::links`] order.
pub(super) struct LinkedParagraphState {
    live: Rc<LiveLinks>,
    mount: Option<Mount>,
    /// Publishes the paragraph's render id, so a link's rect provider can
    /// measure its glyphs.
    anchor: SubtreeAnchor,
    slots: Vec<LinkSlot>,
}

/// Link `index`'s tap recognizer, in `mount`'s arena.
fn tap(live: &Rc<LiveLinks>, mount: &Mount, index: usize) -> Rc<TapGestureRecognizer> {
    let (live, writer) = (Rc::clone(live), mount.writer.clone());
    TapGestureRecognizer::builder(mount.policy.arena.clone())
        .settings(mount.policy.settings.clone())
        .on_tap(move |_| writer.write(|cx| live.activate(cx, index)))
        .build()
}

impl LinkedParagraphState {
    fn slot(&self, mount: &Mount, index: usize) -> LinkSlot {
        let node = FocusNode::with_debug_label("text link");
        let rebuild = mount.rebuild.clone();
        let subscription = node.subscribe(Rc::new(move || {
            rebuild.schedule(RebuildReason::StateChange);
        }));
        LinkSlot {
            tap: tap(&self.live, mount, index),
            rect: link_rect(mount.pipeline.clone(), self.anchor.clone(), index),
            node,
            _subscription: subscription,
        }
    }

    /// Slots for links `from..to`.
    fn slots(&self, from: usize, to: usize) -> Vec<LinkSlot> {
        let Some(mount) = &self.mount else {
            return Vec::new();
        };
        (from..to).map(|index| self.slot(mount, index)).collect()
    }

    fn publish_nodes(&self) {
        *self.live.nodes.borrow_mut() = self
            .slots
            .iter()
            .map(|slot| Rc::clone(&slot.node))
            .collect();
    }

    /// The link holding the primary focus, with the ring while it shows.
    fn link_focus(&self, ctx: &dyn BuildContext) -> Option<TextLinkFocus> {
        // Read on every build, so a new style reaches a ring shown later.
        let style = DefaultFocusRingStyle::of(ctx);
        let link = self
            .slots
            .iter()
            .position(|slot| slot.node.has_primary_focus())?;
        let ring = self.slots[link].node.has_visible_focus().then_some(style);
        Some(TextLinkFocus { link, ring })
    }
}

/// Measures link `index` by its glyphs in the paragraph `anchor` publishes,
/// in the render root's coordinates, from the committed layout. A link
/// `max_lines` or an ellipsis cut has no glyphs: it is measured by the
/// paragraph, so it keeps its place in reading order. `None` while the
/// paragraph is unmounted or the pipeline is busy.
fn link_rect(pipeline: Option<PipelineCell>, anchor: SubtreeAnchor, index: usize) -> RectProvider {
    Rc::new(move || {
        let pipeline = pipeline.as_ref()?;
        let anchor_id = anchor.get()?;
        pipeline
            .try_with_layout(|owner| {
                let root = owner.root_id()?;
                let tree = owner.render_tree();
                let paragraph_id = *tree.children(anchor_id).first()?;
                // The one concrete render object `RichText` mounts under the
                // anchor, through the storage layer's erasure.
                let paragraph = tree
                    .get(paragraph_id)?
                    .as_box()?
                    .render_object()
                    .downcast_ref::<RenderParagraph>()?;
                let bounds = paragraph.link_bounds(index).or_else(|| {
                    let size = owner.box_size(paragraph_id)?;
                    Some(Rect::from_ltwh(0.0, 0.0, size.width, size.height))
                })?;
                Some(
                    owner
                        .transform_to(paragraph_id, root)?
                        .transform_rect(&bounds),
                )
            })
            .flatten()
    })
}

fn cancel<'a>(slots: impl IntoIterator<Item = &'a LinkSlot>) {
    cancel_all(
        slots
            .into_iter()
            .map(|slot| &*slot.tap as &dyn GestureRecognizer),
    );
}

impl ViewState<LinkedParagraph> for LinkedParagraphState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.mount = Some(Mount {
            policy: GesturePolicy::of(ctx),
            writer: ctx.writer_source(),
            pipeline: ctx.pipeline_owner(),
            rebuild: ctx.rebuild_handle(),
        });
        self.slots = self.slots(0, self.live.links.borrow().len());
        self.publish_nodes();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let policy = GesturePolicy::of(ctx);
        let Some(mount) = &mut self.mount else {
            return;
        };
        mount.writer = ctx.writer_source();
        if mount.policy == policy {
            return;
        }
        mount.policy = policy;
        // The replacements are committed before the outgoing recognizers'
        // cancellation runs their callbacks; the focus nodes stay, and with
        // them the focus.
        let outgoing: Vec<_> = self
            .slots
            .iter_mut()
            .enumerate()
            .map(|(index, slot)| std::mem::replace(&mut slot.tap, tap(&self.live, mount, index)))
            .collect();
        cancel_all(outgoing.iter().map(|tap| &**tap as &dyn GestureRecognizer));
    }

    fn did_update_view(&mut self, _old_view: &LinkedParagraph, new_view: &LinkedParagraph) {
        self.live.refresh(new_view);
        let count = new_view.link_count();
        if count > self.slots.len() {
            let added = self.slots(self.slots.len(), count);
            self.slots.extend(added);
            self.publish_nodes();
        } else if count < self.slots.len() {
            let retired = self.slots.split_off(count);
            self.publish_nodes();
            cancel(&retired);
        }
    }

    fn build(&self, view: &LinkedParagraph, ctx: &dyn BuildContext) -> impl IntoView {
        let links = self.mount.as_ref().map(|mount| LinkRouting {
            live: Rc::clone(&self.live),
            writer: mount.writer.clone(),
            taps: self.slots.iter().map(|slot| Rc::clone(&slot.tap)).collect(),
        });
        let paragraph = AnchoredBox::new(
            self.anchor.clone(),
            ResolvedParagraph {
                links,
                link_focus: self.link_focus(ctx),
                ..view.paragraph.clone()
            },
        );
        // One `Focus` per link, the first outermost. The links' semantics
        // live on the paragraph's link nodes, so the `Focus` widgets add none.
        self.slots
            .iter()
            .enumerate()
            .rev()
            .fold(paragraph.boxed(), |child, (index, slot)| {
                let live = Rc::clone(&self.live);
                Actions::new(
                    Focus::new(child)
                        .focus_node(Rc::clone(&slot.node))
                        .rect_provider(Rc::clone(&slot.rect))
                        .include_semantics(false)
                        .on_key_event(move |cx, event| {
                            if opens_a_link(event) {
                                live.activate(cx, index);
                                KeyEventResult::Handled
                            } else {
                                KeyEventResult::Ignored
                            }
                        }),
                )
                .action::<ActivateIntent>(DeclineActivation)
                .action::<ActivationPressIntent>(DeclineActivation)
                .boxed()
            })
    }

    fn dispose(&mut self) {
        // Refuse taps and activations before cancellation can run callbacks.
        self.live.mounted.set(false);
        let outgoing = std::mem::take(&mut self.slots);
        self.live.nodes.borrow_mut().clear();
        cancel(&outgoing);
    }
}

/// Whether `event` opens the focused link: Enter's key-down, as on every
/// platform's links. Space is not: its release is the default bindings'
/// [`ActivateIntent`] too, so the key a link opens on is matched here.
fn opens_a_link(event: &KeyEvent) -> bool {
    SingleActivator::named(NamedKey::Enter)
        .allow_repeats(false)
        .matches(event)
}

/// A focused link's answer to the activation intents: declared but disabled,
/// so Space neither opens the link nor reaches a control around it, and the
/// key keeps bubbling.
#[derive(Clone, Copy)]
struct DeclineActivation;

impl Action<ActivateIntent> for DeclineActivation {
    fn is_enabled(&self, _intent: &ActivateIntent) -> bool {
        false
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        ActionOutcome::NotPerformed
    }
}

impl Action<ActivationPressIntent> for DeclineActivation {
    fn is_enabled(&self, _intent: &ActivationPressIntent) -> bool {
        false
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::NotPerformed
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

/// Run the click or focus request `target` names, on the owner thread inside
/// its UI runtime. Every failure is a dropped action, never a panic: the
/// platform asked for something the tree can no longer do.
fn activate(target: LocalPayloadTarget, call: &ActionCall) {
    let action = call.action();
    if !matches!(action, SemanticsAction::Click | SemanticsAction::Focus) {
        return;
    }
    let payload = match resolve_local_payload(target) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                ?error,
                ?action,
                "a link action was dropped: its handler is owner-local and runs only \
                 inside its ui_runtime"
            );
            return;
        }
    };
    let Ok(activation) = payload.downcast::<LinkActivation>() else {
        tracing::error!("BUG: a link activation ticket resolved to a payload of another type");
        return;
    };
    if action == SemanticsAction::Focus {
        activation.live.focus(activation.index);
    } else {
        activation
            .writer
            .write(|cx| activation.live.activate(cx, activation.index));
    }
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
