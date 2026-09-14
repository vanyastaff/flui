# flui-material Architecture

Per-crate ledger for Material widgets and theming, as required by
[`docs/PORT.md`](../../docs/PORT.md) §Per-crate `ARCHITECTURE.md` template.
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
unrepresentable illegal states over `debug_assert!` alone — see
[`AGENTS.md`](AGENTS.md) Key constraints.

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
