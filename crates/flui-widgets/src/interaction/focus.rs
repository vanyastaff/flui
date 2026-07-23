//! [`Focus`] and [`FocusScope`] — the widgets that put `flui-interaction`'s
//! focus tree into the element tree.
//!
//! ADR-0022. The node/manager layer (`FocusManager`, `FocusNode`,
//! `FocusScopeNode` — tracker H4) predates these widgets; what they add is the
//! lifecycle wiring: a widget-owned node attached under the nearest enclosing
//! scope on mount, moved with [`FocusScopeNode::adopt_node`] when that scope
//! changes, and detached on dispose.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/focus_scope.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `Focus` (`:126-153`), `_FocusState`
//! (`:554-742`), `FocusScope` (`:804-834`, incl. `withExternalFocusNode`).
//!
//! # Divergences, each named (ADR-0022)
//!
//! * **Nodes parent to the nearest focus *node*** — scope or plain `Focus` —
//!   through one provider, Flutter's `_FocusInheritedScope` shape. (An earlier
//!   design flattened to the nearest scope; key bubbling made the node tree's
//!   shape observable and superseded that decision — ADR-0022, ADR-0023.)
//! * **Reparenting happens in `did_change_dependencies`**, not on every build
//!   as Flutter's `_focusAttachment.reparent()` does — the provider notifying
//!   is the only way the enclosing scope changes without a remount. Observable
//!   only through `parentNode`, which is not ported.
//! * **Focus changes apply synchronously.** Flutter batches into
//!   `applyFocusChangesIfNeeded` at end of frame; FLUI's `FocusManager` is
//!   synchronous throughout, so `autofocus` runs inline from `init_state`.
//! * Not ported: `onKey` (legacy), `includeSemantics` (needs the semantics
//!   layer), `parentNode`, `descendantsAreTraversable` (no node-layer flag).
//!   `Focus.of`/`maybeOf`/`FocusScope.of` ARE ported (ADR-0036's
//!   `OverlayScope` precedent, applied here — see [`Focus::of`]) — only their
//!   `scopeOk: true` variant of `Focus.of`/`maybeOf` is not: nothing in this
//!   crate needs a Focus-flavored lookup that also accepts a scope node,
//!   since [`FocusScope::of`] already covers "give me the nearest scope".

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::navigator::AnchoredBox;
use flui_foundation::ListenerId;
use flui_geometry::Rect;
use flui_interaction::routing::{
    FocusAttachment, FocusManager, FocusNode, FocusNodeRegistration, FocusScopeNode,
    KeyEventHandler, RectProvider,
};
use flui_objects::SubtreeAnchor;
use flui_types::geometry::px;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};

use super::shortcuts::DefaultFocusTraversal;

/// Reports whether this widget's node gained or lost the primary focus.
pub type FocusChangeHandler = Rc<dyn Fn(bool)>;

// ============================================================================
// The ambient scope
// ============================================================================

/// Provides the nearest enclosing focus **node** — scope or plain `Focus` —
/// to descendants: Flutter's `_FocusInheritedScope`, which every `Focus`
/// widget provides (`focus_scope.dart:946`). Private: [`Focus`] and
/// [`FocusScope`] are the public surface.
///
/// The parent being a *node*, not always a scope, is what makes the
/// leaf→root key-dispatch walk (ADR-0023) match the widget tree: a
/// `Shortcuts`-style non-scope `Focus` above a field is a node **ancestor**
/// of the field, so keys the field ignores bubble through it.
#[derive(Clone)]
struct FocusParentProvider {
    parent: Rc<FocusNode>,
    revision: u64,
    child: BoxedView,
}

impl std::fmt::Debug for FocusParentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusParentProvider")
            .field("parent_id", &self.parent.id())
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl InheritedView for FocusParentProvider {
    type Data = Rc<FocusNode>;

    fn data(&self) -> &Self::Data {
        &self.parent
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.parent, &old.parent) || self.revision != old.revision
    }
}

impl_inherited_view!(FocusParentProvider);

/// The node a mounting/reparenting focus node hangs under.
///
/// Every presentation is rooted in [`FocusRoot`], so a missing provider is a
/// broken embedder invariant rather than a reason to reach for the
/// lifecycle-only `BuildContext::focus_manager` capability from arbitrary
/// build paths.
pub(crate) fn enclosing_focus_parent(ctx: &dyn BuildContext) -> Rc<FocusNode> {
    ctx.depend_on::<FocusParentProvider, _>(|provider| Rc::clone(&provider.parent))
        .expect(
            "BUG: focus widget mounted outside FocusRoot; every presentation root must install \
             the focus-tree provider",
        )
}

/// Raw lookup behind [`Focus::of`]/[`Focus::maybe_of`]/[`FocusScope::of`]:
/// the nearest enclosing [`FocusParentProvider`]'s node, registering a
/// dependency, with **no** scope-node filtering. Flutter's own
/// `Focus.maybeOf(context, scopeOk: true)` (`focus_scope.dart:452`, tag
/// `3.44.0`) — the call [`FocusScope::of`] makes internally; [`Focus::maybe_of`]
/// layers the `scopeOk: false` filter back on top.
///
/// # Depend, not get
///
/// Unlike `Overlay::maybe_of` (`overlay/mod.rs`), routing through
/// [`BuildContextExt::depend_on`] here is not a FLUI-native divergence from
/// the oracle — it is the loyal port. Flutter's `Focus.maybeOf` calls
/// `context.dependOnInheritedWidgetOfExactType` under its **default**
/// `createDependency: true` (not an override, the way `Overlay.maybeOf`
/// explicitly passes `createDependency: false`), so registering a dependency
/// is what the oracle itself does by default.
///
/// This resolves the SAME [`FocusParentProvider`] marker `Focus`/`FocusScope`'s
/// own `build` already mounts around their child for
/// [`enclosing_focus_parent`]'s mount-time attach/reparent lookups —
/// Flutter's `_FocusInheritedScope`. No second, redundant marker is mounted
/// just for this public entry point.
///
/// Like Flutter's `InheritedNotifier<FocusNode>`, each provider carries a
/// revision advanced by its node listener. `update_should_notify` therefore
/// invalidates dependents on node-state changes as well as identity changes;
/// a build that reads `Focus::of(ctx).has_focus()` stays live.
fn nearest_focus_node(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
    ctx.depend_on::<FocusParentProvider, _>(|provider| Rc::clone(&provider.parent))
}

// ============================================================================
// Presentation root
// ============================================================================

/// Establishes the focus tree and standard keyboard traversal for one
/// presentation.
///
/// Embedders install exactly one `FocusRoot` around each element-tree root.
/// It publishes that build owner's root scope as the first
/// [`FocusParentProvider`] and installs Tab/Shift+Tab traversal against the
/// same owner-local [`FocusManager`]. Descendant widgets therefore never need
/// a process-global manager or a build-phase capability fallback.
#[derive(Clone, Debug, StatefulView)]
pub struct FocusRoot {
    child: BoxedView,
}

impl FocusRoot {
    /// Create the focus root for a presentation subtree.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

/// Presentation-local state behind [`FocusRoot`].
pub struct FocusRootState {
    manager: Option<Rc<FocusManager>>,
}

impl std::fmt::Debug for FocusRootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusRootState")
            .field("initialized", &self.manager.is_some())
            .finish()
    }
}

impl StatefulView for FocusRoot {
    type State = FocusRootState;

    fn create_state(&self) -> Self::State {
        FocusRootState { manager: None }
    }
}

impl ViewState<FocusRoot> for FocusRootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn build(&self, view: &FocusRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let manager = self
            .manager
            .as_ref()
            .expect("BUG: FocusRoot built before init_state");
        FocusParentProvider {
            parent: Rc::clone(manager.root_scope().as_focus_node()),
            revision: 0,
            child: DefaultFocusTraversal::new(view.child.clone())
                .into_view()
                .boxed(),
        }
    }
}

// ============================================================================
// Focus
// ============================================================================

/// Which side is authoritative for the configuration stored on a focus node.
///
/// An external node is always *hosted* by [`Focus`] — attached, reparented,
/// and detached with the widget — but it can follow either configuration
/// policy. The regular [`Focus::focus_node`] path lets the widget manage
/// explicitly supplied attributes. [`Focus::with_external_node`] leaves every
/// node attribute under the caller's control.
#[derive(Clone)]
enum FocusNodeOwnership {
    Internal,
    ManagedExternal(Rc<FocusNode>),
    ExternalSource(Rc<FocusNode>),
}

/// Makes its subtree focusable: owns a [`FocusNode`] (or adopts an external
/// one), attaches it under the nearest enclosing [`FocusScope`] on mount, and
/// detaches it on dispose. Flutter's `Focus` (`focus_scope.dart:126`).
#[derive(Clone)]
// Ported property names (`autofocus`, `can_request_focus`) end with the widget's
// own name; keeping Flutter's names beats a lint-driven rename.
#[allow(clippy::struct_field_names)]
pub struct Focus {
    child: BoxedView,
    node_ownership: FocusNodeOwnership,
    autofocus: bool,
    can_request_focus: Option<bool>,
    skip_traversal: Option<bool>,
    descendants_are_focusable: Option<bool>,
    on_focus_change: Option<FocusChangeHandler>,
    on_key_event: Option<KeyEventHandler>,
    debug_label: Option<&'static str>,
}

