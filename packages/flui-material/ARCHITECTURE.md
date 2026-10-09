# flui-material Architecture

Per-crate ledger for Material widgets and theming.
Design decisions that span more than one module in this crate land here.
Module-level docs may repeat a local note and should cite this file when the
contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far.

---

## Mapping decisions

### SnackBar hover pauses only the addressed display timer

Each mounted presenter holds a hover lease for its exact queued entry.
The lease counts hovered presenters, so one scaffold's exit cannot resume a
timer another scaffold still holds. The entry and messenger are weakly
addressed; an old entry's callbacks cannot change the next entry's timer.
Hover uses the controller's playback rate and retains the display run and its
completion. Replacing or disposing a presenter withdraws its hover admission
before retiring its captures; retired callbacks cannot acquire it again.
Playback-rate changes request their sample through the timer's registry seat;
hover does not rebuild the tree merely to wake that sample.

`snack_bar_display_timer_pauses_while_hovered` drives real pointer hover and
observes the preserved remainder and one Timeout completion.
`unmounting_a_hovered_snack_bar_releases_its_timer_pause` keeps the messenger
and timer mounted while removing the Scaffold, then observes completion.

### Contrast selects authored palettes within the effective brightness family

`MaterialApp` selects light/dark using `ThemeMode` and, for System mode, the
nearest media brightness. Contrast remains independent of an explicit mode.
The selected family's optional contrast theme wins when requested. Missing
contrast themes retain ordinary selection: dark theme, then base theme, then
default; light selection uses base theme, then default. A light contrast theme
is never substituted for a missing dark contrast theme. Authored colors are
not automatically transformed.

`contrast_selects_authored_themes_in_a_retained_app` exercises mode, ambient
brightness, missing slots, live restoration and unrelated size updates.

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

No locks. Material widgets are built and mutated on the UI UI runtime thread;
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

### Drawer release uses admitted gesture policy

Panel translation and scrim opacity consume the controller through
`SlideTransition` and `FadeTransition`. Value ticks update retained render
layers; only status changes request a controller rebuild to mount or remove
the open surface. `drawer_slides_without_rebuilding_per_frame` dispatches a
real edge drag, observes movement and effective scrim alpha in the committed
scene, and checks the frame build report on opening and closing frames for
both edges. Its frames do not dirty the logical root. It also checks panel
tap delivery and scrim dismissal.

One gesture owner survives the edge strip opening into the panel. It changes
only its child and hit extent, preserving the captured contact through rendered
frames. `a_drawer_release_keeps_finger_speed` pumps between moves and measures
painted position and release speed on both sides, including a viewport narrower
than the authored panel. The common `fling_across` admission converts velocity
using that extent (ADR-0182).

Both the closed edge strip and open panel settle from the admitted signed
horizontal component of `DragEndDetails::fling_velocity()` (ADR-0172), before
normalizing by the actual panel width. The drawer's authored fling threshold
and position-based settling remain independent of the contact's minimum and
maximum. `drawer_settling_uses_the_captured_fling_profile` and
`open_drawer_settling_uses_the_captured_fling_profile` observe settled panel
geometry, retained active policy and fresh-contact recovery.

### Drawer cancellation settles without release momentum

Drawer edge and panel consumers use measured velocity only for
`GestureEndReason::Completed`. For `Cancelled` they settle with zero velocity,
choosing the resting endpoint from the current position, as the panel's
preacceptance cancellation already does. Cancellation cannot turn a short
fast movement into a fling toward the opposite endpoint.

The `overlay_contracts` rows
`tests/drawer.rs::cancelled_fast_edge_drag_settles_closed_below_halfway`
and `cancelled_fast_panel_drag_settles_open_above_halfway` dispatch pointer
cancellation, tick the settle animation and check both the public handle and
the mounted scrim, then complete a fresh gesture to the opposite endpoint.
The existing
`a_fast_release_below_halfway_flings_the_drawer_open_rather_than_snapping_shut`
row preserves the ordinary-release velocity contract.

