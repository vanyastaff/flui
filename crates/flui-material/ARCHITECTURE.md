# flui-material Architecture

Per-crate ledger for Material widgets and theming.
Mapping decisions that span more than one module in this crate land here so
later parity work does not treat a deliberate divergence as accidental drift.
Module-level docs may repeat a local note and should cite this file when the
contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far; Flutter source mapping for the full catalog grows as widgets
are touched.

---

## Flutter source mapping

| Flutter source | FLUI module | Notes |
|---|---|---|
| `material/checkbox.dart` `Checkbox` | [`src/checkbox.rs`](src/checkbox.rs) | Dual constructors + private mode enum; see mapping decision below. |
| `material/tab_controller.dart` `TabController` / `DefaultTabController` | [`src/tab_controller.rs`](src/tab_controller.rs) | Release index bounds; see mapping decision below. |
| `material/tab_bar_view.dart` / `tabs.dart` `TabBar` | [`src/tab_bar_view.rs`](src/tab_bar_view.rs), [`src/tabs.rs`](src/tabs.rs) | Release children/tabs↔length agreement. |

---

## Mapping decisions

### Checkbox value/tristate is a private mode enum, not independent fields

**Rule:** Flutter's `Checkbox` takes `bool? value` plus `bool tristate` and
guards with a debug-only constructor assert
(`assert(tristate || value != null)` in `checkbox.dart`). In release the
illegal pair can paint an indeterminate dash while semantics report
unchecked / not mixed — an a11y contradiction. Rust's `Option<bool>` + `bool`
fields reproduce that hole unless the type forbids it.

**Choice:** FLUI stores a private `CheckboxMode::{Binary(bool),
Tristate(Option<bool>)}` and exposes two constructors:

- `Checkbox::new(bool)` — binary on/off; indeterminate is not representable
- `Checkbox::tristate(Option<bool>)` — third (`None`) value always allowed

There is no `.tristate(bool)` builder that could reintroduce the illegal pair.
Semantics flags are derived from the mode via `checkbox_semantics_flags`
(`Tristate(None)` → checked false + mixed; binary never sets mixed).

**Alternatives considered:**

- Release `assert!` in `build` alone (Flutter-shaped) — still admits the pair
  until first build; forgeable in-module; weaker than the type system.
- Public `CheckboxValue` enum on the API surface — stronger than needed for
  callers; dual constructors already close the cross-crate hole.
- Fallible constructor returning `Result` — worse ergonomics for a widget that
  can encode the invariant at compile time.

**Trade-off:** breaking vs Flutter's single constructor + `tristate:` named
arg. Call sites migrate `new(Some(x))` → `new(x)` and
`new(v).tristate(true)` → `tristate(v)`. Steady-state tap cycle, paint marks,
and AccessKit checked/mixed semantics stay aligned with the oracle for every
*legal* state.

**Replacement coverage:**

- Unit: `semantics_flags_agree_with_mode_so_paint_and_a11y_cannot_diverge`,
  constructor mode pins, `draws_the_correct_mark_per_tristate_value`
- Integration (`tests/checkbox.rs`): `indeterminate_tristate_exports_mixed_semantics`,
  `binary_checkbox_never_exports_mixed_semantics`,
  `tristate_some_values_export_checked_not_mixed`

Same public-widget invariant class as GitHub #1101 (tabs length/index):
caller-violable construction contracts ship in release, preferring
unrepresentable illegal states over `debug_assert!` alone.

### TabController length/index is enforced in release

**Rule:** Flutter's `TabController` / `TabBarView` use debug-only asserts for
`initialIndex` / `set_index` bounds and children↔length agreement
(`tab_controller.dart`, `tab_bar_view.dart`). In release, FLUI previously
could store an out-of-range index (listeners rebuild against impossible
state) or hide every `TabBarView` child when lengths disagreed.

**Choice:**

