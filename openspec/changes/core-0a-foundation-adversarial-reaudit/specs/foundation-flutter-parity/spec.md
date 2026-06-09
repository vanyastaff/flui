# Foundation Flutter Parity Specification

## Purpose

Pin the requirements for behavioural and diagnostic-output parity between
`flui-foundation` and the Flutter reference implementation.  This spec covers
four D5-dimension findings: the removed-during-notify semantic divergence (F5),
the surprising `Default for ValueNotifier<T>` that cycle-3 I-5 missed (F11),
the shallow `DiagnosticsProperty` type system that loses type-discriminator
information (F12), and the fully-qualified `type_name` in `to_diagnostics_node`
(F27).

Deferred findings (F10 `MergedListenable`, F13 `ObserverList` de-dup
documentation, F25 cascade disposal order) are outside the scope of this change;
do not spec them here.

Owner crates: `crates/flui-foundation` (`notifier.rs`, `debug.rs`).

Flutter reference: `.flutter/packages/flutter/lib/src/foundation/` (read-only).

---

## Requirements

### Requirement: Removed-during-notify listener MUST NOT fire (F5) [PRIMARY]

When a `ChangeNotifier` listener is removed via `remove_listener(id)` during a
`notify_listeners` iteration, the removed listener MUST NOT be invoked for the
remainder of the current notification pass.

**Fix shape:** The `notify_listeners` snapshot MUST include `(ListenerId,
ListenerCallback)` pairs.  Before invoking each callback in the snapshot loop,
the implementation MUST re-check `self.listeners.lock().contains_key(&id)`.  If
the key is absent (listener was removed after the snapshot was taken), the
callback MUST be skipped.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:480-498` — `removeListener` sets the slot to `null` when `_notificationCallStackDepth > 0`; the `notifyListeners` loop tests `_listeners[i]?.call()` and skips null slots.

**Rust-native breaking change:**
- (a) Previously, FLUI fired a listener that had been removed during the notify
  pass (the snapshot had already cloned the `Arc` before removal, so the callback
  was invoked regardless).
- (b) The Rust-native snapshot-with-ID-recheck matches Flutter's null-slot skip.
  A listener that removes itself or another listener expects Flutter semantics:
  "once removed, the removed listener does not fire for this pass."
- (c) Downstream consumers: `flui-animation` (8 files per cycle-3 audit) and any
  code in `flui-view` / `flui-interaction` that calls `remove_listener` inside a
  listener callback MUST be audited before this task lands to confirm they do not
  rely on "fires once more then is gone" semantics.  A workspace-wide grep for
  `remove_listener` inside a listener closure MUST be performed as part of the
  design phase.

**Acceptance criterion:** SC5 — `cargo test -p flui-foundation
removed_listener_does_not_fire_during_notify` exits 0.

Cross-referenced in: `foundation-concurrency/spec.md` (D2 ordering dimension),
`foundation-soundness/spec.md` (interaction with F6 catch_unwind isolation).

#### Scenario: Self-removing listener does not fire in the same pass (SC5)

- GIVEN a ChangeNotifier with listener A and listener B registered in that order
- GIVEN listener A's callback calls `notifier.remove_listener(listener_b_id)` as
  its body
- WHEN `notify_listeners` is called
- THEN listener A fires (it is processed before the removal happens)
- AND listener B does NOT fire (it was removed before its turn; the ID recheck
  finds the key absent)

#### Scenario: Listener removed from an outer scope does not fire

- GIVEN a ChangeNotifier with listener X registered
- GIVEN a concurrent or nested path that calls `remove_listener(x_id)` before
  `notify_listeners` processes listener X's slot
- WHEN `notify_listeners` is called (or continues to listener X's slot)
- THEN listener X's callback is NOT invoked

#### Scenario: Listeners NOT removed during notify still fire

- GIVEN a ChangeNotifier with 3 listeners, none of which removes itself or any
  other listener
- WHEN `notify_listeners` is called
- THEN all 3 listeners fire (the recheck finds their keys present each time)

---

### Requirement: ValueNotifier MUST NOT implement Default (F11) [PRIMARY]

`impl Default for ValueNotifier<T>` MUST be deleted from
`crates/flui-foundation/src/notifier.rs`.

**Rationale:** Cycle-3 I-5 deleted `Default for Key` and `Default for UniqueKey`
on the grounds that "every call returns a different value — violates
least-surprise for Default."  The identical argument applies to
`ValueNotifier<T>`: every `ValueNotifier::<T>::default()` call creates a NEW
`ChangeNotifier` with its own fresh listener set and `is_disposed = false`.  The
compounding problem is `PartialEq for ValueNotifier<T>` which compares only
`self.value`, not notifier identity.  Two `Default`-constructed
`ValueNotifier<i32>` with value `0` compare `==` but have independent notifier
states — a listener added to one will not fire on the other's `notify`.  Cycle-3
I-5 swept only `Key` and `UniqueKey` by name; `ValueNotifier` was missed.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/change_notifier.dart:505-520` — Flutter's `ValueNotifier<T>` has no default factory; `ValueNotifier(initialValue)` always takes an explicit initial value.

