//! A paragraph whose links a handler activates.
//!
//! Each link gets a tap recognizer of its own, built against the enclosing
//! arena scope, so two contacts on two links recognize independently and a
//! link competes with the gestures around it like any other tap. Each link
//! is also a focus stop of its own: a [`Focus`] per link hosts a node
//! measured by the link's glyphs, so Tab reaches links in reading order
//! among every other control. The paragraph binds Enter and Select to
//! [`ActivateLinkIntent`], which the focused link answers (ADR-0079,
//! amendment for links); Space's release is [`ActivateIntent`], which a link
//! declines, because a link is not a button. A link `max_lines` or an
//! ellipsis cut takes no focus: the paragraph says after layout which links
//! it shows.
//!
//! A link is one link across rebuilds by what it links to and which
//! occurrence of that target it is, not by its place in the text: a link
//! added before the focused one leaves the focus, a press in flight and the
//! accessible node where they were.
//!
//! The recognizers, nodes and handler are owner-local; the render object
//! holds only the lane identities that reach them ([`TextLinkRoute`]): one
//! pointer target per link, and one activation ticket per link that
//! assistive technology clicks and focuses through.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Rect;
use flui_foundation::{ValueKey, ViewKey};
use flui_interaction::focus::{FocusCause, FocusNode, FocusSubscription};
use flui_interaction::{
    GestureRecognizer, InteractionDispatchError, LocalPayloadTarget, PointerDispatch,
    PointerTarget, RecognizerSet, RectProvider, TapGestureRecognizer, cancel_all,
    resolve_local_payload,
};
use flui_objects::{
    RenderParagraph, SemanticsActionRoute, ShownLinksListener, StackFit, SubtreeAnchor,
    TextLinkFocus, TextLinkKey, TextLinkRoute,
};
use flui_painting::typography::{InlineSpan, TextLink};
use flui_platform::keyboard::NamedKey;
use flui_rendering::hit_testing::PointerEvent;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::semantics::{ActionCall, SemanticsAction, SemanticsActionHandler};
use flui_view::element::ElementKind;
use flui_view::{
    BuildContext, EventCx, IntoView, LifecycleContext, RebuildHandle, RebuildReason,
    RenderObjectContext, RenderObjectContextError, StatefulView, StatelessView, View, ViewExt,
    ViewState, WriterSource,
};

use super::rich_text::ResolvedParagraph;
use crate::anchored_box::AnchoredBox;
use crate::interaction::GesturePolicy;
use crate::support::RefCallback;
use crate::{
    Action, ActionOutcome, Actions, ActivateIntent, ActivateLinkIntent, ActivationPressIntent,
    DefaultFocusRingStyle, Focus, Positioned, Shortcuts, SingleActivator, SizedBox, Stack,
};

/// What a link's tap, key and assistive activation read when they fire: the
/// latest handler, whether the paragraph is still mounted, and which links
/// its last layout showed.
struct LiveLinks {
    on_link: RefCell<RefCallback<TextLink>>,
    mounted: Cell<bool>,
    /// One flag per link in text order, from the paragraph's last layout;
    /// empty until it reports, which counts every link shown.
    shown: RefCell<Vec<bool>>,
}

impl LiveLinks {
    /// Run the handler for `link` inside `cx`, with no borrow held while it
    /// runs.
    fn open(&self, cx: &mut EventCx<'_>, link: &TextLink) {
        if !self.mounted.get() {
            return;
        }
        let handler = Rc::clone(&self.on_link.borrow());
        handler(cx, link);
    }

    /// Move the focus to `node`, as assistive technology asked.
    fn focus(&self, node: &Rc<FocusNode>) {
        if self.mounted.get() {
            let _ = node.request_focus_with(FocusCause::Assistive);
        }
    }

    fn is_shown(&self, position: usize) -> bool {
        self.shown.borrow().get(position).copied().unwrap_or(true)
    }
}

/// What makes a link the same link across rebuilds: what it links to, and
/// which occurrence of that target it is in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LinkIdentity {
    link: TextLink,
    occurrence: usize,
}