impl Focus {
    /// A focusable subtree with a widget-owned node.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            node_ownership: FocusNodeOwnership::Internal,
            autofocus: false,
            can_request_focus: None,
            skip_traversal: None,
            descendants_are_focusable: None,
            on_focus_change: None,
            on_key_event: None,
            debug_label: None,
        }
    }

    /// Host `node` and let this widget manage the attributes explicitly set
    /// through the builder methods — Flutter's regular `Focus.focusNode`
    /// path (`focus_scope.dart:159`).
    ///
    /// Omitted attributes retain their current value on an external node.
    /// The caller keeps ownership of the node itself; this widget only
    /// attaches, reparents, and detaches it with its lifecycle. Use
    /// [`with_external_node`](Self::with_external_node) when the node must be
    /// the source of truth for all of its attributes.
    #[must_use]
    pub fn focus_node(mut self, node: Rc<FocusNode>) -> Self {
        self.node_ownership = FocusNodeOwnership::ManagedExternal(node);
        self
    }

    /// Host an external node without ever overwriting its focusability,
    /// traversal, or key-handler attributes.
    ///
    /// This is Flutter's `Focus.withExternalFocusNode`: the caller-owned node
    /// is the source of truth, while the widget still owns the presentation
    /// attachment lifecycle. Configuration builder methods remain useful
    /// when constructing both modes generically, but their node-attribute
    /// values are intentionally ignored in this mode.
    #[must_use]
    pub fn with_external_node(node: Rc<FocusNode>, child: impl IntoView) -> Self {
        Self {
            node_ownership: FocusNodeOwnership::ExternalSource(node),
            ..Self::new(child)
        }
    }

    /// Request focus on mount if the enclosing scope has no focused child —
    /// Flutter's `autofocus` (`:190-205`); at most one child of a scope should
    /// set it.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Whether the node may receive focus at all (`:284-296`).
    #[must_use]
    pub fn can_request_focus(mut self, can: bool) -> Self {
        self.can_request_focus = Some(can);
        self
    }

    /// Whether Tab traversal skips this node while it stays focusable by
    /// request (`:270-282`).
    #[must_use]
    pub fn skip_traversal(mut self, skip: bool) -> Self {
        self.skip_traversal = Some(skip);
        self
    }

    /// Whether descendants of this node may receive focus (`:298-318`).
    #[must_use]
    pub fn descendants_are_focusable(mut self, focusable: bool) -> Self {
        self.descendants_are_focusable = Some(focusable);
        self
    }

    /// Called with `true`/`false` as this widget's node gains/loses the
    /// primary focus — Flutter's `onFocusChange` (`:167`).
    #[must_use]
    pub fn on_focus_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(bool) + 'static,
    {
        self.on_focus_change = Some(Rc::new(handler));
        self
    }

    /// Key handler invoked during the leaf→root dispatch walk while this
    /// node — or a descendant — holds the primary focus: Flutter's
    /// `onKeyEvent` (`:170-180`). Return
    /// [`Handled`](flui_interaction::KeyEventResult::Handled) to consume the
    /// event, [`Ignored`](flui_interaction::KeyEventResult::Ignored) to let it
    /// bubble to the enclosing `Focus`, or
    /// [`SkipRemainingHandlers`](flui_interaction::KeyEventResult::SkipRemainingHandlers)
    /// to stop the bubbling without consuming (ADR-0023).
    #[must_use]
    pub fn on_key_event(mut self, handler: KeyEventHandler) -> Self {
        self.on_key_event = Some(handler);
        self
    }

    /// A label for debug output (`:334`).
    #[must_use]
    pub fn debug_label(mut self, label: &'static str) -> Self {
        self.debug_label = Some(label);
        self
    }

    /// Returns the [`FocusNode`] of the [`Focus`] that most tightly encloses
    /// `ctx` — Flutter's `Focus.of` (`focus_scope.dart:398`, tag `3.44.0`),
    /// `scopeOk: false` only (see the module divergence notes for the
    /// unported `scopeOk: true` variant; [`FocusScope::of`] covers that case).
    ///
    /// # Panics
    ///
    /// Panics with a message naming the missing ancestor if no enclosing
    /// [`Focus`] provides one (Flutter's own assert-time `FlutterError`,
    /// `focus_scope.dart:398-424`). Use [`maybe_of`](Self::maybe_of) for a
    /// non-panicking lookup.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Rc<FocusNode> {
        Self::maybe_of(ctx).expect(
            "Focus::of() was called with a context that does not contain a Focus widget. \
             No Focus widget ancestor could be found starting from the context passed to \
             Focus::of() — wrap the subtree in a Focus, or use Focus::maybe_of with a \
             caller-chosen fallback.",
        )
    }

    /// Returns the [`FocusNode`] of the [`Focus`] that most tightly encloses
    /// `ctx`, registering a dependency — Flutter's `Focus.maybeOf`
    /// (`focus_scope.dart:452`, tag `3.44.0`) with its default
    /// `createDependency: true` (see `nearest_focus_node`'s doc for why
    /// that default, not `Overlay::maybe_of`'s override, is what this
    /// mirrors).
    ///
    /// `None` if the nearest enclosing node is a [`FocusScope`]'s own scope
    /// node rather than a plain [`Focus`] (`scopeOk: false` — a scope only
    /// satisfies [`FocusScope::of`], not this), or if there is no enclosing
    /// [`Focus`]/[`FocusScope`] at all.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
        nearest_focus_node(ctx).filter(|node| !node.is_scope())
    }

    /// The node this widget will drive: the external one, or a fresh one.
    fn make_node(&self) -> Rc<FocusNode> {
        match &self.node_ownership {
            FocusNodeOwnership::Internal => {
                FocusNode::with_debug_label(self.debug_label.unwrap_or("Focus"))
            }
            FocusNodeOwnership::ManagedExternal(node)
            | FocusNodeOwnership::ExternalSource(node) => Rc::clone(node),
        }
    }

    /// Apply this view's node-attribute policy.
    ///
    /// Internal nodes receive a complete configuration, including defaults.
    /// A regular external node receives only explicit overrides, preserving
    /// caller state for omitted properties. A source-of-truth external node
    /// is never mutated. The generation-checked registration carries
    /// ownership across a managed-external rebuild that drops an override:
    /// the handler remains installed by design, but cleanup cannot erase a
    /// value written later by the external owner.
    fn configure(
        &self,
        node: &Rc<FocusNode>,
        key_handler_registration: &mut Option<FocusNodeRegistration>,
    ) {
        match &self.node_ownership {
            FocusNodeOwnership::Internal => {
                node.set_can_request_focus(self.can_request_focus.unwrap_or(true));
                node.set_skip_traversal(self.skip_traversal.unwrap_or(false));
                node.set_descendants_are_focusable(self.descendants_are_focusable.unwrap_or(true));
                if let Some(handler) = &self.on_key_event {
                    *key_handler_registration =
                        Some(node.register_on_key_event(Rc::clone(handler)));
                } else {
                    key_handler_registration.take();
                    node.clear_on_key_event();
                }
            }
            FocusNodeOwnership::ManagedExternal(_) => {
                if let Some(can_request_focus) = self.can_request_focus {
                    node.set_can_request_focus(can_request_focus);
                }
                if let Some(skip_traversal) = self.skip_traversal {
                    node.set_skip_traversal(skip_traversal);
                }
                if let Some(descendants_are_focusable) = self.descendants_are_focusable {
                    node.set_descendants_are_focusable(descendants_are_focusable);
                }
                if let Some(handler) = &self.on_key_event {
                    *key_handler_registration =
                        Some(node.register_on_key_event(Rc::clone(handler)));
                }
            }
            FocusNodeOwnership::ExternalSource(_) => {
                if let Some(registration) = key_handler_registration.take() {
                    registration.relinquish();
                }
            }
        }
    }
}

impl std::fmt::Debug for Focus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Focus")
            .field("autofocus", &self.autofocus)
            .field("debug_label", &self.debug_label)
            .finish_non_exhaustive()
    }
}

impl View for Focus {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Focus {
    type State = FocusState;

