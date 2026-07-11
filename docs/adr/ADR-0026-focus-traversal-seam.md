# ADR-0026 — Focus traversal: groups, edge behavior, and Tab intents

- **Status:** **Proposed — reworked after adversarial review; not yet implementable as a whole.** The first design (api-designer agent, 2026-07-11) **did not survive** the harsh-critic pass: two blockers, five majors, and a structural finding that the chosen primitive was inverted. This ADR records the reworked shape with the critique's requirements as binding constraints. U-A is implementable; U-B/U-C carry open decisions flagged for their deciders.
- **Date:** 2026-07-11
- **Deciders:** chief-architect (the ALT-1 primitive inversion, §3.1; scope-landing semantics, §3.4); api-design-lead (§3.6's `Action::invoke` signature — a deliberate breaking change while consumers are few, Prime Directive #2); qa-lead (§5's pinning tests, incl. the `allows_descendant_focus` guard).
- **Relates to:** ADR-0022 (Focus widgets; §4 named this gap; the geometry fix that unblocked it), ADR-0023 (Shortcuts/Actions/Intent; O-1's resolve-at-own-position divergence is load-bearing here).

---

## 1. Context (verified)

Three gaps: `FocusScopeNode` holds exactly one policy — no subtree can opt into a different order without becoming a scope (`focus_scope.rs:756`); wraparound is baked into `ReadingOrderPolicy` via modulo (`:973`, `:980-984`) — "stop at the edge" and "leave the scope" are inexpressible; and **nothing binds Tab** — `FocusManager::focus_next`/`focus_previous` exist, route-scoped, with zero input-path callers.

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/focus_traversal.dart`, master `3.33.0-0.0.pre-6280-g88e87cd963f`: `FocusTraversalGroup` is `Focus(focusNode: _FocusTraversalGroupNode(policy), canRequestFocus: false, skipTraversal: true, …)` — a **plain node subclass, not a scope** (`:2211-2219`, `:2254-2264`); lookup climbs the node tree (`:2115-2126`). Group-aware sort: `_findGroups` walks raw children stopping at nested scopes, **force-includes the current node even when non-traversable** (`:487-489`), buckets by nearest group node (`:461-482`); `_sortAllDescendants` sorts each bucket with its own policy and flattens groups as contiguous units, stripping group nodes at the end (`:503-573`). **The sorted list is the primitive**: `_moveFocus` works entirely off `sortedNodes` — membership assert (`:611`), positional next/previous, `findFirstFocus` fallback when nothing is focused (`:594-608`), edge behavior applied positionally (`:590-666`), ParentScope = `focusedChild.unfocus(); parentScope.nextFocus()` **then verify focus actually changed** (`:604-630`). `TraversalEdgeBehavior`: closedLoop / leaveFlutterView / parentScope (falls back to closedLoop with no parent, `:148-149`) / stop (`:113-156`). Tab: `WidgetsApp` binds `Tab → NextFocusIntent` (`app.dart:1275-1276`) with `NextFocusAction` whose **key result is `invoke`'s return** — `nextFocus()`'s bool (`focus_traversal.dart:2340-2348`).

## 3. The reworked Rust shape

### 3.1 The primitive inversion (ALT-1 — adopted)

The first design kept `find_next` as the trait's oracle and bolted `sort_descendants` beside it. The critique showed every blocker flowed from that: an O(n²) default, `None` ambiguously meaning both *true edge* and *cursor not in candidates*, and cursor-membership holes. **Adopted instead, Flutter's actual architecture:** `sort_descendants(&[Arc<FocusNode>]) -> Vec<Arc<FocusNode>>` becomes the trait's required method; `sorted_traversal_order(scope, cursor)` (group-aware bucketing, **cursor force-included** even when `skip_traversal`/disabled, `:487-489`) is the only traversal primitive. Next/previous are positional lookups into that list; *edge* is "cursor is last/first", a fact the mutating layer reads; the old `find_next`/`find_previous` leave the trait's required surface. The trait **shrinks**.

This dissolves outright: the edge-vs-missing-cursor ambiguity (critique #2 — a `skip_traversal` cursor no longer jumps to first-of-scope on Tab), the unspecified O(n²) default with its non-termination against wrapping impls (#6a), and the oracle/list duality.

### 3.2 The group node (survived review)

A **plain, non-scope** `FocusNode` carrying `Option<Arc<dyn FocusTraversalPolicy>>`, resolved by climbing the node ancestry — never a `FocusScopeNode`: a group masquerading as a scope corrupts `Focus::autofocus`'s gate (`widgets/focus.rs:355`) and ADR-0022 U3's route-restore, both keyed off `enclosing_scope()` treating any `as_scope()` node as authoritative. `Arc<dyn FocusTraversalPolicy>` is already trigger-9-sanctioned (`port-check.sh:1145`). Widget: `FocusTraversalGroup` = `Focus(can_request_focus(false), skip_traversal(true)).traversal_policy(…)`.

**Fragile invariant, pinned (#8):** a group's `can_request_focus(false)` does not blind its descendants *only* because `allows_descendant_focus` consults `own_can_request_focus` solely for scopes (`focus_scope.rs:636-638`). A well-meaning "fix" to that clause kills every group's subtree — a pinning test (group with `can_request_focus(false) + skip_traversal(true)`; descendants stay focusable and traversable) is part of U-B's definition of done.

**Bucketing eligibility is written out, not spec-by-reference (#6c):** the walk is over raw children (NOT `collect_focusable_nodes` — group nodes are `skip_traversal` and would be filtered before bucketing), stops at nested scopes while adding the scope node itself, force-adds group nodes to their parent's bucket for unit-placement, and strips them from the final list (`:461-482`, `:544-546`). Mid-walk `set_traversal_group_policy` may yield one mixed-policy hop — named, accepted (#9).

### 3.3 Edge behavior

`TraversalEdgeBehavior { ClosedLoop (default), LeaveFlutterView, ParentScope, Stop }` on `FocusScopeNode`. **One** shared resolution fn (#7) both `FocusScopeNode` and `FocusManager` call. `ParentScope` ports Flutter's mechanism, not a cursor hack (#3): unfocus, `parent_scope.focus_next()`, **verify focus changed** — the first design's "retry with this scope's id as cursor" would sort a zero-rect backing node at (0,0) (`FocusScope` installs no rect provider, `widgets/focus.rs:525-529`) and land arbitrarily. `LeaveFlutterView` = unfocus + report-unconsumed; the embedder half has no channel yet (named).

### 3.4 Scope-landing semantics — the open decision (blocker #4, chief-architect)

Two verified facts collide: the group-aware walk adds nested **scope nodes** as candidates (`:443-446`), and FLUI's `request_focus(scope_id)` performs **no forwarding** into the scope's focused child (`focus.rs:261-290`) — Tab would land on an invisible zero-rect backing node, a keyboard black hole. Also: today's `collect_focusable_nodes` **crosses** scope boundaries, so a stop-at-scopes walk *removes reachability that exists now*. The ADR requires one of: (a) port Flutter's scope-forwarding (focusing a scope node redirects to its focused child / policy-first descendant) — the loyal shape, a node-layer change with its own tests; or (b) exclude scope nodes from candidates and define explicit descent. **Recommendation: (a).** U-B does not start until this is decided.

### 3.5 First Tab must work (#5)

`FocusManager::focus_next` with `primary_focus == None` currently returns `false` — Tab in a fresh app would no-op. Flutter falls back to `findFirstFocus` (`:594-608`). FLUI: the shared resolution fn falls back to policy-ordered first focus; `set_first_focus`'s attach-order bug (it takes `collect_focusable_nodes().first()`, a separate pre-existing divergence) is fixed by the same `sorted_traversal_order` call — in the same unit, not as a drive-by.

### 3.6 Tab intents (U-C), and the `Action` signature (api-design-lead)

`NextFocusIntent`/`PreviousFocusIntent` + actions. **The one user-facing snippet, corrected (#1):** `Actions` must be the **outer** widget — FLUI's `Shortcuts` resolves its chain from its own position (ADR-0023 O-1), so `Shortcuts::new(Actions::new(…))` silently dead-keys Tab with zero diagnostics:

```rust
Actions::new(
    Shortcuts::new(app_root)
        .shortcut(SingleActivator::named(NamedKey::Tab), NextFocusIntent)
        .shortcut(SingleActivator::named(NamedKey::Tab).shift(), PreviousFocusIntent),
)
.action(NextFocusAction)
.action(PreviousFocusAction)
```

The nesting constraint is part of the public docs. No auto-install: FLUI has no `WidgetsApp`-equivalent root to hang it on (named; revisit with one).

`NextFocusAction`'s key result is whether focus moved — `invoke`'s return in Flutter. Choice for api-design-lead, **recommendation: the breaking change** (`fn invoke(&self, intent: &T) -> ActionOutcome`, unit-defaultable): two in-repo impls exist, Prime Directive #2 says break now before consumers ossify. The additive alternative (a defaulted `invoke_for_key_result`) buys a permanent two-methods-that-must-agree hazard (an override that skips `invoke` silently diverges the `maybe_invoke` and key paths) — if chosen anyway, that hazard gets a pinning test.

## 4. Units (reordered per the critique's ALT-3)

- **U-A** (implementable now): ALT-1 primitive — `sort_descendants` on the trait, `sorted_traversal_order` with force-included cursor + the §3.2 eligibility table, positional next/previous, `set_first_focus` fix, first-Tab fallback, the shared edge-resolution fn with `ClosedLoop`/`Stop`. `ClosedLoop` default keeps `tab_traversal_follows_geometry_not_attach_order` green — noting honestly that that test cannot catch the #2 ambiguity (its cursor is never `skip_traversal`); a dedicated skip-traversal-cursor test joins U-A.
- **U-B** (gated on §3.4's decision): the group node + `FocusTraversalGroup` widget + `ParentScope` (needs scope-landing + verify-changed) + the #8 pinning test.
- **U-C**: intents/actions per §3.6, gated on the api-design-lead signature call.

## 5. Deliberately absent (named)

Directional traversal (`focusInDirection` unported — `directionalTraversalEdgeBehavior` is meaningless without it); `Navigator.routeTraversalEdgeBehavior` wiring (`ModalRoute`'s scope should default `ParentScope` per `navigator.dart:1277` — a route-layer follow-up on `set_traversal_edge_behavior`); `leaveFlutterView`'s embedder handoff; `FocusTraversalGroup::of`/`maybe_of` (no consumer; matches ADR-0022's `Focus::of` deferral); multi-level nested-group oracle porting (depth-1 verified at U-B; depth-N when a widget nests groups); auto-installed Tab bindings (no root-app widget).

## 6. What the review changed (for the record)

The critique's verdict was *doesn't survive as specced*. Adopted wholesale: ALT-1 (primitive inversion), ALT-3 (unit reordering), the corrected snippet + nesting constraint, Flutter's parent-retry mechanism, the first-Tab fallback, the written-out eligibility table, the shared edge fn, and the #8/#2 pinning tests. Sent to its decider: ALT-2 (breaking `invoke` signature, recommended) vs the fourth-erased-closure shape. The one part that survived intact — the plain-node group with ancestry-resolved policy — is also the one part the first design got structurally right; its rejection of group-as-scope (autofocus gate + route-restore corruption) held under attack.