/// The identities of `text`'s links, in [`InlineSpan::links`] order.
fn identities_of(text: &InlineSpan) -> Vec<LinkIdentity> {
    let links = text.links();
    links
        .iter()
        .enumerate()
        .map(|(position, &link)| LinkIdentity {
            link: link.clone(),
            occurrence: links[..position]
                .iter()
                .filter(|&&seen| seen == link)
                .count(),
        })
        .collect()
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

    fn identities(&self) -> Vec<LinkIdentity> {
        identities_of(&self.paragraph.authored.text)
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
            mounted: Cell::new(true),
            shown: RefCell::new(Vec::new()),
        });
        LinkedParagraphState {
            live,
            mount: None,
            anchor: SubtreeAnchor::new(),
            mounting: self.identities(),
            slots: Vec::new(),
            next_key: 0,
        }
    }
}

/// What the state acquired when it mounted, to build each link's parts.
struct Mount {
    policy: GesturePolicy,
    writer: WriterSource,
    pipeline: Option<PipelineCell>,
    rebuild: RebuildHandle,
    shown: ShownLinksListener,
}

/// One link's owner-local parts, kept while the link keeps its identity.
struct LinkSlot {
    identity: LinkIdentity,
    key: TextLinkKey,
    /// The link's current place in the text, which its rect provider and
    /// the paragraph address it by.
    position: Rc<Cell<usize>>,
    tap: Rc<TapGestureRecognizer>,
    node: Rc<FocusNode>,
    rect: RectProvider,
    /// The lane registrations the render view made for this link, kept
    /// with the link so they follow it when links before it come and go.
    route: Rc<RefCell<Option<TextLinkRoute>>>,
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
    /// The links of the view the element mounts with, which `init_state`
    /// (given no view) makes slots for.
    mounting: Vec<LinkIdentity>,
    slots: Vec<LinkSlot>,
    /// The key the next new link gets; a link keeps its key while it keeps
    /// its identity.
    next_key: u64,
}

/// `link`'s tap recognizer, in `mount`'s arena.
fn tap(live: &Rc<LiveLinks>, mount: &Mount, link: TextLink) -> Rc<TapGestureRecognizer> {
    let (live, writer) = (Rc::clone(live), mount.writer.clone());
    TapGestureRecognizer::builder(mount.policy.arena.clone())
        .settings(mount.policy.settings.clone())
        .on_tap(move |_| writer.write(|cx| live.open(cx, &link)))
        .build()
}