    fn create_state(&self) -> Self::State {
        // The node is configured here — `init_state` has no view reference,
        // and `did_update_view` re-syncs later configurations.
        let node = self.make_node();
        let mut key_handler_registration = None;
        self.configure(&node, &mut key_handler_registration);
        FocusState {
            observed_node: Rc::new(RefCell::new(Rc::clone(&node))),
            observed_was_focused: Rc::new(Cell::new(false)),
            node_revision: Rc::new(Cell::new(0)),
            node,
            key_handler_registration,
            focus_manager: None,
            attachment: None,
            parent: None,
            anchor: SubtreeAnchor::new(),
            rect_provider: None,
            rect_provider_registration: None,
            rebuild_handle: None,
            focus_listener_id: None,
            autofocus: self.autofocus,
            did_autofocus: false,
            on_focus_change: Rc::new(RefCell::new(self.on_focus_change.clone())),
        }
    }
}

/// `_FocusState` (`focus_scope.dart:554`). `pub` because `StatefulView::State`
/// requires it, and re-exported like every other widget's state in this crate
/// (`GestureDetectorState`, `AnimatedAlignState`, …) so a caller can name it.
pub struct FocusState {
    node: Rc<FocusNode>,
    /// Generation-checked ownership of the key handler this host installed.
    ///
    /// External source-of-truth nodes keep caller-owned handlers intact.
    /// Managed external nodes may retain an explicitly installed handler when
    /// a later view omits the override, so ownership is stateful rather than
    /// inferred from only the latest view.
    key_handler_registration: Option<FocusNodeRegistration>,
    /// Shared identity read by the node listener across a live external
    /// node replacement.
    observed_node: Rc<RefCell<Rc<FocusNode>>>,
    /// Last `has_focus` value for the currently observed node.
    observed_was_focused: Rc<Cell<bool>>,
    /// Inherited-provider revision advanced by node notifications.
    node_revision: Rc<Cell<u64>>,
    /// The exact presentation-local manager acquired from the mounting build
    /// owner. Listener removal must go back to this same instance.
    focus_manager: Option<Rc<FocusManager>>,
    /// Generation-checked ownership of this widget's attachment. Holding the
    /// token prevents stale lifecycle callbacks from detaching a newer host.
    attachment: Option<FocusAttachment>,
    /// The node this one currently hangs under; `did_change_dependencies`
    /// moves it when the provider changes.
    parent: Option<Rc<FocusNode>>,
    /// Publishes the child's `RenderId` while mounted, so the node's
    /// [`RectProvider`](flui_interaction::RectProvider) can measure it —
    /// reading-order traversal sorts by this geometry (ADR-0022).
    anchor: SubtreeAnchor,
    /// The live geometry source, retained so a replacement node receives the
    /// same mounted anchor without reacquiring build-only context.
    rect_provider: Option<RectProvider>,
    /// Generation-checked ownership of the geometry source installed on the
    /// current node.
    rect_provider_registration: Option<FocusNodeRegistration>,
    /// Lifecycle-acquired rebuild capability used by the node subscription.
    rebuild_handle: Option<RebuildHandle>,
    /// Listener installed on the current node. It drives both inherited
    /// dependents and the optional focus-edge callback.
    focus_listener_id: Option<ListenerId>,
    /// Captured at `create_state`: `init_state` has no view reference.
    autofocus: bool,
    /// One-shot latch: whether this widget has already attempted its
    /// autofocus request — Flutter's `_didAutofocus` (`focus_scope.dart`).
    /// Set the moment the attempt is made, win or lose (an already-focused
    /// sibling can still make the attempt lose), so a later rebuild that
    /// merely re-asserts the same `autofocus` value does not re-request.
    did_autofocus: bool,
    /// The current `on_focus_change` handler, behind a shared cell so the installed
    /// listener reads the *latest* one. `did_update_view` writes here rather than
    /// reinstalling the listener — a captured-by-value closure would keep firing the
    /// handler from the build that mounted it.
    on_focus_change: Rc<RefCell<Option<FocusChangeHandler>>>,
}

impl std::fmt::Debug for FocusState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusState")
            .field("node_id", &self.node.id())
            .finish_non_exhaustive()
    }
}

impl FocusState {
    fn manager(&self) -> &Rc<FocusManager> {
        self.focus_manager
            .as_ref()
            .expect("BUG: Focus lifecycle used before init_state installed its focus manager")
    }

    /// The rebuild-on-focus-change listener — Flutter's `_handleFocusChanged`
    /// `setState` (`:684-712`): descendants that read the node's state during
    /// build stay current, and `on_focus_change` fires on the edges.
    fn add_focus_listener(&self, node: &Rc<FocusNode>) -> ListenerId {
        let rebuild = self
            .rebuild_handle
            .as_ref()
            .expect("BUG: Focus listener installed before init_state captured its rebuild handle")
            .clone();
        let observed_node = Rc::clone(&self.observed_node);
        let was_focused_for_listener = Rc::clone(&self.observed_was_focused);
        let node_revision = Rc::clone(&self.node_revision);
        let on_focus_change = Rc::clone(&self.on_focus_change);
        node.add_listener(Rc::new(move || {
            let next_revision = node_revision
                .get()
                .checked_add(1)
                .expect("BUG: Focus inherited revision exhausted");
            node_revision.set(next_revision);
            rebuild.schedule(flui_view::RebuildReason::StateChange);

            let node = observed_node.borrow();
            let now_focused = node.has_focus();
            drop(node);
            if was_focused_for_listener.replace(now_focused) != now_focused {
                // FocusNode clones its listener list before dispatch, so
                // user code may synchronously move focus. Clone the
                // callback out of the RefCell before invoking it for the
                // same re-entrancy reason.
                let handler = on_focus_change.borrow().clone();
                if let Some(handler) = handler {
                    handler(now_focused);
                }
            }
        }))
    }

    fn install_focus_listener(&mut self) {
        if self.focus_listener_id.is_some() {
            return;
        }
        self.observed_was_focused.set(self.node.has_focus());
        self.focus_listener_id = Some(self.add_focus_listener(&self.node));
    }

    fn remove_focus_listener(&mut self) {
        if let Some(id) = self.focus_listener_id.take() {
            self.node.remove_listener(id);
        }
    }

    /// `_handleAutofocus` (`focus_scope.dart`, tag `3.44.0`): a one-shot
    /// attempt, made the first time `autofocus` is (or becomes) `true` and
    /// never repeated —
    /// [`FocusState::did_autofocus`] latches regardless of whether the
    /// attempt actually won the focus (an already-focused sibling can still
    /// make it lose). Only when the enclosing scope has nothing focused yet
    /// does the request land; either way the attempt is marked made.
    /// Synchronous — FLUI has no end-of-frame focus batch (module docs).
    ///
    /// Derives the nearest scope from [`FocusState::parent`] (kept current by
    /// `init_state`/`did_change_dependencies`) rather than an ambient
    /// `BuildContext` lookup, so it can run from
    /// [`ViewState::did_update_view`] too — that hook gets no `BuildContext`.
    fn try_autofocus(&mut self) {
        if self.did_autofocus || !self.autofocus {
            return;
        }
        self.did_autofocus = true;
        let scope = self
            .parent
            .as_ref()
            .and_then(|parent| parent.as_scope().or_else(|| parent.enclosing_scope()))
            .unwrap_or_else(|| Rc::clone(self.manager().root_scope()));
        if scope.focused_child().is_none() {
            self.node.request_focus();
        }
    }
}

impl ViewState<Focus> for FocusState {
    /// Listen, attach, autofocus — in that order, so a focus request queued on
    /// an external node before mount is observed when attach fulfills it
    /// (`_FocusState.initState` + `didChangeDependencies`,
    /// `focus_scope.dart:565-630`).
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.focus_manager = Some(ctx.focus_manager());
        self.rebuild_handle = Some(ctx.rebuild_handle());
        let parent = enclosing_focus_parent(ctx);
        self.install_focus_listener();
        let (rect_provider, rect_provider_registration) =
            install_rect_provider(&self.node, &self.anchor, ctx);
        self.rect_provider = Some(rect_provider);
        self.rect_provider_registration = Some(rect_provider_registration);
        self.attachment = Some(
            parent
                .attach_node(&self.node)
                .expect("BUG: Focus could not attach its node to the enclosing focus tree"),
        );
        self.parent = Some(parent);

