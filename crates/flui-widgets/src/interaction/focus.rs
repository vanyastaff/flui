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

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_foundation::ListenerId;
use flui_geometry::Rect;
use flui_interaction::routing::{FocusManager, FocusNode, FocusScopeNode, KeyEventHandler};
use flui_objects::SubtreeAnchor;
use flui_types::geometry::px;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};
use parking_lot::Mutex;

use crate::navigator::AnchoredBox;

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
    parent: Arc<FocusNode>,
    child: BoxedView,
}

impl std::fmt::Debug for FocusParentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusParentProvider")
            .field("parent_id", &self.parent.id())
            .finish_non_exhaustive()
    }
}

impl InheritedView for FocusParentProvider {
    type Data = Arc<FocusNode>;

    fn data(&self) -> &Self::Data {
        &self.parent
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Arc::ptr_eq(&self.parent, &old.parent)
    }
}

impl_inherited_view!(FocusParentProvider);

/// The node a mounting/reparenting focus node hangs under: the nearest
/// provider's node, else the root scope's backing node.
pub(crate) fn enclosing_focus_parent(ctx: &dyn BuildContext) -> Arc<FocusNode> {
    ctx.get::<FocusParentProvider, _>(|provider| Arc::clone(&provider.parent))
        .unwrap_or_else(|| Arc::clone(FocusManager::global().root_scope().as_focus_node()))
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
/// One named divergence, inherited from [`FocusParentProvider`] as it already
/// existed (not introduced here): Flutter's `_FocusInheritedScope` extends
/// `InheritedNotifier<FocusNode>`, so a dependent also rebuilds whenever the
/// resolved `FocusNode` itself fires `notifyListeners` — e.g. a plain gain/
/// loss of focus, not just a reparent. `FocusParentProvider::update_should_notify`
/// only compares node IDENTITY (`Arc::ptr_eq`), so a descendant reading
/// `Focus::of`/`maybe_of`/`FocusScope::of` does NOT automatically rebuild on
/// a focus-state change alone — only on the enclosing node changing outright.
/// Changing that notify contract is a separate, wider-reaching decision
/// (it would also affect `enclosing_focus_parent`'s own callers) and is not
/// attempted here.
fn nearest_focus_node(ctx: &dyn BuildContext) -> Option<Arc<FocusNode>> {
    ctx.depend_on::<FocusParentProvider, _>(|provider| Arc::clone(&provider.parent))
}

// ============================================================================
// Focus
// ============================================================================

/// Makes its subtree focusable: owns a [`FocusNode`] (or adopts an external
/// one), attaches it under the nearest enclosing [`FocusScope`] on mount, and
/// detaches it on dispose. Flutter's `Focus` (`focus_scope.dart:126`).
#[derive(Clone)]
// Ported property names (`autofocus`, `can_request_focus`) end with the widget's
// own name; keeping Flutter's names beats a lint-driven rename.
#[allow(clippy::struct_field_names)]
pub struct Focus {
    child: BoxedView,
    /// An externally owned node, when the caller needs the handle — e.g. to
    /// call `request_focus` from a controller. `None` = widget-owned.
    external_node: Option<Arc<FocusNode>>,
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
            external_node: None,
            autofocus: false,
            can_request_focus: None,
            skip_traversal: None,
            descendants_are_focusable: None,
            on_focus_change: None,
            on_key_event: None,
            debug_label: None,
        }
    }

    /// Use `node` instead of a widget-owned one — Flutter's `Focus.focusNode`
    /// (`focus_scope.dart:159`). The caller keeps ownership; this widget still
    /// attaches, reparents, and detaches it with its own lifecycle.
    #[must_use]
    pub fn focus_node(mut self, node: Arc<FocusNode>) -> Self {
        self.external_node = Some(node);
        self
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
    pub fn of(ctx: &dyn BuildContext) -> Arc<FocusNode> {
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
    /// `createDependency: true` (see [`nearest_focus_node`]'s doc for why
    /// that default, not `Overlay::maybe_of`'s override, is what this
    /// mirrors).
    ///
    /// `None` if the nearest enclosing node is a [`FocusScope`]'s own scope
    /// node rather than a plain [`Focus`] (`scopeOk: false` — a scope only
    /// satisfies [`FocusScope::of`], not this), or if there is no enclosing
    /// [`Focus`]/[`FocusScope`] at all.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Arc<FocusNode>> {
        nearest_focus_node(ctx).filter(|node| !node.is_scope())
    }

    /// The node this widget will drive: the external one, or a fresh one.
    fn make_node(&self) -> Arc<FocusNode> {
        match &self.external_node {
            Some(node) => Arc::clone(node),
            None => FocusNode::with_debug_label(self.debug_label.unwrap_or("Focus")),
        }
    }

    /// Push the view-configured flags and handlers onto `node` — the **full**
    /// configuration, written on every mount and rebuild.
    ///
    /// Each property is set unconditionally; an unset (`None`) property is written
    /// as its FLUI/Flutter default (`can_request_focus` → `true`, `skip_traversal`
    /// → `false`, `descendants_are_focusable` → `true`, `on_key_event` → cleared).
    /// This is what makes a rebuild *reset* a value the view no longer sets: writing
    /// only the `Some` properties (the earlier shape) left a dropped
    /// `skip_traversal(true)` or `on_key_event` lingering on the node. Flutter's
    /// `_FocusState.didUpdateWidget` (`focus_scope.dart:646-682`) writes all of them
    /// the same way. The node is the widget's to drive here, external or owned; a
    /// caller that needs node state the widget must never touch keeps it off these
    /// four properties.
    fn configure(&self, node: &Arc<FocusNode>) {
        node.set_can_request_focus(self.can_request_focus.unwrap_or(true));
        node.set_skip_traversal(self.skip_traversal.unwrap_or(false));
        node.set_descendants_are_focusable(self.descendants_are_focusable.unwrap_or(true));
        match &self.on_key_event {
            Some(handler) => node.set_on_key_event(Rc::clone(handler)),
            None => node.clear_on_key_event(),
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
        self.configure(&node);
        FocusState {
            node,
            parent: None,
            anchor: SubtreeAnchor::new(),
            focus_listener_id: None,
            autofocus: self.autofocus,
            did_autofocus: false,
            on_focus_change: Rc::new(RefCell::new(self.on_focus_change.clone())),
            pending_focus_changes: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// `_FocusState` (`focus_scope.dart:554`). `pub` because `StatefulView::State`
/// requires it, and re-exported like every other widget's state in this crate
/// (`GestureDetectorState`, `AnimatedAlignState`, …) so a caller can name it.
pub struct FocusState {
    node: Arc<FocusNode>,
    /// The node this one currently hangs under; `did_change_dependencies`
    /// moves it when the provider changes.
    parent: Option<Arc<FocusNode>>,
    /// Publishes the child's `RenderId` while mounted, so the node's
    /// [`RectProvider`](flui_interaction::RectProvider) can measure it —
    /// reading-order traversal sorts by this geometry (ADR-0022).
    anchor: SubtreeAnchor,
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
    /// Focus edges captured by the owner-local focus-manager listener and
    /// delivered from owner-local `build`.
    pending_focus_changes: Arc<Mutex<Vec<bool>>>,
}

impl std::fmt::Debug for FocusState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusState")
            .field("node_id", &self.node.id())
            .finish_non_exhaustive()
    }
}

impl FocusState {
    /// The rebuild-on-focus-change listener — Flutter's `_handleFocusChanged`
    /// `setState` (`:684-712`): descendants that read the node's state during
    /// build stay current, and `on_focus_change` fires on the edges.
    fn install_focus_listener(&mut self, rebuild: RebuildHandle) {
        let node_id = self.node.id();
        let pending_focus_changes = Arc::clone(&self.pending_focus_changes);
        self.focus_listener_id = Some(FocusManager::global().add_listener(Rc::new(
            move |previous, current| {
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused != now_focused {
                    pending_focus_changes.lock().push(now_focused);
                    rebuild.schedule();
                }
            },
        )));
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
            .unwrap_or_else(|| Arc::clone(FocusManager::global().root_scope()));
        if scope.focused_child().is_none() {
            self.node.request_focus();
        }
    }
}

impl ViewState<Focus> for FocusState {
    /// Attach, listen, autofocus — in that order, so an autofocus that lands
    /// immediately is already observed by the listener
    /// (`_FocusState.initState` + `didChangeDependencies`,
    /// `focus_scope.dart:565-630`).
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let parent = enclosing_focus_parent(ctx);
        parent.attach_node(&self.node);
        self.parent = Some(parent);

        install_rect_provider(&self.node, &self.anchor, ctx);

        self.install_focus_listener(ctx.rebuild_handle());

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
            .is_none_or(|held| !Arc::ptr_eq(held, &parent))
        {
            parent.adopt_node(&self.node);
            self.parent = Some(parent);
        }
    }

    fn did_update_view(&mut self, _old: &Focus, new_view: &Focus) {
        // Re-sync flags and handlers from the latest configuration
        // (`didUpdateWidget`, `:646-682`). Swapping the *node itself* is not
        // supported: FLUI reconciliation keeps the state, and the external
        // node is read once in `create_state`.
        new_view.configure(&self.node);
        self.autofocus = new_view.autofocus;
        // `didUpdateWidget`'s `oldWidget.autofocus != widget.autofocus` guard
        // (`focus_scope.dart`, tag `3.44.0`) is folded into `try_autofocus`'s
        // own `did_autofocus` latch: a rebuild that flips `autofocus` from
        // `false` to `true` makes the one still-unattempted autofocus
        // request; one that merely repeats an already-`true` value is a
        // no-op either way.
        self.try_autofocus();
        // The listener installed at mount reads this cell, so a rebuild that
        // swaps the handler swaps what actually fires — capturing the handler
        // in the closure instead would pin the *first* one for the widget's
        // whole life.
        self.on_focus_change
            .borrow_mut()
            .clone_from(&new_view.on_focus_change);
    }

    fn dispose(&mut self) {
        // An external node outlives this widget: it must not keep measuring a
        // dead anchor.
        self.node.clear_rect_provider();
        if let Some(id) = self.focus_listener_id.take() {
            FocusManager::global().remove_listener(id);
        }
        // Detach from wherever the node currently hangs — this is the
        // *removal* path, so a focused node releases the primary focus
        // (`dispose`, `:605-616`). A scope parent also cleans its history.
        if let Some(parent) = self.node.parent().or(self.parent.take()) {
            detach_from(&parent, self.node.id());
        }
    }

    /// Every `Focus` provides itself as the parent for descendants —
    /// Flutter's `_FocusInheritedScope` in `_FocusState.build`
    /// (`focus_scope.dart:714-741`) — and anchors the child so the node's
    /// rect provider has a render node to measure.
    fn build(&self, view: &Focus, _ctx: &dyn BuildContext) -> impl IntoView {
        let changes = std::mem::take(&mut *self.pending_focus_changes.lock());
        for focused in changes {
            // Read the *current* handler, not the one captured at install.
            if let Some(handler) = self.on_focus_change.borrow().as_ref() {
                handler(focused);
            }
        }

        FocusParentProvider {
            parent: Arc::clone(&self.node),
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
    node: &Arc<FocusNode>,
    anchor: &SubtreeAnchor,
    ctx: &dyn BuildContext,
) {
    let anchor = anchor.clone();
    let owner = ctx.pipeline_owner();
    node.set_rect_provider(Rc::new(move || {
        let render_id = anchor.get()?;
        let owner = owner.as_ref()?.read();
        let size = owner.box_size(render_id)?;
        let root = owner.root_id()?;
        let transform = owner.transform_to(render_id, root)?;
        Some(transform.transform_rect(&Rect::from_ltwh(px(0.0), px(0.0), size.width, size.height)))
    }));
}

/// Detach `child` from `parent`, routing through the scope API when the
/// parent is one so the focused-child history is cleaned too.
fn detach_from(parent: &Arc<FocusNode>, child: flui_interaction::FocusNodeId) {
    match parent.as_scope() {
        Some(scope) => scope.detach_node(child),
        None => parent.detach_node(child),
    }
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
    external_scope: Option<Arc<FocusScopeNode>>,
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
    pub fn with_external_node(scope: Arc<FocusScopeNode>, child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            external_scope: Some(scope),
        }
    }

    /// Returns the [`FocusScopeNode`] of the nearest enclosing [`Focus`] or
    /// [`FocusScope`], walked up to its scope — Flutter's `FocusScope.of`
    /// (`focus_scope.dart:834`, tag `3.44.0`): `Focus.maybeOf(context,
    /// scopeOk: true)` (unfiltered — [`nearest_focus_node`] directly, not
    /// [`Focus::maybe_of`]'s scope-filtering wrapper), then `.nearestScope`
    /// (itself if it already is a scope, else the nearest scope ancestor —
    /// [`FocusNode::as_scope`]/[`FocusNode::enclosing_scope`], the exact pair
    /// [`FocusState::try_autofocus`] already uses to find "the enclosing
    /// scope" for autofocus purposes).
    ///
    /// Falls back to the process-global root scope
    /// ([`FocusManager::global`]'s [`root_scope`](FocusManager::root_scope))
    /// when there is no enclosing [`Focus`]/[`FocusScope`] at all — Flutter
    /// never returns null here, unlike [`Focus::maybe_of`].
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> Arc<FocusScopeNode> {
        nearest_focus_node(ctx)
            .and_then(|node| node.as_scope().or_else(|| node.enclosing_scope()))
            .unwrap_or_else(|| Arc::clone(FocusManager::global().root_scope()))
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
                Some(scope) => Arc::clone(scope),
                None => FocusScopeNode::with_debug_label("FocusScope"),
            },
            parent: None,
        }
    }
}