**Rust-native breaking change:**
- (a) Removes `impl Default for ValueNotifier<T>` from the public API.
- (b) Callers that use `#[derive(Default)]` on a struct containing
  `ValueNotifier<T>` fields will fail to compile.  Migration: replace the
  derived `Default` with an explicit `impl Default` or constructor that calls
  `ValueNotifier::new(T::default())`.
- (c) Primary blast radius: `flui-animation` (8 files per cycle-3 reference).
  A workspace-wide grep for `ValueNotifier` in struct fields with `#[derive(Default)]`
  MUST be performed in the design phase.

**Acceptance criterion:** SC6 — `! grep -n "impl.*Default.*ValueNotifier"
crates/flui-foundation/src/notifier.rs` exits 0.

#### Scenario: ValueNotifier::default() no longer compiles (SC6 complement)

- GIVEN `crates/flui-foundation/src/notifier.rs` at HEAD (after change)
- WHEN a test attempts to write `let v = ValueNotifier::<i32>::default()`
- THEN the compiler emits an error: "the trait `Default` is not implemented for
  `ValueNotifier<i32>`"

#### Scenario: Explicit ValueNotifier::new() is the only construction path

- GIVEN any struct that previously used `#[derive(Default)]` with a
  `ValueNotifier<T>` field
- WHEN the derive is replaced with an explicit constructor calling
  `ValueNotifier::new(T::default())`
- WHEN `cargo check --workspace --all-targets` is run
- THEN it exits with code 0

#### Scenario: Default impl absent from notifier.rs (SC6)

- GIVEN `crates/flui-foundation/src/notifier.rs` at HEAD
- WHEN `grep -n "impl.*Default.*ValueNotifier"
  crates/flui-foundation/src/notifier.rs` is run
- THEN it exits with code 1 (no matches)

---

### Requirement: DiagnosticsProperty MUST have typed variant discriminators (F12) [PRIMARY]

`crates/flui-foundation/src/debug.rs` MUST expose a `DiagnosticsPropertyKind`
enum with at least the following variants:

| Variant | Semantics | Flutter equivalent |
|---|---|---|
| `Generic { value: String }` | Catch-all; forwards `Display` value | Generic `DiagnosticsProperty<T>` |
| `Enum { type_name: &'static str, variant: &'static str }` | Renders variant name without module path | `EnumProperty<T>` |
| `Flag { active: bool, if_true: &'static str }` | Renders as `name` (true) or `-` (false) | `FlagProperty` |
| `Iterable { count: usize, summary: String }` | Renders with count prefix | `IterableProperty<T>` |
| `OptionalRef { present: bool }` | Renders as `present` / `<none>` | `ObjectFlagProperty<T>` |
| `Stack(Vec<String>)` | Renders as pretty-printed stack trace | `DiagnosticsStackTrace` |

