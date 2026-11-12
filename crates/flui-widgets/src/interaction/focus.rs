//! [`Focus`] and [`FocusScope`] â€” the widgets that put `flui-interaction`'s
//! focus tree into the element tree.
//!
//! ADR-0026. The node/manager layer (`FocusManager`, `FocusNode`,
//! `FocusScopeNode` â€” tracker H4) predates these widgets; what they add is the
//! lifecycle wiring: a widget-owned node attached under the nearest enclosing
//! scope on mount, moved with [`FocusScopeNode::adopt_node`] when that scope
//! changes, and detached on dispose.
//!
//! # Design notes (ADR-0026)
//!
//! * **Nodes parent to the nearest focus *node*** â€” scope or plain `Focus` â€”
//!   through one provider. (An earlier design flattened to the nearest scope;
//!   key bubbling made the node tree's shape observable and superseded that
//!   decision â€” ADR-0026, ADR-0023.)
//! * **Reparenting happens in `did_change_dependencies`**, not on every build
//!   â€” the provider notifying is the only way the enclosing scope changes
//!   without a remount. Observable only through the parent node, which is not
//!   exposed.
//! * **Focus changes apply synchronously.** FLUI's `FocusManager` is
//!   synchronous throughout, so `autofocus` runs inline from `init_state`
//!   rather than being batched to the end of the frame.
//! * Not provided: a legacy raw-key handler, a parent-node accessor, and a
//!   per-node descendants-traversable flag (no node-layer flag). [`Focus::of`],
//!   [`Focus::maybe_of`] and [`FocusScope::of`] exist (ADR-0076's
//!   `OverlayScope` precedent), but there is no `Focus` lookup that also
//!   accepts a scope node: [`FocusScope::of`] already covers "give me the
//!   nearest scope".

use std::{cell::Cell, rc::Rc};

use crate::anchored_box::{AnchoredBox, AnchoredSliver};
use crate::localization::Directionality;
use crate::semantics::{Semantics, SliverSemantics};
use crate::support::value_callback;
use flui_foundation::Subscription;
use flui_foundation::geometry::{Axis, Rect};
use flui_interaction::focus::{
    FocusAttachment, FocusManager, FocusNode, FocusNodeRegistration, FocusScopeNode,
    FocusTraversalOverrides, KeyEventResult, RectProvider, TraversalEdgeBehavior,
};
use flui_objects::SubtreeAnchor;
use flui_platform::keyboard::KeyEvent;
use flui_rendering::semantics::SemanticsConfiguration;
use flui_view::element::ElementKind;
use flui_view::environment::FocusTreeRoot;
use flui_view::prelude::*;
use flui_view::{Provided, ProvidedValue, RebuildHandle};

use super::actions::{EnclosingActions, FocusRecord};
use super::focus_host::{FocusConfig, FocusHost, FocusNodeOwnership};

/// Reports whether this widget's node gained or lost the primary focus, with
/// the dispatch's [`EventCx`] (ADR-0086).
pub type FocusChangeHandler = Rc<dyn Fn(&mut EventCx<'_>, bool)>;

// ============================================================================
// The ambient scope
// ============================================================================

/// The nearest enclosing focus **node**, scope or plain `Focus`, as every
/// focus widget publishes it to its subtree from its own element
/// (ADR-0322 §5). Private: [`Focus`] and [`FocusScope`] are the public
/// surface.
///
/// The parent being a *node*, not always a scope, is what makes the
/// leaf-to-root key-dispatch walk (ADR-0023) match the widget tree: a
/// `Shortcuts`-style non-scope `Focus` above a field is a node **ancestor**
/// of the field, so keys the field ignores bubble through it.
#[derive(Clone)]
pub(super) struct FocusParent {
    pub(super) parent: Rc<FocusNode>,
    pub(super) revision: u64,
}

impl std::fmt::Debug for FocusParent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusParent")
            .field("parent_id", &self.parent.id())
            .field("revision", &self.revision)
            .finish()
    }
}

impl ProvidedValue for FocusParent {
    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.parent, &old.parent) || self.revision != old.revision
    }
}

/// The node a mounting/reparenting focus node hangs under.
///
/// The UI runtime roots every presentation in a [`FocusTreeRoot`], so a
/// missing provider is a broken embedder invariant rather than a reason to
/// reach for the lifecycle-only `LifecycleContext::focus_manager` capability
/// from arbitrary build paths.
pub fn enclosing_focus_parent(ctx: &dyn BuildContext) -> Rc<FocusNode> {
    nearest_focus_node(ctx).expect(
        "BUG: focus widget mounted outside FocusTreeRoot; every presentation root must install \
         the focus-tree root",
    )
}

