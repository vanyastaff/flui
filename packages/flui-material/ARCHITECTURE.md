# flui-material Architecture

Per-crate ledger for Material widgets and theming.
Design decisions that span more than one module in this crate land here.
Module-level docs may repeat a local note and should cite this file when the
contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far.

---

## Mapping decisions

### Catalog events forward the dispatch's write context

Press, selection and value-change setters accept `&mut EventCx` and an
`EventOutcome` result (ADR-0086). Composed controls pass the context they
receive from `InkWell` or the input widget; they do not open a new write
scope per wrapper. Query callbacks keep their return values and receive no
writer. `InkWell`'s keyboard activation is a `CallbackAction`, which runs
inside the key event's dispatch and receives its context (ADR-0086, amending
ADR-0023), so it opens no write scope of its own. Pointer activation forwards
the gesture's context. Both paths retain pressed-state-before-callback
ordering.

`FloatingActionButton::new(child).on_pressed(callback)` uses the same disabled
default and setter shape as the other buttons. An optional generic callback
in the constructor obstructed higher-ranked closure inference and forced
type annotations even for a disabled button. The setter gives the closure
its expected signature directly. **Unasserted:** no test pins this.

`tests/ink_well.rs::pointer_and_keyboard_activation_write_the_owning_signal`
and `tests/checkbox.rs::a_toggle_passes_its_value_and_writer_to_the_callback`
exercise the production dispatch, signal mutation and reader rebuild.

### Checkbox value/tristate is a private mode enum, not independent fields

**Rule:** An `Option<bool>` value paired with a `bool` tristate flag admits an
illegal pair (`tristate == false` with `value == None`). If only a debug assert
guards it, a release build can paint an indeterminate dash while semantics
report unchecked / not mixed — an a11y contradiction. The type has to forbid
the pair.

**Choice:** FLUI stores a private `CheckboxMode::{Binary(bool),
Tristate(Option<bool>)}` and exposes two constructors:

- `Checkbox::new(bool)` — binary on/off; indeterminate is not representable
- `Checkbox::tristate(Option<bool>)` — third (`None`) value always allowed

There is no `.tristate(bool)` builder that could reintroduce the illegal pair.
Semantics flags are derived from the mode via `checkbox_semantics_flags`
(`Tristate(None)` → checked false + mixed; binary never sets mixed).

**Alternatives considered:**

- Release `assert!` in `build` alone — still admits the pair
  until first build; forgeable in-module; weaker than the type system.
- Public `CheckboxValue` enum on the API surface — stronger than needed for
  callers; dual constructors already close the cross-crate hole.
- Fallible constructor returning `Result` — worse ergonomics for a widget that
  can encode the invariant at compile time.

**Trade-off:** a single constructor with a `tristate` flag is not offered.
Call sites migrate `new(Some(x))` → `new(x)` and
`new(v).tristate(true)` → `tristate(v)`. The steady-state tap cycle, paint
marks and AccessKit checked/mixed semantics agree for every legal state.

**Replacement coverage:**

- Integration (`tests/checkbox.rs`): `indeterminate_tristate_exports_mixed_semantics`
  (the `None` state exports mixed). The paint mark for each state and a binary
  checkbox never exporting mixed are **Unasserted:** no test pins this.

Same public-widget invariant class as GitHub #1101 (tabs length/index):
caller-violable construction contracts ship in release, preferring
unrepresentable illegal states over `debug_assert!` alone.

### TabController length/index is enforced in release

**Rule:** Debug-only asserts on `initialIndex` / `set_index` bounds and
children↔length agreement are not enough. In release, `TabController` could
store an out-of-range index (listeners rebuild against impossible state) or
`TabBarView` could hide every child when lengths disagreed.

**Choice:**

- Construction (`TabController::new`, `with_previous`,
  `DefaultTabController::initial_index`): release
  `assert_construction_tab_index` — `(length == 0 && index == 0) || index <
  length`. With `length == 0` only `initialIndex == 0` is accepted.
- Mutation (`set_index` / `animate_to`): release `assert_set_index_in_range`
  — `index < length || length == 0`. When
  `length < 2`, the call remains a no-op after the check (including any
  index on a length-0 controller).
- `TabBarView::build` / `TabBar::build`: release-assert
  `tabs|children.len() == controller.length()` (empty `TabBar` requires
  `length == 0`).

Length shrink via `DefaultTabController` still clamps through
`recreate_for_length_change` before constructing the next controller.
`previous_index` may remain out of range after a shrink; only the live
`index` is construction-validated.

A bounded `TabIndex` newtype was considered but deferred: tab count is
dynamic across controller identities, so type-level encoding alone does not
close `set_index(usize)` without also changing the public mutation API.
Release assert matches the AGENTS temporary fallback for caller-violable
`usize` contracts.

**Trade-off / recovery paths:**

| Violation site | Release outcome |
|----------------|-----------------|
| `set_index` / `animate_to` from app or gesture code | Process panic (not under `build_or_recover`) |
| `TabBar` / `TabBarView` length mismatch inside `build` | Caught → framework `ErrorView` |
| Construction `new` / `initial_index` | Process panic at the call site |

Callers must keep controller length and `TabBar`/`TabBarView` lists in sync —
a requirement that is now enforced in release, checked in `build`.

**Unasserted:** no test pins this.

### `Radio` publishes its group membership, and the role cascade has to prefer it

**Rule:** a behavior the platform accessibility contract requires is dropped
only by decision, recorded where a reader will find it.

