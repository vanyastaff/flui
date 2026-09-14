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
| TabController length/index release invariants (#1101) | Open; same policy class as the Checkbox decision above. |