/// Raw lookup behind [`Focus::of`]/[`Focus::maybe_of`]/[`FocusScope::of`]:
/// the nearest enclosing [`FocusParent`]'s node, else the
/// presentation's root scope from [`FocusTreeRoot::maybe_of`], registering a
/// dependency, with **no** scope-node filtering. This is the call
/// [`FocusScope::of`] makes internally; [`Focus::maybe_of`] layers the
/// scope-node filter back on top.
///
/// # Depend, not get
///
/// Unlike `Overlay::maybe_of` (`overlay/mod.rs`), this registers a dependency
/// through [`BuildContextExt::depend_on`], so a build that reads it rebuilds
/// when the enclosing node changes.
///
/// This resolves the SAME [`FocusParent`] value `Focus`/`FocusScope` publish
/// from their own elements for [`enclosing_focus_parent`]'s mount-time
/// attach/reparent lookups. No second, redundant marker is mounted just for
/// this public entry point. The root
/// fallback is reached only by a widget with no focus widget above it; every
/// focus widget sits below the presentation's one `FocusTreeRoot`, so the
/// nearest provider is always the nearer node.
///
/// Each provider carries a revision advanced by its node listener. `update_should_notify` therefore
/// invalidates dependents on node-state changes as well as identity changes;
/// a build that reads `Focus::of(ctx).has_focus()` stays live.
fn nearest_focus_node(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
    ctx.depend_on::<FocusParent, _>(|provider| Rc::clone(&provider.parent))
        .or_else(|| FocusTreeRoot::maybe_of(ctx))
}

// ============================================================================
// Focus
// ============================================================================

/// Makes its subtree focusable: owns a [`FocusNode`] (or adopts an external
/// one), attaches it under the nearest enclosing [`FocusScope`] on mount, and
/// detaches it on dispose.
#[derive(Clone)]
pub struct Focus {
    child: BoxedView,
    config: FocusConfig,
    include_semantics: bool,
    rect_provider: Option<RectProvider>,
    sliver: bool,
}

