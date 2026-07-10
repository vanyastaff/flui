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
//! * **Nodes parent to the nearest *scope*** (U1.2): a `Focus` nested under a
//!   non-scope `Focus` hangs beside it, not beneath it. FLUI's traversal
//!   consults scopes and per-node flags, never the node tree's shape, so the
//!   flattening is unobservable today; revisit with `FocusTraversalGroup`.
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
use flui_interaction::routing::{FocusManager, FocusNode, FocusScopeNode, KeyEventHandler};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};

/// Reports whether this widget's node gained or lost the primary focus.
pub type FocusChangeHandler = Arc<dyn Fn(bool) + Send + Sync>;

// ============================================================================
// The ambient scope
// ============================================================================

/// Provides the nearest enclosing [`FocusScopeNode`] to descendants — the
/// `GestureArenaScope` pattern. Private: [`FocusScope`] is the public surface.
#[derive(Clone)]
struct FocusScopeProvider {
    scope: Arc<FocusScopeNode>,
    child: BoxedView,
}

impl std::fmt::Debug for FocusScopeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeProvider")
            .field("scope_id", &self.scope.as_focus_node().id())
            .finish_non_exhaustive()
    }
}

impl InheritedView for FocusScopeProvider {
    type Data = Arc<FocusScopeNode>;

    fn data(&self) -> &Self::Data {
        &self.scope
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Arc::ptr_eq(&self.scope, &old.scope)
    }
}

impl_inherited_view!(FocusScopeProvider);

/// The scope a mounting/reparenting node belongs under: the nearest provider,
/// else the manager's root scope (`FocusScope.of`'s fallback,
/// `focus_scope.dart:843-850`).
pub(crate) fn enclosing_scope(ctx: &dyn BuildContext) -> Arc<FocusScopeNode> {
    ctx.get::<FocusScopeProvider, _>(|provider| Arc::clone(&provider.scope))
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

    /// Key handler invoked while this node holds the primary focus; return
    /// `true` to mark the event handled — Flutter's `onKeyEvent` (`:170-180`),
    /// minus the ancestor bubbling FLUI's flat dispatch does not do yet
    /// (ADR-0022 §4).
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

    /// Push the view-configured flags and handlers onto `node`. Only the
    /// properties the view actually sets are written, so an external node
    /// keeps its own configuration elsewhere.
    fn configure(&self, node: &Arc<FocusNode>) {
        if let Some(can) = self.can_request_focus {
            node.set_can_request_focus(can);
        }
        if let Some(skip) = self.skip_traversal {
            node.set_skip_traversal(skip);
        }
        if let Some(focusable) = self.descendants_are_focusable {
            node.set_descendants_are_focusable(focusable);
        }
        if let Some(handler) = &self.on_key_event {
            node.set_on_key_event(Arc::clone(handler));
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
            scope: None,
            focus_listener_id: None,
            autofocus: self.autofocus,
            on_focus_change: self.on_focus_change.clone(),
        }
    }
}

/// `_FocusState` (`focus_scope.dart:554`). `pub` only because
/// `StatefulView::State` requires it; not re-exported.
pub struct FocusState {
    node: Arc<FocusNode>,
    /// The scope this node currently hangs under; `did_change_dependencies`
    /// moves it when the provider changes.
    scope: Option<Arc<FocusScopeNode>>,
    focus_listener_id: Option<ListenerId>,
    /// Captured at `create_state`: `init_state` has no view reference.
    autofocus: bool,
    on_focus_change: Option<FocusChangeHandler>,
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
    fn install_focus_listener(
        &mut self,
        rebuild: RebuildHandle,
        on_focus_change: Option<FocusChangeHandler>,
    ) {
        let node_id = self.node.id();
        self.focus_listener_id = Some(FocusManager::global().add_listener(Arc::new(
            move |previous, current| {
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused != now_focused {
                    if let Some(handler) = &on_focus_change {
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
        let scope = enclosing_scope(ctx);
        scope.attach_node(&self.node);
        self.scope = Some(scope);

        let on_focus_change = self.on_focus_change.clone();
        self.install_focus_listener(ctx.rebuild_handle(), on_focus_change);

        // `_handleAutofocus` (`:625-630`): only when the scope has nothing
        // focused yet. Synchronous — FLUI has no end-of-frame focus batch
        // (module docs).
        if self.autofocus
            && let Some(scope) = &self.scope
            && scope.focused_child().is_none()
        {
            self.node.request_focus();
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // The provider changed: move the node — with focus — under the new
        // scope. `_focusAttachment.reparent()` in `didChangeDependencies`
        // (`focus_scope.dart:618-623`), via ADR-0022 U1.3's adopt.
        let scope = enclosing_scope(ctx);
        if self
            .scope
            .as_ref()
            .is_none_or(|held| !Arc::ptr_eq(held, &scope))
        {
            scope.adopt_node(&self.node);
            self.scope = Some(scope);
        }
    }

    fn did_update_view(&mut self, _old: &Focus, new_view: &Focus) {
        // Re-sync flags and handlers from the latest configuration
        // (`didUpdateWidget`, `:646-682`). Swapping the *node itself* is not
        // supported: FLUI reconciliation keeps the state, and the external
        // node is read once in `create_state`.
        new_view.configure(&self.node);
        self.autofocus = new_view.autofocus;
        self.on_focus_change.clone_from(&new_view.on_focus_change);
    }

    fn dispose(&mut self) {
        if let Some(id) = self.focus_listener_id.take() {
            FocusManager::global().remove_listener(id);
        }
        // Detach from wherever the node currently hangs — this is the
        // *removal* path, so a focused node releases the primary focus
        // (`dispose`, `:605-616`).
        if let Some(scope) = self
            .node
            .parent()
            .and_then(|parent| parent.as_scope())
            .or(self.scope.take())
        {
            scope.detach_node(self.node.id());
        }
    }

    fn build(&self, view: &Focus, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
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

/// The state behind [`FocusScope`]. `pub` only because `StatefulView::State`
/// requires it; not re-exported.
pub struct FocusScopeState {
    scope: Arc<FocusScopeNode>,
    /// The scope this scope's backing node hangs under.
    parent: Option<Arc<FocusScopeNode>>,
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
        let parent = enclosing_scope(ctx);
        parent.attach_node(self.scope.as_focus_node());
        self.parent = Some(parent);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // An enclosing provider changed: move this scope — subtree, focus and
        // all — under the new parent (ADR-0022 U1.3).
        let parent = enclosing_scope(ctx);
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
        if let Some(parent) = self
            .scope
            .as_focus_node()
            .parent()
            .and_then(|node| node.as_scope())
            .or(self.parent.take())
        {
            parent.detach_node(self.scope.as_focus_node().id());
        }
    }

    fn build(&self, view: &FocusScope, _ctx: &dyn BuildContext) -> impl IntoView {
        FocusScopeProvider {
            scope: Arc::clone(&self.scope),
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