        self.try_autofocus();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // The provider changed: move the node — with focus — under the new
        // parent. `_focusAttachment.reparent()` in `didChangeDependencies`
        // (`focus_scope.dart:618-623`), via ADR-0022's adopt.
        let parent = enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &parent))
        {
            self.attachment
                .as_ref()
                .expect("BUG: a mounted Focus must retain its FocusAttachment")
                .reparent(&parent)
                .expect("BUG: Focus could not reparent within its presentation focus tree");
            self.parent = Some(parent);
        }
    }

    fn did_update_view(&mut self, old_view: &Focus, new_view: &Focus) {
        // Install the current callback before any node replacement can emit a
        // focus edge. The node subscription always reads this cell.
        self.on_focus_change
            .borrow_mut()
            .clone_from(&new_view.on_focus_change);

        let node_changed = match &new_view.node_ownership {
            FocusNodeOwnership::Internal => {
                !matches!(&old_view.node_ownership, FocusNodeOwnership::Internal)
            }
            FocusNodeOwnership::ManagedExternal(node)
            | FocusNodeOwnership::ExternalSource(node) => !Rc::ptr_eq(node, &self.node),
        };

        if node_changed {
            let replacement = new_view.make_node();
            let mut replacement_key_handler_registration = None;
            new_view.configure(&replacement, &mut replacement_key_handler_registration);
            let replacement_rect_provider_registration = self
                .rect_provider
                .as_ref()
                .map(|provider| replacement.register_rect_provider(Rc::clone(provider)));
            let replacement_focus_listener_id = self.add_focus_listener(&replacement);

            // Observe the replacement before the core transaction delivers
            // its stable-tree notification. Keep the previous `has_focus`
            // bit: when a focused descendant moves with the subtree, both old
            // and replacement nodes have focus and no synthetic edge fires.
            *self.observed_node.borrow_mut() = Rc::clone(&replacement);
            let attachment = self
                .attachment
                .take()
                .expect("BUG: a mounted Focus must retain its FocusAttachment");
            let replacement_attachment = attachment
                .replace_node(&replacement)
                .expect("BUG: Focus could not atomically replace its attached node");

            // The completed transaction made the old token stale and retired
            // the old node, so ancillary state can now be removed without
            // touching a newer host.
            self.key_handler_registration.take();
            self.rect_provider_registration.take();
            if let Some(listener_id) = self
                .focus_listener_id
                .replace(replacement_focus_listener_id)
            {
                self.node.remove_listener(listener_id);
            }
            self.node = replacement;
            self.key_handler_registration = replacement_key_handler_registration;
            self.rect_provider_registration = replacement_rect_provider_registration;
            self.attachment = Some(replacement_attachment);
        } else {
            // Re-sync flags and handlers from the latest configuration
            // (`didUpdateWidget`, `:646-682`).
            new_view.configure(&self.node, &mut self.key_handler_registration);
        }

        self.autofocus = new_view.autofocus;
        // `didUpdateWidget`'s `oldWidget.autofocus != widget.autofocus` guard
        // (`focus_scope.dart`, tag `3.44.0`) is folded into `try_autofocus`'s
        // own `did_autofocus` latch: a rebuild that flips `autofocus` from
        // `false` to `true` makes the one still-unattempted autofocus
        // request; one that merely repeats an already-`true` value is a
        // no-op either way.
        self.try_autofocus();
    }

    fn dispose(&mut self) {
        // An external node outlives this widget: it must not keep measuring a
        // dead anchor or a widget-owned key handler. Only the current
        // generation owns those registrations: a stale host must not erase
        // state installed by a newer host of the same external node.
        let owns_attachment = self
            .attachment
            .as_ref()
            .is_some_and(FocusAttachment::is_attached);
        if owns_attachment {
            self.key_handler_registration.take();
            self.rect_provider_registration.take();
        } else if let Some(registration) = self.key_handler_registration.take() {
            // A superseded attachment has no authority to mutate this node.
            registration.relinquish();
        }
        if !owns_attachment && let Some(registration) = self.rect_provider_registration.take() {
            registration.relinquish();
        }
        self.remove_focus_listener();
        // The generation-checked attachment is the sole detach authority:
        // an old widget host cannot detach a node that a newer host adopted.
        if let Some(attachment) = self.attachment.take() {
            let _ = attachment.detach();
        }
        self.parent = None;
        self.rebuild_handle = None;
        self.focus_manager = None;
    }

    /// Every `Focus` provides itself as the parent for descendants —
    /// Flutter's `_FocusInheritedScope` in `_FocusState.build`
    /// (`focus_scope.dart:714-741`) — and anchors the child so the node's
    /// rect provider has a render node to measure.
    fn build(&self, view: &Focus, _ctx: &dyn BuildContext) -> impl IntoView {
        FocusParentProvider {
            parent: Rc::clone(&self.node),
            revision: self.node_revision.get(),
            child: BoxedView(Box::new(
                AnchoredBox::new(self.anchor.clone(), view.child.clone()).into_view(),
            )),
        }
    }
}

/// Wire `node`'s rect to `anchor`'s render node: measured lazily at traversal
/// time against committed layout — `box_size` + `transform_to` the render
/// root, the `HeroHandle::bounding_box_in` shape. `None` (fall back to the
/// stored rect) while unmounted or before first layout.
pub(crate) fn install_rect_provider(
    node: &Rc<FocusNode>,
    anchor: &SubtreeAnchor,
    ctx: &dyn BuildContext,
) -> (RectProvider, FocusNodeRegistration) {
    let anchor = anchor.clone();
    let owner = ctx.pipeline_owner();
    let provider: flui_interaction::RectProvider = Rc::new(move || {
        let render_id = anchor.get()?;
        let owner = owner.as_ref()?.read();
        let size = owner.box_size(render_id)?;
        let root = owner.root_id()?;
        let transform = owner.transform_to(render_id, root)?;
        Some(transform.transform_rect(&Rect::from_ltwh(px(0.0), px(0.0), size.width, size.height)))
    });
    let registration = node.register_rect_provider(Rc::clone(&provider));
    (provider, registration)
}

// ============================================================================
// FocusScope
// ============================================================================

/// A [`Focus`] whose node is a [`FocusScopeNode`]: descendants attach under it
/// rather than the enclosing scope, Tab traversal cycles within it, and its
/// focused-child history remembers who to restore. Flutter's `FocusScope`
/// (`focus_scope.dart:804-834`).
#[derive(Clone)]
pub struct FocusScope {
    child: BoxedView,
    /// An externally owned scope node — Flutter's
    /// `FocusScope.withExternalFocusNode` (`:826-834`), the constructor a
    /// route uses so *it* can drive the scope. `None` = widget-owned.
    external_scope: Option<Rc<FocusScopeNode>>,
}

impl FocusScope {
    /// A scope with a widget-owned node.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            external_scope: None,
        }
    }

    /// Use `scope` instead of a widget-owned node — the caller keeps the
    /// handle (`FocusScope.withExternalFocusNode`, `:826-834`).
    pub fn with_external_node(scope: Rc<FocusScopeNode>, child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            external_scope: Some(scope),
        }
    }

    /// Returns the [`FocusScopeNode`] of the nearest enclosing [`Focus`] or
    /// [`FocusScope`], walked up to its scope — Flutter's `FocusScope.of`
    /// (`focus_scope.dart:834`, tag `3.44.0`): `Focus.maybeOf(context,
    /// scopeOk: true)` (unfiltered — `nearest_focus_node` directly, not
    /// [`Focus::maybe_of`]'s scope-filtering wrapper), then `.nearestScope`
    /// (itself if it already is a scope, else the nearest scope ancestor —
    /// [`FocusNode::as_scope`]/[`FocusNode::enclosing_scope`], the exact pair
    /// `FocusState::try_autofocus` already uses to find "the enclosing
    /// scope" for autofocus purposes).
    ///
    /// [`FocusRoot`] guarantees that even a bare application subtree has the
    /// standard traversal `Focus` under the build owner's root scope, so this
    /// lookup never reaches for the lifecycle-only focus-manager capability
    /// from `build`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Rc<FocusScopeNode> {
        nearest_focus_node(ctx)
            .and_then(|node| node.as_scope().or_else(|| node.enclosing_scope()))
            .expect(
                "BUG: FocusScope::of called outside FocusRoot; every presentation root must \
                 install the focus-tree provider",
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
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for FocusScope {
    type State = FocusScopeState;

    fn create_state(&self) -> Self::State {
        FocusScopeState {
            scope: match &self.external_scope {
                Some(scope) => Rc::clone(scope),
                None => FocusScopeNode::with_debug_label("FocusScope"),
            },
            focus_manager: None,
            attachment: None,
            parent: None,
            node_revision: Rc::new(Cell::new(0)),
            rebuild_handle: None,
            focus_listener_id: None,
        }
    }
}

/// The state behind [`FocusScope`]. `pub` because `StatefulView::State` requires
/// it; re-exported with the rest of the crate's widget states.
pub struct FocusScopeState {
    scope: Rc<FocusScopeNode>,
    /// The presentation-local manager that owns this scope.
    focus_manager: Option<Rc<FocusManager>>,
    /// Generation-checked ownership of this widget's scope attachment.
    attachment: Option<FocusAttachment>,
    /// The node this scope's backing node hangs under.
    parent: Option<Rc<FocusNode>>,
    /// Inherited-provider revision advanced by backing-node notifications.
    node_revision: Rc<Cell<u64>>,
    rebuild_handle: Option<RebuildHandle>,
    focus_listener_id: Option<ListenerId>,
}

impl std::fmt::Debug for FocusScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeState")
            .field("scope_id", &self.scope.as_focus_node().id())
            .finish_non_exhaustive()
    }
}

impl FocusScopeState {
    fn add_focus_listener(&self, node: &Rc<FocusNode>) -> ListenerId {
        let revision = Rc::clone(&self.node_revision);
        let rebuild = self
            .rebuild_handle
            .as_ref()
            .expect("BUG: FocusScope listener installed before init_state")
            .clone();
        node.add_listener(Rc::new(move || {
            let next = revision
                .get()
                .checked_add(1)
                .expect("BUG: FocusScope inherited revision exhausted");
            revision.set(next);
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }))
    }
}