impl Focus {
    /// A focusable subtree with a widget-owned node.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            config: FocusConfig::new(),
            include_semantics: true,
            rect_provider: None,
            sliver: false,
        }
    }

    /// Measure the node with `provider` instead of the child's box: for a
    /// focusable part of a render object, such as a link in a paragraph.
    /// Traversal order and the reveal on focus read it. Taken when the
    /// widget mounts; a later view's provider is not installed.
    #[must_use]
    pub(crate) fn rect_provider(mut self, provider: RectProvider) -> Self {
        self.rect_provider = Some(provider);
        self
    }

    /// Wrap a sliver child: the node is anchored, annotated and measured
    /// with the sliver protocol (its rect is the sliver's painted region).
    /// Taken when the widget mounts.
    #[must_use]
    pub(crate) fn sliver(mut self) -> Self {
        self.sliver = true;
        self
    }

    /// Install weak explicit links; unavailable targets fall back to traversal policy.
    #[must_use]
    pub fn traversal_overrides(mut self, overrides: FocusTraversalOverrides) -> Self {
        self.config.traversal_overrides = Some(overrides);
        self
    }

    /// Whether the subtree tells assistive technology it is focusable and
    /// when it holds the focus â€” `true` by default.
    /// Without it a screen reader cannot follow keyboard focus: Narrator
    /// read nothing when Tab moved to the counter's button on a live window.
    #[must_use]
    pub fn include_semantics(mut self, include: bool) -> Self {
        self.include_semantics = include;
        self
    }

    /// Host `node` and let this widget manage the attributes explicitly set
    /// through the builder methods.
    ///
    /// Omitted attributes retain their current value on an external node.
    /// The caller keeps ownership of the node itself; this widget only
    /// attaches, reparents, and detaches it with its lifecycle. Use
    /// [`with_external_node`](Self::with_external_node) when the node must be
    /// the source of truth for all of its attributes.
    #[must_use]
    pub fn focus_node(mut self, node: Rc<FocusNode>) -> Self {
        self.config.node_ownership = FocusNodeOwnership::ManagedExternal(node);
        self
    }

    /// Host an external node without ever overwriting its focusability,
    /// traversal, or key-handler attributes.
    ///
    /// The caller-owned node is the source of truth, while the widget still owns the presentation
    /// attachment lifecycle. Configuration builder methods remain useful
    /// when constructing both modes generically, but their node-attribute
    /// values are intentionally ignored in this mode.
    #[must_use]
    pub fn with_external_node(node: Rc<FocusNode>, child: impl IntoView) -> Self {
        let mut focus = Self::new(child);
        focus.config.node_ownership = FocusNodeOwnership::ExternalSource(node);
        focus
    }

    /// Request focus on mount if the enclosing scope has no focused child;
    /// at most one child of a scope should set it.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.config.autofocus = autofocus;
        self
    }

    /// Whether the node may receive focus at all (`:284-296`).
    #[must_use]
    pub fn can_request_focus(mut self, can: bool) -> Self {
        self.config.can_request_focus = Some(can);
        self
    }

    /// Whether Tab traversal skips this node while it stays focusable by
    /// request (`:270-282`).
    #[must_use]
    pub fn skip_traversal(mut self, skip: bool) -> Self {
        self.config.skip_traversal = Some(skip);
        self
    }

    /// Whether descendants of this node may receive focus (`:298-318`).
    #[must_use]
    pub fn descendants_are_focusable(mut self, focusable: bool) -> Self {
        self.config.descendants_are_focusable = Some(focusable);
        self
    }

    /// Called with `true`/`false` as this widget's node gains/loses the
    /// primary focus, and the
    /// dispatch's `&mut EventCx<'_>`, so it writes a signal directly
    /// (ADR-0086): `.on_focus_change(move |cx, focused| has_focus.set(cx, focused))`.
    ///
    /// Framework autofocus and reconfiguration run in lifecycle hooks outside
    /// the signal build guard, so their focus edges can write too. An application
    /// that explicitly requests focus inside its own `build` still has its
    /// resulting signal writes refused by the guard.
    #[must_use]
    pub fn on_focus_change<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, bool) -> R + 'static,
        R: EventOutcome,
    {
        self.config.on_focus_change = Some(value_callback(handler));
        self
    }

    /// Called with `true`/`false` as this widget's node gains/loses *visible*
    /// focus ([`FocusNode::has_visible_focus`]): the primary focus, moved or
    /// last used by a key, traversal or assistive technology. A pointer press
    /// that focuses the node does not raise it, and a key pressed afterwards
    /// does, with no change of primary focus. A control that draws its own
    /// focus indication follows this rather than
    /// [`on_focus_change`](Self::on_focus_change).
    #[must_use]
    pub fn on_visible_focus_change<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, bool) -> R + 'static,
        R: EventOutcome,
    {
        self.config.on_visible_focus_change = Some(value_callback(handler));
        self
    }

    /// Key handler invoked during the leafâ†’root dispatch walk while this
    /// node â€” or a descendant â€” holds the primary focus. Return
    /// [`Handled`](flui_interaction::KeyEventResult::Handled) to consume the
    /// event, [`Ignored`](flui_interaction::KeyEventResult::Ignored) to let it
    /// bubble to the enclosing `Focus`, or
    /// [`SkipRemainingHandlers`](flui_interaction::KeyEventResult::SkipRemainingHandlers)
    /// to stop the bubbling without consuming (ADR-0023).
    ///
    /// The handler runs after routing, so it receives the dispatch's
    /// `&mut EventCx<'_>` and may write signals; it keeps its decision as the
    /// return value, so a write's `Result` is not reported for it: call
    /// [`report`](EventOutcome::report) on a write whose refusal should be
    /// logged (ADR-0086 Â§6).
    #[must_use]
    pub fn on_key_event<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &KeyEvent) -> KeyEventResult + 'static,
    {
        self.config.on_key_event = Some(Rc::new(handler));
        self
    }

    /// A label for debug output (`:334`).
    #[must_use]
    pub fn debug_label(mut self, label: &'static str) -> Self {
        self.config.debug_label = Some(label);
        self
    }

    /// Returns the [`FocusNode`] of the [`Focus`] that most tightly encloses
    /// `ctx`. Only plain [`Focus`] nodes qualify; [`FocusScope::of`] covers
    /// the scope-node case.
    ///
    /// # Panics
    ///
    /// Panics with a message naming the missing ancestor if no enclosing
    /// [`Focus`] provides one. Use [`maybe_of`](Self::maybe_of) for a
    /// non-panicking lookup.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Rc<FocusNode> {
        Self::maybe_of(ctx).expect(
            "Focus::of() was called with a context that does not contain a Focus widget. \
             No Focus widget ancestor could be found starting from the context passed to \
             Focus::of() â€” wrap the subtree in a Focus, or use Focus::maybe_of with a \
             caller-chosen fallback.",
        )
    }

    /// Returns the [`FocusNode`] of the [`Focus`] that most tightly encloses
    /// `ctx`, registering a dependency (see `nearest_focus_node`'s doc for why
    /// it depends, unlike `Overlay::maybe_of`).
    ///
    /// `None` if the nearest enclosing node is a [`FocusScope`]'s own scope
    /// node rather than a plain [`Focus`] (a scope only
    /// satisfies [`FocusScope::of`], not this), or if there is no enclosing
    /// [`Focus`]/[`FocusScope`] at all.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
        nearest_focus_node(ctx).filter(|node| !node.is_scope())
    }
}

