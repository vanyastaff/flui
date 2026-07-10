# ADR-0022 — The `Focus` / `FocusScope` widget seam

- **Status:** **Proposed — design only.** No code lands with this ADR; the units below are the implementation plan. Written 2026-07-10 from a fresh read of `.flutter/` and the FLUI focus layer.
- **Date:** 2026-07-10
- **Deciders:** chief-architect; consult interaction owner (`flui-interaction` `FocusManager`/`FocusNode` API additions, U1), view owner (inherited reparenting contract, U2), repository owner (public API: `Focus`, `FocusScope`), qa-lead (traversal and key-routing tests).
- **Relates to:** closes ADR-0020 §Seam 6 ("no `Focus`/`FocusScope` **widget**"); builds on tracker H4 (the node/manager layer, done 2026-06-30); unblocks `Actions`/`Shortcuts` (B1.1) and `ModalRoute`'s per-route focus scope; consumes ADR-0021's `HeroScope`/`GestureArenaScope` ambient-provider pattern.

---

## 1. Context

Everything below the widget layer already exists and is public in `flui-interaction`:

- **`FocusManager`** (`routing/focus.rs:126-507`): a process-global `OnceLock` singleton (`FocusManager::global()`) holding `root_scope`, `primary_focus`, focus-change listeners, per-node and global key handlers, and `active_scope` (the modal override). `AppBinding::handle_input` already routes every `PlatformInput::Keyboard` event into `FocusManager::global().dispatch_key_event` (`flui-app/src/app/binding.rs:937-940`).
- **`FocusNode`** (`routing/focus_scope.rs:139-521`): `Arc`-shared, with `can_request_focus` / `skip_traversal` / `descendants_are_focusable` flags, `on_key_event`, `request_focus`/`unfocus`, and ancestor/descendant walks. Parenting is deliberately internal — only `FocusScopeNode::attach_node`/`detach_node` mutate the tree.
- **`FocusScopeNode`** (`:623-791`): focused-child history, `autofocus`, `traps_focus`, and a pluggable `FocusTraversalPolicy` (reading-order with wraparound).

What is missing is exactly ADR-0020 §Seam 6's sentence: *there is no widget that attaches a `FocusNode` to an element, reparents it under the nearest enclosing scope, and makes a route's scope the active one on mount.* The two in-tree consumers prove the gap by working around it:

- `EditableText` attaches its node **to the root scope directly** and registers a manager-level listener it can never remove — it gates the callback with a disposed-flag `AtomicBool` instead (`editable_text.rs:117-119`, `:172-188`), because `FocusManager` has `add_listener` but no per-listener removal (`focus.rs:333-338`, only `clear_listeners`).
- `TextField`'s tap-to-focus cannot name the node that was tapped: it walks the root scope's children and focuses *the first node with a key handler* (`text_field.rs:143-161`; the gap is documented at `:85-96`). Multi-field forms are broken by design until a widget owns the node.

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/focus_scope.dart`, master `3.33.0-0.0.pre-6280-g88e87cd963f`:

- **`Focus`** (`:126-153`): `child`, `focusNode` (external), `parentNode`, `autofocus`, `onFocusChange`, `onKeyEvent`, `canRequestFocus`, `skipTraversal`, `descendantsAreFocusable`, `descendantsAreTraversable`, `includeSemantics`, `debugLabel`. `Focus.of(context)` is a `dependOnInheritedWidgetOfExactType<_FocusInheritedScope>` (`focus_manager.dart:401`, `:461`).
- **`FocusScope`** (`:804-834`): a `Focus` whose node is a `FocusScopeNode`, plus `FocusScope.withExternalFocusNode` — **the constructor `ModalRoute` uses** for its per-route scope.
- **`_FocusState` lifecycle** (`:554-742`), the contract a port must honor:
  1. `initState` → `_initNode()`: configure node flags from the widget (only when the node is internal), `focusNode.attach(context, …)` → a `FocusAttachment`, then `addListener(_handleFocusChanged)`.
  2. `didChangeDependencies` → `_focusAttachment.reparent()` then `_handleAutofocus()` (once).
  3. `build` → `_focusAttachment.reparent(parent: widget.parentNode)` **every build**, then wrap the child in `_FocusInheritedScope(node, child)`.
  4. `deactivate` → reparent parks the node on the root, so a `GlobalKey` move keeps focus state.
  5. `dispose` → `removeListener`, `detach`, and dispose the node **only if internal**.
- **Navigator integration**: the navigator owns one `FocusNode` (`navigator.dart:3763`), wraps its overlay in `Focus` (`:5978`), and route restoration calls `navigator.focusNode.enclosingScope?.requestFocus()` (`:273`, `:311`). Each `ModalRoute` wraps its page in `FocusScope.withExternalFocusNode` so traversal stays inside the route.

## 3. The Rust shape

Four units, dependency-ordered. The ambient-provider pattern is `GestureArenaScope`'s (`interaction/gesture_arena_scope.rs`) and `HeroScope`'s: an `InheritedView` carrying an `Arc` handle, read by descendants with `get`/`depend_on`.

### U1 — `flui-interaction` prerequisites (no widgets yet)

1. **Per-listener removal on `FocusManager`**: `add_listener` returns a `ListenerId` (the `flui-foundation` type every other notifier uses) and `remove_listener(ListenerId)` exists. Migrate `EditableText`'s disposed-flag workaround to it in the same unit — the workaround is the test that the API is sufficient.
2. **Scope-relative attach for a plain node under a plain node**: today only `FocusScopeNode::attach_node` parents a node. A `Focus` widget nested under another non-scope `Focus` needs `FocusNode`-to-`FocusNode` parenting, or the documented decision that FLUI parents every widget-owned node to its nearest *scope* (flattening non-scope nesting). **Decision: flatten to the nearest scope for U2** — Flutter's traversal semantics only consult scopes and traversal flags, and FLUI's `ReadingOrderPolicy` sorts by rect, not tree shape; record the divergence and revisit when `FocusTraversalGroup` lands.
3. **No reparent primitive**: FLUI reparents by `detach_node` + `attach_node`. `detach_child` clears `primary_focus` if the moved subtree held focus (`focus_scope.rs:494-499`), so a naive detach+attach **drops focus on reparent** where Flutter's `FocusAttachment.reparent` preserves it. U1 adds `FocusScopeNode::adopt_node` (attach that preserves an existing primary focus and history entry) or narrows `detach_child`'s clearing to genuinely-removed subtrees. This is the one behavioral seam that must be red-checked at the node layer before any widget exists.

### U2 — the `Focus` and `FocusScope` widgets (`flui-widgets`)

- `Focus`: a `StatefulView` owning an `Arc<FocusNode>` (or accepting an external one via `.focus_node(node)`); builders for `autofocus`, `can_request_focus`, `skip_traversal`, `descendants_are_focusable`, `on_focus_change`, `on_key_event`, `debug_label`. `includeSemantics` is deferred with the semantics layer; `onKey` (legacy) is not ported; `parentNode` is deferred until a caller exists.
- `FocusScope`: the same over `Arc<FocusScopeNode>`, plus `FocusScope::with_external_node(node, child)` — the `ModalRoute` constructor.
- A private `FocusScopeProvider` `InheritedView` (Data = `Arc<FocusScopeNode>`) provides the nearest scope; `Focus`/`FocusScope` read it in `init_state` and re-read in `did_change_dependencies` (FLUI's reparent point — **not** every build; FLUI's inherited dependency notifies on provider change, which is the only time the parent scope can change without a remount. Divergence from Flutter's reparent-every-build, recorded; it is observable only through `parentNode`, which is deferred).
- Lifecycle: `init_state` = resolve scope + attach + autofocus-once + focus-listener (via U1's `ListenerId`); `dispose` = remove listener + detach (+ nothing for an external node, which its owner disposes). Trigger #22 is untouched: no frame capability is acquired anywhere near `build`.
- `Focus::of`-style lookups stay Rust-shaped: descendants use `ctx.depend_on::<FocusScopeProvider, _>` directly; no static `of(context)` API is added until a consumer (Actions/Shortcuts) wants one.

### U3 — the per-route focus scope (`ModalRoute`)

`ModalRoute` creates a `FocusScopeNode`, wraps the page subtree in `FocusScope::with_external_node`, and drives `FocusManager::set_active_scope` from the route lifecycle it already observes: the top route's scope becomes active when it becomes current, and the revealed route's scope is restored on pop (`navigator.dart:273`, `:311`). `traps_focus` — already a node-layer flag — becomes meaningful here: Tab traversal inside a modal stays inside it (`focus.rs` `active_scope` already scopes `focus_next`). Removes the `modal_route.rs:49-50` "No FocusScope" divergence note.

### U4 — consumers, export, and the parity gate

`EditableText` owns a real `Focus` wiring (ambient scope, not root), `TextField` tap-to-focus targets **its own node** (closing `text_field.rs:85-96`), the export guard gains the new private types, and the gate re-verifies against `test/widgets/focus_scope_test.dart` oracles: focus follows request, unfocus, Tab order under a scope, scope isolation across routes, autofocus-once, and dispose-releases-focus.

## 4. What is deliberately absent (all named)

- **Highlight modes** (`FocusHighlightMode`/`Strategy`, `focus_manager.dart:1554-1624`): needs pointer-vs-key input tracking; nothing consumes it until Material focus rings.
- **`FocusTraversalGroup` / directional traversal / `TraversalEdgeBehavior`**: FLUI has one reading-order policy; groups land with a real traversal consumer.
- **`Actions` / `Shortcuts`**: the next seam up, and the reason `Focus::of`-style lookup stays unexported for now. Separate ADR once this one lands.
- **`onKey` (legacy)** and `KeyEventResult::SkipRemainingHandlers` propagation: FLUI's dispatch is flat (global handlers, then the focused node); bubbling up the ancestry is added when a widget needs interception, likely with `Shortcuts`.
- **Semantics integration** (`includeSemantics`): blocked on the semantics layer's focus actions.
- **`deactivate`-parks-on-root** (Flutter `:632-643`): FLUI keeps focus state across `GlobalKey` moves only if U1's adopt primitive is used by reactivation; the `GlobalKey`-move-keeps-focus behavior is *not* claimed until a test pins it.

## 5. Consequences

**Good.** The node layer needs only two small additions (listener removal, adopt-without-dropping-focus); everything else is widget plumbing over proven patterns (`GestureArenaScope`, `HeroScope`, ADR-0021 §7m's route-lifecycle observation). `AppBinding` already dispatches keys into the manager, so no platform work is needed.

**Bad.** `FocusManager` is a free global singleton, not binding-owned — two embedders in one process would share focus. That is today's reality (`EditableText` already depends on it) and is out of this ADR's scope, but U1 should not deepen the coupling: widgets take the manager through the scope provider's nodes, touching `FocusManager::global()` only at the root-scope fallback.

**Risk.** The reparent-drops-focus hazard (U1.3) is the kind of silent divergence the FlippedCurve episode (ADR-0021 §7o addendum) showed can hide under green gates: it is only observable when a focused widget moves between scopes. The U1 red-check requirement exists for that reason.