impl LinkedParagraphState {
    /// Give the state a slot per identity, in order: a link keeps the slot
    /// of the same identity, a new one gets a new slot and key, and the
    /// slots no link has any more are retired.
    fn reconcile(&mut self, identities: Vec<LinkIdentity>) {
        let Some(mount) = &self.mount else {
            return;
        };
        let mut outgoing: Vec<Option<LinkSlot>> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(Some)
            .collect();
        let mut slots = Vec::with_capacity(identities.len());
        for (position, identity) in identities.into_iter().enumerate() {
            let kept = outgoing
                .iter_mut()
                .find_map(|slot| slot.take_if(|slot| slot.identity == identity));
            let slot = kept.unwrap_or_else(|| {
                let key = TextLinkKey(self.next_key);
                self.next_key = self
                    .next_key
                    .checked_add(1)
                    .expect("BUG: a paragraph made 2^64 links");
                new_slot(&self.live, &self.anchor, mount, identity, key)
            });
            slot.position.set(position);
            slots.push(slot);
        }
        self.slots = slots;
        // The paragraph's last report addressed the links by their old
        // places; the next layout reports again.
        self.live.shown.borrow_mut().clear();
        let retired: Vec<LinkSlot> = outgoing.into_iter().flatten().collect();
        cancel(&retired);
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

fn new_slot(
    live: &Rc<LiveLinks>,
    anchor: &SubtreeAnchor,
    mount: &Mount,
    identity: LinkIdentity,
    key: TextLinkKey,
) -> LinkSlot {
    let node = FocusNode::with_debug_label("text link");
    let rebuild = mount.rebuild.clone();
    let subscription = node.subscribe(Rc::new(move || {
        rebuild.schedule(RebuildReason::StateChange);
    }));
    let position = Rc::new(Cell::new(0));
    LinkSlot {
        tap: tap(live, mount, identity.link.clone()),
        rect: link_rect(mount.pipeline.clone(), anchor.clone(), Rc::clone(&position)),
        identity,
        key,
        position,
        node,
        route: Rc::new(RefCell::new(None)),
        _subscription: subscription,
    }
}

/// Records which links the paragraph's layout shows and rebuilds the
/// paragraph when that changed, so a link the layout cut takes no focus.
fn shown_links_listener(live: &Rc<LiveLinks>, rebuild: RebuildHandle) -> ShownLinksListener {
    let live = Rc::downgrade(live);
    ShownLinksListener::new(move |shown| {
        let Some(live) = live.upgrade() else {
            return;
        };
        if live.shown.borrow().as_slice() != shown {
            *live.shown.borrow_mut() = shown.to_vec();
            rebuild.schedule(RebuildReason::StateChange);
        }
    })
}

/// Measures the link at `position` by its glyphs in the paragraph `anchor`
/// publishes, in the render root's coordinates, from the committed layout.
/// A link with no glyphs is measured by the paragraph. `None` while the
/// paragraph is unmounted or the pipeline is busy.
fn link_rect(
    pipeline: Option<PipelineCell>,
    anchor: SubtreeAnchor,
    position: Rc<Cell<usize>>,
) -> RectProvider {
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
                let size = owner.box_size(paragraph_id)?;
                let own = Rect::from_ltwh(0.0, 0.0, size.width, size.height);
                // Within the paragraph's box: a last line's glyph box can end
                // a rounding error past it, which would put the link in one
                // reading-order row with whatever touches the paragraph.
                let bounds = paragraph
                    .link_bounds(position.get())
                    .and_then(|glyphs| glyphs.intersect(&own))
                    .unwrap_or(own);
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
        let rebuild = ctx.rebuild_handle();
        self.mount = Some(Mount {
            policy: GesturePolicy::of(ctx),
            writer: ctx.writer_source(),
            pipeline: ctx.pipeline_owner(),
            shown: shown_links_listener(&self.live, rebuild.clone()),
            rebuild,
        });
        let mounting = std::mem::take(&mut self.mounting);
        self.reconcile(mounting);
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
            .map(|slot| {
                let replacement = tap(&self.live, mount, slot.identity.link.clone());
                std::mem::replace(&mut slot.tap, replacement)
            })
            .collect();
        cancel_all(outgoing.iter().map(|tap| &**tap as &dyn GestureRecognizer));
    }

    fn did_update_view(&mut self, _old_view: &LinkedParagraph, new_view: &LinkedParagraph) {
        *self.live.on_link.borrow_mut() = Rc::clone(&new_view.on_link);
        let identities = new_view.identities();
        let unchanged = identities.len() == self.slots.len()
            && identities
                .iter()
                .zip(&self.slots)
                .all(|(identity, slot)| *identity == slot.identity);
        if !unchanged {
            self.reconcile(identities);
        }
    }

    fn build(&self, view: &LinkedParagraph, ctx: &dyn BuildContext) -> impl IntoView {
        let links = self.mount.as_ref().map(|mount| LinkRouting {
            live: Rc::clone(&self.live),
            writer: mount.writer.clone(),
            shown: mount.shown.clone(),
            links: self
                .slots
                .iter()
                .map(|slot| RoutedLink {
                    key: slot.key,
                    link: slot.identity.link.clone(),
                    tap: Rc::clone(&slot.tap),
                    node: Rc::clone(&slot.node),
                    route: Rc::clone(&slot.route),
                })
                .collect(),
        });
        let paragraph = AnchoredBox::new(
            self.anchor.clone(),
            ResolvedParagraph {
                links,
                link_focus: self.link_focus(ctx),
                ..view.paragraph.clone()
            },
        );
        // The paragraph, and beside it one zero-size focus stop per link,
        // keyed by the link: a stop stays with its link, and the paragraph
        // with its element, when links before them come and go.
        let mut children = Vec::with_capacity(self.slots.len() + 1);
        children.push(paragraph.boxed());
        children.extend(self.slots.iter().enumerate().map(|(position, slot)| {
            LinkFocusStop {
                key: ValueKey::new(slot.key.0),
                live: Rc::clone(&self.live),
                link: slot.identity.link.clone(),
                node: Rc::clone(&slot.node),
                rect: Rc::clone(&slot.rect),
                shown: self.live.is_shown(position),
            }
            .boxed()
        }));
        let open = |key| SingleActivator::named(key).allow_repeats(false);
        Shortcuts::new(Stack::new(children).fit(StackFit::Passthrough))
            .shortcut(open(NamedKey::Enter), ActivateLinkIntent)
            .shortcut(open(NamedKey::Select), ActivateLinkIntent)
    }

    fn dispose(&mut self) {
        // Refuse taps and activations before cancellation can run callbacks.
        self.live.mounted.set(false);
        let outgoing = std::mem::take(&mut self.slots);
        cancel(&outgoing);
    }
}

/// A link's focus stop: a zero-size `Focus` beside the paragraph whose node
/// is measured by the link's glyphs. It answers [`ActivateLinkIntent`] and
/// declines the activation intents; the link's semantics live on the
/// paragraph's link node, so the stop publishes none. A link the layout cut
/// takes no focus.
#[derive(Clone)]
struct LinkFocusStop {
    key: ValueKey<u64>,
    live: Rc<LiveLinks>,
    link: TextLink,
    node: Rc<FocusNode>,
    rect: RectProvider,
    shown: bool,
}

impl View for LinkFocusStop {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for LinkFocusStop {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let stop = Actions::new(
            Focus::new(SizedBox::shrink())
                .focus_node(Rc::clone(&self.node))
                .rect_provider(Rc::clone(&self.rect))
                .can_request_focus(self.shown)
                .include_semantics(false),
        )
        .action::<ActivateLinkIntent>(OpenLink {
            live: Rc::clone(&self.live),
            link: self.link.clone(),
        })
        .action::<ActivateIntent>(DeclineActivation)
        .action::<ActivationPressIntent>(DeclineActivation);
        Positioned::new(stop).left(0.0).top(0.0)
    }
}

/// Enter or Select on a focused link: opens it.
#[derive(Clone)]
struct OpenLink {
    live: Rc<LiveLinks>,
    link: TextLink,
}

impl Action<ActivateLinkIntent> for OpenLink {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ActivateLinkIntent) -> ActionOutcome {
        self.live.open(cx, &self.link);
        ActionOutcome::Performed
    }
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

/// One link as the paragraph's render view registers it.
#[derive(Clone)]
struct RoutedLink {
    key: TextLinkKey,
    link: TextLink,
    tap: Rc<TapGestureRecognizer>,
    node: Rc<FocusNode>,
    route: Rc<RefCell<Option<TextLinkRoute>>>,
}

/// What the paragraph's render view registers in the owner lane for each
/// link, in text order: the link's recognizer behind a pointer target, and
/// its activation behind a payload ticket. It also hands the paragraph the
/// listener its layout reports shown links to.
#[derive(Clone)]
pub(super) struct LinkRouting {
    live: Rc<LiveLinks>,
    writer: WriterSource,
    shown: ShownLinksListener,
    links: Vec<RoutedLink>,
}

impl std::fmt::Debug for LinkRouting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkRouting")
            .field("links", &self.links.len())
            .finish_non_exhaustive()
    }
}