impl std::fmt::Debug for Focus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Focus")
            .field("autofocus", &self.config.autofocus)
            .field("debug_label", &self.config.debug_label)
            .finish_non_exhaustive()
    }
}

impl View for Focus {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Focus {
    type State = FocusState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        FocusState {
            host: FocusHost::new(&self.config),
            anchor: SubtreeAnchor::new(),
            rect_provider: self.rect_provider.clone(),
            sliver: self.sliver,
        }
    }
}

/// The state behind [`Focus`]. `pub` because `StatefulView::State`
/// requires it, and re-exported like every other widget's state in this crate
/// (`GestureDetectorState`, `AnimatedAlignState`, …) so a caller can name it.
pub struct FocusState {
    /// The node and its place in the focus tree (`focus_host.rs`).
    host: FocusHost,
    /// Publishes the child's `RenderId` while mounted, so the node's
    /// [`RectProvider`] can measure it —
    /// reading-order traversal sorts by this geometry (ADR-0026).
    anchor: SubtreeAnchor,
    /// The view's own geometry source, if it named one. Taken in
    /// `init_state`, where the host receives it or the anchor's.
    rect_provider: Option<RectProvider>,
    /// The child is a sliver: anchored and annotated with the sliver
    /// protocol, measured from its committed sliver geometry.
    sliver: bool,
}

impl std::fmt::Debug for FocusState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusState")
            .field("node_id", &self.host.node().id())
            .finish_non_exhaustive()
    }
}

/// The one-shot autofocus request of a control whose node hangs under
/// `parent`: it lands when the enclosing scope has nothing focused yet, or
/// holds only the route's provisional first focus, which it replaces.
pub(crate) fn request_autofocus(
    parent: Option<&Rc<FocusNode>>,
    manager: &FocusManager,
    node: &Rc<FocusNode>,
) {
    let scope = parent
        .and_then(|parent| parent.as_scope().or_else(|| parent.enclosing_scope()))
        .unwrap_or_else(|| Rc::clone(manager.root_scope()));
    let provisional = scope.take_provisional_first_focus();
    if provisional || scope.focused_child().is_none() {
        let _ = node.request_focus();
    }
}

/// The record of a widget at `ctx` whose bindings are the `Actions` chain
/// above it: the chain and the platform convention it is built under, with a
/// dependency on both, so a changed one is recorded again (ADR-0079).
pub(super) fn ambient_focus_record(ctx: &dyn BuildContext) -> Option<FocusRecord> {
    EnclosingActions::depend_on(ctx).map(|chain| FocusRecord::at(ctx, chain))
}

/// [`record_focus_context`] of the [`ambient_focus_record`]: what `Focus`
/// and `FocusScope` record.
fn record_action_chain(
    ctx: &dyn BuildContext,
    node: &Rc<FocusNode>,
    held: &mut Option<FocusRecord>,
    registration: &mut Option<FocusNodeRegistration>,
) {
    record_focus_context(node, ambient_focus_record(ctx), held, registration);
}