impl ViewState<FocusScope> for FocusScopeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.focus_manager = Some(ctx.focus_manager());
        self.rebuild_handle = Some(ctx.rebuild_handle());
        self.focus_listener_id = Some(self.add_focus_listener(self.scope.as_focus_node()));
        let parent = enclosing_focus_parent(ctx);
        self.attachment = Some(
            parent
                .attach_node(self.scope.as_focus_node())
                .expect("BUG: FocusScope could not attach to the enclosing focus tree"),
        );
        self.parent = Some(parent);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // An enclosing provider changed: move this scope — subtree, focus and
        // all — under the new parent (ADR-0022).
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
    }

    fn did_update_view(&mut self, old_view: &FocusScope, new_view: &FocusScope) {
        let scope_changed = match (
            old_view.external_scope.as_ref(),
            new_view.external_scope.as_ref(),
        ) {
            (Some(old), Some(new)) => !Rc::ptr_eq(old, new),
            (None, None) => false,
            _ => true,
        };
        if !scope_changed {
            return;
        }

        let replacement = new_view
            .external_scope
            .clone()
            .unwrap_or_else(|| FocusScopeNode::with_debug_label("FocusScope"));
        let replacement_listener_id = self.add_focus_listener(replacement.as_focus_node());
        let attachment = self
            .attachment
            .take()
            .expect("BUG: a mounted FocusScope must retain its FocusAttachment");
        self.attachment = Some(
            attachment
                .replace_node(replacement.as_focus_node())
                .expect("BUG: FocusScope could not atomically replace its attached scope"),
        );
        if let Some(listener_id) = self.focus_listener_id.replace(replacement_listener_id) {
            self.scope.as_focus_node().remove_listener(listener_id);
        }
        self.scope = replacement;
    }

    fn dispose(&mut self) {
        if let Some(listener_id) = self.focus_listener_id.take() {
            self.scope.as_focus_node().remove_listener(listener_id);
        }
        if let Some(attachment) = self.attachment.take() {
            let _ = attachment.detach();
        }
        self.parent = None;
        self.rebuild_handle = None;
        self.focus_manager = None;
    }

    fn build(&self, view: &FocusScope, _ctx: &dyn BuildContext) -> impl IntoView {
        FocusParentProvider {
            parent: Rc::clone(self.scope.as_focus_node()),
            revision: self.node_revision.get(),
            child: view.child.clone(),
        }
    }
}

// ============================================================================
// ExcludeFocus
// ============================================================================

/// Prevents its subtree from receiving focus while exclusion is active.
///
/// Exclusion is active by default. Activating it unfocuses an already-focused
/// descendant, which is not automatically restored when exclusion is disabled.
/// FLUI currently clears primary focus to `None`; unlike Flutter, it does not
/// yet move focus to the enclosing scope's previously focused child.
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

#[cfg(test)]
mod tests {
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    /// A root that can drop the focus subtree without changing its own type —
    /// `swap_root` dispatches by `TypeId`.
    #[derive(Clone)]
    struct Host {
        show: bool,
        scope: Rc<FocusScopeNode>,
        node: Rc<FocusNode>,
        autofocus: bool,
        on_focus_change: Option<FocusChangeHandler>,
    }

    #[derive(Clone, StatelessView)]
    struct ExcludeHost {
        excluding: bool,
        node: Rc<FocusNode>,
    }

