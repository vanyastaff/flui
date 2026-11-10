# ADR-0026: Focus widgets and focus traversal

- **Status:** Accepted (2026-07-10)
- **Date:** 2026-07-11
- **Amended:** 2026-10-11 — removed the self-referential absorption label; Focus widget and traversal rules remain in this record.
- **Absorbs:** [retired ADR-0022](https://github.com/vanyastaff/flui/blob/aaecd27847a6338538f70e230a1aa01a018b0fd6/docs/adr/ADR-0022-focus-widget-seam.md), the Focus widget seam consolidated here.
- **Superseded in part by:** ADR-0079 (§4's `Actions(Shortcuts(child))` nesting rule)
- **Superseded-by:** [ADR-0160](ADR-0160-focus-reading-order.md) (§2's required
  traversal-policy signature only; its sorted-list resolver remains in force)
- **Superseded-by:** [ADR-0165](ADR-0165-focus-policy-groups-and-directional-traversal.md)
  (§2's scope-only resolver and §3's scope-only parent retry; groups and directional
  traversal below are now implemented)
- **Related:** [ADR-0314](ADR-0314-the-focus-ledgers-are-the-notification-channels.md) —
  the listener ledgers named in §1 are that record's notification channels; its owned
  registrations replace the `ListenerId` forms this record once taught.

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
  be the field's ancestor to see the keys it ignores. A widget with no focus widget above it
  parents to the presentation's root scope, which `flui_view::environment::FocusTreeRoot`
  publishes: the UI runtime installs one above each root, reading the scope from its own
  `FocusManager` in `init_state`.
- Lifecycle: `init_state` resolves the parent, attaches, applies autofocus once and registers a
  focus listener; `did_change_dependencies` reparents through `FocusScopeNode::adopt_node`,
  which keeps primary focus across the move; `dispose` drops the subscription (which
  withdraws the listener) and detaches (an external node is left to its owner). The manager
  comes from the lifecycle context (ADR-0078), one per UI runtime.
- The node layer gained `adopt_node` (reparent without dropping focus). Its listener
  ledgers are the notification channels now
  ([ADR-0314](ADR-0314-the-focus-ledgers-are-the-notification-channels.md)): node listeners hold the
  owned `Subscription` and the manager's change listeners subscribe through
  `FocusManager::listen`; no listener id is exposed.
- `ModalRoute` creates a scope, wraps its page in `FocusScope::with_external_node`, makes the
  scope the manager's active scope when the route becomes current, and on reveal
  (`did_pop_next`) restores the scope's remembered focused child, or unfocuses a field left on
  a covered route.
- Traversal geometry is lazy: each `Focus` anchors its child and installs a `RectProvider` that
  measures the anchor when traversal asks, cleared on dispose.

### 2. The sorted list is the traversal primitive

`FocusTraversalPolicy` originally required `sort_descendants`; its signature
is superseded by [ADR-0160](ADR-0160-focus-reading-order.md).
`FocusScopeNode::sorted_traversal_order(cursor)`, with the cursor force-included even when it
is `skip_traversal` or disabled, is the only traversal primitive. Next and previous are
positional lookups; "at the edge" means the cursor is last or first. One pure resolver,
`resolve_traversal(current, forward) -> ResolvedStep`, serves `focus_next`/`focus_previous`,
the scope-local variants and `set_first_focus`, so there is one edge switch. With nothing
focused, Tab falls back to the policy-ordered first or last node.

### 3. Edge behavior

`TraversalEdgeBehavior { ClosedLoop (default), Stop, ParentScope, LeaveView }` lives on
`FocusScopeNode`. `ParentScope` answers `RetryInParent`, and the caller re-resolves in the
enclosing scope with the same cursor. That works because FLUI's candidate walk crosses scope
boundaries, so the step lands on the first node outside the inner scope. `LeaveView`
unfocuses and reports the key unconsumed; the embedder handoff has no channel yet.

### 4. Tab is an intent, and `Action::invoke` reports an outcome

`NextFocusIntent`/`PreviousFocusIntent` are handled by `NextFocusAction`/`PreviousFocusAction`.
`Action::invoke` returns `ActionOutcome` (`Performed`/`NotPerformed`), and `Action` has a
defaulted `to_key_event_result(intent, outcome)`, so a Tab that moves nothing leaves the key
unconsumed for an outer handler. `Actions` must wrap `Shortcuts`, because `Shortcuts` resolves
its action chain from its own position (ADR-0023); the reverse nesting silently dead-keys Tab.
`DefaultFocusTraversal` packages the Tab/Shift+Tab bindings in that order. The UI runtime
installs no key bindings: `flui_widgets::app_root(view)` wraps a root in
`DefaultFocusTraversal`, and it is the one function every composition root calls on each root it
attaches, `flui-app`'s runners for every window and `flui-testing`'s `HeadlessHost` (Flutter's
`WidgetsApp` installs the same bindings at the app root). Embedders that compose their own root
call it too.

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

`FocusTraversalGroup` and directional traversal are implemented by ADR-0165.

## Not implemented

- Highlight modes, `includeSemantics`, legacy `onKey`.
- `ModalRoute` defaulting to `ParentScope`.
- Flutter's `deactivate`-parks-on-root. Focus surviving a `GlobalKey` move is not claimed until
  a test pins it.

## Consequences

- The node layer grew `adopt_node` and its listener ledgers became the
  notification channels (ADR-0314); everything else is widget plumbing over the same
  inherited-provider pattern `GestureArenaScope` and `HeroScope` use.
- One resolver over one sorted list means edge behavior, first Tab and `set_first_focus` cannot
  drift apart.
- Losing focus on reparent is only observable when a focused widget moves between scopes;
  `adopt_node` has a node-layer test that fails for the naive detach-then-attach.
- A root attached without `app_root` has a focus tree but no Tab. Two tests run Tab and
  Shift+Tab through each composition root: `tab_moves_focus_in_every_window_a_runner_installs`
  (`flui-app`, a main and a secondary window of a headless run) and
  `tab_moves_focus_in_a_headless_host_attached_app` (`flui-testing`).

## Alternatives rejected

- **`find_next`/`find_previous` as the policy's oracle**, with a sort beside it: an O(n²)
  default, `None` meaning both "edge" and "cursor not a candidate", and two sources of order.
- **A group as a `FocusScopeNode`**: breaks autofocus and route restore, which both key off
  `enclosing_scope()`.
- **Flattening widget nodes to the nearest scope**: key bubbling could no longer reach a
  non-scope `Focus` above a field.