/// Record `chain` on the focus `node`, where a `Shortcuts` above resolves an
/// intent while the node holds the primary focus. `held` and `registration`
/// are the caller's memory of what it recorded last; an unchanged record
/// that is still the node's current one is left alone.
pub(super) fn record_focus_context(
    node: &Rc<FocusNode>,
    chain: Option<FocusRecord>,
    held: &mut Option<FocusRecord>,
    registration: &mut Option<FocusNodeRegistration>,
) {
    let unchanged = match (&chain, &*held) {
        (Some(new), Some(held)) => new.same_as(held),
        (None, None) => true,
        _ => false,
    };
    if unchanged
        && registration
            .as_ref()
            .is_none_or(FocusNodeRegistration::is_current)
    {
        return;
    }
    // Register the new record before the old token drops: the old one is no
    // longer current then, so dropping it leaves the new one in place.
    *registration = chain.as_ref().map(|record| record.register_on(node));
    *held = chain;
}

impl ViewState<Focus> for FocusState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rect_provider = match self.rect_provider.take() {
            Some(provider) => provider,
            None if self.sliver => sliver_rect_provider(&self.anchor, ctx),
            None => box_rect_provider(&self.anchor, ctx),
        };
        self.host
            .init(ctx, rect_provider, ambient_focus_record(ctx));
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.host
            .dependencies_changed(ctx, ambient_focus_record(ctx));
    }

    fn did_update_view(&mut self, _old_view: &Focus, new_view: &Focus) {
        self.host.update(&new_view.config);
    }

    fn dispose(&mut self) {
        self.host.dispose();
    }

    /// Anchors the child so the node's rect provider has a render node to
    /// measure.
    ///
    /// The subtree publishes whether its node can take focus (`focusable`)
    /// and, while it holds the primary focus, `focused`. The node listener rebuilds this widget on
    /// every focus edge, so the flag follows the focus. Assistive focus requests
    /// use this same mounted node and its normal admission policy.
    fn build(&self, view: &Focus, _ctx: &dyn BuildContext) -> impl IntoView {
        // The child always sits under one `Semantics`, whether the node can
        // take focus or not: whether it can changes with the page above
        // (a popup route makes everything under it unfocusable), and a
        // subtree whose wrapper appears and disappears is torn down and
        // mounted again, which cancels a gesture in flight under it. Only a
        // node that can take focus annotates it. A node that cannot â€” the one
        // a `Shortcuts` hosts its key handler on â€” leaves the configuration
        // empty, which adds nothing to the semantics tree; setting even a
        // false flag would gather the subtree into one node.
        if self.sliver {
            // A sliver annotation publishes the flags; assistive focus on a
            // sliver lands on its rows, which carry their own `Focus`.
            let mut configuration = SemanticsConfiguration::default();
            if view.include_semantics && self.host.node().can_request_focus() {
                configuration.set_focusable(true);
                configuration.set_focused(self.host.node().has_primary_focus());
            }
            let child = SliverSemantics::from_configuration(configuration)
                .child(view.child.clone())
                .into_view()
                .boxed();
            return BoxedView::new(AnchoredSliver::new(self.anchor.clone(), child).into_view());
        }
        let semantics = if view.include_semantics && self.host.node().can_request_focus() {
            let semantics = Semantics::new()
                .focusable(true)
                .focused(self.host.node().has_primary_focus());
            // Absent once another owner adopted the node (`FocusHost`).
            match self.host.assistive_focus_request() {
                Some(request) => semantics.on_focus(move |_cx| request()),
                None => semantics,
            }
        } else {
            Semantics::new()
        };
        let child = semantics.child(view.child.clone()).into_view().boxed();
        BoxedView::new(AnchoredBox::new(self.anchor.clone(), child).into_view())
    }

    /// Every `Focus` is the parent of the focus nodes under it.
    fn provide(&self, _view: &Focus, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        self.host.provide(values);
    }
}

/// The rect of `anchor`'s render node.
fn box_rect_provider(anchor: &SubtreeAnchor, ctx: &dyn LifecycleContext) -> RectProvider {
    let anchor = anchor.clone();
    render_box_rect_provider(move || anchor.get(), ctx)
}

/// The rect of the box render node `render_id` names: measured lazily at
/// traversal time against committed layout — `box_size` + `transform_to` the
/// render root, the `HeroHandle::bounding_box_in` shape. `None` (fall back to
/// the stored rect) while unmounted, checked out, or awaiting complete
/// layout.
pub(super) fn render_box_rect_provider(
    render_id: impl Fn() -> Option<flui_foundation::RenderId> + 'static,
    ctx: &dyn LifecycleContext,
) -> RectProvider {
    let owner = ctx.pipeline_owner();
    Rc::new(move || {
        let render_id = render_id()?;
        let owner = owner.as_ref()?;
        owner
            .try_with_layout(|owner| {
                let size = owner.box_size(render_id)?;
                let root = owner.root_id()?;
                let transform = owner.transform_to(render_id, root)?;
                Some(transform.transform_rect(&Rect::from_ltwh(0.0, 0.0, size.width, size.height)))
            })
            .flatten()
    })
}