    impl StatelessView for ExcludeHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            ExcludeFocus::new(
                Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node)),
            )
            .excluding(self.excluding)
        }
    }

    #[derive(Clone, StatelessView)]
    struct FocusDependencyProbe {
        builds: Rc<Cell<usize>>,
        focused: Rc<Cell<bool>>,
    }

    impl StatelessView for FocusDependencyProbe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            self.builds.set(self.builds.get() + 1);
            self.focused.set(Focus::of(ctx).has_focus());
            SizedBox::new(1.0, 1.0)
        }
    }

    impl View for Host {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl StatelessView for Host {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if !self.show {
                return SizedBox::new(1.0, 1.0).into_view().boxed();
            }
            let mut focus = Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&self.node))
                .autofocus(self.autofocus);
            if let Some(handler) = &self.on_focus_change {
                let handler = Rc::clone(handler);
                focus = focus.on_focus_change(move |focused| handler(focused));
            }
            FocusScope::with_external_node(Rc::clone(&self.scope), focus)
                .into_view()
                .boxed()
        }
    }

    /// Flutter parity: `focus_scope_test.dart`'s `"Descendants of ExcludeFocus
    /// aren't focusable."` (a request while excluding refuses) and
    /// `"ExcludeFocus doesn't transfer focus to another descendant."` (turning
    /// exclusion on evicts an already-focused descendant without picking a new
    /// one), tag `3.44.0`. Also the idempotent-toggle and no-auto-refocus-on-
    /// re-enable properties, which the oracle tests do not separately cover.
    #[test]
    fn exclude_focus_refuses_allows_evicts_idempotently_and_does_not_refocus() {
        let node = FocusNode::with_debug_label("exclude-focus-unit-child");
        let mut harness = mount(ExcludeHost {
            excluding: true,
            node: Rc::clone(&node),
        });
        let manager = harness.focus_manager();
        node.request_focus();
        assert!(manager.primary_focus().is_none());

        harness.swap_root(ExcludeHost {
            excluding: false,
            node: Rc::clone(&node),
        });
        node.request_focus();
        assert!(node.has_primary_focus());

        harness.swap_root(ExcludeHost {
            excluding: true,
            node: Rc::clone(&node),
        });
        assert!(manager.primary_focus().is_none());
        harness.swap_root(ExcludeHost {
            excluding: true,
            node: Rc::clone(&node),
        });
        assert!(manager.primary_focus().is_none());

        harness.swap_root(ExcludeHost {
            excluding: false,
            node: Rc::clone(&node),
        });
        assert!(manager.primary_focus().is_none());
        node.request_focus();
        assert!(node.has_primary_focus());
        manager.unfocus();
    }

    /// The mount shape (`_FocusState.initState` + `FocusScope`,
    /// `focus_scope.dart:565-630`): the widget scope hangs under the
    /// presentation's standard shortcut focus, the node hangs under the
    /// widget scope, and unmounting detaches both and releases primary focus.
    ///
    /// Flutter parity: `focus_scope_test.dart`'s `'Removing a FocusScope
    /// removes its node from the tree'` (the unmount-detaches-both half) and
    /// `'Autofocus works'` (the autofocus-on-mount half), tag `3.44.0`.
    ///
    /// Red-check: make `enclosing_scope` always answer the root scope — the
    /// node parents to the root and the first assertion fails.
    #[test]
    fn a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases() {
        let scope = FocusScopeNode::with_debug_label("host-scope");
        let node = FocusNode::with_debug_label("host-node");
        let mut harness = mount(Host {
            show: true,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });
        let manager = harness.focus_manager();

        assert_eq!(
            node.parent().map(|parent| parent.id()),
            Some(scope.as_focus_node().id()),
            "the node hangs under the widget scope, not the root"
        );
        let traversal_parent = scope
            .as_focus_node()
            .parent()
            .expect("the widget scope has the presentation traversal parent");
        assert_eq!(traversal_parent.debug_label(), Some("Shortcuts"));
        assert_eq!(
            traversal_parent.parent().map(|parent| parent.id()),
            Some(manager.root_scope().as_focus_node().id()),
            "the presentation traversal focus hangs under the root scope"
        );
        assert!(
            node.has_primary_focus(),
            "autofocus focused the node on mount"
        );
        assert_eq!(
            scope.focused_child().map(|focused| focused.id()),
            Some(node.id())
        );

        harness.swap_root(Host {
            show: false,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });

        assert!(!node.is_attached(), "unmount detached the node");
        assert!(
            !scope.as_focus_node().is_attached(),
            "unmount detached the widget scope"
        );
        assert!(
            manager.primary_focus().is_none(),
            "a disposed focused widget releases the primary focus"
        );
    }

    /// A rebuild that flips `autofocus` from `false` to `true` makes the
    /// still-unattempted autofocus request — Flutter's `didUpdateWidget`
    /// re-running `_handleAutofocus` on an `autofocus` change
    /// (`focus_scope_test.dart`'s "Can autofocus a node.", tag 3.44.0), not
    /// just `initState`/`didChangeDependencies`.
    ///
    /// Red-check (verified): drop the `try_autofocus()` call from
    /// `did_update_view` — the node mounted with `autofocus: false` never
    /// requests focus on the later rebuild, and the assertion fails.
    #[test]
    fn a_rebuild_that_turns_on_autofocus_requests_focus() {
        let scope = FocusScopeNode::with_debug_label("rebuild-autofocus-scope");
        let node = FocusNode::with_debug_label("rebuild-autofocus-node");
        let mut harness = mount(Host {
            show: true,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: false,
            on_focus_change: None,
        });
        let manager = harness.focus_manager();
        assert!(!node.has_primary_focus(), "sanity: not focused on mount");

        harness.swap_root(Host {
            show: true,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });

        assert!(
            node.has_primary_focus(),
            "the rebuild's autofocus: true made its one-shot request"
        );

        // A second rebuild that merely repeats `autofocus: true` must not
        // re-attempt: nothing else focused now, so an unfocus followed by a
        // repeated-`true` rebuild staying unfocused proves the latch, not a
        // silently-passing accident.
        manager.unfocus();
        harness.swap_root(Host {
            show: true,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });
        assert!(
            !node.has_primary_focus(),
            "the one-shot latch does not re-request on a value-repeating rebuild"
        );

        manager.unfocus();
    }

    /// `autofocus` yields when the scope already focused something
    /// (`_handleAutofocus`, `focus_scope.dart:625-630`): with two autofocus
    /// siblings, the first to mount wins and the second is skipped.
    ///
    /// Flutter parity: `focus_scope_test.dart`'s `"Won't autofocus a node if
    /// one is already focused."`, tag `3.44.0`.
    ///
    /// Red-check: drop the `focused_child().is_none()` gate in `init_state` —
    /// the second steals the focus and both assertions flip.
    #[test]
    fn autofocus_yields_to_an_already_focused_scope() {
        let scope = FocusScopeNode::with_debug_label("autofocus-scope");
        let first = FocusNode::with_debug_label("first");
        let second = FocusNode::with_debug_label("second");
        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            crate::Column::new(vec![
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Rc::clone(&first))
                    .autofocus(true)
                    .into_view()
                    .boxed(),
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Rc::clone(&second))
                    .autofocus(true)
                    .into_view()
                    .boxed(),
            ]),
        ));
        let manager = harness.focus_manager();

        assert!(first.has_primary_focus(), "the first autofocus wins");
        assert!(!second.has_primary_focus(), "the second yields");

        manager.unfocus();
    }

    /// `on_focus_change` fires on the edges — `true` on gain, `false` on loss
    /// (`Focus.onFocusChange`, `focus_scope.dart:167`).
    ///
    /// Red-check: report `was_focused` instead of `now_focused` in
    /// `install_focus_listener` — the recorded edges invert.
    #[test]
    fn on_focus_change_reports_gain_and_loss() {
        let scope = FocusScopeNode::with_debug_label("edge-scope");
        let node = FocusNode::with_debug_label("edge-node");
        let edges = Rc::new(RefCell::new(Vec::<bool>::new()));
        let recorded = Rc::clone(&edges);
        let mut harness = mount(Host {
            show: true,
            scope: Rc::clone(&scope),
            node: Rc::clone(&node),
            autofocus: false,
            on_focus_change: Some(Rc::new(move |focused| recorded.borrow_mut().push(focused))),
        });
        let manager = harness.focus_manager();

        node.request_focus();
        assert_eq!(
            edges.borrow().as_slice(),
            [true],
            "the focus-manager notification phase delivers the gain outside build"
        );
        harness.tick();
        manager.unfocus();
        assert_eq!(
            edges.borrow().as_slice(),
            [true, false],
            "the loss is delivered by focus notification, not a later build"
        );
        harness.tick();
        assert_eq!(
            edges.borrow().as_slice(),
            [true, false],
            "gain then loss, exactly once each"
        );
    }

    #[test]
    fn focus_of_dependency_rebuilds_when_the_node_focus_changes() {
        let node = FocusNode::with_debug_label("dependency-node");
        let builds = Rc::new(Cell::new(0));
        let focused = Rc::new(Cell::new(false));
        let mut harness = mount(
            Focus::new(FocusDependencyProbe {
                builds: Rc::clone(&builds),
                focused: Rc::clone(&focused),
            })
            .focus_node(Rc::clone(&node)),
        );
        let initial_builds = builds.get();

        node.request_focus();
        harness.tick();
        assert!(focused.get());
        assert!(
            builds.get() > initial_builds,
            "the inherited dependency rebuilt after focus gain"
        );

        let focused_builds = builds.get();
        harness.focus_manager().unfocus();
        harness.tick();
        assert!(!focused.get());
        assert!(
            builds.get() > focused_builds,
            "the inherited dependency rebuilt after focus loss"
        );
    }

    /// A configurable `Focus` whose flags/handlers change across a `swap_root`, so
    /// the inner `Focus`'s `did_update_view` → `configure` runs with a new config.
    #[derive(Clone)]
    struct Configurable {
        node: Rc<FocusNode>,
        scope: Rc<FocusScopeNode>,
        can_request_focus: Option<bool>,
        skip_traversal: Option<bool>,
        on_key_event: Option<KeyEventHandler>,
        on_focus_change: Option<FocusChangeHandler>,
    }

    impl View for Configurable {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl StatelessView for Configurable {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let mut focus = Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node));
            if let Some(can) = self.can_request_focus {
                focus = focus.can_request_focus(can);
            }
            if let Some(skip) = self.skip_traversal {
                focus = focus.skip_traversal(skip);
            }
            if let Some(handler) = &self.on_key_event {
                focus = focus.on_key_event(Rc::clone(handler));
            }
            if let Some(handler) = &self.on_focus_change {
                let handler = Rc::clone(handler);
                focus = focus.on_focus_change(move |focused| handler(focused));
            }
            FocusScope::with_external_node(Rc::clone(&self.scope), focus)
                .into_view()
                .boxed()
        }
    }

    #[derive(Clone, StatelessView)]
    struct ScopeSwapHost {
        external_scope: Option<Rc<FocusScopeNode>>,
        node: Rc<FocusNode>,
    }

    impl StatelessView for ScopeSwapHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let child = Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node));
            match &self.external_scope {
                Some(scope) => FocusScope::with_external_node(Rc::clone(scope), child)
                    .into_view()
                    .boxed(),
                None => FocusScope::new(child).into_view().boxed(),
            }
        }
    }

    #[derive(Clone, StatelessView)]
    struct ParentNodeSwapHost {
        parent: Rc<FocusNode>,
        child: Rc<FocusNode>,
    }

    impl StatelessView for ParentNodeSwapHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            Focus::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.child)))
                .focus_node(Rc::clone(&self.parent))
        }
    }

    /// On the regular external-node path, omitted attributes read through to
    /// the node's current values. Dropping an explicit override therefore
    /// preserves the value already installed on that caller-owned node —
    /// Flutter's `Focus.focusNode` getter/update contract.
    #[test]
    fn an_external_node_keeps_managed_values_when_overrides_are_dropped() {
        use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
        use flui_interaction::routing::KeyEventResult;

        let scope = FocusScopeNode::with_debug_label("cfg-scope");
        let node = FocusNode::with_debug_label("cfg-node");
        let mut harness = mount(Configurable {
            node: Rc::clone(&node),
            scope: Rc::clone(&scope),
            can_request_focus: Some(false),
            skip_traversal: Some(true),
            on_key_event: Some(Rc::new(|_event| KeyEventResult::Handled)),
            on_focus_change: None,
        });
        let key = || KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        };
        assert!(
            !node.can_request_focus(),
            "configured can_request_focus(false)"
        );
        assert!(node.skip_traversal(), "configured skip_traversal(true)");
        assert_eq!(
            node.handle_key_event(&key()),
            KeyEventResult::Handled,
            "the configured key handler runs"
        );

        // Rebuild with none of the three set.
        harness.swap_root(Configurable {
            node: Rc::clone(&node),
            scope: Rc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: None,
        });

        assert!(
            !node.can_request_focus(),
            "the managed external node retains its current false value"
        );
        assert!(
            node.skip_traversal(),
            "the managed external node retains its current true value"
        );
        assert_eq!(
            node.handle_key_event(&key()),
            KeyEventResult::Handled,
            "the installed handler remains the external node's current value"
        );

        // Even after the override disappears, this host remembers that it
        // installed the handler and removes it when it releases the node.
        harness.swap_root(SizedBox::new(1.0, 1.0));
        assert_eq!(
            node.handle_key_event(&key()),
            KeyEventResult::Ignored,
            "a released managed node does not retain a widget-owned callback"
        );
    }

    /// Cleanup is tied to the exact handler generation installed by this
    /// widget, not merely to the node identity. A caller may replace the
    /// handler while the node is hosted; unmounting the stale registration
    /// must preserve that newer value.
    #[test]
    fn managed_node_cleanup_cannot_erase_a_later_external_handler() {
        use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
        use flui_interaction::routing::KeyEventResult;

        let node = FocusNode::with_debug_label("generation-node");
        let scope = FocusScopeNode::with_debug_label("generation-scope");
        let mut harness = mount(Configurable {
            node: Rc::clone(&node),
            scope,
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: Some(Rc::new(|_| KeyEventResult::Handled)),
            on_focus_change: None,
        });
        node.set_on_key_event(Rc::new(|_| KeyEventResult::SkipRemainingHandlers));

        harness.swap_root(SizedBox::new(1.0, 1.0));
        let key = KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        };
        assert_eq!(
            node.handle_key_event(&key),
            KeyEventResult::SkipRemainingHandlers,
            "generation-checked cleanup preserves the later external writer"
        );
    }

    /// `with_external_node` makes every node attribute caller-owned, including
    /// the key handler. Conflicting widget builders cannot mutate it, and
    /// disposal cannot erase it.
    #[test]
    fn a_source_of_truth_external_node_is_never_reconfigured() {
        use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
        use flui_interaction::routing::KeyEventResult;

        let node = FocusNode::with_debug_label("source-node");
        node.set_can_request_focus(false);
        node.set_skip_traversal(true);
        node.set_descendants_are_focusable(false);
        node.set_on_key_event(Rc::new(|_| KeyEventResult::Handled));

        let mut harness = mount(
            Focus::with_external_node(Rc::clone(&node), SizedBox::new(10.0, 10.0))
                .can_request_focus(true)
                .skip_traversal(false)
                .descendants_are_focusable(true)
                .on_key_event(Rc::new(|_| KeyEventResult::Ignored)),
        );
        let key = KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        };

        assert!(!node.can_request_focus());
        assert!(node.skip_traversal());
        assert!(!node.descendants_are_focusable());
        assert_eq!(node.handle_key_event(&key), KeyEventResult::Handled);

        harness.swap_root(SizedBox::new(1.0, 1.0));
        assert!(!node.is_attached(), "the widget still owns the attachment");
        assert!(!node.can_request_focus());
        assert!(node.skip_traversal());
        assert!(!node.descendants_are_focusable());
        assert_eq!(
            node.handle_key_event(&key),
            KeyEventResult::Handled,
            "the caller-owned handler survives widget disposal"
        );
    }

    #[test]
    fn a_rebuild_replaces_the_external_node_without_leaking_attachment_or_handler() {
        use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
        use flui_interaction::routing::{FocusRequestOutcome, KeyEventResult};

        let scope = FocusScopeNode::with_debug_label("node-replacement-scope");
        let first = FocusNode::with_debug_label("first");
        let replacement = FocusNode::with_debug_label("replacement");
        let handler: KeyEventHandler = Rc::new(|_| KeyEventResult::Handled);
        let mut harness = mount(Configurable {
            node: Rc::clone(&first),
            scope: Rc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: Some(Rc::clone(&handler)),
            on_focus_change: None,
        });
        let manager = harness.focus_manager();
        let key = KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        };

        first.request_focus();
        assert_eq!(
            replacement.request_focus(),
            FocusRequestOutcome::Queued,
            "a detached replacement may queue focus before the rebuild"
        );

        harness.swap_root(Configurable {
            node: Rc::clone(&replacement),
            scope: Rc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: Some(handler),
            on_focus_change: None,
        });

        assert!(!first.is_attached(), "the superseded node was detached");
        assert!(
            replacement.has_primary_focus(),
            "the replacement attached and fulfilled its queued request"
        );
        assert_eq!(
            first.handle_key_event(&key),
            KeyEventResult::Ignored,
            "the widget-owned handler was removed from the old external node"
        );
        assert_eq!(
            replacement.handle_key_event(&key),
            KeyEventResult::Handled,
            "the replacement received the current handler"
        );
        assert_eq!(
            manager.listener_count(),
            0,
            "a Focus without an edge callback installs no manager subscription"
        );
    }

    #[test]
    fn a_live_focus_scope_swap_preserves_its_descendant_subtree_and_focus() {
        let first = FocusScopeNode::with_debug_label("first external scope");
        let second = FocusScopeNode::with_debug_label("second external scope");
        let third = FocusScopeNode::with_debug_label("third external scope");
        let node = FocusNode::with_debug_label("scope swap descendant");
        let mut harness = mount(ScopeSwapHost {
            external_scope: Some(Rc::clone(&first)),
            node: Rc::clone(&node),
        });
        node.request_focus();

        harness.swap_root(ScopeSwapHost {
            external_scope: Some(Rc::clone(&second)),
            node: Rc::clone(&node),
        });
        assert!(!first.as_focus_node().is_attached());
        assert!(Rc::ptr_eq(
            &node.parent().expect("descendant remains parented"),
            second.as_focus_node()
        ));
        assert!(node.has_primary_focus());

        harness.swap_root(ScopeSwapHost {
            external_scope: None,
            node: Rc::clone(&node),
        });
        let internal = node
            .parent()
            .and_then(|parent| parent.as_scope())
            .expect("external-to-internal installs a fresh scope");
        assert!(!Rc::ptr_eq(&internal, &second));
        assert!(!second.as_focus_node().is_attached());
        assert!(node.has_primary_focus());

        harness.swap_root(ScopeSwapHost {
            external_scope: Some(Rc::clone(&third)),
            node: Rc::clone(&node),
        });
        assert!(!internal.as_focus_node().is_attached());
        assert!(Rc::ptr_eq(
            &node.parent().expect("descendant remains parented"),
            third.as_focus_node()
        ));
        assert!(node.has_primary_focus());
    }

    #[test]
    fn replacing_a_parent_focus_node_keeps_the_focused_child_attached() {
        let first_parent = FocusNode::with_debug_label("first parent");
        let replacement_parent = FocusNode::with_debug_label("replacement parent");
        let child = FocusNode::with_debug_label("focused child");
        let mut harness = mount(ParentNodeSwapHost {
            parent: Rc::clone(&first_parent),
            child: Rc::clone(&child),
        });
        child.request_focus();

        harness.swap_root(ParentNodeSwapHost {
            parent: Rc::clone(&replacement_parent),
            child: Rc::clone(&child),
        });

        assert!(!first_parent.is_attached());
        assert!(Rc::ptr_eq(
            &child.parent().expect("the child remains in the focus tree"),
            &replacement_parent
        ));
        assert!(
            child.has_primary_focus(),
            "a descendant primary focus survives its parent-node replacement"
        );
    }

    /// Changing `on_focus_change` across a rebuild takes effect: the listener reads
    /// the current handler, not the one captured when it was installed.
    ///
    /// Red-check: in `did_update_view`, stop updating the shared cell — the listener
    /// keeps the first handler, `first` fires and `second` is never called.
    #[test]
    fn a_rebuild_swaps_the_on_focus_change_handler() {
        let scope = FocusScopeNode::with_debug_label("swap-scope");
        let node = FocusNode::with_debug_label("swap-node");
        let first = Rc::new(RefCell::new(Vec::<bool>::new()));
        let second = Rc::new(RefCell::new(Vec::<bool>::new()));

        let first_rec = Rc::clone(&first);
        let mut harness = mount(Configurable {
            node: Rc::clone(&node),
            scope: Rc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: Some(Rc::new(move |focused| first_rec.borrow_mut().push(focused))),
        });
        let manager = harness.focus_manager();

        // Rebuild with a different handler.
        let second_rec = Rc::clone(&second);
        harness.swap_root(Configurable {
            node: Rc::clone(&node),
            scope: Rc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: Some(Rc::new(move |focused| {
                second_rec.borrow_mut().push(focused);
            })),
        });

        node.request_focus();
        harness.tick();
        manager.unfocus();
        harness.tick();

        assert!(
            first.borrow().is_empty(),
            "the superseded handler no longer fires"
        );
        assert_eq!(
            second.borrow().as_slice(),
            [true, false],
            "the current handler fires the gain/loss edges"
        );
    }

    // ------------------------------------------------------------------
    // Focus::of / Focus::maybe_of / FocusScope::of
    // ------------------------------------------------------------------

    /// Which tree shape a [`FocusOfProbe`] is mounted under — one reusable
    /// host below instead of a bespoke type per shape.
    #[derive(Clone, Copy)]
    enum FocusOfShape {
        /// No Focus/FocusScope ancestor at all.
        Bare,
        /// A single plain `Focus` directly wrapping the probe.
        OneFocus,
        /// Two nested plain `Focus` widgets — the probe sits under the INNER
        /// one, so a correct lookup must not stop at the outer one.
        NestedFocus,
        /// A bare `FocusScope` directly wrapping the probe (no plain `Focus`
        /// in between) — the scope-vs-node distinction.
        BareScope,
        /// `FocusScope`, then a plain `Focus`, then the probe —
        /// `FocusScope::of` must walk past the plain `Focus` to the scope.
        ScopeThenFocus,
    }

    /// A leaf that records what [`Focus::maybe_of`] and [`FocusScope::of`]
    /// resolve to from its own build context.
    #[derive(Clone, StatelessView)]
    struct FocusOfProbe {
        found_node: Rc<RefCell<Option<Rc<FocusNode>>>>,
        found_scope: Rc<RefCell<Option<Rc<FocusScopeNode>>>>,
    }

    impl StatelessView for FocusOfProbe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            *self.found_node.borrow_mut() = Focus::maybe_of(ctx);
            *self.found_scope.borrow_mut() = Some(FocusScope::of(ctx));
            SizedBox::new(1.0, 1.0)
        }
    }

    /// Composes a [`FocusOfProbe`] under `shape`, or drops the whole subtree
    /// when `show` is `false` — the same toggle-to-unmount idiom `Host`/
    /// `ExcludeHost` above use, so a test can `swap_root` back to a bare leaf
    /// at the end and let real `dispose()` detach every node this mounted.
    #[derive(Clone, StatelessView)]
    struct FocusOfHost {
        shape: FocusOfShape,
        show: bool,
        outer_node: Rc<FocusNode>,
        inner_node: Rc<FocusNode>,
        scope: Rc<FocusScopeNode>,
        found_node: Rc<RefCell<Option<Rc<FocusNode>>>>,
        found_scope: Rc<RefCell<Option<Rc<FocusScopeNode>>>>,
    }

    impl StatelessView for FocusOfHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if !self.show {
                return SizedBox::new(1.0, 1.0).into_view().boxed();
            }
            let probe = FocusOfProbe {
                found_node: Rc::clone(&self.found_node),
                found_scope: Rc::clone(&self.found_scope),
            };
            match self.shape {
                FocusOfShape::Bare => probe.into_view().boxed(),
                FocusOfShape::OneFocus => Focus::new(probe)
                    .focus_node(Rc::clone(&self.outer_node))
                    .into_view()
                    .boxed(),
                FocusOfShape::NestedFocus => {
                    Focus::new(Focus::new(probe).focus_node(Rc::clone(&self.inner_node)))
                        .focus_node(Rc::clone(&self.outer_node))
                        .into_view()
                        .boxed()
                }
                FocusOfShape::BareScope => {
                    FocusScope::with_external_node(Rc::clone(&self.scope), probe)
                        .into_view()
                        .boxed()
                }
                FocusOfShape::ScopeThenFocus => FocusScope::with_external_node(
                    Rc::clone(&self.scope),
                    Focus::new(probe).focus_node(Rc::clone(&self.outer_node)),
                )
                .into_view()
                .boxed(),
            }
        }
    }

    /// Builds a fresh [`FocusOfHost`] with brand-new nodes/scope and empty
    /// result cells for `shape`.
    fn focus_of_host(shape: FocusOfShape) -> FocusOfHost {
        FocusOfHost {
            shape,
            show: true,
            outer_node: FocusNode::with_debug_label("focus-of-outer"),
            inner_node: FocusNode::with_debug_label("focus-of-inner"),
            scope: FocusScopeNode::with_debug_label("focus-of-scope"),
            found_node: Rc::new(RefCell::new(None)),
            found_scope: Rc::new(RefCell::new(None)),
        }
    }

    /// A presentation always has the standard traversal `Focus`: a bare app
    /// subtree resolves it through `Focus::maybe_of`, while
    /// `FocusScope::of` resolves its enclosing root scope.
    #[test]
    fn bare_presentation_resolves_default_focus_and_root_scope() {
        let host = focus_of_host(FocusOfShape::Bare);
        let mut harness = mount(host.clone());
        let manager = harness.focus_manager();

        let resolved_node = host
            .found_node
            .borrow()
            .clone()
            .expect("FocusRoot installs the standard traversal Focus");
        assert_eq!(resolved_node.debug_label(), Some("Shortcuts"));
        let resolved_scope = host
            .found_scope
            .borrow()
            .clone()
            .expect("the probe's build must have run");
        assert!(
            Rc::ptr_eq(&resolved_scope, manager.root_scope()),
            "the presentation traversal Focus belongs to the root scope"
        );

        harness.swap_root(FocusOfHost {
            show: false,
            ..host
        });
    }

    /// A descendant's `Focus::maybe_of` resolves the one enclosing `Focus`'s
    /// own node.
    #[test]
    fn focus_maybe_of_returns_the_nearest_enclosing_focus_node() {
        let host = focus_of_host(FocusOfShape::OneFocus);
        let mut harness = mount(host.clone());

        let resolved = host
            .found_node
            .borrow()
            .clone()
            .expect("Focus::maybe_of must find the enclosing Focus's node");
        assert!(
            Rc::ptr_eq(&resolved, &host.outer_node),
            "Focus::maybe_of must resolve THIS Focus's own node"
        );

        harness.swap_root(FocusOfHost {
            show: false,
            ..host
        });
    }

    /// Oracle: `'Focus.of stops at the nearest Focus widget.'`
    /// (`focus_scope_test.dart`, tag `3.44.0`) — nesting two plain `Focus`
    /// widgets, a descendant's lookup must resolve the INNER one, never
    /// reaching past it to the outer one.
    #[test]
    fn focus_maybe_of_nearest_wins_over_an_outer_focus() {
        let host = focus_of_host(FocusOfShape::NestedFocus);
        let mut harness = mount(host.clone());

        let resolved = host
            .found_node
            .borrow()
            .clone()
            .expect("Focus::maybe_of must find the nearest enclosing Focus's node");
        assert!(
            Rc::ptr_eq(&resolved, &host.inner_node),
            "the NEAREST Focus must win"
        );
        assert!(
            !Rc::ptr_eq(&resolved, &host.outer_node),
            "must not resolve the outer Focus instead of the inner one"
        );

        harness.swap_root(FocusOfHost {
            show: false,
            ..host
        });
    }

    /// Oracle: `'Focus.of stops at the nearest Focus widget.'`
    /// (`focus_scope_test.dart`, tag `3.44.0`) — the `Focus.maybeOf(element2),
    /// isNull` assertion: a bare enclosing `FocusScope` (no plain `Focus` in
    /// between) does not satisfy `Focus::maybe_of` (`scopeOk: false`), even
    /// though `FocusScope::of` still resolves the scope itself.
    #[test]
    fn focus_maybe_of_returns_none_for_a_bare_enclosing_scope() {
        let host = focus_of_host(FocusOfShape::BareScope);
        let mut harness = mount(host.clone());

        assert!(
            host.found_node.borrow().is_none(),
            "a bare enclosing FocusScope must not satisfy Focus::maybe_of — \
             only a plain Focus counts"
        );
        let resolved_scope = host
            .found_scope
            .borrow()
            .clone()
            .expect("the probe's build must have run");
        assert!(
            Rc::ptr_eq(&resolved_scope, &host.scope),
            "FocusScope::of must still resolve the enclosing scope itself"
        );

        harness.swap_root(FocusOfHost {
            show: false,
            ..host
        });
    }

    /// `FocusScope::of` walks past an intervening plain `Focus` to the
    /// nearest enclosing SCOPE — Flutter's `.nearestScope` — rather than
    /// stopping at (or being refused by) the plain `Focus` the way
    /// `Focus::maybe_of` would be.
    #[test]
    fn focus_scope_of_walks_up_past_a_plain_focus_to_the_nearest_scope() {
        let host = focus_of_host(FocusOfShape::ScopeThenFocus);
        let mut harness = mount(host.clone());

        let resolved_node =
            host.found_node.borrow().clone().expect(
                "Focus::maybe_of must find the plain Focus between the scope and the probe",
            );
        assert!(Rc::ptr_eq(&resolved_node, &host.outer_node));
        let resolved_scope = host
            .found_scope
            .borrow()
            .clone()
            .expect("the probe's build must have run");
        assert!(
            Rc::ptr_eq(&resolved_scope, &host.scope),
            "FocusScope::of must walk past the plain Focus to the enclosing scope"
        );

        harness.swap_root(FocusOfHost {
            show: false,
            ..host
        });
    }

    /// A stateless leaf that runs an arbitrary `on_build` closure once —
    /// mirrors `overlay/tests.rs`'s own `Peek`, kept file-local since only
    /// this one test needs a caller-supplied closure (the others above reuse
    /// `FocusOfHost`/`FocusOfProbe`).
    #[derive(Clone)]
    struct Peek<F: Fn(&dyn BuildContext) + Clone + 'static>(F);

    impl<F: Fn(&dyn BuildContext) + Clone + 'static> View for Peek<F> {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl<F: Fn(&dyn BuildContext) + Clone + 'static> StatelessView for Peek<F> {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            (self.0)(ctx);
            SizedBox::new(1.0, 1.0)
        }
    }

    /// `FocusRoot` makes `Focus::of` total for every normal presentation by
    /// installing the standard traversal focus above application content.
    #[test]
    fn focus_of_resolves_the_presentation_traversal_focus() {
        let resolved: Rc<RefCell<Option<Rc<FocusNode>>>> = Rc::new(RefCell::new(None));
        let resolved_for_probe = Rc::clone(&resolved);
        let probe = Peek(move |ctx: &dyn BuildContext| {
            *resolved_for_probe.borrow_mut() = Some(Focus::of(ctx));
        });

        let _harness = mount(probe);

        let node = resolved
            .borrow()
            .clone()
            .expect("the probe's build resolves the root traversal Focus");
        assert_eq!(node.debug_label(), Some("Shortcuts"));
    }
}

