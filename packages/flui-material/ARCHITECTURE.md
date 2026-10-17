# flui-material Architecture

Per-crate ledger for Material widgets and theming.
Design decisions that span more than one module in this crate land here.
Module-level docs may repeat a local note and should cite this file when the
contract is shared.

Adopted incrementally: sections below cover the decisions this crate has
recorded so far.

---

## Mapping decisions

### `InkWell` owns its ripple; no ink-feature registry

Each `InkWell` owns its feedback (Compose's `Indication` model): a state layer
whose alpha animates toward `StateLayerAlpha` for the strongest active state,
and a short queue of waves, painted by a background `CustomPaint` under the
child and clipped to the well's shape. There is no registry on an ancestor
`Material`, so ink never bleeds into a neighbour. While ripples run, the wave
carries the pressed alpha and the state layer leaves `Pressed` out; reduced
motion keeps the state layer (with `Pressed`) and grows no wave. Colours come
from the well's `overlay_color` (alpha included), else the ambient
`RippleConfiguration` colour or the theme's `on_surface` at the token alpha; a
`RippleScope` without a configuration turns both parts off. Waves follow the
raw contact through a `Listener`, so a press shows before the tap is
recognised; `GestureDetector::on_tap_cancel` fades a contact that stops being
a tap. The 100 ms `Pressed` window stays on the tap. The state layer drops
`Pressed` only while a wave carries it, and shows a held contact as pressed
when no wave runs (reduced motion). An idle well attaches no painter, so it
records nothing.

The `ink_feedback_contracts` table pins the tokens, the wave's radius and fade
timing (a release waits out the 75 ms fade-in; a cancel or a drag past the
slop fades at once), clipping between adjacent wells, finite geometry for a
huge well, a press without a wave, reduced motion and a disabled
configuration through the recorded display list.

### Drawer admission follows position and run ownership

A drawer is closed when its normalized progress is zero and it owns no active
run. Directional settlement alone does not prove an endpoint: grabbing a closing
panel stops its reverse run at the current interior position. The panel and scrim
remain mounted, cancellation settles from position, and starting an opening run
at zero mounts before the first value sample. Render transitions still publish
ordinary value ticks without rebuilding the widget each frame.

`grabbing_a_closing_drawer_preserves_its_panel` checks both alignments through
public pointer input and committed scene geometry, with cancellation and release
on both sides of halfway, dragging to zero, mounting an opening run at zero,
a fresh closing gesture and unmount withdrawal. The existing
`drawer_slides_without_rebuilding_per_frame` pins retained frame publication.

### Replacing a display timer retires its outgoing owner

The messenger installs and starts the new display owner before retiring the
outgoing one. Retirement withdraws its registry seat, cancels its run and
releases status captures outside state borrows. Saved controller observers
cannot revive the retired timer or deliver its old timeout.

`a_second_display_timer_retires_the_first` exercises the existing mounted
Messenger fault seam in the completion-recovery table. It delivers a second
entrance completion without an intervening timer cancellation, observes
registry reentry from cancellation, capture retirement, one replacement
timeout and unmount withdrawal. This replacement seam is private: public
queue advancement cancels before the next entrance. The public
`snack_bar_keeps_its_display_duration_under_reduced_motion` separately checks
the mounted presentation's visible interval and Timeout delivery.

### Typography presets retain explicit numeric growth profiles

Default typography preserves authored font sizes, weights, spacing and line
height. Its sizing intent selects a growth profile independently of those values
(ADR-0183). This is FLUI's projection of its presets, not a Material scaling
formula or a promise of native custom-font identity.

| Preset | Numeric growth profile |
|---|---|
| display large, medium, small | LargeTitle |
| headline large, medium, small | Title1, Title2, Title3 |
| title large, medium, small | Title3, Headline, Subheadline |
| label large, medium, small | Callout, Caption1, Caption2 |
| body large, medium, small | Body, Body, Footnote |

The public `retained_theme_roles_paint_distinct_numeric_sizing_answers` row
uses mounted theme styles with the same authored size but different profiles,
reads actual painted glyph sizes, and replaces/removes policy without forcing
the root to rebuild. Color-only descendants inherit their parent profile;
explicit profile and Fixed descendants replace it.

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
The semantics state is derived from the mode via `checkbox_state`
(`Tristate(None)` → `Toggled::Mixed`; binary never publishes `Mixed`).

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

### Tab position is derived from one source, never written back

`TabController` keeps two values: the committed `index` and a continuous
`position()`. The position has exactly one source at a time — the controller's
own `AnimatedValue<f64>` when no `TabBarView` is attached, the attached view's
`PageController::page()` while one is — so there is no offset write-back from
the pager and no "index is changing" counter. `animate_to` commits the index at
once and retargets the source (`animate_to_page` for a pager); `set_index`
jumps it. A swipe moves the pager directly and the index commits only when the
pager's scroll activity stops, to the page it rests on. The owned value runs on
the `VsyncScope` clock that a `TabBar` or `DefaultTabController` binds, so
reduced motion settles it on the next frame; a drag is direct manipulation and
follows the finger under reduced motion too. While the controller itself moves
the pager, a stop it causes (a jump interrupting a running tab animation) does
not commit the interrupted page; a jump goes through the scroll controller so it
stops that run instead of being overtaken by it. A controller has one position
source, so it drives at most one mounted `TabBarView`: a second view on it is
refused at build (an `ErrorView`) and the first keeps the pager. The owned value is taken out of its
cell while it is driven, so a position listener that re-enters the controller
never meets a held borrow; a nested request is applied by the outer drive.

The indicator is one rect painted from the position on a repaint listenable,
without rebuilding the bar. `TabIndicatorAnimation::Linear` interpolates both
edges; `Elastic` eases the right edge by `sin(πt/2)` and the left by
`1 − cos(πt/2)`. Both are functions of the position alone, so the leading edge
leads in either direction of travel and a reversal paints the same rect for the
same position. Right-to-left rows mirror the fractions.

`a_drag_moves_the_position_and_indicator_but_not_the_index` (full and reduced
motion), `a_released_drag_commits_the_page_it_settles_on`,
`a_jump_during_a_tab_animation_keeps_the_jumped_index`,
`a_second_view_on_one_controller_is_refused`,
`a_tap_on_the_indicator_band_reaches_the_tab`,
`a_tap_without_a_view_animates_the_position_and_settles`,
`the_elastic_indicator_stretches_mid_way_between_tabs`,
`the_indicator_mirrors_in_a_right_to_left_row` and
`reduced_motion_settles_the_owned_position_on_the_first_frame` read the
painted indicator from the recorded display list.

### Page transitions are a per-platform theme map over the widgets-layer hook

The route machinery that lets a page pick up a transition it was not given is
`flui-widgets`' `PageTransitions` hook (ADR-0221); which transition runs is
Material policy and lives here. `ThemeData::page_transitions_theme` maps each
`TargetPlatform` to a `PageTransition`, defaulting to fade-forwards on Android
(450 ms, `EASE_IN_OUT_CUBIC_EMPHASIZED`), the shared parallax slide on iOS and
macOS (500 ms, the one copy in `flui-widgets`, ADR-0221 D4) and zoom elsewhere
(300 ms); shared axis and fade through (300 ms) are opt-in variants, `None`
cuts, and `Custom` takes any `PageTransitionBuilder`. `MaterialApp` installs
the resolved page builder above its navigator, with the dialog's 150 ms fade
in the popup slot, inside an `AnimatedTheme`, so a theme change animates the
colours and reaches routes pushed after it, never one already running.

`page_transitions_by_platform_and_variant` checks each default and variant at
its duration boundary (still running just before, settled after) and against
its curve at mid-run; `a_material_dialog_fades_in_over_150_ms` pins the
dialog and `page_transitions_theme_defaults_and_equality` the map.

### `OpenContainer` morphs inside its route, not in a separate overlay flight

The container transform is a route whose `PageTransitionBuilder` draws one
surface running from the source's rect to the navigator's: a `PageRoute`, or a
`PopupRoute` on a navigator a `Router` drives (`NavigatorHandle::admits_pages`),
which owns its pages and admits only popups. It shares only the
flight seat with `Hero` (the anchor, the measurement and the frozen placeholder,
reached through `flui_sdk::widgets::__private`): it has no tag matching and no
divert, so it needs no flight manager. The morph's frame is new, so its origin
is unknown until it has been laid out and measured; until then the morph is
offstage and the source still shows in its seat, so no frame draws the source
shifted by the navigator's offset. The morph measures its frame in a post-frame
callback after the frame it mounts in, and on every later tick; a route that is
already settled when the morph mounts (a zero duration) never ticks, so the
post-frame measurement is what brings it onstage. On that first measurement the source freezes
at its committed size with its child offstage, so neighbours do not move and
the child's state survives; it is released only once the morph has left the tree *and* the route
has completed, so a page dropped while covered keeps the source frozen.

The source is measured in the morph frame's coordinates (both mapped through the
render root, so a scale or rotation above the navigator cancels instead of being
applied twice) on every tick of the
route animation, so the exit lands where the source is now. That only holds if
nothing moves the page below during the morph, so the route sets
`PageRoute::animates_route_below(false)`: a theme's covered transition (Zoom
scales the page below to 1.05) would otherwise scale the source under the morph.
The result is delivered from a task on the tree's `AsyncDriver` that owns its
own token, so a page the navigator accepted reports its value even after the
container unmounts. The route resolves its result when the pop is accepted, but
the task holds the value until the flight lands (the morph has left the tree and
the source is back), so `on_closed` never runs under the closing transform; the
landing only wakes the task, because it can be reached from the morph's
`dispose`, where user code must not run.