/// Wire `node`'s rect to `anchor`'s render node, for a widget that hosts its
/// node without a [`Focus`].
pub fn install_rect_provider(
    node: &Rc<FocusNode>,
    anchor: &SubtreeAnchor,
    ctx: &dyn LifecycleContext,
) -> (RectProvider, FocusNodeRegistration) {
    let provider = box_rect_provider(anchor, ctx);
    let registration = node.register_rect_provider(Rc::clone(&provider));
    (provider, registration)
}

/// The rect of a sliver anchor: the sliver's painted region, `paint_extent`
/// along its axis by its cross-axis extent, mapped to the render root. `None`
/// while unmounted or not laid out.
fn sliver_rect_provider(anchor: &SubtreeAnchor, ctx: &dyn LifecycleContext) -> RectProvider {
    let anchor = anchor.clone();
    let owner = ctx.pipeline_owner();
    Rc::new(move || {
        let render_id = anchor.get()?;
        let owner = owner.as_ref()?;
        owner
            .try_with_layout(|owner| {
                let (constraints, geometry) = owner.sliver_layout(render_id)?;
                let root = owner.root_id()?;
                let transform = owner.transform_to(render_id, root)?;
                let (width, height) = match constraints.axis() {
                    Axis::Vertical => (constraints.cross_axis_extent, geometry.paint_extent),
                    Axis::Horizontal => (geometry.paint_extent, constraints.cross_axis_extent),
                };
                Some(transform.transform_rect(&Rect::from_ltwh(0.0, 0.0, width, height)))
            })
            .flatten()
    })
}

// ============================================================================
// FocusScope
// ============================================================================

/// A [`Focus`] whose node is a [`FocusScopeNode`]: descendants attach under it
/// rather than the enclosing scope, Tab traversal cycles within it, and its
/// focused-child history remembers who to restore.
#[derive(Clone)]
pub struct FocusScope {
    child: BoxedView,
    /// An externally owned scope node, the way a
    /// route drives its own scope. `None` = widget-owned.
    external_scope: Option<Rc<FocusScopeNode>>,
    edge_behavior: Option<TraversalEdgeBehavior>,
}

impl FocusScope {
    /// A scope with a widget-owned node.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            external_scope: None,
            edge_behavior: None,
        }
    }

    /// Use `scope` instead of a widget-owned node â€” the caller keeps the
    /// handle (`FocusScope.withExternalFocusNode`, `:826-834`).
    pub fn with_external_node(scope: Rc<FocusScopeNode>, child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            external_scope: Some(scope),
            edge_behavior: None,
        }
    }

    /// Configure traversal at this scope's edge.
    #[must_use]
    pub fn edge_behavior(mut self, edge: TraversalEdgeBehavior) -> Self {
        self.edge_behavior = Some(edge);
        self
    }

    /// Returns the [`FocusScopeNode`] of the nearest enclosing [`Focus`] or
    /// [`FocusScope`], walked up to its scope: the unfiltered
    /// `nearest_focus_node` (not [`Focus::maybe_of`]'s scope-filtering
    /// wrapper), then its nearest scope
    /// (itself if it already is a scope, else the nearest scope ancestor â€”
    /// [`FocusNode::as_scope`]/[`FocusNode::enclosing_scope`], the exact pair
    /// `FocusState::try_autofocus` already uses to find "the enclosing
    /// scope" for autofocus purposes).
    ///
    /// The presentation's [`FocusTreeRoot`] guarantees that even a bare
    /// application subtree resolves to the root scope, so this lookup never
    /// reaches for the lifecycle-only focus-manager capability from `build`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Rc<FocusScopeNode> {
        nearest_focus_node(ctx)
            .and_then(|node| node.as_scope().or_else(|| node.enclosing_scope()))
            .expect(
                "BUG: FocusScope::of called outside FocusTreeRoot; every presentation root must \
                 install the focus-tree root",
            )
    }
}