/// The state behind [`FocusScope`]. `pub` because `StatefulView::State` requires
/// it; re-exported with the rest of the crate's widget states.
pub struct FocusScopeState {
    scope: Arc<FocusScopeNode>,
    /// The node this scope's backing node hangs under.
    parent: Option<Arc<FocusNode>>,
}

impl std::fmt::Debug for FocusScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeState")
            .field("scope_id", &self.scope.as_focus_node().id())
            .finish_non_exhaustive()
    }
}

impl ViewState<FocusScope> for FocusScopeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let parent = enclosing_focus_parent(ctx);
        parent.attach_node(self.scope.as_focus_node());
        self.parent = Some(parent);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // An enclosing provider changed: move this scope — subtree, focus and
        // all — under the new parent (ADR-0022).
        let parent = enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Arc::ptr_eq(held, &parent))
        {
            parent.adopt_node(self.scope.as_focus_node());
            self.parent = Some(parent);
        }
    }

    fn dispose(&mut self) {
        if let Some(parent) = self.scope.as_focus_node().parent().or(self.parent.take()) {
            detach_from(&parent, self.scope.as_focus_node().id());
        }
    }

    fn build(&self, view: &FocusScope, _ctx: &dyn BuildContext) -> impl IntoView {
        FocusParentProvider {
            parent: Arc::clone(self.scope.as_focus_node()),
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
    use crate::test_harness::{FOCUS_TEST_LOCK, mount};

    /// A root that can drop the focus subtree without changing its own type —
    /// `swap_root` dispatches by `TypeId`.
    #[derive(Clone)]
    struct Host {
        show: bool,
        scope: Arc<FocusScopeNode>,
        node: Arc<FocusNode>,
        autofocus: bool,
        on_focus_change: Option<FocusChangeHandler>,
    }

    #[derive(Clone, StatelessView)]
    struct ExcludeHost {
        excluding: bool,
        node: Arc<FocusNode>,
    }

    impl StatelessView for ExcludeHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            ExcludeFocus::new(
                Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Arc::clone(&self.node)),
            )
            .excluding(self.excluding)
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
                .focus_node(Arc::clone(&self.node))
                .autofocus(self.autofocus);
            if let Some(handler) = &self.on_focus_change {
                let handler = Rc::clone(handler);
                focus = focus.on_focus_change(move |focused| handler(focused));
            }
            FocusScope::with_external_node(Arc::clone(&self.scope), focus)
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let node = FocusNode::with_debug_label("exclude-focus-unit-child");
        let mut harness = mount(ExcludeHost {
            excluding: true,
            node: Arc::clone(&node),
        });
        node.request_focus();
        assert_eq!(manager.primary_focus(), None);

        harness.swap_root(ExcludeHost {
            excluding: false,
            node: Arc::clone(&node),
        });
        node.request_focus();
        assert!(node.has_primary_focus());

        harness.swap_root(ExcludeHost {
            excluding: true,
            node: Arc::clone(&node),
        });
        assert_eq!(manager.primary_focus(), None);
        harness.swap_root(ExcludeHost {
            excluding: true,
            node: Arc::clone(&node),
        });
        assert_eq!(manager.primary_focus(), None);

        harness.swap_root(ExcludeHost {
            excluding: false,
            node: Arc::clone(&node),
        });
        assert_eq!(manager.primary_focus(), None);
        node.request_focus();
        assert!(node.has_primary_focus());
        manager.unfocus();
    }

    /// The mount shape (`_FocusState.initState` + `FocusScope`,
    /// `focus_scope.dart:565-630`): the widget scope hangs under the root
    /// scope, the node hangs under the widget scope — **not** the root — and
    /// unmounting detaches both and releases the primary focus.
    ///
    /// Flutter parity: `focus_scope_test.dart`'s `'Removing a FocusScope
    /// removes its node from the tree'` (the unmount-detaches-both half) and
    /// `'Autofocus works'` (the autofocus-on-mount half), tag `3.44.0`.
    ///
    /// Red-check: make `enclosing_scope` always answer the root scope — the
    /// node parents to the root and the first assertion fails.
    #[test]
    fn a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("host-scope");
        let node = FocusNode::with_debug_label("host-node");
        let mut harness = mount(Host {
            show: true,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });

        assert_eq!(
            node.parent().map(|parent| parent.id()),
            Some(scope.as_focus_node().id()),
            "the node hangs under the widget scope, not the root"
        );
        assert_eq!(
            scope.as_focus_node().parent().map(|parent| parent.id()),
            Some(manager.root_scope().as_focus_node().id()),
            "the widget scope hangs under the root scope"
        );
        assert!(
            node.has_primary_focus(),
            "autofocus focused the node on mount"
        );
        assert_eq!(scope.focused_child(), Some(node.id()));

        harness.swap_root(Host {
            show: false,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });

        assert!(!node.is_attached(), "unmount detached the node");
        assert!(
            !scope.as_focus_node().is_attached(),
            "unmount detached the widget scope"
        );
        assert_eq!(
            manager.primary_focus(),
            None,
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("rebuild-autofocus-scope");
        let node = FocusNode::with_debug_label("rebuild-autofocus-node");
        let mut harness = mount(Host {
            show: true,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: false,
            on_focus_change: None,
        });
        assert!(!node.has_primary_focus(), "sanity: not focused on mount");

        harness.swap_root(Host {
            show: true,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
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
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: true,
            on_focus_change: None,
        });
        assert!(
            !node.has_primary_focus(),
            "the one-shot latch does not re-request on a value-repeating rebuild"
        );

        manager.unfocus();
        manager.root_scope().detach_node(scope.as_focus_node().id());
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("autofocus-scope");
        let first = FocusNode::with_debug_label("first");
        let second = FocusNode::with_debug_label("second");
        let _harness = mount(FocusScope::with_external_node(
            Arc::clone(&scope),
            crate::Column::new(vec![
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Arc::clone(&first))
                    .autofocus(true)
                    .into_view()
                    .boxed(),
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Arc::clone(&second))
                    .autofocus(true)
                    .into_view()
                    .boxed(),
            ]),
        ));

        assert!(first.has_primary_focus(), "the first autofocus wins");
        assert!(!second.has_primary_focus(), "the second yields");

        manager.unfocus();
        manager.root_scope().detach_node(scope.as_focus_node().id());
    }

    /// `on_focus_change` fires on the edges — `true` on gain, `false` on loss
    /// (`Focus.onFocusChange`, `focus_scope.dart:167`).
    ///
    /// Red-check: report `was_focused` instead of `now_focused` in
    /// `install_focus_listener` — the recorded edges invert.
    #[test]
    fn on_focus_change_reports_gain_and_loss() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("edge-scope");
        let node = FocusNode::with_debug_label("edge-node");
        let edges = Arc::new(parking_lot::Mutex::new(Vec::<bool>::new()));
        let recorded = Arc::clone(&edges);
        let mut harness = mount(Host {
            show: true,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: false,
            on_focus_change: Some(Rc::new(move |focused| recorded.lock().push(focused))),
        });

        node.request_focus();
        harness.tick();
        manager.unfocus();
        harness.tick();
        assert_eq!(
            edges.lock().as_slice(),
            [true, false],
            "gain then loss, exactly once each"
        );

        manager.root_scope().detach_node(scope.as_focus_node().id());
    }

    /// A configurable `Focus` whose flags/handlers change across a `swap_root`, so
    /// the inner `Focus`'s `did_update_view` → `configure` runs with a new config.
    #[derive(Clone)]
    struct Configurable {
        node: Arc<FocusNode>,
        scope: Arc<FocusScopeNode>,
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
            let mut focus =
                Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Arc::clone(&self.node));
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
            FocusScope::with_external_node(Arc::clone(&self.scope), focus)
                .into_view()
                .boxed()
        }
    }

    /// A rebuild that drops a property resets it to its default and clears the key
    /// handler — `configure` writes the *full* configuration, not just the `Some`
    /// fields (the reviewer's `did_update_view` finding).
    ///
    /// Red-check: revert `configure` to write only the `Some(...)` properties — the
    /// dropped `skip_traversal`/`can_request_focus`/`on_key_event` linger and every
    /// reset assertion fails.
    #[test]
    fn a_rebuild_resets_dropped_focus_config() {
        use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
        use flui_interaction::routing::KeyEventResult;

        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("cfg-scope");
        let node = FocusNode::with_debug_label("cfg-node");
        let mut harness = mount(Configurable {
            node: Arc::clone(&node),
            scope: Arc::clone(&scope),
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
            node: Arc::clone(&node),
            scope: Arc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: None,
        });

        assert!(node.can_request_focus(), "reset to the default true");
        assert!(!node.skip_traversal(), "reset to the default false");
        assert_eq!(
            node.handle_key_event(&key()),
            KeyEventResult::Ignored,
            "the dropped key handler was cleared"
        );

        manager.root_scope().detach_node(scope.as_focus_node().id());
    }

    /// Changing `on_focus_change` across a rebuild takes effect: the listener reads
    /// the current handler, not the one captured when it was installed.
    ///
    /// Red-check: in `did_update_view`, stop updating the shared cell — the listener
    /// keeps the first handler, `first` fires and `second` is never called.
    #[test]
    fn a_rebuild_swaps_the_on_focus_change_handler() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("swap-scope");
        let node = FocusNode::with_debug_label("swap-node");
        let first = Arc::new(Mutex::new(Vec::<bool>::new()));
        let second = Arc::new(Mutex::new(Vec::<bool>::new()));

        let first_rec = Arc::clone(&first);
        let mut harness = mount(Configurable {
            node: Arc::clone(&node),
            scope: Arc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: Some(Rc::new(move |focused| first_rec.lock().push(focused))),
        });

        // Rebuild with a different handler.
        let second_rec = Arc::clone(&second);
        harness.swap_root(Configurable {
            node: Arc::clone(&node),
            scope: Arc::clone(&scope),
            can_request_focus: None,
            skip_traversal: None,
            on_key_event: None,
            on_focus_change: Some(Rc::new(move |focused| second_rec.lock().push(focused))),
        });

        node.request_focus();
        harness.tick();
        manager.unfocus();
        harness.tick();

        assert!(
            first.lock().is_empty(),
            "the superseded handler no longer fires"
        );
        assert_eq!(
            second.lock().as_slice(),
            [true, false],
            "the current handler fires the gain/loss edges"
        );

        manager.root_scope().detach_node(scope.as_focus_node().id());
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
        found_node: Rc<RefCell<Option<Arc<FocusNode>>>>,
        found_scope: Rc<RefCell<Option<Arc<FocusScopeNode>>>>,
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
        outer_node: Arc<FocusNode>,
        inner_node: Arc<FocusNode>,
        scope: Arc<FocusScopeNode>,
        found_node: Rc<RefCell<Option<Arc<FocusNode>>>>,
        found_scope: Rc<RefCell<Option<Arc<FocusScopeNode>>>>,
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
                    .focus_node(Arc::clone(&self.outer_node))
                    .into_view()
                    .boxed(),
                FocusOfShape::NestedFocus => {
                    Focus::new(Focus::new(probe).focus_node(Arc::clone(&self.inner_node)))
                        .focus_node(Arc::clone(&self.outer_node))
                        .into_view()
                        .boxed()
                }
                FocusOfShape::BareScope => {
                    FocusScope::with_external_node(Arc::clone(&self.scope), probe)
                        .into_view()
                        .boxed()
                }
                FocusOfShape::ScopeThenFocus => FocusScope::with_external_node(
                    Arc::clone(&self.scope),
                    Focus::new(probe).focus_node(Arc::clone(&self.outer_node)),
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

    /// Without any enclosing `Focus`/`FocusScope`, `Focus::maybe_of` is
    /// `None` and `FocusScope::of` falls back to the root scope.
    #[test]
    fn focus_maybe_of_is_none_and_focus_scope_of_falls_back_to_root_without_an_ancestor() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let host = focus_of_host(FocusOfShape::Bare);
        let mut harness = mount(host.clone());

        assert!(
            host.found_node.borrow().is_none(),
            "Focus::maybe_of must return None with no Focus/FocusScope ancestor"
        );
        let resolved_scope = host
            .found_scope
            .borrow()
            .clone()
            .expect("the probe's build must have run");
        assert!(
            Arc::ptr_eq(&resolved_scope, manager.root_scope()),
            "FocusScope::of must fall back to the root scope with no ancestor"
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let host = focus_of_host(FocusOfShape::OneFocus);
        let mut harness = mount(host.clone());

        let resolved = host
            .found_node
            .borrow()
            .clone()
            .expect("Focus::maybe_of must find the enclosing Focus's node");
        assert!(
            Arc::ptr_eq(&resolved, &host.outer_node),
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let host = focus_of_host(FocusOfShape::NestedFocus);
        let mut harness = mount(host.clone());

        let resolved = host
            .found_node
            .borrow()
            .clone()
            .expect("Focus::maybe_of must find the nearest enclosing Focus's node");
        assert!(
            Arc::ptr_eq(&resolved, &host.inner_node),
            "the NEAREST Focus must win"
        );
        assert!(
            !Arc::ptr_eq(&resolved, &host.outer_node),
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

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
            Arc::ptr_eq(&resolved_scope, &host.scope),
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let host = focus_of_host(FocusOfShape::ScopeThenFocus);
        let mut harness = mount(host.clone());

        let resolved_node =
            host.found_node.borrow().clone().expect(
                "Focus::maybe_of must find the plain Focus between the scope and the probe",
            );
        assert!(Arc::ptr_eq(&resolved_node, &host.outer_node));
        let resolved_scope = host
            .found_scope
            .borrow()
            .clone()
            .expect("the probe's build must have run");
        assert!(
            Arc::ptr_eq(&resolved_scope, &host.scope),
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

    /// `Focus::of` panics with a message naming the failing call and hinting
    /// at the non-panicking fallback — Flutter's own assert-time
    /// `FlutterError` (`focus_scope.dart:398-424`), matching the
    /// `Overlay::of` precedent (`overlay/tests.rs`).
    ///
    /// The panic is caught with `catch_unwind` **inside** the probe's own
    /// build, rather than expecting it to unwind through `mount`: the
    /// framework's own `build_or_recover` catches a `build()` panic to keep
    /// one bad widget from taking down the whole test process.
    #[test]
    fn focus_of_panics_with_a_helpful_message_without_a_focus_ancestor() {
        let _guard = FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let message: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let message_for_probe = Arc::clone(&message);
        let probe = Peek(move |ctx: &dyn BuildContext| {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Focus::of(ctx)));
            if let Err(payload) = outcome {
                let text = payload
                    .downcast_ref::<&str>() // PORT-CHECK-OK-DOWNCAST: test-only extraction of a caught panic's message, not V-type smuggling
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned()) // PORT-CHECK-OK-DOWNCAST: same panic-message extraction, the `String`-payload case
                    .unwrap_or_default();
                *message_for_probe.lock() = Some(text);
            }
        });

        let _harness = mount(probe);

        let text = message
            .lock()
            .clone()
            .expect("Focus::of must panic without a Focus ancestor");
        assert!(
            text.contains("Focus::of") && text.contains("Focus widget"),
            "panic message must name the failing call, got: {text:?}"
        );
        assert!(
            text.contains("Focus::maybe_of"),
            "panic message must hint at the non-panicking fallback, got: {text:?}"
        );
    }
}

#[cfg(test)]
mod traversal_tests {
    use flui_interaction::routing::FocusManager;
    use flui_view::ViewExt;

    use super::*;
    use crate::test_harness::{FOCUS_TEST_LOCK, mount};
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
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("traversal-scope");
        let a = FocusNode::with_debug_label("a-middle");
        let b = FocusNode::with_debug_label("b-top");
        let c = FocusNode::with_debug_label("c-bottom");

        let positioned = |top: f32, node: &Arc<FocusNode>| {
            Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Arc::clone(node)))
                .left(0.0)
                .top(top)
                .width(10.0)
                .height(10.0)
                .into_view()
                .boxed()
        };
        let _harness = mount(FocusScope::with_external_node(
            Arc::clone(&scope),
            Stack::new(vec![
                positioned(50.0, &a),
                positioned(0.0, &b),
                positioned(100.0, &c),
            ]),
        ));

        assert_eq!(
            b.rect().min_y().0,
            0.0,
            "sanity: the provider measures committed layout"
        );
        assert_eq!(a.rect().min_y().0, 50.0);

        manager.set_active_scope(Some(Arc::clone(&scope)));
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
        manager.set_active_scope(None);
        manager.root_scope().detach_node(scope.as_focus_node().id());
    }
}