**Why:** the mutually-exclusive-group flag is what separates a radio from a
checkbox to a screen reader: the two publish identical checked state, and only
the group flag says the checked state is one-of-a-set rather than independent.

**Choice:** `Radio::build` publishes
`.checked(selected).in_mutually_exclusive_group(true).enabled(interactive)`,
and `Semantics` grew the `in_mutually_exclusive_group(true)` builder this
needed (it previously had no such method, which is why the flag was unwired).

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
it.

A single node for a tile plus its radio is a *merge*: wrapping the `ListTile` in
`MergeSemantics` folds the radio's own node into the parent's data, and
measured it exports **one** node resolving `RadioButton` — the reorder above is
what makes that merged node a radio (it resolves `Button` with the cascade
reverted). That composition is pinned in `tests/list_tile.rs`. The precedence
itself is flui-semantics' decision and is recorded in full in
[`crates/flui-semantics/ARCHITECTURE.md`](../../crates/flui-semantics/ARCHITECTURE.md).

**What is still not wired, named rather than implied.** Platform-conditional
`selected` and `hint` fields are not published: FLUI's semantics surface has no
platform dimension, so this is a deliberate drop of a platform-dependent
behavior, not an oversight.
`focus_node` / `autofocus` keep the whole-substrate `InkWell` gap `Checkbox` and
`Switch` already name. And **no `Radio` gains a semantics *action*** — publishing
the group flag changes what the control *is* to a screen reader, not what a
platform request can *do* to it. `InkResponse` does not publish a tap semantics
action, so a screen reader cannot activate the control through the semantics
tree. In FLUI the widget-layer gesture callback is `Rc<dyn Fn(..)>` while
`SemanticsActionHandler` is `Arc<dyn Fn(..) + Send + Sync>`, so bridging the two
is a storage-and-lifetime decision that owes its own design record; until then
activation reaches the tree only through the pointer path.

**Replacement coverage:**

- `a_mounted_radio_announces_as_a_radio_button` (`tests/radio.rs`) — the direct
  case.
- `merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button`
  (`tests/list_tile.rs`) — the absorbed case, and the only leg that can fail on
  the precedence: the merged node carries the tile's `IsButton` together with
  the radio's checked and group flags, and resolves `Button` with the cascade
  reverted. Mounting the radio as the render *root* makes it form its own node,
  so nothing merges and the composition never arises.

### `TextFormField` is a `FormField<String>` over `TextField`, and takes the user's edits from `on_changed`

**Choice:** a `FormField<String>` whose builder returns a `TextField` with the
field's error written into the decoration, and a `reset` that writes
`initialValue` back into the controller. A field error
replaces the decoration's `error_text`, so it reaches `InputDecorator`'s error
line and the error caret colour exactly as a hand-set error does; with no
field error a caller-set `error_text` stays. The user's
edits come from `TextField::on_changed` rather than a controller listener,
because FLUI's controller listeners are `Send + Sync` and cannot reach the
owner-thread field state; the controller is read before the field validates or
saves (`flui_sdk::widgets::__private::TextFormFieldCore`, shared with
`RawTextFormField`; this type supplies only the Material input), so a caller's own controller edit is still validated and
saved but does not count as the user's interaction — see `flui-widgets`
mapping decision 31. `initialValue` and `controller` are two constructors
(`new`, `with_initial_value`) rather than an assert.

**Tests** (`tests/text_form_field.rs`):
`validator_error_reaches_the_input_decorator_error_line` (no "Required" is
rendered when the builder does not write the field's error into the
decoration).

---

## Thread safety

No locks. Material widgets are built and mutated on the UI realm thread;
shared interaction state uses `WidgetStatesController` / `Rc` callbacks as
elsewhere in the widget layer.

---

## Friction log

| Item | Notes |
|------|-------|
| Cupertino tab index still `debug_assert!` | `flui-cupertino` `CupertinoTabScaffold` — audit for the same policy when that surface is retouched. |
| Bounded `TabIndex` API | Optional follow-up if mutation ergonomics need fallible `try_set_index` without panic. |

### NavigationBar configuration bounds hold in every profile

`NavigationBar` requires at least two destinations and a selected index within
that immutable list. Construction and the selection setter reject violations
in release as well as debug; no bar can publish a selection outside its own
list. These are the existing programmer-error contracts, now enforced where
the configuration is constructed. There is no mutable controller or externally
replaceable list on this widget, so adding a public count/index type would add
a conversion without removing another source of count disagreement.

`navigation_and_layout_contracts` includes the public rows
`an_empty_destination_list_is_rejected`, `a_single_destination_is_rejected`,
`an_index_at_the_destination_count_is_rejected` and
`an_unrepresentable_destination_index_is_rejected`; pointer delivery and disabled
destinations remain covered in that family.

### SnackBarAction claims activation before invoking application code

The one-shot claim belongs to the retained action state. Its installed
callback claims the shared `Cell` at event time, so two complete pointer
contacts before a rebuild cannot execute the action twice. It schedules the
disabled configuration before invoking application code. A callback panic
propagates with the action still claimed; dismissal follows only a successful
callback. A newly mounted action starts with a fresh claim.

The `overlay_contracts` rows
`tests/snack_bar.rs::action_press_closes_the_snack_bar_and_is_single_fire`
and `action_callback_panic_disables_the_button_and_fresh_action_progresses`
exercise consecutive contacts before a frame, successful dismissal, disabled
button semantics after an ordinary callback panic, and fresh-action input.
The pointer recovery row observes the resulting disabled button; it does not
isolate scheduling order because `InkWell` also schedules pressed-state
updates before invoking the action.