impl std::fmt::Debug for FocusScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScope")
            .field("external", &self.external_scope.is_some())
            .finish_non_exhaustive()
    }
}

impl View for FocusScope {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for FocusScope {
    type State = FocusScopeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        let scope = match &self.external_scope {
            Some(scope) => Rc::clone(scope),
            None => widget_owned_scope(),
        };
        if let Some(edge) = self.edge_behavior {
            scope.set_traversal_edge_behavior(edge);
        }
        FocusScopeState {
            scope,
            owns_scope: self.external_scope.is_none(),
            focus_manager: None,
            attachment: None,
            parent: None,
            node_revision: Rc::new(Cell::new(0)),
            rebuild_handle: None,
            focus_subscription: None,
            action_chain: None,
            context_registration: None,
        }
    }
}

/// A scope node a [`FocusScope`] creates for itself: once the widget lets it
/// go, a late request on it answers
/// [`Stale`](flui_interaction::FocusRequestOutcome::Stale).
fn widget_owned_scope() -> Rc<FocusScopeNode> {
    let scope = FocusScopeNode::with_debug_label("FocusScope");
    flui_interaction::__runtime::mark_widget_owned(scope.as_focus_node());
    scope
}

/// The state behind [`FocusScope`]. `pub` because `StatefulView::State` requires
/// it; re-exported with the rest of the crate's widget states.
pub struct FocusScopeState {
    scope: Rc<FocusScopeNode>,
    /// Whether `scope` is the one this widget made for itself. A refused
    /// replacement leaves the hosted scope behind the view, so a rebuild
    /// compares the view with this, not with the previous view.
    owns_scope: bool,
    /// The presentation-local manager that owns this scope.
    focus_manager: Option<Rc<FocusManager>>,
    /// Generation-checked ownership of this widget's scope attachment.
    attachment: Option<FocusAttachment>,
    /// The node this scope's backing node hangs under.
    parent: Option<Rc<FocusNode>>,
    /// Inherited-provider revision advanced by backing-node notifications.
    node_revision: Rc<Cell<u64>>,
    rebuild_handle: Option<RebuildHandle>,
    focus_subscription: Option<Subscription>,
    /// The `Actions` chain visible at this scope, recorded on its backing
    /// node: the node itself can hold the primary focus (ADR-0079).
    action_chain: Option<FocusRecord>,
    /// Generation-checked ownership of that record.
    context_registration: Option<FocusNodeRegistration>,
}

impl std::fmt::Debug for FocusScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeState")
            .field("scope_id", &self.scope.as_focus_node().id())
            .finish_non_exhaustive()
    }
}

impl FocusScopeState {
    fn add_focus_listener(&self, node: &Rc<FocusNode>) -> Subscription {
        let revision = Rc::clone(&self.node_revision);
        let rebuild = self
            .rebuild_handle
            .as_ref()
            .expect("BUG: FocusScope listener installed before init_state")
            .clone();
        node.subscribe(move || {
            let next = revision
                .get()
                .checked_add(1)
                .expect("BUG: FocusScope inherited revision exhausted");
            revision.set(next);
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        })
    }
}