- Construction (`TabController::new`, `with_previous`,
  `DefaultTabController::initial_index`): release
  `assert_construction_tab_index` — `(length == 0 && index == 0) || index <
  length`. Stricter than Flutter's constructor assert when `length == 0`
  (Flutter allows any non-negative `initialIndex`; FLUI requires `0`).
- Mutation (`set_index` / `animate_to`): release `assert_set_index_in_range`
  matching Flutter `_changeIndex` — `index < length || length == 0`. When
  `length < 2`, the call remains a no-op after the check (including any
  index on a length-0 controller).
- `TabBarView::build` / `TabBar::build`: release-assert
  `tabs|children.len() == controller.length()` (empty `TabBar` requires
  `length == 0`).

Length shrink via `DefaultTabController` still clamps through
`recreate_for_length_change` before constructing the next controller.
`previous_index` may remain out of range after a shrink (Flutter
`_copyWithAndDispose`); only the live `index` is construction-validated.

A bounded `TabIndex` newtype was considered but deferred: tab count is
dynamic across controller identities, so type-level encoding alone does not
close `set_index(usize)` without also changing the public mutation API.
Release assert matches the AGENTS temporary fallback for caller-violable
`usize` contracts while the API stays Flutter-shaped.

**Trade-off / recovery paths:**

| Violation site | Release outcome |
|----------------|-----------------|
| `set_index` / `animate_to` from app or gesture code | Process panic (not under `build_or_recover`) |
| `TabBar` / `TabBarView` length mismatch inside `build` | Caught → framework `ErrorView` |
| Construction `new` / `initial_index` | Process panic at the call site |

Callers must keep controller length and `TabBar`/`TabBarView` lists in sync —
the same requirement the oracle documents, now non-optional. Sync-in-`build`
is stricter than Flutter's post-frame debug tab-count check.

**Replacement coverage:**

- Unit: `set_index_out_of_range_fails_the_release_invariant`,
  `set_index_on_a_zero_length_controller_is_a_no_op` (incl. nonzero index),
  `new_rejects_an_out_of_range_initial_index`,
  `default_tab_controller_initial_index_rejects_out_of_range`
- Integration: `a_children_count_mismatched_with_the_controllers_length_builds_an_error`,
  `a_tab_count_mismatched_with_the_controllers_length_builds_an_error`

### `Radio` publishes its group membership, and the role cascade has to prefer it

**Rule:** [`AGENTS.md`](../../AGENTS.md) Design stance ("Flutter is a reference, not a spec") — a behavior the
reference handles is dropped only by decision, recorded where a reader will find
it.

**Oracle:** `material/radio.dart` builds its UI through
`RawRadio<T>` (`radio.dart:553`), and `RawRadio` is where the accessibility
contract lives: `widgets/raw_radio.dart` wraps its child in
`Semantics(inMutuallyExclusiveGroup: true, checked: value, selected:
accessibilitySelected, hint: semanticsHint, child: buildToggleableWithChild(…))`,
and `widgets/toggleable.dart` adds the inner `Semantics(enabled: isInteractive)`
around the child. The `inMutuallyExclusiveGroup` flag is what separates a radio
from a checkbox to a screen reader: the two publish identical checked state, and
only the group flag says the checked state is one-of-a-set rather than
independent. `selected` and `hint` are `TargetPlatform`-conditional in the
oracle, computed two lines above the `Semantics` call — Android/Fuchsia/Linux/
Windows null both, iOS/macOS set `selected` to the value and supply a hint only
for an *unselected* radio (the selected state is announced by the platform
already, so a hint would duplicate it).

**Choice:** `Radio::build` publishes the same flag —
`.checked(selected).in_mutually_exclusive_group(true).enabled(interactive)` —
and `Semantics` grew the `in_mutually_exclusive_group(true)` builder this needed
(it previously had no such method, which is why the flag was unwired).

**Why the flag alone was not enough, and what else had to change.** Publishing
it exposed a defect one layer up, in the role cascade rather than in this
widget. `resolve_role` tests one flag before another to pick a single role out of
the union a node carries, and it tested `IsButton` before the checkable flags —
so a node that carried both resolved to `Button` and the radio-ness was
discarded. Checkable state is the more specific of the two and now wins.

