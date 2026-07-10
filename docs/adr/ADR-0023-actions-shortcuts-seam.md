# ADR-0023 — The `Shortcuts` / `Actions` seam, and bubbling key dispatch

- **Status:** **Proposed — design.** Units land incrementally; the header is updated as they do. Written 2026-07-10 from a fresh read of `.flutter/` and the FLUI key-dispatch path.
- **Date:** 2026-07-10
- **Deciders:** chief-architect; consult interaction owner (U1 changes `FocusManager::dispatch_key_event`'s contract), repository owner (public API: `CallbackShortcuts`, `SingleActivator`, later `Shortcuts`/`Actions`/`Intent`), qa-lead (dispatch-order and modifier-matching tests).
- **Relates to:** builds on ADR-0022 (the `Focus` widget whose `on_key_event` this makes live); B1.1's `Actions`/`Shortcuts` line (`ROADMAP.md:217`); closes ADR-0022 §4's "bubbling up the ancestry is added when a widget needs interception, likely with `Shortcuts`".

---

## 1. Context — one dispatch gap, two disconnected channels

FLUI has **two per-node key-handler channels that do not meet**:

- the `FocusManager::key_handlers` map (`focus.rs:444`) — the only channel
  `dispatch_key_event` (`focus.rs:495-518`) consults. `EditableText` registers
  here (`editable_text.rs:161`).
- the `FocusNode::on_key_event` field (`focus_scope.rs:308`), read only by
  `FocusNode::handle_key_event` (`focus_scope.rs:422`) — **which has no
  non-test caller**. ADR-0022 U2's `Focus::on_key_event` builder writes this
  field, so the shipped builder is currently inert on the platform dispatch
  path. That is a U2 gap this ADR both records and closes (U1).

Dispatch today is flat: global handlers, then the focused node's map entry,
`bool` out, ancestors never consulted (`focus.rs:495-518`). Flutter's dispatch
is a **leaf→root walk** over `[primaryFocus, ...ancestors]` calling each node's
`onKeyEvent` and combining `KeyEventResult`s — `ignored` continues upward,
`handled` stops and consumes, `skipRemainingHandlers` stops without consuming
(`focus_manager.dart:2278-2302`, `combineKeyEventResults` `:98-110`). FLUI's
`KeyEventResult` enum already exists and is re-exported (`focus_scope.rs:97-105`)
but nothing produces `SkipRemainingHandlers` and dispatch ignores the type
entirely.

`Shortcuts` is, mechanically, nothing but a `Focus(canRequestFocus: false,
onKeyEvent: manager.handleKeypress)` wrapper (`shortcuts.dart:1134-1143`): a
matched activator resolves to an `Intent`, which is dispatched through
`Actions` **at the primary focus context**, not the `Shortcuts` context
(`shortcuts.dart:925`). So the ancestry walk is the entire delivery mechanism —
a `Shortcuts` above the focused field only ever sees a key because the field
*ignored* it and the event bubbled.

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/`, master `3.33.0-0.0.pre-6280-g88e87cd963f`:

- **Bubbling**: `focus_manager.dart:2222-2329` (`handleKeyMessage`); the walk at
  `:2278-2302`; any non-`ignored` result breaks the walk.
- **`Shortcuts`** (`shortcuts.dart:1004-1143`): `Map<ShortcutActivator, Intent>`,
  optional external `ShortcutManager`; `handleKeypress` (`:922-938`) returns the
  action's `toKeyEventResult`, else `ignored` (or `skipRemainingHandlers` when
  `modal`).
- **Activators**: `SingleActivator` (`:433-581`) — trigger key + four modifier
  booleans matched **exactly** (`:560-565`), `includeRepeats` default true, only
  down/repeat events. `LogicalKeySet` (`:288-322`) — exact pressed-set match.
  `CharacterActivator` (`:682-768`) — matches the produced character, ignores
  shift.
- **`CallbackShortcuts`** (`:1181-1231`): the Intent-free variant — bindings
  straight to callbacks, same `Focus(onKeyEvent:)` wrapper.
- **`Actions`** (`actions.dart:729`): `Map<Type, Action<Intent>>`;
  `maybeInvoke`/`invoke` walk `_ActionsScope` ancestors for the first scope
  whose map holds an *enabled* action for the intent's runtime type
  (`:1032-1044`); `ActionDispatcher.invokeAction` targets
  `context ?? primaryFocus?.context` (`:656-663`). `Action.toKeyEventResult`
  (`:312-314`): `consumesKey ? handled : skipRemainingHandlers`.
  `CallbackAction<T>` (`:606`), `DoNothingAction(consumesKey: false)`
  (`:1506-1517`).

## 3. The Rust shape

Four units, dependency-ordered. The FLUI-side primitives already on the shelf:
`KeyEventResult` (unused), `FocusNode::ancestors()`, the `keyboard_types`
`Modifiers` bitflags with `ctrl()/shift()/alt()/meta()` accessors, and
ADR-0022's `Focus::on_key_event`.

### U1 — bubbling key dispatch (`flui-interaction`)

`dispatch_key_event` becomes Flutter's walk:

1. Global handlers first (FLUI's existing "early handlers" analogue — kept).
2. If a primary focus exists: for each node in `[primary, ...ancestors()]`,
   combine that node's two channels — the `on_key_event` field
   (`handle_key_event`, which finally gains its caller) and the legacy
   `key_handlers` map entry (`bool` mapped to `Handled`/`Ignored`) — with
   Flutter's `combineKeyEventResults` semantics. `Ignored` → next ancestor;
   `Handled` → stop, event consumed; `SkipRemainingHandlers` → stop, **not**
   consumed (`focus_manager.dart:2288-2301`).
3. Return `bool` (consumed) as today; `AppBinding::handle_input` discards it
   either way (`binding.rs:937-940` — unchanged).

The map channel is kept, not deprecated: `EditableText` uses it and both
channels are per-node, so consulting both at each step is one `HashMap` lookup.
`FocusNode::handle_key_event` must also gain the `SkipRemainingHandlers`
pass-through it currently collapses (`focus_scope.rs:422-429` returns only
`Handled`/`Ignored`).

**This makes ADR-0022's `Focus::on_key_event` live.** Red-checks: a focused
child that returns `Handled` starves the ancestor's handler; one that returns
`Ignored` feeds it; `SkipRemainingHandlers` starves the ancestor *and* reports
unconsumed.

### U2 — `SingleActivator` + `CallbackShortcuts` (`flui-widgets`)

- `SingleActivator`: `new(key: impl Into<Key>)` plus `.control()`, `.shift()`,
  `.alt()`, `.meta()`, `.allow_repeats(bool)` builders. `matches(&KeyEvent)`
  is Flutter's `accepts` (`shortcuts.dart:576-581`): down-or-allowed-repeat,
  trigger equality, **exact** modifier match against the event's `Modifiers`
  bitflags (each of the four booleans must equal the event's flag).
  `LogicalKeySet` is *not* ported (its exact-pressed-set semantics need a
  `HardwareKeyboard`-style pressed-set tracker FLUI does not have — deferred,
  named). `CharacterActivator` waits for a consumer.
- `CallbackShortcuts` (`shortcuts.dart:1181`): `bindings:
  Vec<(SingleActivator, Arc<dyn Fn() + Send + Sync>)>`, built as
  `Focus::new(child).can_request_focus(false).on_key_event(…)` exactly as
  Flutter builds it (`:1225-1231`). First matching binding fires; handled iff
  one fired.

`CallbackShortcuts` ships first because it is genuine Flutter API with no
Intent layer, and it exercises U1 end-to-end (a shortcut above a focused
`TextField` fires only for keys the field ignored — pinned by test).

### U3 — `Intent` / `Action` / `Actions` (`flui-widgets`)

The typed layer. Rust shape for Flutter's `Map<Type, Action<Intent>>`:

- `trait Intent: Any + Send + Sync` (marker). `TypeId` is the map key, as
  Flutter's `Type` is. The `dyn Any` boundary here is the sanctioned FR-029 #4
  erasure category (port-check trigger #9 allowlists `Any`); the downcast
  happens inside the typed wrapper, never against a `View`.
- `Action<T: Intent>` as a trait (`is_enabled(&T) -> bool` default true,
  `invoke(&T)`, `consumes_key(&T) -> bool` default true) + `CallbackAction<T>`.
  Stored erased behind a private `ErasedAction` (monomorphized wrapper holding
  the `TypeId` and downcasting internally).
- `Actions` widget: an `InheritedView` scope (`_ActionsScope`) holding the map;
  `Actions::maybe_invoke::<T>(ctx, intent)` walks scopes via repeated ancestor
  lookups for the first enabled action — the `GestureArenaScope` pattern
  stacked, matching `_visitActionsAncestors` (`actions.dart:759-790`).
- Invocation context is the **primary focus** element, as Flutter's
  (`shortcuts.dart:925`): FLUI resolves it by walking from the focused node's
  registered element… **open question O-1**: FLUI's `FocusNode` does not
  record a `BuildContext`/element. Until it does, `Actions.maybe_invoke` takes
  the caller's `ctx`, and `Shortcuts` (U4) resolves actions from *its own*
  subtree instead of the focused leaf's. Divergence recorded up front; fixing
  it needs a node→element registry (a `Focus`-widget concern, cheap once
  wanted).

### U4 — `Shortcuts` (the Intent-mapped widget)

`Shortcuts { shortcuts: Vec<(SingleActivator, Box<dyn Intent>)>, child }`, a
`Focus(can_request_focus: false).on_key_event` wrapper whose handler finds the
first matching activator and hands its intent to `Actions` (per U3's O-1
resolution), returning the action's `to_key_event_result` — `Handled` for a
consuming action, `SkipRemainingHandlers` for a `consumes_key = false` one
(`actions.dart:312-314`), `Ignored` when nothing matched.

## 4. What is deliberately absent (all named)

- **`LogicalKeySet`** — needs a pressed-set tracker (`HardwareKeyboard`
  analogue); `SingleActivator` covers the catalog's real uses.
- **`CharacterActivator`** — waits for a consumer (IME/character-level
  shortcuts).
- **`ShortcutManager` as public API / `Shortcuts.manager`** — one manager per
  widget until someone needs to share one.
- **`ShortcutRegistrar`/`ShortcutRegistryEntry`**, **`Actions.handler`**,
  **`DoNothingAndStopPropagationTextIntent`-style text plumbing** — no
  consumers.
- **Early/late handler tiers** (`focus_manager.dart:2249-2273`, `:2305-2324`)
  — FLUI's global handlers stay a single pre-walk tier.
- **`includeSemantics`** — with the semantics layer, as in ADR-0022.

## 5. Consequences

**Good.** U1 is small (one function's body plus a `handle_key_event` fix),
repairs a shipped-but-inert ADR-0022 surface, and is exactly the Flutter
contract. Everything above it reuses proven patterns: ambient providers,
builder-configured widgets, `keyboard_types` matching primitives.

**Bad.** O-1 (no node→element mapping) means intent dispatch initially resolves
actions from the `Shortcuts` subtree, not the focused leaf's context — visible
only when an `Actions` map sits *between* the focused leaf and the `Shortcuts`
widget, which no current FLUI consumer does. It must be fixed before
`Actions`-dependent text editing intents land.

**Risk.** Changing `dispatch_key_event` order could regress `EditableText`
(global-then-focused today; global-then-walk tomorrow — the focused node is the
walk's first stop, so its behavior is unchanged unless an ancestor also
registers, which is the new feature). The U1 tests pin the old single-node
behavior alongside the new bubbling.
