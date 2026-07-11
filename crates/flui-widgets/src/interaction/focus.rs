//! [`Focus`] and [`FocusScope`] — the widgets that put `flui-interaction`'s
//! focus tree into the element tree.
//!
//! ADR-0022 U2. The node/manager layer (`FocusManager`, `FocusNode`,
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
//! # Divergences, each named (ADR-0022 §3-§4)
//!
//! * **Nodes parent to the nearest focus *node*** — scope or plain `Focus` —
//!   through one provider, Flutter's `_FocusInheritedScope` shape. (ADR-0022
//!   U1.2 originally flattened to the nearest scope; ADR-0023's key bubbling
//!   made the node tree's shape observable and superseded that decision.)
//! * **Reparenting happens in `did_change_dependencies`**, not on every build
//!   as Flutter's `_focusAttachment.reparent()` does — the provider notifying
//!   is the only way the enclosing scope changes without a remount. Observable
//!   only through `parentNode`, which is not ported.
//! * **Focus changes apply synchronously.** Flutter batches into
//!   `applyFocusChangesIfNeeded` at end of frame; FLUI's `FocusManager` is
//!   synchronous throughout, so `autofocus` runs inline from `init_state`.
//! * Not ported: `onKey` (legacy), `includeSemantics` (needs the semantics
//!   layer), `parentNode`, `descendantsAreTraversable` (no node-layer flag),
//!   and `Focus.of`/`maybeOf` (descendants read the provider directly; a
//!   public lookup waits for `Actions`/`Shortcuts`).

use std::sync::Arc;

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
pub type FocusChangeHandler = Arc<dyn Fn(bool) + Send + Sync>;

// ============================================================================
// The ambient scope
// ============================================================================

/// Provides the nearest enclosing focus **node** — scope or plain `Focus` —
/// to descendants: Flutter's `_FocusInheritedScope`, which every `Focus`
/// widget provides (`focus_scope.dart:946`). Private: [`Focus`] and
/// [`FocusScope`] are the public surface.
///
/// The parent being a *node*, not always a scope, is what makes the
/// leaf→root key-dispatch walk (ADR-0023 U1) match the widget tree: a
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

/// The nearest enclosing **scope** — `FocusScope.of`'s fallback contract
/// (`focus_scope.dart:843-850`): the nearest provider node when it is itself a
/// scope, else that node's enclosing scope, else the root scope.
pub(crate) fn enclosing_scope(ctx: &dyn BuildContext) -> Arc<FocusScopeNode> {
    let parent = enclosing_focus_parent(ctx);
    parent
        .as_scope()
        .or_else(|| parent.enclosing_scope())
        .unwrap_or_else(|| Arc::clone(FocusManager::global().root_scope()))
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
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.on_focus_change = Some(Arc::new(handler));
        self
    }

    /// Key handler invoked during the leaf→root dispatch walk while this
    /// node — or a descendant — holds the primary focus: Flutter's
    /// `onKeyEvent` (`:170-180`). Return
    /// [`Handled`](flui_interaction::KeyEventResult::Handled) to consume the
    /// event, [`Ignored`](flui_interaction::KeyEventResult::Ignored) to let it
    /// bubble to the enclosing `Focus`, or
    /// [`SkipRemainingHandlers`](flui_interaction::KeyEventResult::SkipRemainingHandlers)
    /// to stop the bubbling without consuming (ADR-0023 U1).
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
            Some(handler) => node.set_on_key_event(Arc::clone(handler)),
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
            on_focus_change: Arc::new(Mutex::new(self.on_focus_change.clone())),
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
    /// reading-order traversal sorts by this geometry (ADR-0022 §4).
    anchor: SubtreeAnchor,
    focus_listener_id: Option<ListenerId>,
    /// Captured at `create_state`: `init_state` has no view reference.
    autofocus: bool,
    /// The current `on_focus_change` handler, behind a shared cell so the installed
    /// listener reads the *latest* one. `did_update_view` writes here rather than
    /// reinstalling the listener — a captured-by-value closure would keep firing the
    /// handler from the build that mounted it.
    on_focus_change: Arc<Mutex<Option<FocusChangeHandler>>>,
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
        let on_focus_change = Arc::clone(&self.on_focus_change);
        self.focus_listener_id = Some(FocusManager::global().add_listener(Arc::new(
            move |previous, current| {
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused != now_focused {
                    // Read the *current* handler, not the one captured at install.
                    if let Some(handler) = on_focus_change.lock().as_ref() {
                        handler(now_focused);
                    }
                    rebuild.schedule();
                }
            },
        )));
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

        // `_handleAutofocus` (`:625-630`): only when the enclosing scope has
        // nothing focused yet. Synchronous — FLUI has no end-of-frame focus
        // batch (module docs).
        if self.autofocus && enclosing_scope(ctx).focused_child().is_none() {
            self.node.request_focus();
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // The provider changed: move the node — with focus — under the new
        // parent. `_focusAttachment.reparent()` in `didChangeDependencies`
        // (`focus_scope.dart:618-623`), via ADR-0022 U1.3's adopt.
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
        // The listener installed at mount reads this cell, so a rebuild that
        // swaps the handler swaps what actually fires — capturing the handler
        // in the closure instead would pin the *first* one for the widget's
        // whole life.
        (*self.on_focus_change.lock()).clone_from(&new_view.on_focus_change);
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
    node.set_rect_provider(Arc::new(move || {
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
        // all — under the new parent (ADR-0022 U1.3).
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
                let handler = Arc::clone(handler);
                focus = focus.on_focus_change(move |focused| handler(focused));
            }
            FocusScope::with_external_node(Arc::clone(&self.scope), focus)
                .into_view()
                .boxed()
        }
    }

    /// The mount shape (`_FocusState.initState` + `FocusScope`,
    /// `focus_scope.dart:565-630`): the widget scope hangs under the root
    /// scope, the node hangs under the widget scope — **not** the root — and
    /// unmounting detaches both and releases the primary focus.
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

    /// `autofocus` yields when the scope already focused something
    /// (`_handleAutofocus`, `focus_scope.dart:625-630`): with two autofocus
    /// siblings, the first to mount wins and the second is skipped.
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
        let _harness = mount(Host {
            show: true,
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            autofocus: false,
            on_focus_change: Some(Arc::new(move |focused| recorded.lock().push(focused))),
        });

        node.request_focus();
        manager.unfocus();
        assert_eq!(
            edges.lock().as_slice(),
            [true, false],
            "gain then loss, exactly once each"
        );

        manager.root_scope().detach_node(scope.as_focus_node().id());
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
    /// the ADR-0022 §4 traversal-geometry gap, closed: every `Focus` anchors
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