/// The lane payload one link's assistive actions resolve.
struct LinkActivation {
    live: Rc<LiveLinks>,
    writer: WriterSource,
    link: TextLink,
    node: Rc<FocusNode>,
}

/// Run the click or focus request `target` names on its link, on the owner
/// thread inside its UI runtime. Every failure is a dropped action, never a
/// panic: the platform asked for something the tree can no longer do.
fn run_link_action(target: LocalPayloadTarget, call: &ActionCall) {
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
        activation.live.focus(&activation.node);
    } else {
        activation
            .writer
            .write(|cx| activation.live.open(cx, &activation.link));
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
    /// The listener the paragraph reports its shown links to.
    pub(super) fn shown_links_listener(&self) -> &ShownLinksListener {
        &self.shown
    }

    fn activation(&self, routed: &RoutedLink) -> Rc<dyn std::any::Any> {
        Rc::new(LinkActivation {
            live: Rc::clone(&self.live),
            writer: self.writer.clone(),
            link: routed.link.clone(),
            node: Rc::clone(&routed.node),
        })
    }

    /// Register `routed` in the owner lane, or `None` when no lane is active
    /// (a detached mount): the link then stays plain text.
    fn register(
        &self,
        ctx: &RenderObjectContext<'_>,
        routed: &RoutedLink,
    ) -> Option<TextLinkRoute> {
        let pointer = ctx.register_pointer(pointer_handler(&routed.tap));
        let activation = ctx.register_local_payload(self.activation(routed));
        match (pointer, activation) {
            (Ok(pointer), Ok(target)) => {
                let handler: SemanticsActionHandler =
                    Rc::new(move |call| run_link_action(target, &call));
                Some(TextLinkRoute::new(
                    routed.key,
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

    /// Replace `routed`'s handlers under `route`'s registrations, which keeps
    /// the published activation handler. A registration the lane lost is
    /// made anew; one the lane cannot be reached for is kept as it is, so
    /// unmount still releases it.
    fn replace(
        &self,
        ctx: &RenderObjectContext<'_>,
        route: &TextLinkRoute,
        routed: &RoutedLink,
    ) -> Option<TextLinkRoute> {
        let replaced = ctx
            .replace_pointer(route.pointer_target(), pointer_handler(&routed.tap))
            .and_then(|()| {
                ctx.replace_local_payload(route.activation().target(), self.activation(routed))
            });
        match replaced {
            Ok(()) => Some(route.clone()),
            Err(RenderObjectContextError::Interaction(InteractionDispatchError::TargetGone)) => {
                tracing::debug!("a link's lane registration was gone; registering anew");
                release_route(ctx, route);
                self.register(ctx, routed)
            }
            Err(error) => {
                tracing::debug!(?error, "a link's handlers were not replaced");
                Some(route.clone())
            }
        }
    }

    /// The routes for `routing`'s links, in text order. Each link keeps the
    /// registrations it made before, wherever it now is; the paragraph's
    /// `existing` routes no link holds any more are released.
    pub(super) fn sync(
        routing: Option<&Self>,
        ctx: &RenderObjectContext<'_>,
        existing: &[TextLinkRoute],
    ) -> Vec<TextLinkRoute> {
        let mut routes = Vec::new();
        if let Some(routing) = routing {
            for routed in &routing.links {
                let held = routed.route.borrow().clone();
                let route = match held {
                    Some(route) => routing.replace(ctx, &route, routed),
                    None => routing.register(ctx, routed),
                };
                routed.route.borrow_mut().clone_from(&route);
                // Routes stay in text order: past a link that could not be
                // routed, the rest stay plain text too.
                match route {
                    Some(route) => routes.push(route),
                    None => break,
                }
            }
            for routed in routing.links.iter().skip(routes.len()) {
                routed.route.borrow_mut().take();
            }
        }
        for route in existing {
            if !routes
                .iter()
                .any(|kept| kept.pointer_target() == route.pointer_target())
            {
                release_route(ctx, route);
            }
        }
        routes
    }

    /// Remove `routes` from the owner lane and from the links that held
    /// them, as the paragraph unmounts.
    pub(super) fn release(
        routing: Option<&Self>,
        ctx: &RenderObjectContext<'_>,
        routes: &[TextLinkRoute],
    ) {
        for route in routes {
            release_route(ctx, route);
        }
        for routed in routing.iter().flat_map(|routing| &routing.links) {
            routed.route.borrow_mut().take();
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