The existing `DiagnosticsProperty::new(name, value)` constructor MUST remain as
a forwarding constructor to the `Generic` variant — no existing caller breaks.

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/diagnostics.dart` — `EnumProperty<T>`, `FlagProperty`, `IterableProperty<T>`, `ObjectFlagProperty<T>`, `MessageProperty`, `StringProperty`, `IntProperty`, `DoubleProperty`, `PercentProperty`, `DiagnosticsStackTrace`, `DiagnosticsBlock`.

**Why this matters:** Without typed variants, a `RenderFlex.direction = Axis.Horizontal` property renders identically to a plain string `"Axis::Horizontal"` — devtools loses the ability to distinguish enum properties from string properties, flag properties from boolean properties, etc.

**Acceptance criterion:** SC13 — `grep -n "DiagnosticsPropertyKind"
crates/flui-foundation/src/debug.rs` exits 0.

#### Scenario: DiagnosticsPropertyKind enum exists with required variants (SC13)

- GIVEN `crates/flui-foundation/src/debug.rs` at HEAD
- WHEN `grep -n "DiagnosticsPropertyKind" crates/flui-foundation/src/debug.rs`
  is run
- THEN it exits with code 0
- AND the output lists at least the 6 required variants

#### Scenario: Generic variant preserves existing constructor API

- GIVEN existing code calling `DiagnosticsProperty::new("direction",
  &self.direction)`
- WHEN compiled against the updated `debug.rs`
- THEN it compiles without modification (the constructor forwards to `Generic`)

#### Scenario: Enum variant renders without module path

- GIVEN `DiagnosticsPropertyKind::Enum { type_name: "Axis", variant:
  "Horizontal" }`
- WHEN rendered via `DiagnosticsProperty::format_with_style` (or equivalent)
- THEN the output contains `"Horizontal"` and does NOT contain
  `"flui_types::axis::Axis::Horizontal"` or any fully-qualified path

#### Scenario: Flag variant renders conditional name vs dash

- GIVEN `DiagnosticsPropertyKind::Flag { active: true, if_true: "expanded" }`
- WHEN rendered
- THEN the output contains `"expanded"`
- GIVEN the same with `active: false`
- WHEN rendered
- THEN the output contains `"-"` (or equivalent absent-indicator)

---

### Requirement: to_diagnostics_node MUST strip module path from type_name (F27) [PRIMARY]

`Diagnosticable::to_diagnostics_node` in `crates/flui-foundation/src/debug.rs`
MUST derive the `DiagnosticsNode` name from the simple (unqualified) type name,
NOT from `std::any::type_name::<Self>()` directly.

Required fix (mirrors `Id::fmt` at `crates/flui-foundation/src/id.rs:312`):
```rust
let full_name = std::any::type_name::<Self>();
let simple_name = full_name.rsplit("::").next().unwrap_or(full_name);
let mut node = DiagnosticsNode::new(simple_name);
```

**Flutter ref:** `.flutter/packages/flutter/lib/src/foundation/diagnostics.dart:1920-1930` — Flutter's `runtimeType.toString()` returns the simple class name (`"RenderPadding"`, not `"package:flutter/src/rendering/proxy_box.dart:RenderPadding"`).

**Rust-native breaking change:**
- (a) devtools and inspector output changes: fully-qualified names become simple
  names (e.g. `"flui_rendering::objects::render_padding::RenderPadding"` →
  `"RenderPadding"`).
- (b) Simple names match Flutter's DevTools output format; they are easier to
  read and search.
- (c) Tests that string-match the full module path in `DiagnosticsNode.name`
  will break.  This is intentional — breaking changes are explicitly allowed per
  project lead mandate.

#### Scenario: to_diagnostics_node uses simple name

- GIVEN a type `flui_rendering::objects::render_padding::RenderPadding`
  implementing `Diagnosticable`
- WHEN `render_padding_instance.to_diagnostics_node()` is called
- THEN `node.name` equals `"RenderPadding"` NOT
  `"flui_rendering::objects::render_padding::RenderPadding"`

#### Scenario: rsplit pattern is consistent with Id::fmt

- GIVEN any type `T` whose fully-qualified name follows the pattern
  `"a::b::c::TypeName"`
- WHEN `std::any::type_name::<T>().rsplit("::").next()` is evaluated
- THEN it returns `"TypeName"` (consistent with the strip in `Id::fmt`)

#### Scenario: Types without module separators are unchanged

- GIVEN a type whose `type_name` returns a bare name (no `::`), e.g. `"MyType"`
- WHEN `rsplit("::").next().unwrap_or(full_name)` is applied
- THEN it returns `"MyType"` unchanged (the `unwrap_or` fallback is correct)

---

## Cross-references

### F6 — notify_listeners listener-panic isolation (D5 parity aspect)

The primary requirement for `catch_unwind`-based listener isolation is specified
in `foundation-soundness/spec.md § Requirement: notify_listeners MUST isolate
each listener with catch_unwind`.  The D5 parity dimension is: Flutter's
`notifyListeners` wraps each listener in `try/catch` and emits
`FlutterError.reportError`; FLUI MUST match this contract.