The rows of `open_container_morph_by_scenario` read the committed scene:
`half time` (rect strictly between, curve value, placeholder keeps the marker
still; fails with the page below animating), `reverse re-measure` (fails without
the per-tick measurement), `pop result`, `flipped exit`, `fade schedules`,
`reduced motion`, `semantics`, `opens once`, `untappable`, `offset navigator`
(fails with the source frozen and the morph drawn before its frame is measured)
and `router` (fails with a `PageRoute` pushed onto the Router's navigator);
`pop result` also fails with the result delivered under the exit, and
`zero duration` fails with only the tick listener measuring the frame.

### `Radio` publishes the radio role, and a forced merge keeps it over a tile's button

**Rule:** a behavior the platform accessibility contract requires is dropped
only by decision, recorded where a reader will find it.

**Why:** the role is what separates a radio from a checkbox to a screen reader:
the two publish the same toggled state, and only `RadioButton` says the state is
one-of-a-set rather than independent.

**Choice:** `Radio::build` publishes `.radio(selected).enabled(interactive)`,
which sets `SemanticsRole::RadioButton` and the toggled state together.

Inside a real `ListTile` the tile's node and the radio's node conflict (each
declares a role, and both declare `enabled`), so they stay **separate** nodes:
the tile exports `Button` and the radio `RadioButton`. A single node for a tile
plus its radio is a *forced* merge: wrapping the `ListTile` in `MergeSemantics`
folds the radio into the tile's node, and `more_specific_role` keeps
`RadioButton` over `Button`. That composition is pinned in `tests/list_tile.rs`;
the precedence itself is flui-semantics' decision, recorded in
[`crates/flui-semantics/ARCHITECTURE.md`](../../crates/flui-semantics/ARCHITECTURE.md).

**What is still not wired, named rather than implied.** Platform-conditional
`selected` and `hint` fields are not published: FLUI's semantics surface has no
platform dimension, so this is a deliberate drop of a platform-dependent
behavior, not an oversight.
`focus_node` / `autofocus` keep the whole-substrate `InkWell` gap `Checkbox` and
`Switch` already name. And **no `Radio` gains a semantics *action*** — publishing
the radio role changes what the control *is* to a screen reader, not what a
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
  the precedence: the forced merge meets the tile's `Button` and the radio's
  `RadioButton`, and resolves `RadioButton` because checkable roles rank above `Button`; with
  that order reversed it would resolve `Button`.
  Mounting the radio as the render *root* makes it form its own node,
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
using that extent (ADR-0188).

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

### A drawer threshold changes scaffold structure only when two slots reorder

Drawer callbacks commit the handle's opened state and forward the application
callback independently of rebuilding Scaffold. Its child order depends only
on the end drawer's opened state when both drawer slots exist. Only a change
to that ordering schedules a scaffold rebuild; a single drawer crossing its
halfway threshold updates render-owned slide and scrim geometry directly.
DrawerController's status subscription still mounts and removes its panel.

`drawer_slides_without_rebuilding_per_frame` observes captured pointer moves
and virtual settling frames through the mounted tree. It asserts no builds
for single-drawer movement across halfway, sixteen pixels of panel movement
per input sample and matching scrim opacity, followed by panel and scrim taps.
`incoming_end_drawer_scrim_covers_the_outgoing_start_panel` keeps the old
panel visibly covering a tested point while the new drawer enters. Its tap
must reach the incoming scrim and close that drawer, without reaching the
outgoing panel's callback. Disabling the ordering update makes that callback
receive the tap instead.