impl ViewState<FocusScope> for FocusScopeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.scope
            .set_text_direction(Directionality::maybe_of(ctx).unwrap_or_default());
        self.focus_manager = Some(ctx.focus_manager());
        self.rebuild_handle = Some(ctx.rebuild_handle());
        self.focus_subscription = Some(self.add_focus_listener(self.scope.as_focus_node()));
        let parent = enclosing_focus_parent(ctx);
        self.attachment = Some(
            parent
                .attach_node(self.scope.as_focus_node())
                .expect("BUG: FocusScope could not attach to the enclosing focus tree"),
        );
        self.parent = Some(parent);
        record_action_chain(
            ctx,
            self.scope.as_focus_node(),
            &mut self.action_chain,
            &mut self.context_registration,
        );
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.scope
            .set_text_direction(Directionality::maybe_of(ctx).unwrap_or_default());
        // An enclosing provider changed: move this scope â€” subtree, focus and
        // all â€” under the new parent (ADR-0026).
        let parent = enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &parent))
        {
            self.attachment
                .as_ref()
                .expect("BUG: a mounted FocusScope must retain its FocusAttachment")
                .reparent(&parent)
                .expect("BUG: FocusScope could not reparent within its presentation focus tree");
            self.parent = Some(parent);
        }
        record_action_chain(
            ctx,
            self.scope.as_focus_node(),
            &mut self.action_chain,
            &mut self.context_registration,
        );
    }

    fn did_update_view(&mut self, old_view: &FocusScope, new_view: &FocusScope) {
        // Against the hosted scope, not the previous view: a replacement
        // refused earlier left the scope behind the view it was asked for.
        let scope_changed = match new_view.external_scope.as_ref() {
            Some(new) => !Rc::ptr_eq(new, &self.scope),
            None => !self.owns_scope,
        };
        if !scope_changed {
            if let Some(edge) = new_view.edge_behavior {
                self.scope.set_traversal_edge_behavior(edge);
            } else if new_view.external_scope.is_none() && old_view.edge_behavior.is_some() {
                self.scope
                    .set_traversal_edge_behavior(TraversalEdgeBehavior::default());
            }
            return;
        }

        let replacement = new_view
            .external_scope
            .clone()
            .unwrap_or_else(widget_owned_scope);
        replacement.set_text_direction(self.scope.text_direction());
        if let Some(edge) = new_view.edge_behavior {
            replacement.set_traversal_edge_behavior(edge);
        }
        let replacement_subscription = self.add_focus_listener(replacement.as_focus_node());
        let replacement_context_registration = self
            .action_chain
            .as_ref()
            .map(|record| record.register_on(replacement.as_focus_node()));
        let attachment = self
            .attachment
            .take()
            .expect("BUG: a mounted FocusScope must retain its FocusAttachment");
        match attachment.replace_node(replacement.as_focus_node()) {
            Ok(replacement_attachment) => self.attachment = Some(replacement_attachment),
            Err(error) => {
                // Refused before anything changed â€” a focus listener is
                // running. Keep the current scope; the next update tries the
                // replacement again.
                tracing::warn!(
                    %error,
                    "FocusScope kept its scope: the replacement was refused during a focus \
                     notification"
                );
                self.attachment = Some(attachment);
                drop((replacement_subscription, replacement_context_registration));
                return;
            }
        }
        let previous_subscription = self.focus_subscription.replace(replacement_subscription);
        // The replaced scope's record goes with it; the replacement carries
        // the same chain.
        self.context_registration = replacement_context_registration;
        self.scope = replacement;
        self.owns_scope = new_view.external_scope.is_none();
        drop(previous_subscription);
    }

    fn dispose(&mut self) {
        drop(self.focus_subscription.take());
        // Only the current attachment may clear the record on an external
        // scope node a newer host may already have adopted.
        let owns_attachment = self
            .attachment
            .as_ref()
            .is_some_and(FocusAttachment::is_attached);
        if let Some(registration) = self.context_registration.take()
            && !owns_attachment
        {
            registration.relinquish();
        }
        if let Some(attachment) = self.attachment.take() {
            let _ = attachment.detach();
        }
        self.parent = None;
        self.rebuild_handle = None;
        self.focus_manager = None;
    }

    fn build(&self, view: &FocusScope, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }

    fn provide(&self, _view: &FocusScope, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        values.value(FocusParent {
            parent: Rc::clone(self.scope.as_focus_node()),
            revision: self.node_revision.get(),
        });
    }
}

// ============================================================================
// ExcludeFocus
// ============================================================================

/// Prevents its subtree from receiving focus while exclusion is active.
///
/// Exclusion is active by default. Activating it unfocuses an already-focused
/// descendant, which is not automatically restored when exclusion is disabled.
/// Primary focus is cleared to `None`; it is not
/// yet moved to the enclosing scope's previously focused child.
/// Descendants' own request-focus flags are not rewritten.
#[derive(Clone, StatelessView)]
pub struct ExcludeFocus {
    excluding: bool,
    child: BoxedView,
}

impl ExcludeFocus {
    /// Creates an excluding focus boundary around `child`.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            excluding: true,
            child: child.into_view().boxed(),
        }
    }

    /// Whether focus is excluded from the subtree (default `true`).
    ///
    /// See [`ExcludeFocus`] for eviction and focus-destination semantics.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }
}

impl std::fmt::Debug for ExcludeFocus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExcludeFocus")
            .field("excluding", &self.excluding)
            .finish_non_exhaustive()
    }
}

impl StatelessView for ExcludeFocus {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Focus::new(self.child.clone())
            .can_request_focus(false)
            .skip_traversal(true)
            .descendants_are_focusable(!self.excluding)
    }
}