The composition that reaches it is `Semantics::new().button(true).child(Radio…)`,
which merges onto one node: measured, that node resolves `Button` under the old
order and `RadioButton` under the new one, same node id. **It is not the
`ListTile` composition.** A real `ListTile` publishes `.enabled(..)` and a
`Radio` publishes `.enabled(..)` too, `is_compatible_with` treats that overlap as
a conflict, and the two therefore never merge — they form **separate** nodes.
Because they do not merge, the radio keeps a node of its own, and that node is
what this change fixes: measured through this crate's own `MediaQuery`-wrapped
fixture (`tests/list_tile.rs`'s `themed`), a `Radio` inside a real `ListTile`
exports `[GenericContainer, Button, RadioButton]` — **it does announce as a
radio**, and the group flag published here is what makes it. The cascade reorder
is not load-bearing there: the radio's own node carries no `IsButton`, so it
resolves `RadioButton` under either arm order. So the precedence neither fixes
nor is exercised by the tile composition, while publishing the flag *does* fix
it. The reference splits a bare tile the same way — by its predicate, traced
below; no reference test mounts that bare composition — and its one-node oracle
is a *merge*: `RadioListTile` wraps its `ListTile` in `MergeSemantics`
(`material/radio_list_tile.dart`, tag `3.44.0`), and that is what
`test/material/radio_list_tile_test.dart`'s `testWidgets('RadioListTile
semantics')` asserts as one node carrying `isButton` and the radio's own flags
together (the `isButton` there depends on the test passing
`internalAddSemanticForOnTap: true`). Its compatibility predicate is not the reason — `isCompatibleWith`
rejects actions on the same raw bit intersection FLUI uses and rejects flags
through `_flags.hasConflictingFlags(..)`, whose tristate `hasConflict` is
`both != none`, i.e. the same "both carry the trait" test as FLUI's bit
intersection for `hasEnabledState`, `isFocusable` and `hasCheckedState` — so the
reference's own pipeline gives the radio a `SemanticsNode` of its own under the
tile, and `MergeSemantics` folds it into the parent's data afterwards. The
like-for-like FLUI composition is therefore `MergeSemantics` over the tile, and
measured it exports **one** node resolving `RadioButton` — the reorder above is
what makes that merged node a radio (it resolves `Button` with the cascade
reverted). That composition is pinned in `tests/list_tile.rs` beside the bare
one. A comparison of a bare FLUI tile against a merged reference one is not a
divergence in `is_compatible_with`, and is not recorded as one. (`hasConflictingFlags` lives in the
engine's `lib/ui`, outside `.flutter`; its body was read from a local SDK at
framework `3.44.8` / engine `0cd6107`, not the pinned tag — provenance stated
because it is the one link that cannot be checked at `3.44.0`.) The precedence
itself is flui-semantics' decision and is recorded in full in
[`crates/flui-semantics/ARCHITECTURE.md`](../flui-semantics/ARCHITECTURE.md).

**What is still not wired, named rather than implied.** The oracle's
`selected` and `hint` fields are `TargetPlatform`-conditional
(`widgets/raw_radio.dart`) and FLUI's semantics surface has no platform
dimension, so neither is published — a deliberate drop of a
platform-dependent behavior, not an oversight.
`focus_node` / `autofocus` keep the whole-substrate `InkWell` gap `Checkbox` and
`Switch` already name. And **no `Radio` gains a semantics *action*** — publishing
the group flag changes what the control *is* to a screen reader, not what a
platform request can *do* to it. This is where FLUI is narrower than the
reference rather than equal to it: Flutter's `InkResponse` publishes
`Semantics(onTap: …)` itself, so the oracle's node carries
`actions: [tap, focus]` alongside those flags (`radio_list_tile_test.dart`, tag
`3.44.0`), and a screen reader can activate the control through the semantics
tree. In FLUI the widget-layer gesture callback is `Rc<dyn Fn(..)>` while
`SemanticsActionHandler` is `Arc<dyn Fn(..) + Send + Sync>`, so bridging the two
is a storage-and-lifetime decision that owes its own design record; until then
activation reaches the tree only through the pointer path. Naming this as a
present gap rather than reading "the `InkResponse` owns activation" as parity.

**Replacement coverage** (`tests/radio.rs`):

- `a_mounted_radio_announces_as_a_radio_button` /
  `a_mounted_radio_does_not_announce_as_a_checkbox` — the direct case, with the
  negative half asserted separately so a resolution that answered
  `RadioButton` for everything would not pass.
- `a_radio_nested_under_an_annotated_ancestor_still_announces_as_a_radio_button`
  — the absorbed case, and the only leg that can fail on the precedence:
  mounting the radio as the render *root* makes it form its own node, so nothing
  merges and the composition never arises.
- `a_radio_without_a_tap_handler_still_announces_as_a_radio_button` — kind and
  interactivity are independent; a missing `on_changed` does not make it
  something else.
- `a_checkbox_still_announces_as_a_checkbox` /
  `a_checkbox_never_announces_as_a_radio_button` (`tests/checkbox.rs`) — the
  other arm of the cascade. Publishing the group flag for radios must not leak
  onto the other checkables, which would announce every checkbox as a radio.
  Both pass before *and* after the reorder, since a checkbox carries neither
  `IsButton` nor the group flag: they guard the flag against leaking, not the
  cascade order.
- `a_radio_inside_a_list_tile_announces_as_a_radio_button` (`tests/list_tile.rs`)
  — the composition a user actually writes, and the only test here that mounts it
  through a `MediaQuery` ancestor. It asserts both roles the composition exports
  (`Button` from the tile's tap target, `RadioButton` from the radio's separate
  node) and reddens to `[GenericContainer, Button, CheckBox]` when the group flag
  is removed from `Radio::build`.

### `TextFormField` is a `FormField<String>` over `TextField`, and takes the user's edits from `on_changed`

**Oracle:** `material/text_form_field.dart` (tag `3.44.0`): a
`FormField<String>` whose builder returns a `TextField` with
`decoration.copyWith(errorText: field.errorText)`, a controller listener that
calls `didChange`, and a `reset` that writes `initialValue` back into the
controller.

**Choice:** the same composition, with two named differences. A field error
replaces the decoration's `error_text`, so it reaches `InputDecorator`'s error
line and the error caret colour exactly as a hand-set error does; with no
field error a caller-set `error_text` stays, as `copyWith(errorText: null)`
keeps it. The user's
edits come from `TextField::on_changed` rather than a controller listener,
because FLUI's controller listeners are `Send + Sync` and cannot reach the
owner-thread field state; the controller is read before the field validates or
saves (`flui_widgets::__private::TextFormFieldCore`, shared with
`RawTextFormField`; this type supplies only the Material input), so a caller's own controller edit is still validated and
saved but does not count as the user's interaction — see `flui-widgets`
mapping decision 31. `initialValue` and `controller` are two constructors
(`new`, `with_initial_value`) rather than an assert.

**Tests** (`tests/text_form_field.rs`):
`validator_error_reaches_the_input_decorator_error_line` (no "Required" is
rendered when the builder does not write the field's error into the
decoration) and `reset_restores_the_initial_value`.

---

## Thread safety

No locks. Material widgets are built and mutated on the UI realm thread;
shared interaction state uses `WidgetStatesController` / `Rc` callbacks as
elsewhere in the widget layer.

---

## Friction log

| Item | Notes |
|------|-------|
| Full Flutter source mapping table | Deferred; fill as individual widgets are re-touched. |
| Cupertino tab index still `debug_assert!` | `flui-cupertino` `CupertinoTabScaffold` — audit for the same policy when that surface is retouched. |
| Bounded `TabIndex` API | Optional follow-up if mutation ergonomics need fallible `try_set_index` without panic. |