### Data-table checkbox spacing consumes the resolved theme cascade

The presence of a custom checkbox margin is resolved together with its value:
the widget override wins over the theme, and either tier gives the first data
column its full horizontal padding. Only the absence of both tiers selects
the default half-padding beside the checkbox. Heading and data cells consume
the same resolved choice.

The `selection_control_contracts` rows
`themed_checkbox_margin_matches_the_same_widget_margin` and
`checkbox_margin_override_beats_the_theme_without_changing_default_spacing`
compare actual mounted heading/data text insets and checkbox-column widths;
they retain widget precedence and the unchanged default-spacing control.


### Tab-bar height follows actual tab overrides

A nonempty secondary tab bar allocates the largest requested content height
plus its indicator band. The default tab height is the empty-bar fallback,
not a lower bound on explicit smaller overrides. Preferred size and the
mounted layout consume the same calculation. Mixed default/override tabs and
larger overrides keep their existing maximum-height behavior.

The `navigation_and_layout_contracts` rows
`small_tab_height_overrides_determine_the_mounted_bar_height` and
`mixed_and_empty_tab_bars_keep_their_content_height_rules` observe preferred
size and actual loose-parent layout, with bottom indicator-band coordinates.


### Completion failure does not strand the accepted snack-bar queue

After a dismissed entry is popped, its completion, the next entrance and
scaffold rebuild delivery are independent attempts. The earliest caught failure
propagates after the advancement guard is released; secondary opaque payloads
are retained. Scaffold rebuild handles are snapshotted before host callbacks,
so those callbacks cannot run under the registration RefCell borrow. A caught
failure retains the completed entry and failed-delivery handles rather than
running opaque teardown in competition with the authoritative failure.

This does not contain a callback body's panic competing with its consumed
`FnOnce` captures' destruction before control returns, or a double panic during
ordinary aggregate retirement. Display-timer cancellation before the pop retains
its separate existing failure boundary.

`overlay_contracts` row
`a_completion_panic_still_advances_the_accepted_snack_bar_queue` observes the
original ordinary completion panic, the next accepted bar's mounted entrance
and eventual Timeout on virtual frames, then a fresh show/remove operation.

The private `a_panicking_completion_does_not_lock_future_queue_operations`
family separately injects completion, entrance-listener and scaffold-delivery
failures, individually and in chronological competition. Lifecycle-acquired
rebuild probes have distinct mounted owner inboxes so each fanout wake is
observable. Their private map registration is a delivery fault seam, not
supported cross-UI runtime Messenger topology. Rows check the first failure, every
eligible delivery, actual subsequent rebuilds and accepted queue progress.
Four bounded children add hostile secondary payloads with several panicking
destructors and assert that none retire before or after recovery. Readiness is
published after mounting; setup and operation have separate deadlines.


### Drawer drag extent follows its constrained declared panel width

The standard Drawer enforces its configured width under finite loose Align
constraints. DrawerController uses an existing LayoutBuilder to cap its drag
and velocity divisor by the incoming maximum width; the tight slot minimum
must not expand a smaller declared panel. Before layout its configured extent
remains the fallback. Generic custom children must declare matching panel_width;
this is not a descendant-size measurement API. A collapsed or unusable extent
ignores movement and settles by position without dividing velocity by it.

The existing drawer family rows
`narrow_start_drawer_cancel_uses_its_actual_panel_extent` and
`narrow_end_drawer_cancel_uses_its_actual_panel_extent` use real cancelled
contacts to cross half of a 100-pixel mounted panel, then reverse-close and
reopen it. Actual material geometry pins the constrained panel width.
`smaller_configured_drawer_keeps_its_declared_panel_extent` and
`ordinary_drawer_keeps_its_configured_extent_in_a_wider_viewport` preserve
50-in-100 and 304-in-400 behavior.
`retained_drawer_recomputes_its_extent_after_a_collapsed_resize` keeps a contact
across a zero-width resize, then uses the retained handle and a new contact
after resizing to 100 pixels.