#[cfg(test)]
mod traversal_tests {
    use flui_view::ViewExt;

    use super::*;
    use crate::test_harness::mount;
    use crate::{Positioned, SizedBox, Stack};

    /// Widget-mounted nodes traverse in **reading order**, not attach order —
    /// the ADR-0022 traversal-geometry gap, closed: every `Focus` anchors
    /// its child and installs a rect provider, so `ReadingOrderPolicy` sorts
    /// real committed geometry. The attach order (`a`, `b`, `c`) is chosen so
    /// the on-screen order (`b`, `a`, `c`) is **not** one of its rotations:
    /// from `a`, geometry says `c` next, attach order would say `b`.
    ///
    /// Red-check (the pre-fix behavior): skip `install_rect_provider` in
    /// `init_state` — every rect reads zero, the sort degenerates to attach
    /// order, and the first assertion gets `b`.
    #[test]
    fn tab_traversal_follows_geometry_not_attach_order() {
        let scope = FocusScopeNode::with_debug_label("traversal-scope");
        let a = FocusNode::with_debug_label("a-middle");
        let b = FocusNode::with_debug_label("b-top");
        let c = FocusNode::with_debug_label("c-bottom");

        let positioned = |top: f32, node: &Rc<FocusNode>| {
            Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
                .left(0.0)
                .top(top)
                .width(10.0)
                .height(10.0)
                .into_view()
                .boxed()
        };
        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            Stack::new(vec![
                positioned(50.0, &a),
                positioned(0.0, &b),
                positioned(100.0, &c),
            ]),
        ));
        let manager = harness.focus_manager();

        assert_eq!(
            b.rect().min_y().0,
            0.0,
            "sanity: the provider measures committed layout"
        );
        assert_eq!(a.rect().min_y().0, 50.0);

        a.request_focus();

        manager.focus_next();
        assert!(
            c.has_primary_focus(),
            "after the middle node comes the bottom one — reading order, not attach order"
        );
        manager.focus_next();
        assert!(b.has_primary_focus(), "wraparound lands on the top node");
        manager.focus_next();
        assert!(a.has_primary_focus(), "then the middle again");

        manager.unfocus();
    }
}
