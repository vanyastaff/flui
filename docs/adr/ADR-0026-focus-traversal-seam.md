# ADR-0026: Focus widgets and focus traversal

- **Status:** Accepted
- **Date:** 2026-07-11
- **Absorbs:** ADR-0026

## Context

`flui-interaction` owns the focus node layer: `FocusManager` (primary focus, active scope,
key handlers), `FocusNode` (flags, key handler, request/unfocus) and `FocusScopeNode`
(focused-child history, autofocus, a pluggable `FocusTraversalPolicy`). Two things sit on top
of it: widgets that attach a node to an element and parent it correctly, and a traversal model
that can express edge behavior and Tab. Without the widgets, `EditableText` attached to the
root scope and `TextField`'s tap-to-focus had to guess which node was tapped. Without the
traversal model, wraparound was baked into the policy, "stop at the edge" was inexpressible,
and nothing bound Tab.

Reference: Flutter `widgets/focus_scope.dart` (`Focus`, `FocusScope`, `_FocusState`) and
`widgets/focus_traversal.dart` (`FocusTraversalGroup`, `_sortAllDescendants`, `_moveFocus`,
`TraversalEdgeBehavior`, `NextFocusAction`).

## Decision

### 1. `Focus` and `FocusScope` widgets

- `Focus` (`flui-widgets`, `interaction/focus.rs`) is a stateful view owning an
  `Rc<FocusNode>` or accepting an external one (`Focus::with_external_node`). `FocusScope` is
  the same over `Rc<FocusScopeNode>`; `FocusScope::with_external_node` is what `ModalRoute`
  uses for its per-route scope.
- A private `FocusParentProvider` inherited view publishes the nearest focus **node**. Widget
  nodes parent to that node rather than being flattened to the nearest scope: key bubbling
  (ADR-0023) makes the node tree's shape observable, so a non-scope `Focus` above a field must
  be the field's ancestor to see the keys it ignores.
- Lifecycle: `init_state` resolves the parent, attaches, applies autofocus once and registers a
  focus listener; `did_change_dependencies` reparents through `FocusScopeNode::adopt_node`,
  which keeps primary focus across the move; `dispose` removes the listener and detaches (an
  external node is left to its owner). The manager comes from the lifecycle context
  (ADR-0078), one per realm.
- The node layer gained two primitives for this: `remove_listener(ListenerId)` and
  `adopt_node` (reparent without dropping focus).
- `ModalRoute` creates a scope, wraps its page in `FocusScope::with_external_node`, makes the
  scope the manager's active scope when the route becomes current, and on reveal
  (`did_pop_next`) restores the scope's remembered focused child, or unfocuses a field left on
  a covered route.
- Traversal geometry is lazy: each `Focus` anchors its child and installs a `RectProvider` that
  measures the anchor when traversal asks, cleared on dispose.

### 2. The sorted list is the traversal primitive

`FocusTraversalPolicy` has one required method, `sort_descendants`.
`FocusScopeNode::sorted_traversal_order(cursor)`, with the cursor force-included even when it
is `skip_traversal` or disabled, is the only traversal primitive. Next and previous are
positional lookups; "at the edge" means the cursor is last or first. One pure resolver,
`resolve_traversal(current, forward) -> ResolvedStep`, serves `focus_next`/`focus_previous`,
the scope-local variants and `set_first_focus`, so there is one edge switch. With nothing
focused, Tab falls back to the policy-ordered first or last node.

### 3. Edge behavior

`TraversalEdgeBehavior { ClosedLoop (default), Stop, ParentScope, LeaveFlutterView }` lives on
`FocusScopeNode`. `ParentScope` answers `RetryInParent`, and the caller re-resolves in the
enclosing scope with the same cursor. That works because FLUI's candidate walk crosses scope
boundaries, so the step lands on the first node outside the inner scope. `LeaveFlutterView`
unfocuses and reports the key unconsumed; the embedder handoff has no channel yet.

### 4. Tab is an intent, and `Action::invoke` reports an outcome

`NextFocusIntent`/`PreviousFocusIntent` are handled by `NextFocusAction`/`PreviousFocusAction`.
`Action::invoke` returns `ActionOutcome` (`Performed`/`NotPerformed`), and `Action` has a
defaulted `to_key_event_result(intent, outcome)`, so a Tab that moves nothing leaves the key
unconsumed for an outer handler. `Actions` must wrap `Shortcuts`, because `Shortcuts` resolves
its action chain from its own position (ADR-0023); the reverse nesting silently dead-keys Tab.
`DefaultFocusTraversal` packages the Tab/Shift+Tab bindings in that order, and `FocusRoot`
installs it for every standard presentation (Flutter's `WidgetsApp` does the same at the app
root); custom embedders can install it themselves.

## Flutter divergences

- **Reparent point.** Flutter reparents on every `build`. FLUI reparents in
  `did_change_dependencies`, the only time the parent can change without a remount. The
  difference is only observable through `parentNode`, which is not ported.
- **Focus changes apply synchronously.** Flutter batches them to the end of the frame.
- **Nested scopes traverse flat.** Flutter treats each nested scope as a unit and implements
  `ParentScope` as unfocus, retry, verify-changed. FLUI's walk crosses scopes, so a route's
  subtree traverses flat and `ParentScope` needs no rule for landing on a scope node. Observable
  only when a route nests scopes and expects Tab to enter and leave them as blocks; nothing in
  the tree does.
- **`Action::invoke` changed signature** instead of gaining a second defaulted method: two
  methods that must agree is a permanent hazard, and the break cost two in-repo impls.

## Not implemented

- `FocusTraversalGroup`. When it lands it is a plain non-scope node carrying a policy, found by
  climbing the node ancestry. It must not be a scope, which would corrupt autofocus and route
  restore. Its `can_request_focus(false)` must not hide its descendants; that holds only
  because `allows_descendant_focus` reads a node's own flag for scopes alone, so the group needs
  a test pinning it.
- Directional traversal, highlight modes, `includeSemantics`, legacy `onKey`.
- `ModalRoute` defaulting to `ParentScope`.
- Flutter's `deactivate`-parks-on-root. Focus surviving a `GlobalKey` move is not claimed until
  a test pins it.

## Consequences

- The node layer grew two small primitives; everything else is widget plumbing over the same
  inherited-provider pattern `GestureArenaScope` and `HeroScope` use.
- One resolver over one sorted list means edge behavior, first Tab and `set_first_focus` cannot
  drift apart.
- Losing focus on reparent is only observable when a focused widget moves between scopes;
  `adopt_node` has a node-layer test that fails for the naive detach-then-attach.

## Alternatives rejected

- **`find_next`/`find_previous` as the policy's oracle**, with a sort beside it: an O(n²)
  default, `None` meaning both "edge" and "cursor not a candidate", and two sources of order.
- **A group as a `FocusScopeNode`**: breaks autofocus and route restore, which both key off
  `enclosing_scope()`.
- **Flattening widget nodes to the nearest scope**: key bubbling could no longer reach a
  non-scope `Focus` above a field.
