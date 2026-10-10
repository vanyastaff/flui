# flui-widgets architecture

## Inherited presentation data

`MediaQuery` lives in the lower `media_query` module, below text, interaction and
application composition. Consumers import its crate-root types; the `app` module
owns the shell and `SafeArea`, not the source of inherited presentation data.
The module DAG enforces this direction. Field-specific dependency behavior is
pinned by `a_size_only_change_rebuilds_size_and_whole_readers_only`.

`WidgetsApp` resolves supported resources from ordered `preferred_locales`,
subscribing only to that media field when no explicit locale is authored.
Complete locale identity determines exact matches before the established
script/region/language fallback (ADR-0173). A nested `MediaQuery` remains a whole
replacement; it does not fall through to an ancestor for an unavailable field.
`locale_override_removal_uses_the_nearest_current_preferences` pins override
removal and nested-provider precedence. The runtime's
`preferred_locales_select_resources_and_direction` exercises loaded resources.

`RichText` reads inherited text sizing and weight adjustment during its stateless
build and passes them with its unchanged authored spans to a private render view.
Native settings and inherited lookups do not enter `RenderObjectContext`. The paragraph
update reports layout and semantics invalidation when sizing changes, so the same
render object produces updated geometry. Weight changes likewise invalidate layout
and semantics; EditableText forwards the same inherited adjustment to its painter.
The common shaping policy runs after authored span inheritance (ADR-0174).
`Text` composes this path after merging its ambient style. `MediaQueryData`
carries one validated numeric sizing policy rather than a writable scalar.
The nearest provider replaces the outer policy, including fixed sizing;
copying parent data preserves its policy while changing another field.
Without a provider, the render pipeline supplies the policy. Removing a provider
restores the current outer authority. Raw system text-scale observations retain
their separate admitted range `1/64..=64`. Initial sizing is pinned by
`media_text_scaling_changes_the_laid_out_text`; live updates, restoration from
authored size and nested override retention by
`a_text_scale_change_relayouts_a_preserved_text_subtree`.

`Icon` resolves its square and glyph from one logical size. Its default stays
fixed; `IconThemeData::apply_text_scaling` opts both into the nearest inherited
policy, including an explicit icon-size override. `RenderIcon` resolves during
measurement and shapes the glyph at that admitted size with fixed sizing,
without applying inheritance again. Ordinary text retains
its inherited sizing and weight preferences. The public producer row
`icon_text_sizing_keeps_box_and_glyph_together` checks raster glyph keys and the
allocated square across live changes, exact answers, fractional sizes and theme
policy; tight constraints can allocate a rectangle without changing glyph size.
The widget's canonical Semantics wrapper publishes an explicitly authored label
as an image, including a labelled absent glyph. Decorative icons publish no font
codepoint label. `icon_labels_reach_the_assembled_accessibility_tree` checks the
assembled tree across label replacement and removal.

`EditableText` subscribes during its own build and carries sizing through its
appearance value to `RenderEditable`. The editor retains authored styles and
document offsets; selection and caret position use the newly laid-out paragraph.
The default caret follows the laid-out line height, including empty text and
authored font sizes, through `RenderEditable`'s automatic-height policy.
`RawTextField` preserves that default. An explicitly configured caret height
remains a logical length; removing the override restores automatic height. Mount/update
equivalence and restoration of glyph/caret geometry are pinned by
`inherited_text_sizing_updates_editable_glyphs_and_caret`.

## Accessibility of hidden retained children

`Visibility::maintain_size` preserves layout while hidden, with child semantics
excluded by default. `maintain_semantics` explicitly retains accessibility and
requires `maintain_size`, so the retained child keeps its normal geometry.
Pointer interactivity and focus remain independently configured. The composed
`VisibilityGate` forwards retention to `RenderVisibility`'s existing child
visitation hook; it also provides the same default and opt-in when used directly.
No global rule ties paint suppression to semantics suppression.

`retained_visibility_hides_child_semantics_by_default` pins the visible premise
and hidden default in the assembled a11y tree.
`retained_visibility_updates_semantics_without_changing_layout` updates a
mounted widget through visible, hidden and explicitly retained configurations,
asserting the child's accessibility label and unchanged allocated size.

## Overlay entry identity admission

Overlay entry allocation admits the final nonzero identity once and then refuses
construction permanently, so an entry's removal authority and its consumer view
key are never shared with a later entry. The rejected builder is retained
rather than dropped (ADR-0127).

## Terminal navigation ownership

The mounted navigator acquires its frame registry through `VsyncScope::maybe_of`
during lifecycle initialization and dependency changes. Each transition peer
publishes a weak callback to its route-owned `DrivenController`. The navigator
commits the clock and snapshots those callbacks before invoking them, outside
registry guards. A route withdraws its handle before rebinding and restores it
only if disposal has not superseded it. Unmount clears the clock even when an
external navigator handle retains the route state. Registry replacement keeps
the last sampled elapsed time, rather than restarting the transition.
`scope_replacement_moves_an_existing_route_without_restarting_it` pins progress
and withdrawal through a mounted navigator.

Navigation bindings hold the navigator's registry weakly. The navigator closes
the registry before retiring its history, so a closure installed by a modal
route or overlay entry cannot keep its route alive through the registry or
enqueue work for a navigator that is gone. A lookup through a closed registry
returns nothing and a mutation does nothing. Modal, transition and hero handles
that outlive the navigator keep only their own state; a transition handle does
not keep a modal page alive.

Route and navigator command identities never wrap: the integer maximum
permanently refuses allocation, so a caught capacity failure cannot reissue an
identity held by history or a command target. A rejected route and its
replacement result are retained rather than dropped (ADR-0127).

An admission reserves every identity it spends, route and overlay entry alike,
before its first observable side effect: a recorded Router page, a filled
binding slot, an inserted entry, a dismissed predecessor (`pop_and_push_named`
reserves before it pops), a hero frozen as a placeholder. Batch replacement
reserves for every member before preparing any. Exhaustion therefore fails the
whole operation with nothing changed, and caller-owned values carried across
the reservation (a named request's arguments, a result, a predicate, a
Router's unseeded initial values) are retained on refusal. Spent reservations
are not returned.

Route, router, overlay, modal and hero owners withdraw each owned value from
shared state before dropping it, so no destructor runs under an internal lock.
After the first destructor failure in a retirement, or while the thread is
already panicking, the remaining values are retained rather than dropped
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)); the first
failure propagates. Router parser inputs and partially built router clones are
owned before any user code runs, so a failure there drops or retains them under
the same rule. A route result and the waker waiting for it are retired
separately. Tested by `delivered_route_results_remain_completed` and
`terminal_binding_authority_is_closed_before_route_retirement`.

## Event callback phase boundary

`InteractiveViewer`, `RefreshIndicator`, `PopScope`, `AnimatedSize`,
`Dismissible`, `Draggable` and `PageView` event setters receive `EventCx`
(ADR-0086), and so do `CallbackShortcuts` bindings and `Action::invoke`, which
run inside the key event's dispatch. Composite input
handlers forward their child's context; wheel and pinch claim handlers keep
their query signature and open the viewer's lifecycle-acquired `WriterSource`
only for the interaction notifications. `PopScope` preserves synchronous
navigation outcome delivery and the existing observer ordering.

Animation listeners accept owner-local captures. `AnimatedSize` observes
completion counts during build but invokes `on_end` after the frame.
`Dismissible` likewise calculates transitions with layout constraints, then
queues the event payloads on the owner-local post-frame lane; its fully-slid
input-time completion bypass remains synchronous. Animation-listener
notifications are never delivered synchronously to user code: user effects must
not execute during a FLUI build. Deferred events use the latest configured
callback and are cancelled when their widget is disposed. A missing or closed
post-frame lane reports a warning and drops the event, never dispatches inline.

Queued `GestureDetector` assistive activation also resolves the current callback
at delivery, after configuration updates, and checks that the state is still
mounted. Each accepted platform action remains a distinct FIFO command across
tap and long-press kinds; only the rebuild used to wake the UI thread may
coalesce. Removing a handler or disposing the widget cancels delivery; queued
requests never retain an obsolete user closure. The delivery-time recheck and
cancellation are pinned by
`queued_semantics_delivery_rechecks_the_callback_and_mount_lifetime`, and FIFO
order across a tap and a long press by
`a_panicking_assistive_action_does_not_discard_the_fifo_tail`. Two accepted
actions of the same kind run their handler twice on the next frame, are never
replayed on a later one, and a further action still arrives after the batch
drains: `repeated_assistive_taps_are_delivered_once_each_and_keep_making_progress`
and `repeated_assistive_long_presses_are_delivered_once_each_and_keep_making_progress`.
Post-frame entries hold only a weak reference to the detector-owned delivery
target. Teardown therefore releases the live callbacks and presentation-bound
writer even when an aborted or absent frame leaves the queue entry pending;
draining that entry later is an inert no-op.

`Draggable`'s drag session opens each callback's write through the widget's
`WriterSource`; its configuration cell is owner-local and never borrowed across
user code. An unmount mid-drag cancels from `dispose`, which runs in
`finalize_tree` outside any build, so the cancel callbacks' writes land; the
feedback layer is removed before that cancel runs user code. `PageView`'s
controller listener only records each page change
and schedules a rebuild; `build` queues one post-frame entry per recorded page,
and delivery reads the current callback (mapping decision 37).

Tests: `tests/draggable_events.rs`, `tests/page_view_events.rs`, and the
`event_cx_tests` module of `tests/shortcuts.rs`.

`DragTarget`'s `on_accept`/`on_leave`/`on_move` and `Semantics`' action
handlers take `EventCx` too. Their render data must stay `Send + Sync`, so each
widget registers its owner-local state (the drag target's slot, the node's
action table, each with its `WriterSource`) in the interaction lane and stores
only the lane's `LocalPayloadTarget` ticket; the drag session and the semantics
action handler resolve it inside the UI runtime (ADR-0086 §3, amended 2026-09-30;
mapping decisions 1 and 17). The unmounted `LocalHistoryEntry::on_remove`
navigation primitive is not migrated: it needs an explicit navigation
write-context contract rather than an invented ambient writer.

The user-facing widget catalog: configuration objects over the `flui-objects`
render catalog, plus the stateful widgets that own gesture, focus, routing and
overlay behavior. Layer rules, dependency direction, and the crate's place in
the workspace DAG live in [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md)
and this crate's `[package.metadata.flui] layer`; this file
records the per-widget design decisions that would otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs
here is a decision local to this crate: a widget's internal shape, a callback
bound, a payload type.

## Module layers

The crate is one crate with a declared import direction between its top-level
modules. `[package.metadata.flui.modules]` in `Cargo.toml` is the authority:
`layers` lists the modules bottom to top, and non-test code of a module names
only modules in lower layers. `cargo xtask module-dag -p flui-widgets` checks
it, following root re-exports (`crate::SizedBox`) and the `prelude` and
`__private` relays back to the module that owns each name.
`cargo xtask module-dag --print-edges` lists the edges it sees.

`#[cfg(test)]` code is exempt, as dev-dependencies are between crates: the
tests build fixtures from widgets of any layer (`Column` under the overlay
tests, `SizedBox` under the clip and paint tests).

`interaction` sits above `overlay`, `animated`, `stack` and `clip` because
`Draggable` and `Dismissible` are composites: `Draggable` inserts its feedback
into the `Overlay` and `Dismissible` slides its child with a `Stack`, a
`ClipRect` and a `VsyncScope`. The focus widgets (`Focus`,
`Actions`, `Shortcuts`) live in `interaction` too; they import each other in
a cycle, so they can become a node of their own only after they move together
into one module.

`router` sits in a layer of its own above `navigator` and below `app`: the
`Router` builds a `Navigator` and places its pages on it (and wraps each page
in `Semantics`), and `WidgetsApp::router` roots an app in a `Router`. The
derive's helpers (`router::__derive`) live in `router` too, beside the trait
they serve.

`form` sits in the top widget layer beside `icon` and `app`: a text form
field composes `RawTextField` (`text`), a `Focus` wrapper (`interaction`) and
the error line's `Column` and `Semantics`, and nothing below names a form.
The clipboard intents live in `interaction`, not `text`, so the
`DefaultFocusTraversal` that `FocusRoot` builds can bind them without naming
the text module that answers them.

A new module takes a place in `layers`; a move that changes the direction
edits `layers` in the same change, which is where it is reviewed. An edge the
layers refuse and that cannot move yet goes into `exceptions` with the ADR
whose change removes it and the date it was added.

## Integration tests

The scenario functions in `tests/*.rs` are rows of the capability-family tables in
`tests/contracts.rs`: one `#[test]` per family, each row run in turn and every failing row
named in the panic message. The test names cited in this document are those row names;
find one with `rg <name> tests/`.

## Mapping decisions

### Implicit retarget captures the displayed sample before changing easing

When a target changes, implicit animations read their current value or eased
progress before configuring the new curve. The replacement tween starts at that
sample. Container and alignment properties share an optional restart progress,
so unchanged properties are re-anchored at the same instant as changed ones.
A curve-only update keeps the existing run timeline.

The public rows `opacity_retarget_with_a_new_curve_keeps_the_displayed_sample`,
`padding_retarget_with_a_new_curve_keeps_the_displayed_sample`,
`container_retarget_with_a_new_curve_keeps_the_displayed_sample`,
`align_retarget_with_a_new_curve_keeps_the_displayed_sample` and
`rotation_retarget_with_a_new_curve_keeps_the_displayed_sample` pin continuity
through rendered opacity, layout and a transform layer. The container row also
keeps an unchanged height continuous. The separate row
`changing_only_the_curve_keeps_the_existing_deadline` pins the original completion
deadline. It does not prove position or velocity continuity for curve-only
changes. Transferring velocity into replacement motion remains part of the
retarget design.

### Focused document selection uses the normal action chain

`DefaultFocusTraversal` binds `SelectAllTextIntent` to Cmd+A on macOS/iOS and
Ctrl+A elsewhere, using the existing command activator and primary-focus
resolution (ADR-0023, ADR-0079). `EditableText` records its nearest action on
its focus node. Selection covers the complete UTF-8 document, including an
obscured field, without changing text, reporting `on_changed`, or acquiring a
clipboard. The action declines during IME composition and resumes after commit.
Disabled fields cannot retain focus. The `text_editing` family pins this with
`select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit`,
`select_all_without_a_focused_text_field_leaves_the_key_unconsumed`, and
`select_all_defers_to_an_active_composition_and_recovers_after_commit`.

### A field commits its composition before it loses its input

`EditableText` commits an active composition, keeping its text, through
`TextInputHandle::complete_composition` with its own token (ADR-0142 item 4):
on blur before the client detaches, on a pointer-down on the field before the
caret moves, and on a paste before the clipboard's text lands, so paste is
enabled during a composition while select-all still declines it. The commit
runs owner code (`on_changed`): the transition's later steps run in the same
`OwnerCalls` scope and its first failure is resumed after them. Tests:
`owner_code_is_contained_at_every_point`'s `editable: a blur during preedit …`
rows (push and pull) and
`a_pointer_down_and_a_paste_commit_the_composition_first`.

### Navigation commits precede observer effects

Router mutations commit the navigator history and typed route stack before
delivering observer callbacks. A reentrant pop therefore sees the admitted
route. `go` recomputes its shared prefix after popup dismissal, which can itself
reenter navigation. Outgoing and temporary route values retire outside stack
borrows; after a failure, remaining opaque owners are retained to preserve that
failure. This implements ADR-0093's single navigation authority.
`router_and_widgets_app` covers reentrant push/replace/go, popup-prefix changes,
observer failure and next navigation; `router_observer_failure_and_retirement_competition`
covers competing observer and route-destructor failures.

### Route locations preserve segment identity

`RoutePath` percent-encodes each segment with a context-specific ASCII set and
decodes exactly once after splitting, so an escaped slash stays inside one route
value. Malformed percent escapes and invalid decoded UTF-8 are rejected. Plus and
dot segments are literal path data, not form decoding or relative-URL operations.
`route_locations_preserve_encoded_segment_identity` pins canonical spellings and
rejection through the public routing API.

### Scrollbar drag takes ownership of scroll activity

A minimum thumb length is capped by the available track. Thumb dragging jumps
the position through its activity protocol, cancelling animation before later
ticks can overwrite the drag. The `scroll_physics_and_activity` rows
`scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded` and
`dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick` drive
actual pointer input, timed ticks and a subsequent animation.

### Editable text separates its document from the visible viewport

Single-line editing retains full document measurement while horizontal
displacement reveals the caret. Pointer queries add that displacement; IME
rectangles subtract it and intersect the actual allocated viewport. Exact
queries outside it return `PointOutside`. Glyphs, selection, composition and
caret paint share the viewport clip. The `text_editing` rows
`long_input_reveals_the_caret_and_maps_visible_pointer_positions` and
`editable_paint_places_long_text_under_the_viewport_clip` pin the producer and
paint commands, including RTL and obscured cases.

### Network responses belong to their registry

`NetworkImage` includes its registry's weak allocation identity in both decoded
cache and pending-load keys (ADR-0118). HTTP headers and policies can change the
response at the same URL; different registries must not share those responses.
Clones sharing one registry still share keys. Retained keys cannot retain the
registry's HTTP pool or runtime, and their addresses cannot alias a later owner.
**Test:** `network_images_scope_cached_and_pending_responses_to_the_registry`.

### 1. `DragTarget` publishes a shared `DragTargetSlot`, not its `State`

**Choice:** the target keeps an owner-local `Rc<DragTargetSlot>` — a
non-generic object that owns the entered list (keyed by `PointerId`), holds the
current build's callbacks type-erased behind a private `TargetCallbacks` trait,
and carries the target element's `RebuildHandle` and `WriterSource` so a
transition can schedule a rebuild and open its callbacks' `EventCx`. The slot
is registered in the owner's interaction lane, and the hit-test payload is only
the lane's `LocalPayloadTarget` ticket, which the drag session resolves back to
the slot inside pointer dispatch. `DragTargetState<T>` keeps only the slot and
reads its candidate/rejected lists back out of it; `build` refreshes the
callbacks into the slot so a rebuilt view's closures are the ones a later
transition invokes.

**Why the payload is not the state.** Two independent reasons, both
structural:

- A hit-test payload is `Arc<dyn Any + Send + Sync>` (`HitTestEntry::metadata`).
  Neither a `DragTargetState` nor a slot holding `Rc<dyn Fn …>` callbacks can
  be one, so the payload is a ticket and the slot stays in the lane.
- FLUI's callbacks live on the *view*, which the state does not own. A payload
  reaching only the state could not invoke them at all.

**Consequences:**

- **`on_accept`, `on_leave` and `on_move` take `&mut EventCx<'_>`** and run
  synchronously inside the drag's dispatch, so a drop lands before the
  draggable's `on_drag_end`
  (`a_drop_writes_through_the_targets_on_accept_before_the_draggable_completes`).
  `on_will_accept` is a query and takes none. None of the four needs
  `Send + Sync`. The *builder* stays `Rc`: it produces a `BoxedView`, which is
  owner-local by construction, and it is only ever called from `build`.
- The veto (`on_will_accept`) stays **synchronous**, which a deferred
  drain-on-next-build queue would have lost — a drag has to know at move time
  which targets are candidates.
- The slot outlives its element by `Rc`. A target that leaves the tree
  mid-drag is `retire`d in `dispose`, and every later transition is a no-op —
  a "not mounted" early return in a form that cannot be forgotten at one
  call site. `did_drop` on a retired slot returns `false`, so the drag reports
  the drop as *not* accepted. **Unasserted:** no test pins this.
- `DragTarget` tags itself `HitTestBehavior::Translucent`.
  Making it configurable stays a named deferral.

**Replacement tests:** the `DragTargetSlot` protocol, and the enter/move/leave/drop
ordering for nested and overlapping targets. **Unasserted:** no test pins this.

### 2. `DragTargetDetails` carries a target-local position as well as a global one

**Choice:** `offset` is the global position; `local_offset` adds the
same point mapped through the hit entry's own global-to-local transform.

**Why:** callback code cannot reach a render object, so it has no way to
map a global point itself — a target would have had *no* way to learn
where the drag is in its own space. Taking the value from
`HitTestEntry::transform` composes the entire ancestor chain, so it is exact
under scale and rotation, where subtracting a remembered origin is not.

**Replacement test:** a transformed target told the drag position in its own
space. **Unasserted:** no test pins this.

### 3. `Draggable` recovers its global position through a private origin probe

**Need:** the drag hit-tests at `globalPosition + feedbackOffset` on every
move, so it needs the global position of the pointer.

**Choice:** `Draggable` mounts a payload-free `DragOrigin` view as its
`Listener`'s direct child. That view's `find_render_object()` resolves to the
`Listener`'s own render node, and the drag converts its
`Listener`-local pointer positions to the root's space with
`PipelineOwner::local_to_global` before probing.

**Why, and why the probe survived the dispatch fix.** Pointer dispatch now
carries both spaces: a `Listener` callback receives a `PointerDispatch` whose
`global` half is the platform's own value, never re-derived from a transform.
That closed the general defect — but it did not reach this widget, because
`Draggable` does not consume pointer events directly. It feeds them to a
`MultiDragGestureRecognizer`, and the `GestureRecognizer` contract
(`add_pointer`, `handle_event`) still carries a *single* space, as does every
`Drag*Details` struct it produces. `DragSession`'s accumulated position, its
axis restriction, and `to_global` are all built on that one space being the
`Listener`'s. Handing the recognizer the global event instead would make
`global_position` truthful and `local_position` a lie — the same defect
relabelled — so the probe stays until the recognizers carry the pair.

**What the recognizers need, so the follow-up is not re-derived from scratch:**
a local/global position pair threaded through the drag recognizers'
initial and last positions and handed to every detail struct, with the velocity
tracker sampling the *local* half and deltas mapped to global. That means the
trait signatures, `RecognizerBase`'s tracked position, and each of the ten
recognizers' internal position plumbing — a change of its own size.

**Alternatives rejected:**

- Reading `DragUpdateDetails::global_position` — rejected when this was
  written, because the field was then fed the already-localized value and was a
  global position in name only. Issue #908 has since carried the dispatch pair
  through `GestureRecognizer::handle_event`, so the field is now genuinely
  global. It still does not replace the probe: the probe answers where the
  draggable's own NODE is, which no pointer event carries at all, and the
  session converts an accumulated, axis-restricted position rather than a raw
  event position.
- Assuming translation-only ancestors and adding a remembered origin — wrong
  under any `Transform`, and wrong silently.
- Stashing the `Listener`'s `dispatch.global` in a cell and having
  `DragSession::to_global` read it instead of converting — exact only under
  translation-only ancestors, because the session converts its *accumulated,
  axis-restricted* position rather than the raw event position. That trades
  exactness under scale and rotation for exactness across a mid-contact
  transform change, which is not a clear win, and it fixes only this widget
  while `GestureDetector`'s drag details keep lying.

**Consequences named rather than left to be discovered:**

- A drag carrying no data discovers nothing: `ErasedDragData` erases a concrete
  value, not an `Option`, so a data-less drag has no representation here.
- `axis` restriction applies to deltas in the `Listener`'s space rather than the
  root's, which differs only under a rotating ancestor.
- **A transform that changes mid-contact is still converted inconsistently.**
  Pointer dispatch localizes with the `HitTestEntry` transform captured in the
  route resolved at `PointerDown`, while `local_to_global` converts with the
  tree's *current* transform. A frame that moves or scales the `Listener`
  between two moves therefore has the drag convert a stale local point through
  a fresh matrix, and the probe lands off the pointer until the contact ends.
  The value that fixes this now exists at the dispatch boundary, but it stops
  at the `Listener`, one layer above where this widget reads its position. Not
  worked around.

**Replacement tests:** real pointer input across a tree where the draggable and
the targets are at different offsets, so a local-position implementation enters
targets the pointer was never over. **Unasserted:** no test pins this.

### 4. Named routes split into six untyped entry points and two typed ones, and a request that cannot be served is a typed error

**Rule:** gated as [ADR-0024](../../docs/adr/ADR-0024-named-routes-seam.md) §7.3.

**Problem:** a name that no table entry or generator resolves, and a caller's
result type that differs from the route's, are both failures a
`Future<T?>`-shaped push cannot express.

**Choice:** two shapes, not one.

- Six untyped operations — `push_named`, `push_replacement_named`,
  `push_replacement_named_with`, `pop_and_push_named`,
  `pop_and_push_named_with`, `push_named_and_remove_until` — return
  `Result<RouteId, NamedRouteError>`. They never name the route's result type,
  so their only error is `Unresolved`.
- One typed operation, `push_named_typed::<T>`, additionally returns the route's
  `RouteResult<T>` and can answer `ResultType { name, expected, actual }`. It
  compares `TypeId::of::<T>()` against a `TypeId` the `GeneratedRoute` captured
  at construction, so the refusal lands **before** the carrier can reach a front
  door — expressed in the type system as `GeneratedRoute::checked` yielding a
  `TypedPush<T>` token that is the only thing with a `push` method.

Both failures are total **for the operation itself**: it pushes, pops, replaces
and removes nothing, and the generated route is disposed (§7).

That qualifier is load-bearing, and the earlier revision of this entry lacked it.
Resolution runs a user factory, and although §9 no longer *offers* a way to
navigate from one, a factory that captures a handle can still do it — so a
factory that navigates and *then* declines has already changed the stack and
notified observers by the time the name comes back unresolved: `push_named`
returns `Err(Unresolved)` with the stack one deeper and `["push", "changeTop"]`
observed. **Unasserted:** no test pins this. Those are the factory's own
mutations, deliberate on its part, and they are not rolled back for the same
reason §5 does not undo a factory's nested push: it is not this operation's to
undo. What the guarantee covers is that **the failing operation adds nothing of
its own**.

**Why not type every entry point.** That was the first design, and it was worse
than the bug it fixed. `push_named::<()>("/settings")` against a
`PageRoute<i32>` generator would return `Err` and *not navigate* — a caller who
just wants to go to a screen has no reason to know what that screen completes
with. Trading a
silent-`None` for a refusal-to-navigate in the common case is a regression. The
split keeps the strong guarantee exactly where the caller has asserted a type.
Typed siblings for replacement and remove-until are deliberately not offered:
no consumer yet, and each is purely additive later.

**Why a typed error.** Two reasons, one per variant:

- A route name is *caller input*, not a framework invariant, and
  [`PANIC-POLICY`](../../docs/PANIC-POLICY.md) puts caller input on the `Result`
  side; a debug-only assert would behave differently in release.
- The type mismatch is genuinely detectable in Rust: FLUI's registration stays
  typed, so the generated route knows its own `Output`. Reporting it as an indistinguishable `None` at delivery — this ADR's
  own first answer — would have thrown that information away for nothing.

**Consequence, named rather than left to be discovered:** the *result* a `_with`
variant delivers to the departing route keeps the ordinary delivery-time
contract (`pop_with`'s), because the navigator still cannot know the departing
route's `Output`. Only `push_named_typed`'s `T` is checked early. So
`push_replacement_named_with(name, Wrong)` still completes the replaced route
with `None` and a log line, exactly as `push_replacement_with` does.

**Replacement tests:** in `tests/navigator_public.rs` —
`push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing`
for the guarantee the typed entry point keeps, asserting the error's own fields
*and* that the stack and the observer stream are unchanged; deleting the
`TypeId` comparison in `GeneratedRoute::checked` fails it. The untyped half — a
registered `PageRoute<i32>` reached through `push_named` and the other five
untyped operations with no `T` in sight, the regression the split prevents, and
an unresolvable name leaving the stack and the observer stream untouched:
**Unasserted:** no test pins this.

### 5. Named operations capture their target, then resolve, then act on the captured route

**Rule:** as §4 above; same ADR.

**Choice:** read the target route's id **first**, then resolve the name, then act
on that captured id. Resolution happens before any mutation, so an unresolvable
name changes nothing; and the operation acts on the route the caller meant, not
on whatever happens to be on top after resolution.

**Why the capture is needed, and what it cost to learn.** Resolving runs a user
factory, and a factory can navigate — `RouteRequest::navigator()` advertises
exactly that. So "resolve, then act on the current top" is a
time-of-check/time-of-use bug: a factory that pushes during resolution makes the
following `pop()` remove the *nested* route while the caller's target survives
uncompleted, and `pop_and_push_named_with` then delivers the caller's result
value to a route it has never heard of. Four entry points shared the shape.
`push_named_and_remove_until` did not, for a structural reason worth preserving:
its removal is defined by a **predicate**, not by a captured top, so a nested
push is swept along with everything else — do not harmonise it into the captured
shape.

**Three consequences, all only for a re-entrant factory:**

1. **Ordering.** The nested `didPush` precedes the dismissal of the caller's
   route, because the pop is deferred until after resolution.

   **The `RouteRequest::navigator()` accessor is withdrawn (§9), so these two
   consequences change meaning rather than disappearing.** They were the price of
   a capability the crate offered. They are now what still happens if a factory mutates the stack
   — which the crate no longer offers a way to do, and cannot prevent, because a
   factory is an `Rc<dyn Fn>` and closures capture freely. Measured, not assumed:
   a factory that captures a handle reproduces the window exactly, and reverting
   the capture-then-resolve fix reproduces the original defect through it
   (`stack=[root, victim, arrived]`, the caller's route surviving uncompleted).
   So the defences below are not overhead left behind by a retired feature; they
   are the operations being correct unconditionally.
2. **Kind, on the dismissal path.** Once a factory has pushed on top, the
   caller's route is no longer the top, and a route that is not on top cannot be
   popped. `dismiss_captured` therefore has three cases, all documented on it and
   all asserted: still on top → an ordinary `pop` (`didPop`, and
   `Route::did_pop` may still refuse); buried → removed by id (`didRemove`);
   already gone → a no-op, and the operation still returns `Ok` with its new
   route pushed.
3. **Identity, on the replacement path.** `didReplace(new, old)` names the route
   the replacement **actually completed** — the captured one — and this had to be
   fixed as a second-order consequence of the capture itself. Completing by id
   while deriving the observation from position left two sources of truth that
   agreed only while nothing could run in between; with a re-entrant factory they
   diverged, and observers were told the *factory's* route had been replaced
   while it was still on the stack, with the genuinely replaced route never
   reported at all. An observer acting on that would tear down a live route.
   `RouteEntry::replacing` now carries the id the completion used, resolved once,
   and the observation reads it. Note this is invisible to a kind-only assertion:
   the stream is identical either way, which is why the pin asserts the payload.

   The sibling arms keep the positional answer, deliberately: `Push` means "the
   route below", which is positional by definition and replaces nothing, and the
   generic mid-stack `Replace` resolves its target positionally in the first
   place, so for it position *is* the single source of truth.

Non-re-entrant calls — every ordinary one — are unaffected and still emit
`["pop", "changeTop", "push", "changeTop"]`.

**Every case, measured rather than described.** Two of these are not what a
reader would guess, which is why the table is here and not a summary:

| operation | factory | stream | `didReplace(new, old)` |
|---|---|---|---|
| `pop_and_push_named` | none | `pop, changeTop, push, changeTop` | never emits it |
| `pop_and_push_named` | pushes (target buried) | `push, changeTop, **remove**, push, changeTop` | — |
| `pop_and_push_named` | pops (target gone) | `pop, changeTop, push, changeTop` | — |
| `push_replacement_named` | none | `replace, changeTop` | `(new, target)` |
| `push_replacement_named` | pushes (target buried) | `push, changeTop, replace, changeTop` | `(new, target)` |
| `push_replacement_named` | pops (target gone) | `pop, changeTop, replace, changeTop` | `(new, None)` |
| `push_replacement_named` | target not present (mid-exit) | `replace, changeTop` | `(new, None)` |

The two surprises:

- **A buried replacement still reports the captured target**, not `None`. Only
  *gone* and *not present* report `None`. The buried case is the one that used to
  be wrong, so it is the one worth stating.
- **The "gone" pop-and-push case is indistinguishable from the no-factory case.**
  The factory's own `pop` produces the `pop`, and the operation's dismissal is
  then a no-op — so consequence 2 shows only in the *buried* case, not in every
  factory mutation.

**Undelivered results.** A caller-supplied result that reaches no route is
**reported and dropped outside any guard**. The location is the load-bearing
half, not the log: its `Drop` is user code, and this crate has already been bitten
by running user code under a non-reentrant mutex — the failure there is a hang,
not a test failure. `warn` when there was no route to deliver to; `error` when a
route received it and the type did not match its `Output`. One wording per
condition, on one path, so this sentence is true in exactly one way.

**The invariant is what to rely on, not a count: every caller-supplied result is
either delivered to a route or reported exactly once, and dropped outside any
guard.** The list below is illustrative — it is here because most of these are not
the edge cases a reader expects, not as an inventory to keep in step: an empty stack, a top mid-exit-transition, a
target already completed, an id belonging to another navigator, an unmounted
handle, a named capture that came back empty, a user `Route::did_pop` returning
`false` (a public trait whose default is `true`, and ADR-0024 sanctions user
routes), `maybe_pop_with` under a `PopScope` veto — which reports *handled* while
discarding, so it is worse than the case below — and **`maybe_pop` on a lone
route**.

A tenth condition, "a result displaced from an entry that already carried one",
is armed for in `arm_pop` and is believed **unreachable**: every arming site
flushes inside the same locked section, and every flush arm takes the pending
result. It is left armed rather than removed, because reachability here is a
property of the current call graph and this feature has already watched that
graph change four times.

That last one is not an edge case at all. `Route::pop_disposition` is
`isFirst ? bubble : pop`, so the bottom-most route **bubbles by design**; before
this was fixed, `maybe_pop_with(v)` on a one-route navigator discarded the
caller's value on *every* call, under the guard. The commonest possible stack
shape was the undelivered-result path.

**Ordering, and exactly what it covers.** An operation's own observations reach
observers **before anything a re-entrant *drop* triggers**: a `pop_with` whose
payload's `Drop` pops again is observed as `didPop(target)` then `didPop(middle)`
— cause before effect. That is measured both ways; reporting before the flush
outcome is applied inverts it to `[middle, target]`, from which an observer cannot
reconstruct the sequence. So the drains apply the outcome first and report second.

**It does not cover every re-entrancy, and the scope is load-bearing.** `apply`
runs step 0 — everything the flush owes user code, including deferred `PopScope`
effects (`notify_pop_invoked`, `drain_local_history`) — *before* step 1 delivers
to observers. So a navigation issued from a `PopScope` callback **is** observed
before the operation that triggered it. That is a different path from a value's
`Drop`, it is not closed by the drain ordering above, and the guarantee here is
deliberately worded to the drop case rather than generalised.
**Unasserted:** no test pins this. `apply` deliberately runs
re-entrant user code — deferred `PopScope` effects, observer delivery,
`Route::dispose` — and a value's `Drop` running after all of it is a consequence
landing where consequences belong.

**How they were found, because the axis was wrong twice.** The first enumeration
read `history.rs`, where the state machine *consumes* a result — and missed the
sites where a *decision* discards one, which live on the handle. The second
enumerated `Option<AnyResult>` — the **erased** type — and missed the two sites
that drop the caller's value *before* erasure, on a `?` that returns while it is
still `TO`. Those two sweeps have **zero overlap** — not "the first one missed
some", but two disjoint sets, which is what proves the axis was wrong rather than
the search sloppy. The correct axis is the caller-supplied generic,
traced from the parameter to a delivery or a report, accounting for every early
return in between.

Worth knowing when editing these: four of the six methods carrying a caller value
are safe only because `Some(Box::new(result))` happens to be their first
expression. An early return added above it reintroduces the defect silently.

**Where the target is resolved.** The unnamed front doors resolve theirs at
flush time, because nothing can run between their call and the flush. The named
ones capture it *before* resolving, because a factory can. That difference is
the whole of this entry.

**Replacement tests:** each behavior below is one a test would have to catch;
the success path cannot see the resolve-before-pop ordering, because both
orderings pop and then push.

- An unresolvable name passed to `pop_and_push_named[_with]` pops nothing and
  delivers its result to nobody. **Unasserted:** no test pins this.
- A re-entrant factory: the operation acts on the captured target, not on the
  current top. **Unasserted:** no test pins this.
- A pop-and-push is not a replacement wearing its name. Stack shape and result
  delivery cannot tell those apart, so the discriminator is the observer stream:
  a pop-and-push emits `didPop` + `didPush`, a replacement emits `didReplace` and
  neither. **Unasserted:** no test pins this.
- Point 3 on the `didReplace` **payload**: deriving the reported id positionally
  makes it name the factory's route, and the observer stream alone does not
  change when the identity is wrong. **Unasserted:** no test pins this.

### 6. Named-route registration lives on the handle, and the app builder will replace the table wholesale

**Rule:** as §4 above; ADR-0024.

**Choice:** all three register on `NavigatorHandle` — `route(name, factory)`,
`on_generate_route`, `on_unknown_route` — and one private `RouteRegistry`
resolves them in order (table → generator → unknown). FLUI's
`Navigator` widget is a thin shell over the handle and every push already goes
through it, so a widget-level generator would need widget-diff plumbing to reach
the same place.

**Why this is recorded now, before the second registration site exists.**
`WidgetsApp::routes(..)` is a later slice, and two registration sites with no
defined winner is how a feature acquires an unfixable bug. The contract is fixed
here: **the app builder replaces the table wholesale at mount; the handle
mutators serve imperative or late registration.** A `WidgetsApp` rebuild whose
`routes` map changed therefore clears entries a previous mount installed, and
does *not* clear a generator or a table entry registered directly on the handle
after mount.

**Registrations are not mount-scoped, and that asymmetry with observers is
deliberate.** `NavigatorState::dispose` detaches observers, because an observer
holds a handle exactly while the navigator is mounted. It does **not** clear the
registry: an app that registers once against a handle it retains would
otherwise get `Unresolved` from every `push_named` after its first unmount.
Clearing on dispose was tried and reverted for exactly that reason.
**Unasserted:** no test pins this.

**Consequence, named rather than left to be discovered:** a factory that clones
its own `NavigatorHandle` in still closes an `Arc` cycle through the registry,
and nothing reclaims it implicitly. §9 removed the *reason* to capture one —
`RouteRequest` hands the factory its navigator — so this is no longer what the
natural code does, and `NavigatorHandle::clear_routes` is the explicit escape for
a caller who captured anyway. Caller-controlled by necessity: only the caller
knows whether it intends to register again.

**Replacement tests:**
`a_factory_that_pushes_re_entrantly_does_not_deadlock` in
`tests/navigator_public.rs`. The lifecycle contract above (registrations
surviving an unmount and remount, `clear_routes` dropping every registration
including the generator hooks), the resolution order (a table entry wins and the
generator is never consulted; `on_unknown_route` runs only after the generator
declined, and sees the caller's payload) and per-handle registries:
**Unasserted:** no test pins this.

### 7. A generated route that is never pushed still runs `dispose`

**Rule:** as §4 above — "never lose an edge case by accident".

**Choice:** `GeneratedRoute` holds its erased route in an `Option` and
implements `Drop`, which forwards to `Route::dispose` through a
`dispose_unpushed` arm on the private `ErasedPush` trait. The push takes the
`Option`, so the drop that follows a successful push finds nothing and cannot
double-dispose. `TypedPush<T>` owns the `GeneratedRoute` rather than the box, so
a checked-but-unpushed token disposes through the same path.

**Why it needs saying.** In Rust `dispose` is not `Drop`, and this codebase's
route machinery assumes a never-installed route gets disposed. The refusal path
in §4 makes an unpushed `GeneratedRoute` a **routine** occurrence rather than a
rare one — every wrong-`T` `push_named_typed`, and every factory that builds a
route and then answers `None` after all — so the obligation is load-bearing, not
theoretical. It is also invisible: nothing observes the omission except a route
that quietly never released what it held.

**Test:**
`push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing`
(`tests/navigator_public.rs`) counts `dispose()` calls on a probe route and
asserts exactly one. Deleting `impl Drop for GeneratedRoute` fails it.

### 8. `RouteSettings::with_arguments_shared` relays a payload without changing its identity

**Rule:** as §4 above.

**Need:** tests assert a route's arguments by pointer identity, not value
equality, so relaying one settings object's arguments onto another must keep
the same payload.

**Choice:** `with_arguments<T>(value)` keeps taking the value and minting a
fresh `Arc` (it is the construction case), and a second constructor
`with_arguments_shared(RouteArguments)` forwards an existing payload untouched.

**Why:** `RouteSettings`' own `PartialEq` compares the payload by `Arc::ptr_eq`,
so a relay through `with_arguments` silently changes the answer to the exact
question identity-based assertions ask — and the two constructors are one character apart at
the call site with no type error between them.

**What it does *not* claim.** It is not the only way to move a payload: a caller
holding a `RouteSettings` can already `clone()` the `Arc` out of
[`arguments`](RouteSettings::arguments) and carry it by hand, and the test that
motivated this constructor could have done exactly that. What the constructor
buys is that the identity-preserving relay is *expressible as a builder call*,
so the safe form is as short as the unsafe one — rather than a fact about
`with_arguments` that every relay site has to remember. That is a real but
modest gain, and it is stated here as such.

**Replacement tests:** the identity-preserving relay through
`RouteSettings::with_arguments_shared` (and the contrast: `with_arguments` on an
identical value is *not* `ptr_eq`), and through the keyed counterpart
`RouteKey::with_arguments_shared`, which exists because `RouteKey::with_arguments`
takes its payload by value and would wrap an `Arc` in another `Arc` — making the
factory's `argument::<OriginalType>()` answer `None` silently.
**Unasserted:** no test pins this.

### 9. A route factory is handed the request only — the navigator accessor is withdrawn

**Choice:** the factory takes a `RouteRequest<'_>` carrying the request only —
`settings()`, `name()`, `argument::<T>()`. A redirect is expressed by *returning a
different route*, which is what a factory is for.

**Superseded, kept visible with its correction.** This entry originally added a
`navigator()` accessor handing the factory its own handle, on the argument below.
Both are withdrawn, for two reasons:

- **The argument was circular.** It existed to remove the *reason* to capture a
  handle. But a route's content never needed one either — a `RouteContentBuilder`
  receives `&dyn BuildContext` and `NavigatorHandle::maybe_of(ctx)` resolves from
  it. With no need to capture there was
  no cycle to avoid, and the accessor's only remaining use was navigating
  *during resolution*.
- **It had zero production call sites.** All four were tests, every one
  exercising that window.

What it did **not** do is close the window, and this was measured before the
removal rather than assumed: a factory that captures a handle mutates during
resolution identically, and with the capture-then-resolve defence reverted it
reproduces the original defect through that path. So removing the accessor
withdraws the *advertised* path and not the possible one. §5's defences stay, and
`RouteRequest`'s own docs now say **survivable**, not supported.

A runtime guard was considered and rejected: to earn deleting the defences it
would have to refuse mutation from `pop()` and `remove_route()` too, which return
`bool` — leaving silent failure, a `docs/PANIC-POLICY.md` violation, or a partial
guard that does not close the window anyway.

**Why the handle is not captured.** Rust does not collect cycles, and
the registry is owned by the navigator. A factory that captured a
`NavigatorHandle` — the obvious way to navigate from inside one — closed
`Arc<NavigatorShared>` → registry → `Rc<dyn Fn>` → handle → back on itself, so
the navigator's route stack, overlay entries and observers were never reclaimed,
not even after it unmounted. Nothing in the crate could break that cycle for the
user. Passing the handle in removes the *reason* to capture one, which is a fix
rather than a warning; the warning survives only as "if you capture one anyway,
the cycle is yours".

Note the route's *content* never needed a captured handle: a
`RouteContentBuilder` receives a `&dyn BuildContext` and `NavigatorHandle::maybe_of(ctx)`
resolves from it. So after this change
there is no remaining case where capturing is the right answer.

**Consequences, named rather than left to be discovered.** Two, and the second
is the one that cost a review round:

- It is a breaking change to every registration call site, deliberately so — a
  compile error at each, which is the cheapest it will ever be.
- **It made re-entrant navigation *advertised*, which is what surfaced §5's
  hazard.** Handing the factory a navigator turned "a factory could conceivably
  navigate" into a documented, ergonomic shape with a passing test. §5's
  captured-target fix, its three observable consequences, and §4's qualifier about a
  declining factory's own mutations were all written because of that. Withdrawing
  the accessor un-advertises the shape; it does not un-reach it, so all three
  survive the withdrawal — see the measurement above. What the accessor really
  cost, then, was not the defences (those defend an invariant that was always
  worth defending) but the six rounds it took to notice they were needed.

**Replacement tests:**
`a_route_key_carries_its_result_type_from_registration_to_delivery` pins that
the caller's arguments reach the factory through the request; that the request
also carries the caller's name: **Unasserted:** no test pins this. Whether
`navigator()` returned *this* navigator rather than any navigator is a claim
whose subject no longer exists, so nothing pins it and nothing needs to.
`a_factory_that_pushes_re_entrantly_does_not_deadlock` now obtains its handle by
an ordinary capture, through an `Rc<RefCell<Option<NavigatorHandle>>>` cell,
since the accessor it used to call no longer exists. It still pins that the
registry guard is released before the factory runs — holding it deadlocks the
owner thread — and its claim is now the weaker *survivable*, not *supported*.

### 10. `RouteKey<T>` moves the result-type check from run time to the registration site

**Rule:** as §9 above.

**Problem:** a route addressed by a bare string is re-typed unchecked, so
nothing connects `'/details'` to the `PageRoute<Order>` behind it.

**Choice:** `RouteKey<T>` — a `&'static str` plus `PhantomData<fn() -> T>`,
`const`-constructible so an app declares its routes once as constants.
`route_keyed(key, factory)` is bounded `R: NavigatorRoute + Route<Output = T>`,
so registering a `SimpleRoute<String>` under a `RouteKey<u32>` **does not
compile**; `push_keyed(key)` infers `T` from the key, so the caller writes no
turbofish and can produce no `ResultType`. `Clone`/`Copy`/`PartialEq`/`Eq`/`Hash`/`Debug`
are written by hand rather than derived: a derive would add `T: Clone`/`T: Hash`
bounds, and a key's identity is its *name* — the `T` is a compile-time promise
with no runtime representation. That is what lets keys live in a `HashMap` for an
`Output` that is neither `Hash` nor `Eq`.

Arguments ride on `RouteKey::with_arguments(args)`, producing a `KeyedSettings<T>` that
`push_keyed` takes via `impl Into<_>` — mirroring the string path's
`impl Into<RouteSettings>`, so `push_keyed(ORDER)` and
`push_keyed(ORDER.with_arguments(id))` are one method. Deliberately **not**
`push_keyed_with(key, args)`: on this handle `_with` means "a result delivered to
the departing route" (`pop_with`, `push_replacement_with`, `remove_route_with`,
`maybe_pop_with`), and spending that suffix on arguments would make one word mean
two things.

**Why `KeyedSettings<T>` and not a bare `RouteSettings`** — the question
`untyped()` provokes, since it converts between them. The type parameter is what
makes `push_keyed` *inferrable*: `push_keyed(ORDER.with_arguments(id))` needs `T`
to arrive from the value rather than from a turbofish, and that is the whole
ergonomic difference from `push_named_typed::<T>(..)`. A bare `RouteSettings`
would erase `T` at the builder and force the turbofish back, collapsing the keyed
path into the string path with extra syntax. So `KeyedSettings<T>` carries the
key's promise from `RouteKey` all the way to the push.

**`KeyedSettings::untyped`** is the explicit, one-way exit from that promise: it
discards the key's result type so a key's *arguments* can ride an untyped
operation — the one of eight `push_keyed` did not serve. It is a named verb
rather than a `From` impl on purpose. Dropping `T` on an untyped operation is not
a downgrade — it is §4's documented semantics, and it was already reachable as
`push_replacement_named(ORDER.name())` — but as a `From` the discard would be an
invisible coercion inside `impl Into<RouteSettings>`. The verb makes it the
caller's decision, visible at the call site.

**Replacement test:** a key's arguments carried onto an untyped operation, which
returning `RouteSettings::named(name)` without the payload would break.
**Unasserted:** no test pins this.

**The hole, stated rather than hidden.** A `RouteKey` type-checks one
*registration site*; the table it registers into is keyed by **name**. So any two
sites that disagree about one name meet at run time and the key's promise stops
describing what is registered. Three ways in, and the first is keyed-only — this
is *not* a defect confined to mixing the typed and untyped paths, which is what
this entry claimed before the keyed-vs-keyed case was written down and run:

- two `RouteKey`s spelling the same string with different `T`
  (`RouteKey::<u32>::new("/order")` and `RouteKey::<String>::new("/order")`),
  each self-consistent, both compiling, the second replacing the first;
- the same name bound through the untyped `route`;
- `on_generate_route` answering the name when the table misses.

Closing it properly needs a type-carrying registry key, which the string-keyed
table cannot express and the `on_generate_route` fallback would defeat
anyway.

**What the `TypeId` guard actually buys: a pre-mutation, non-panicking
failure.** Not type safety — the `RouteResult` downcast underneath is checked, so
a silently wrong result was never reachable. Removing the guard does not produce
a `RouteResult<u32>` fed by a `String` route; it makes `TypedPush::push` land the
push and *then* fail its `BUG:` `expect`, panicking mid-operation with the stack
already mutated. That is the stronger and more honest claim.

**Tests:**
`a_route_key_carries_its_result_type_from_registration_to_delivery` (dropping
`push_keyed`'s result handle fails it). A `RouteKey`'s identity being its name
and costing its `Output` type no bounds, and the two collisions — typed-vs-untyped and
keyed-vs-keyed, the case that falsified this entry's original claim — reported
as a clean `Err` rather than a **panic** inside `TypedPush::push` after the push
has landed: **Unasserted:** no test pins this.

### 11. A route's `settings` are write-only, so the factory relays values instead — recorded, with its trigger

**Choice:** the request's name and arguments are not relayed onto the built
route. No FLUI route builder accepts a `RouteSettings` —
`SimpleRoute` / `PageRoute` / `PopupRoute` offer `.named(name)` only — so a
named-pushed route's own `settings().argument::<T>()` is always `None`. The
factory reads `request.argument::<T>()` and **moves the value into the content
builder** instead.

**Why not close it now.** The relay would be public API on three builders
populating a field nothing can read. `ErasedRoute` — the framework's only view of
a route once it is in the history — does not expose `settings()` at all, and
workspace-wide the only consumers are two internal delegations
(`modal_route.rs`, `page_route.rs`) and one `Debug` field. There is no
`ModalRoute::of`, no observer that reports a route name, no name-based finder. A
pushed route's settings are written by its author and read by nobody. And the
capability a relay would serve is served by moving the value
into the builder, which is a typed capture.

**The trigger — this is the part that makes recording correct rather than a
trap.** The first read path added — an observer reporting route names, a
`ModalRoute::of`, a name-based route finder — makes the empty `settings`
silently wrong, and `with_settings` on `SimpleRoute` / `PageRoute` / `PopupRoute`
must land **in that same change**, not after it. `RouteSettings::with_arguments_shared`
(§8) already exists as the identity-preserving half such a builder needs.

**Test:** none, and deliberately — there is no behavior to pin, only
an absent capability. What is pinned is the *documentation*: `RouteRequest::settings`
no longer claims a relay that does not exist, which is what its doc said before.

### 12. A name re-registered with a different `Output` is reported at the registration site

**Rule:** as §11 above. The house rule for caller error in this repo is
repair-and-warn, not refuse.

**Problem:** a hazard `RouteKey<T>` creates by making a promise the name-keyed
table cannot keep.

**Choice:** each table entry stores `TypeId::of::<R::Output>()` and its
`type_name` beside the factory — `route` has `R` in scope and `route_keyed` knows
the key's `T`, so it costs one line and 16 bytes an entry. A re-registration that
changes a name's `Output` type emits a **latched** `tracing::warn!` naming the
route and both type names.

**Why a warn and not a `Result`.** Three reasons, in order of weight:

- It collides with §6's own contract. "The app builder replaces the table
  wholesale at mount" means an app deliberately changing a route's `Output`
  between rebuilds is making a *legitimate* change; refusing it would be wrong
  even restricted to type conflicts.
- It is programmer error at startup, and `Result` at every `route_keyed` call
  site is a large tax for that.
- Repair-and-warn is what this repo already does for caller misconfiguration.

Latched rather than per-call because an app that rebuilds its table in a loop
would otherwise emit one warning per pass. The latch is per **registry**, not a
process-global `static`, so one navigator's conflict cannot silence another's.

**The decision and the latch commit together, under the lock.** A review raised
that latching *after* the emit would let a `tracing` subscriber re-entering
registration during the warn see a stale flag and emit a second warning. Measured:
that half is not reachable — `tracing` suppresses re-entrant event dispatch on the
same thread, so the inner `warn!` never reaches a subscriber and the event count
is 1 either way. Committing both in one locked step keeps a subscriber that
re-enters registration from observing a stale latch.

**What this does not close.** The erased fall-through: a table entry that
declines and an `on_generate_route` / `on_unknown_route` that answers with a
differently-typed route. Nothing at those hooks' registration sites knows what
`Output` they will produce, so `GeneratedRoute::checked`'s runtime comparison
stays — with its documented remit shrunk to exactly that one shape for keyed
callers, instead of implying it is the general guard.

**A registration is user code, and displacing one drops it.** Every path that
replaces or discards a registration — `register_named`, `register_generator`,
`register_unknown_fallback`, `clear` — moves the displaced closure out of the
locked block and drops it with the guard released. Its captured state may run
arbitrary `Drop`, and a `Drop` that reaches back into the registry deadlocks a
non-reentrant `parking_lot::Mutex`. This is not hypothetical for `clear`: the
closure it drops is, by design, the one that captured a `NavigatorHandle`. There
is no compile-time oracle — dropping under the guard compiles clean and hangs —
so it is pinned by a test rather than by review, and the pin was written after
all four paths had shipped with the defect.

**Tests:**
`a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry`
covers all four displacement paths; reverting any one of them to drop under the
guard makes it **hang** rather than fail, which is why it asserts progress
counters as it goes rather than only at the end. The warning itself — once per
navigator, naming both result types, silent for a same-type replacement — the
re-entrant subscriber above, and the declining keyed entry whose generator's type
is still checked: **Unasserted:** no test pins this.

### 13. A `PopScope` callback runs before the observers, and may navigate, so an effect can be observed before its cause

**Rule:** as §4 above; same ADR.

**Choice:** `apply` runs step 0 — everything the flush owes user
code, including deferred `PopScope` effects — before step 1 delivers to observers.
So a `PopScope` callback runs **before** `NavigatorObserver.didPop`.

**Re-entrancy is permitted.** Refusing a synchronous navigation from inside the
callback would mean never having to sequence the interleaving; FLUI permits it,
deliberately, and the permission exists because refusing re-entrancy is what
produced a fan-out deadlock here. **The inversion is the price of that.** A
`PopScope` callback that navigates is observed before the pop that invoked it:

```
["push(RouteId(3), prev=Some(RouteId(1)))",
 "pop(RouteId(2),  prev=Some(RouteId(1)))"]
```

**Why not restore the refusal.** A debug-lock equivalent would revert a
restriction removed on purpose; refusing re-entrancy is the easier prohibition,
and this design sequences the interleaving instead.

**Replacement test:** a `PopScope` callback that navigates, permitted and observed
before the pop that caused it; swapping step 0 and step 1 would yield `[pop, push]`.
**Unasserted:** no test pins this.

### 14. `ParentDataView` ancestry is checked at attach, with catalog diagnostic labels

**Rule:** framework-user
composition errors must not surface as internal render-protocol panics.

**Choice:** misuse (e.g. `Expanded` under `Stack`) is rejected early with a
diagnostic naming the widget, its typical ancestor and the ownership chain, and
debug and release agree on the contract. It is expressed in Rust
as:

- `ParentDataView::{debug_type_name, typical_ancestor_description}` — catalog
  widgets (`Expanded`/`Flexible`/`Positioned`/`TableCell`/`LayoutId`) override
  with short labels and ancestor families;
- `RenderObject::child_parent_data_type_id` — the render parent declares the
  `ParentData` `TypeId` it expects on children;
- `ElementTree::apply_ancestor_parent_data` validates provider `TypeId` against
  that expectation **before** `set_parent_data` / `apply_parent_data_config`, and
  panics with the same semantic message in debug and release.

**Why not wait for layout.** Leaving the mismatch to
`BoxLayoutCtx::from_erased`'s `debug_assert!` made release/profile diverge, hid
the offending widget behind `TypeId` text, and let secondary bootstrap
`InvalidGeometry` panics mask the primary failure in harnesses.

**Tests:** `parent_data_ancestry.rs` (`expanded_under_stack_…`,
`positioned_under_row_…`) — assert the attach-seam diagnostic and that the
message is not `BoxLayoutCtx::from_erased`. Happy paths remain in
`flex_parent_data.rs` / `stack_positioned.rs`.

### 15. `Container` is one render object, not a conditional widget stack

**Rule:** a convenience widget's implementation shape must
not make the caller's unkeyed child state depend on which cosmetic options are
set.

**Problem:** a `Container` built as a conditional stack of `Align` / `Padding` /
`ColoredBox` / `DecoratedBox` / `ConstrainedBox` / margin `Padding` / `Transform`,
one level per set field, inserts or removes a level between the parent and the
child whenever a field is toggled, so reconciliation diverges there and an
unkeyed stateful child below is rebuilt from scratch. Alternatives (keyed
reparenting, a "compressed element" holding the intermediate widgets, a local
deactivated-element map) each duplicate an existing mechanism or add a new one.

**Choice:** render-level composition. `Container` is a `RenderView`
over one `RenderContainer` (`flui-objects`) that carries margin, additional
constraints, padding, alignment, color, decoration and transform as *fields*.
The child's slot is therefore structurally fixed and no option can move it.

**What it costs.** One alternative keeps composition in the render layer, with a
composed box building a real render-object subtree
(`RenderPadding` → `RenderDecoratedBox` → …) behind one widget/element. FLUI's
is a single render object that *re-derives* that subtree's geometry, paint,
hit-test and intrinsics as its own code. The upside is one node and no
composition machinery to build. The cost is that every contract the levels
would have inherited has to be re-proved here, and that is where this object's
defects have actually come from: an absent level and a zero-inset level are
not the same thing for hit-testing (which is why `padding` is `Option` and the
child-recursion gate is conditional — see **Hit-testing** below), and the
layered-range predicate now exists in two places (issue #1143). A composed
shape would make those classes unrepresentable rather than tested-for. It is a
legitimate future reshape, not a defect in this one; tracked in issue #1144.
Render-level composition fixes `Container` and not the general class, so any
other conditional-layer widget in this catalog keeps the same hazard.

Three properties follow:

* **State survives every toggle** with no `GlobalKey`, no retake, and no
  lifecycle churn — nothing for the caller to opt into, and no reparenting
  semantics leaking into an unmoved subtree.
* **The element tree is where the win is, not the render tree.** A conditional
  stack inflates and deflates an *element* per toggled option, and elements are
  not free: an upstream report on the equivalent widget-stack `Container`
  (2026-04-12) measures element inflation/deflation as a bottleneck during fast
  scrolling — "a thousand cuts problem" — and names constraining `Container` to
  a single element as a direct improvement. That is the load-bearing argument
  for this shape. The render-node accounting below is a cost statement, not
  the justification; read it as "what this costs", not "why we did it". FLUI has
  no equivalent measurement of its own yet, so this rests on an upstream
  maintainer's profiling, not ours.
* **Node count depends on whether there is a child.** With a child, identity
  (no options at all) in a widget stack passes the child straight through —
  zero extra nodes — so the stack is cheaper there. At exactly one option,
  the stack is also exactly one extra node (a single `padding` builds
  one `RenderPadding`), so node count ties. `RenderContainer` only wins on
  count from two options up, where the stack would otherwise add one level
  per option (up to seven if every option is set). **Childless, the stack is
  never free**: it reaches for a two-node placeholder (`LimitedBox` +
  `ConstrainedBox`) even with no option set at all (`Container()`), so
  `RenderContainer` already wins there. The only childless tie is a *tight*
  effective constraint — both `width` and `height` set, or an explicit tight
  `constraints` — which suppresses the placeholder and leaves the stack a
  single `ConstrainedBox` against one node here; a lone `width` does not
  qualify, since `BoxConstraints::is_tight` requires both axes, so that case
  still takes the placeholder and costs three. Every
  other childless option (color, padding, decoration, an alignment paired
  with a fixed size) only grows the stack's node count further, never brings
  it back below one. **What node count never buys, in either regime, is
  node weight**: `RenderContainer` carries every field — alignment, padding,
  margin, color, decoration, additional constraints, transform, plus the
  committed child offset/size/baselines — whether or not that option is
  set, so it is heavier than whichever single-purpose object the stack
  would have used, in every configuration including identity. The reason
  for this shape is the stable slot, not a cheaper or lighter
  `Container`.

**Intrinsics:** a tight additional width or height answers before the child
is queried, matching `RenderConstrainedBox`. Without that short-circuit a
`LayoutBuilder` (or any child that rejects speculative intrinsic queries)
would be asked even though the result is discarded.

**Parent-data transparency:** a widget-stack identity `Container` (every option
absent) would build to the child itself, so `Row → Container → Expanded` and
`Stack → Container → Positioned` attach the parent-data widget directly to
Flex/Stack. A `RenderView` always inserts `RenderContainer`
(`ParentData = BoxParentData`) between them, so those trees panic at
`apply_ancestor_parent_data`. That is a named consequence of the stable-slot
choice, not an accidental drop: restoring identity passthrough would rebuild
the unkeyed child's state the moment any option is toggled on. The supported
shape is `Row → Expanded → Container` / `Stack → Positioned → Container`.
**Unasserted:** no test pins this.

Parent data is not the only consequence of that always-a-node choice.
Hit-testing has the same shape one level up: `RenderContainer` always bounds
the incoming position against its own box before doing anything else, while
an identity widget-stack `Container` is not a node at all and so bounds nothing. A
child whose own `hit_test` deliberately does not bound itself — `RenderTransform`
is the documented case — is therefore reachable outside the container's box in
that stack and not here, whenever *no* margin and *none* of the five gated
properties are set. Measured under a shared `RenderPadding` parent with a
scaled child, three of four probe points outside the box hit in the stack's tree
and miss here. Unlike the margin-band gate below, this one is **not** closed:
the gated-level reasoning that fixes that case does not extend to the outer
gate, because at identity there is no level to reason about — the node itself
is the difference. Tracked in issue #1143.

**Collapsed branch:** the stack's three childless shapes — the placeholder
`LimitedBox(0, 0, child: ConstrainedBox(expand))`, an empty `Align`, and no
inner widget at all — all resolve to the same box, so `RenderContainer` has no
childless branch. `harness_container_childless_fills_bounded_and_collapses_unbounded`
pins the box it resolves to against hand-computed sizes (a bounded axis fills,
an unbounded one collapses, each axis independently); the equality with each of
the three shapes, diffed against `RenderContainer` and against each other:
**Unasserted:** no test pins this.

**Not carried over:** `foregroundDecoration`, `clipBehavior`, `isAntiAlias` and
`transformAlignment` have no FLUI `Container` setter. These are four
different kinds of gap, not one undifferentiated "not yet":

- **`clipBehavior` does not fit this shape at all.** [`PaintEffects`] gives a
  node exactly one clip slot, wrapping everything the node's `paint` records
  as one fragment. A stacked `ClipPath` (the `clipBehavior != Clip.none`
  branch) sits between `ColoredBox` and `DecoratedBox`:
  it clips the padding, color and child, and explicitly does **not** clip
  the decoration (`DecoratedBox` wraps the already-clipped `current`
  afterward, unclipped). `RenderContainer::paint` records decoration, color
  and child as one fragment, so a `PaintEffects.clip` here would clip the
  decoration too — wrong. Adding this needs a paint-level re-split (a second
  recorded fragment, or a clip scoped to a sub-range of one), not a new
  `Option` field.
- **`foregroundDecoration` is additive.** A second decoration field, a
  `paint_box_decoration` call after the child (painted on top rather than
  behind), and a hit arm —
  `DecoratedBox`'s own doc states a foreground decoration participates in
  `hitTestSelf` exactly like the background one does.
- **`transformAlignment` is additive but not local.** It needs an
  `alignment: Option<Alignment>` field folded into the pivot the way
  [`RenderTransform::effective_transform`] already combines one with its own
  base matrix, and that combined value would have to move together through
  `apply_paint_transform`, `hit_test`'s inverse, `paint_translation`,
  `skip_paint` and `owns_effect_layer` — every site that reads `self.transform`
  today.
- **`isAntiAlias` is additive and narrow.** In the stack it is a
  `ColoredBox`-only flag; nothing else in the stack reads it. FLUI's own color fill
  (`ctx.canvas().draw_rect(rect, &Paint::fill(color))`) always anti-aliases
  (`Paint::fill`'s default), with `Paint::with_anti_alias` already available
  to turn it off — adding the setter is one field plus one call-site change,
  not a structural gap.

A `BoxDecoration` border's thickness is separately still not folded into the
effective padding (`_paddingIncludingDecoration`) because
`flui_painting::styling::BoxDecoration` exposes no border insets. `color`
and `decoration` are not mutually exclusive: FLUI accepts both and paints color
over the decoration — the order the widget stack would have produced
(`DecoratedBox` enclosing `ColoredBox`) — rather than panicking.

**Tests:** the geometry the collapsed stack owes is pinned against
the stack itself by `harness_container_matches_the_widget_stack_it_collapses`
(size, child size, absolute child position and hit path, over eight
configurations spanning both wet layout and hit-testing, each making a
different level decide). Chrome self-hit uses the same half-open gate as the
stacked `DecoratedBox`: the same differential probes the exclusive max edges of
the chrome. The decorated box's own paint rect inside the margin — the level a
stacked `DecoratedBox` exposes and the one a single node no longer exposes as a
separate render object — state stability of an unkeyed child as each optional
property toggles (on `Container` and `AnimatedContainer`), tight additional
constraints answering an intrinsic without querying a `LayoutBuilder` child, and
baselines adding the child's offset: **Unasserted:** no test pins this.

**Hit-testing gates the child behind the SAME boxes the stack does — only
when a level exists to gate on, and none always does.** `RenderContainer::
hit_test` tests the child before the decoration/color path (a child hittable
in a cut-out the decoration's rounded corners exclude must stay reachable),
but ordering is not the only thing that has to match the stack: the stack
inserts `Padding`/`ColoredBox`/`DecoratedBox`/`ConstrainedBox`/`Align`
between `Padding(margin)` and the child only when `padding`/`color`/
`decoration`/`additional_constraints`/`alignment` (respectively) is set —
`_paddingIncludingDecoration` is null, and so no `Padding` level exists,
precisely when `padding` is unset (FLUI's `BoxDecoration` never contributes
a padding of its own, so a decoration alone can't supply one either). With
NONE of those five set, the composed shape is `Padding(margin) → child`
with nothing between them, and nothing gates a hit-test there either — a
`RenderContainer` that always applied the `inner_size` gate regardless would
reject a tap the real stack accepts. `padding` is therefore `Option
<EdgeInsets>` on `RenderContainer`, not a plain `EdgeInsets` defaulting to
zero: `None` (unset) and `Some(EdgeInsets::ZERO)` (explicitly zero) are
geometrically identical but hit-test differently, since an explicit zero
inset still gets a real (zero-inset) level.

Once at least one of those five IS set, every level that exists reports the
same margin-offset `inner_size` box and rejects a position outside it
(`is_within_own_size`) before ever reaching the child, and when an alignment
is set specifically, the stack additionally inserts an `Align` level gating
on the narrower CONTENT box (inside the margin AND the padding) — a gate
whose OWN condition needs nothing else, since an `Align` level exists
whenever alignment does, independent of whichever of the other four are
also set. A collapsed node that let a child overflow past either gate, once
its condition holds, would expose a child whose own `hit_test` does not
bound itself to its laid-out box — `RenderTransform` deliberately does not,
so a scaled child stays hittable across its whole visually-overflowing
area — to a tap the real stack rejects.

Pinned in both directions by two cases of
`harness_container_matches_the_widget_stack_it_collapses`: a non-zero margin
with nothing else set (the overflowing child's tap in the margin band DOES hit)
against the same margin with a real padding level added (the identical tap does
NOT). `padding` is `Option<EdgeInsets>` on `ContainerStackCase` for the same
reason it is on `RenderContainer` — a composed tree that always inserted a
zero-inset `Padding` level would silently endorse the difference instead of
detecting it. The narrower content-box gate (padding with alignment set does not
expose an overflowing child, and the same padding without alignment does not
narrow the gate): **Unasserted:** no test pins this.

### 16. A push's entrance-transition future is awaited outside the flush that installed it

**Rule:** a flush-timing constraint governs the local placement, so the decision
belongs here; [ADR-0064](../../docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md)
records the cross-crate design this decision consumes.

**Choice:** `PushCompletion::Animating(AnimationRunFuture)` carries the future
`AnimationController::forward()` (or an equivalent run-starting call) returns,
but the continuation that awaits it is registered from `NavigatorShared::apply`
— after the flush that produced the entry has released the history lock —
never from inside `RouteEntry::handle_push` itself. `RouteHistory::flush()`
re-drains any `RouteCommand`s a route raised between its own passes, so a
continuation registered mid-flush on an already-resolved future (a
zero-duration push, or one canceled before the flush even returns) would
settle within that same flush rather than on the next one. The continuation itself may only
push `RouteCommand::PushCompleted(id)` onto the `Send` route-command queue and
schedule the Navigator's rebuild through `NavigatorShared::settle_wake`, read
at the moment the continuation fires rather than captured at registration:
`NavigatorHandle::push` flushes immediately, so a route pushed before the
Navigator mounts registers its continuation while that slot is still empty,
and only a later mount fills it. Cancellation settles the entry exactly like
completion — there is no separate "the push was canceled" state at this
layer, only whichever lifecycle state the entry has moved to by the time the
queued command is drained.

### 17. `Semantics` action handlers are owner-local `EventCx` callbacks behind one `Send + Sync` ticket handler per node

**Rule:** the lane mechanism is ADR-0086 §3 (amended 2026-09-30); how this
widget uses it is local to this crate, so it is recorded here rather than in an
ADR.

**Choice:** eleven payload-free builders — `on_tap`, `on_long_press`,
`on_scroll_{left,right,up,down}`, `on_increase`, `on_decrease`,
`on_show_on_screen`, `on_focus`, `on_blur` — take
`Fn(&mut EventCx<'_>) -> R`; `on_set_text` takes `Fn(&mut EventCx<'_>, &str)`,
`on_scroll_to_offset` takes `Fn(&mut EventCx<'_>, f64, f64)`, and `on_action`
takes any `SemanticsAction` with `Fn(&mut EventCx<'_>, Option<ActionArgs>)`
for what the typed set does not cover. None needs `Send + Sync`: the closures
never reach the configuration.

**Why the handlers stay owner-local, and how the storage bound is met.**
`SemanticsActionHandler` is
`Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>`
(`crates/flui-semantics/src/action.rs`), stored in a `SemanticsConfiguration`
that rides in the annotation render object, which `RenderView::RenderObject`
pins `Send + Sync + 'static`. The handler is *not* invoked across a thread —
resolution is owner-local (`PipelineOwner::resolve_semantics_action`) and the
UI runtime drains it at a frame boundary, inside its entry. So the widget keeps its
closures and its `WriterSource` in the interaction lane as one table per node,
stores only the lane's ticket on the render object (`SemanticsActionRoute`),
and advertises every action through one `Send + Sync` handler that holds the
ticket and resolves it when invoked. This is the shape the surveyed frameworks
point at — none puts `Send` on the handler; Bevy, Iced, Slint and Dioxus/Blitz
put it on something the widget owns — reached through ADR-0086's lane payload.

**Consequences, named rather than left to be discovered:**

- **The ordinary "activation toggles this control's own state" closure is a
  signal write.** `on_increase(move |cx| value.update(cx, |v| *v += 1))`
  (`an_action_handler_writes_a_signal_and_rebuilds_its_reader`); a refused
  write is reported, not panicked
  (`a_refused_write_in_an_action_handler_is_reported_not_panicked`).
- **An action invoked outside its UI runtime is dropped with a warning.** A caller
  that holds a `SemanticsActionInvocation` and invokes it with no UI runtime entered
  has no owner to run the closure in
  (`an_action_invoked_outside_its_ui_runtime_is_dropped_with_a_warning`). A node
  mounted in a detached render-object context advertises none of these
  actions, so no platform sees a control nothing can run
  (`a_detached_mount_advertises_no_actions`). Unmount releases the node's
  table from the lane, closures and their captures with it
  (`unmounting_a_node_releases_its_action_table`; a `DragTarget`'s slot the
  same way, `unmounting_a_target_releases_its_slot`).
- **This publishes actions; it does not make any shipped control
  activatable by itself.** No Material or Cupertino widget gains a semantics
  action here; `Button`, `Checkbox` and `ListTile` publish no tap semantics of
  their own, so that wiring belongs at `InkResponse`'s layer when it lands.
  `GestureDetector` advertises its own `on_tap`/`on_long_press` and delivers
  them one frame late through its local post-frame bridge, while a raw
  `Semantics` handler runs synchronously in the drain.

**A rebuild with a fresh closure costs no semantics update.** The
configuration compares handlers with `Arc::ptr_eq`
(`crates/flui-semantics/src/configuration.rs`). The per-node ticket handler is
minted once, at mount, and reused by every update, which replaces only the
lane's table under the same ticket; so a closure literal in `build` leaves the
mounted configuration equal and `set_configuration` answers `NONE`, and the
action runs the rebuilt closure
(`rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one`).

**Builder inventory, and what has no builder.** FLUI's action vocabulary
(`crates/flui-semantics/src/action.rs`) has 24 `SemanticsAction` variants, of
which **9 have no inbound route at all** — the eight the translation table drops
deliberately (the four cursor moves, copy, cut, paste, and dismiss) plus
`DidGainAccessibilityFocus`, which is advertised outbound and unreachable
inbound. Of the 15 routable ones, 13 have a typed builder and `SetSelection` /
`CustomAction` are reachable through `on_action` only. `expand` / `collapse`
have no `SemanticsAction` variant to map to, so they are not merely unwired. `on_blur` is deliberately *not* the
mirror of `on_focus`: the platform reports losing accessibility focus as a
notification about something that already happened, whereas a focus request is a
command the node may refuse.

**A payload that did not cross the seam is dropped, not defaulted.**
`on_set_text` and `on_scroll_to_offset` trace a `warn!` and do nothing when
`ActionArgs` arrives without the matching payload. `""` and `(0.0, 0.0)` are
both legitimate values a platform can mean, so substituting either turns a lost
payload into a silent edit or a scroll to the origin — a wrong result that reads
as a right one.

**Tests** (`tests/semantics.rs`), each with what it can fail on:

- `a_covered_retained_form_stays_absent_after_a_late_controller_update`
  observes both assembled semantics and actual owner-flush packets. Queries
  follow the current root's child links, so disconnected historical payloads
  cannot count as exposed controls. A focused retained form disappears under
  an opaque entry, remains absent after a real controller notification without
  forcing the root dirty, refuses its old action and returns with its draft and
  action intact. This is a headless overlay contract; it does not establish
  native adapter behavior or elapsed route-transition completion.
- `a_tap_handler_round_trips_from_a_platform_click_to_the_callback` — the
  acceptance test, and it asserts both halves: the node *tells* the platform the
  action exists (`supports_action(Action::Click)`) and pressing it *runs* the
  callback exactly once. A node passing only the first half is the dead control
  this surface exists to rule out.
- The negative control — a node with no tap handler advertises no click — so
  the advertise half is not satisfied by a node that advertises everything.
  **Unasserted:** no test pins this.
- The payload path — a set-text request carries its payload into the handler —
  which no payload-free test reaches. **Unasserted:** no test pins this.
- `a_set_text_request_without_a_payload_is_dropped_rather_than_emptied` — a
  request that lost its payload resolves, and the handler does not run.
- `the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set` —
  derives the unreachable set from the live translation table and asserts it
  equals the documented nine, so a table change that closes one is loud rather
  than a silent improvement nobody notices.
- `the_exhaustive_routing_list_agrees_with_the_translation_table` — the routing
  predicate is written out exhaustively and then checked against the production
  table, so it cannot drift into a second copy of the answer. Because
  `accesskit::Action` is not `#[non_exhaustive]`, an upstream release that adds
  a variant stops this file compiling rather than silently dropping it.
- `block_user_actions` refusing a click the node still holds a handler for, in
  both halves. The refusal half: the request errors *and* the handler does not
  run. The advertise half: the blocked node does **not** advertise the click —
  `blocks_user_actions` narrows the effective action set that snapshot export and
  input dispatch both consult, so the node is invisible to assistive technology
  instead of a control it can see and press to no effect.
  **Unasserted:** no test pins this.

**Red→green, measured rather than asserted.** Both halves of the acceptance test
were shown to fail against a mutated builder and then pass against the restored
one: replacing the handler invocation with a discarded binding turns
`a_tap_handler_round_trips_…` red on *its own* delivery assertion
(`left: 0, right: 1` — the advertise assertion above it stays green, so the two
halves are independently pinned).

### 18. Replacing a `HeroController` retires its in-flight flights, restoring both heroes

**Rule:** FLUI names the rule, justifies it, and pins it with a test. This entry
is local to the crate, so it lives here rather than in an ADR.

**Choice:** when a `HeroController` is detached — replaced by
`NavigatorHandle::add_observer` (which takes the auto-installed default),
removed by `remove_observer`, or its navigator unmounts —
`HeroController::did_detach` calls `FlightManager::finish_all`, which **aborts**
every flight still in the air: it removes each overlay entry and calls
`end_flight(false)` on **both** heroes. Distinguishing an abort from the
ordinary `finish` is load-bearing: a normal `finish` ends one hero hidden and
the other revealed, chosen by the terminal animation status, whereas an abort has no status to choose with
and must leave both pages — which stay alive and in the stack — showing their
real children rather than a blank placeholder.

**Why abort rather than leave the flight.** The controller is a swappable
observer, so a detach does not tear down the tree and frozen placeholders would
stay visible. The flight it
launched is retired through a `Weak<FlightManager>` held by the overlay entry's
shuttle (`FlightManager::finish`'s `manager.upgrade()`). A detached controller
drops its `FlightManager`, so that upgrade returns `None` from then on: the
flight can never be retired, its overlay entry leaks, and the shuttle paints at
its end rect forever. Finishing/cancelling (not transferring to the replacement)
is the rule because a flight is owned by the controller that launched it — its
`from`/`to` route animation values and its gesture wiring all belong to that
controller's measurement pass, and re-parenting them onto a replacement would
mean guessing a flight plan the replacement never measured.

**Consequences, named rather than left to be discovered:**

- **The auto observer's flight is gone the moment a manual controller is
  installed.** `NavigatorHandle::add_observer` calls `did_detach` on the
  auto-installed default before attaching the new observer, so installing a
  `HeroController` mid-flight retires the auto observer's flight immediately —
  the overlay count returns to its pre-flight value in the same call. The
  fixture `gesture_fixture_with`'s doc documents this rather than the previous
  "survives `install`" leak.
- **`finish`'s retired-then-drain discipline is preserved.** An abort goes
  through the same `retire` (drop the flight from the registry, park it in
  `retired`, schedule the coalesced end-of-frame drain) as a normal `finish`,
  because `did_detach` runs owner-local (from `add_observer`/`remove_observer`/
  dispose) and never inside an animation status listener — but the park still
  costs nothing and keeps the drop outside the animation listener family, the
  one invariant the type docs rest on.

**Replacement test:** with two same-tagged hero pages pushed so the auto observer
launches a real programmatic flight, installing a manual controller returns the
overlay count to its pre-flight value, leaves the replacement controller with no
inherited flight, and clears both heroes' placeholders; deleting the
`finish_all` call from `did_detach` would leave the overlay count one entry high
and both placeholders set. **Unasserted:** no test pins this.

### 19. Word and grapheme movement use ICU4X (UAX #29), not dictionary segmentation

**Rule:** Design stance ("Look around before settling") — search the market/existing dependency graph
before adding one, and cite what an unmatched reference actually needs.

**Choice:** the controller's grapheme steps (arrow keys, Backspace, Delete,
the selection setters' snap) and keyboard word jumps walk the boundaries in
`flui_painting::text_boundaries`: ICU4X's grapheme segmenter and its word
segmenter for non-complex scripts, the data Parley already brings and clusters
the painted text by (ADR-0092 §6). The obscuring mask and the text store's
obscured ranges count the same clusters. Double-tap word selection
(`TextPainter::get_word_boundary`, flui-painting mapping decision 15) picks
from the same word segments with its own tie-break. So a caret an arrow key
places is one a tap can place, and a Backspace removes what was drawn as one
character. `flui-widgets` has no `unicode-segmentation` dependency, and
`deny.toml` bans it for every FLUI crate.

A query segments from the start of the line that holds its offset (a break
after LF is mandatory for graphemes and words), so a step costs the length of
its line, not of the buffer; the backward word jump walks forward one line at
a time and allocates nothing.

**Background:** ICU's word-mode break iterator (the usual backing for Ctrl+Arrow
word-jump and double-tap word selection) is **dictionary-based**
for two distinct groups: Thai, Lao, Khmer, and Myanmar (scripts with no
spaces between words at all, where the Unicode Standard Annex #29
default algorithm — rule-based, no lexicon — cannot find a linguistically
correct boundary on its own), and separately Chinese/Japanese, via ICU's
`cjdict` word-frequency dictionary (these scripts DO have UAX
#29-recognized character-class boundaries, so the rule-based default
does not fail outright the way it does for the first group — it just
segments per character/script-run rather than per linguistic word).

**Why no dictionary.** The dictionary and LSTM data — both the
Thai/Lao/Khmer/Myanmar lexicons and `cjdict` — are the expensive part,
megabytes of data, not an algorithm, and Parley's `complex-scripts` feature,
which would bring them, stays off (ADR-0092 gate 4). Every script with
UAX #29-recognized boundaries (Latin, Cyrillic, Greek, Arabic, Hebrew,
Hangul, and more) already works without it.

**Tests:** `the_editor_steps_the_graphemes_the_painter_snaps_to`
(`tests/editable_text.rs`, the `text_editing` table) steps the arrow keys and
Backspace through a ZWJ family, two flags, stacked combining marks, CR LF and
the conjunct "क्षि", and compares the stops with
`flui_painting::text_boundaries::graphemes`. `unicode-segmentation` 1.13.3
clusters every one of those cases the same way, so the row passes on the
earlier controller too: it pins the shared contract, and the `deny.toml` ban
is what fails if the second segmenter comes back.

**Consequences, named rather than left to be discovered:**

- **Thai/Lao/Khmer/Myanmar word-jump and word-select do not find any
  linguistically meaningful boundary** — no whitespace and no
  UAX #29-recognized word-class transition to key on means the segmenter
  falls back to its rule-based default (effectively per-cluster stops).
- **Chinese/Japanese word-jump and word-select segment per
  character/script-run, not per linguistic word** — `"東京"` ("Tokyo",
  one word to ICU's `cjdict`) splits into `"東"` and `"京"` here, because
  Han characters have no special UAX #29 word-class merging rule by
  default (unlike consecutive Katakana, which UAX #29's own `WB13` rule
  does merge — a script-specific rule already correct here, no dictionary
  needed for it). A known limitation, not a silent bug.
  **Unasserted:** no test pins this.
- **Every other script named here — Arabic (clusters-only, see the caveat
  below), Latin with apostrophes, emoji clusters — segments correctly** per
  UAX #29's own rules, which is the ceiling this crate claims for them; no
  claim is made about Hebrew.
- **Grapheme-cluster correctness is unaffected.** The dictionary gap is
  specific to WORD boundaries; cluster boundaries (caret, Backspace,
  Delete) come from ICU4X's grapheme segmenter, a different UAX #29 mode
  with no dictionary dependency, and are correct for
  every script including all the ones named above.
- **The word-jump modifier's platform source is compile-time only, and
  that is already known wrong for at least one real target.**
  `is_word_jump_modifier` resolves `TargetPlatform::current()` — a
  `cfg(target_os)` compile-time constant — once per `EditableText` key
  handler, in `build_key_handler`. `wasm32` matches none of
  `current()`'s `target_os` arms and falls through to `Unknown` →
  Control, but a `flui` app compiled to `wasm32` and running in a Safari
  tab on macOS needs Alt instead: native Ctrl+Arrow is the OS's own
  Spaces-switch shortcut there, so a page that binds Control for
  word-jump would never even see the chord. Web and embedded targets
  need `TargetPlatform` resolved at RUNTIME, not baked in at compile
  time — the exact gap `GestureSettings::native`'s own doc already flags
  for the analogous gesture-settings case
  (`flui-interaction/src/settings.rs:286-292`: "Anything that can host
  more than one platform feel ... must resolve a runtime
  `TargetPlatform` and call `for_platform` instead"). `word_jump_modifier`
  is already split into a pure `(platform) -> Modifiers` function
  specifically so a future runtime-resolved source has one place to
  plug in — `build_key_handler`'s `let platform = TargetPlatform::current();`
  — without touching the mapping table itself. Not fixed here: this
  change did not add a runtime-platform-resolution mechanism to
  `flui-widgets`, and inventing one for this one call site would be
  premature relative to `GestureSettings`' own still-open gap.

**Replacement tests:**
`two_space_run_word_boundary`, a row of flui-painting's `caret_contract`
(`crates/flui-painting/tests/caret_contract.rs`), pins a specific boundary rather than a loose "some boundary was found" check:
`"foo  bar"` at 3/4/5, where an offset inside the two-space run selects the
whole run. The controller's word-jump stops (skipping trailing whitespace, a
whitespace run as one stop, an apostrophe inside a word, the CJK per-character
split), the word-jump modifier's platform table, and the rest of the
`get_word_boundary` tie-break matrices (`"foo bar"` at 0/3/4/7; `"(foo"` at 1;
`"日本語"` at 3; `"foo, bar"` at 3/4; leading/trailing whitespace at a buffer's
own edges; ZWJ emoji clusters): **Unasserted:** no test pins this. For Arabic
the claim is only that a word jump never lands inside a char and always makes
progress, not any cluster-vs-word segmentation for that script.

### 20. `GestureDetector` composes AROUND `Listener`, not inside it, for double-tap word selection

**Rule:** Design stance ("Look around before settling") — reuse tested framework machinery
(`flui_interaction::DoubleTapGestureRecognizer` via
`flui_widgets::GestureDetector`) rather than hand-rolling tap-count/slop/
timeout tracking a second time inside `EditableText`'s own pointer
handlers.

**Choice:** `EditableTextState::wrap_double_tap_word_select` composes the
existing, generic `flui_widgets::GestureDetector` as the OUTER parent of
`install_pointer_handlers`'s `Listener`-based field, not the reverse.
`Listener` stays exactly as it already was — arena-free (its own
established, tested contract) — so plain tap-to-place-caret and
drag-to-select are byte-for-byte unaffected by the double-tap
recognizer's presence next to them. `GestureDetector`'s
`DoubleTapGestureRecognizer` watches the SAME pointer stream in parallel
(both layers use `HitTestBehavior::Opaque`, so both receive every
contact) and its `on_double_tap_down` callback — a genuine new capability
this change adds to `GestureDetector`/`DoubleTapGestureRecognizer`, not
previously exposed — widens the caret `Listener` just placed into the
enclosing word, via
[`TextPainter::get_word_boundary`](#19-word-and-grapheme-movement-use-icu4x-uax-29-not-dictionary-segmentation)
— ICU4X word segmentation, over the same segments Ctrl/Alt+Arrow word-jump
walks, and NOT the same function: the keyboard
path's `next_word_boundary`/`prev_word_boundary`
(`crates/flui-widgets/src/text/controller.rs`) answer a directional
"next/previous stop" query with their own asymmetric tie-break, while
`get_word_boundary` answers "which segment is under this exact
position" with a different one — see decision #19's own tie-break note.
A double-tap and a keyboard word-jump can therefore disagree at the
exact boundary between two segments; unifying them behind one primitive
would need a dependency edge `flui-widgets` does not currently have
(`flui-painting` is a dev-dependency only) and is not attempted here.
`drag_anchor.set(None)` in the same callback stops the plain tap/drag
handling this composition wraps from clobbering the word selection on
the second contact's next move before it lifts — near-guaranteed on
touch, where a finger is essentially never perfectly still between down
and up.

**Why not a text-specific detector.** A specialized gesture-detector subtype
would also own triple-tap and drag-selection-handle gestures that do not exist
yet; building one for a single callback would be premature machinery for what
`GestureDetector`'s existing generic API, widened by one method, already covers.

**Consequences, named rather than left to be discovered:**

- **`on_double_tap` itself is deliberately left unset** on this detector
  — word selection only needs the earlier `on_double_tap_down` signal, not
  confirmation that the second contact also lifted cleanly.
- **This exposed a real participation-gating gap in `GestureDetector`
  itself**, fixed in the same change:
  `RecognizerGroup::double_tap_active` checked only whether `on_double_tap`
  was set, so a detector configured with only `on_double_tap_down` would
  never have joined the arena and its own callback would never have
  fired. Caught before it shipped by writing a detector-only test first.

**Replacement tests:**
`flui-widgets::tests::editable_text::a_double_tap_selects_the_word_under_it`
is the end-to-end proof that a double-tap on a mounted `EditableText` actually
selects the word, not just that the underlying callback fires. Red-check: skip
wrapping `install_pointer_handlers`'s return value in
`wrap_double_tap_word_select` — the selection stays collapsed after the second
tap. It also pins the DOWN-not-UP timing, since it asserts the word selection
on the second contact's DOWN before that contact lifts, and the
participation-gating fix, since this detector sets `on_double_tap_down` alone
and would otherwise never join the arena.

### 21. The `Router` is derived from the route type, and its handle is lifecycle-only

**Choice:** `Router<R>` (`src/router/`, ADR-0093) needs only `R: Routable` —
`to_path`, `from_path`, and a provided `back_stack` — and keeps its stack
of `R` itself; the parser and delegate are the route type. The page per value
is a `PageRoute<()>` named with the value's path, on a `Navigator` the router
builds, so transitions, heroes and `PopScope` are unchanged.
`Router::<R>::handle` takes `&dyn LifecycleContext` (ADR-0078), so a handle
is acquired in `init_state`/`did_change_dependencies` and resolves the
**nearest** `Router<R>`, as `Navigator`'s handle does. The page builder
and transitions are shared with every page, so a parent rebuild reaches the
pages already on the stack; a transition duration is fixed when a page is
placed, because the page's animation controller is made with it.

**Pinned by:** the `compile_fail` doctest on `Router::handle`. A nested handle
resolving the nearest `Router<R>`, a parent rebuild reaching the pages already
on the stack, a handle with no `Router` above it answering `NoRouter`, and a
hand-written `Routable` round trip: **Unasserted:** no test pins this.

### 22. A Router's navigator refuses pages pushed through its facade, and admits pageless popups

**Choice:** every page on a Router's stack has a path (ADR-0093 §2), and only
the Router places, replaces or seeds pages. The navigator a Router builds is
*addressed*. A plain `push` (and `push_named`) admits only routes whose
binding slot a framework `TransitionRoute` marked `TransitionGroup::Default`
— `PopupRoute` — and refuses `PageRoute`, `SimpleRoute` and every
third-party route. The doors that replace, sweep or seed
(`push_replacement[_with]`, `push_and_remove_until`, `seed_initial`, and the
named `push_replacement_named[_with]`, `pop_and_push_named[_with]` and
`push_named_and_remove_until`) refuse every route, a popup included, because
what they remove or place beneath belongs to the Router, whose stack would
otherwise name a page that is gone. The typed doors cannot return a `Result`
without a public signature change, so a refusal disposes the route unpushed,
returns a `RouteResult` already complete with `None`, logs `tracing::error!`
with the reason, and fails a `debug_assert!`. The named doors answer
`NamedRouteError::NotAddressable` before anything is resolved, dismissed or
pushed. Popups stay admitted, because `show_dialog` pushes a `PopupRoute` on
the root navigator, until dialogs move to overlay entries (ADR-0093 §4). Every
pop the navigator makes — the facade's, a back gesture's, a barrier's —
reaches the Router's stack through an internal `NavigatorObserver`, so the
location follows it.

**Pinned by:** `popup_routes_are_admitted_and_leave_the_location_alone`. A
`PageRoute` pushed through the facade refused as not addressable, the doors that
remove or seed refusing even a popup, and a facade pop updating the Router's
location: **Unasserted:** no test pins this.

### 23. A Router never pops or removes its last page

**Choice:** a Router always has a location. The navigator records the pages
the Router places, and no pop or removal takes the last present one, whether
it is on top or beneath a popup: `RouterHandle::pop` answers `Ok(false)`, the
facade's `pop`, `pop_with` and `remove_route[_with]` answer `false`,
`maybe_pop` bubbles (as a lone route does), and `pop_until` stops
there, all with the stack unchanged. A top page that handles the pop itself
(a local-history entry) still pops, since that removes no page.

**Pinned by:** `router_never_pops_its_last_page` (the handle's `pop`, and the
facade's `pop` and `pop_with`). `pop_until` stopping at the last page, and the
last page beneath a popup surviving removal and pops: **Unasserted:** no test
pins this.

### 24. `go` reconciles by common prefix

**Choice:** `RouterHandle::go(location)` derives the new stack with
`Routable::back_stack` — every prefix of the path that parses, the full path
required — and keeps the longest common prefix with the current stack
(compared with `PartialEq`). A new stack that is a prefix of the current one
pops back to it with exit transitions; otherwise the pages above the common
prefix are removed and the new pages are placed in one flush, the ones
beneath the new top entering quietly (`RouteLifecycle::Add`) and only the new
top running its entrance. The pages that stay keep their state; a page above
the divergence point is rebuilt. When the full path does not match, `go` and
`Router::from_location` report `RouteParseError::NoMatch` and change nothing,
rather than falling back to a default route.

**Pinned by:** `router_opens_at_a_location_with_its_back_stack` (the matching
prefix chain beneath an opened location, across the gap `/note`, and
`Router::from_location` reporting `RouteParseError::NoMatch` for a path that
does not match in full). `go` reconciling only the diverging tail, adding the
new back stack beneath the new top, and leaving the stack alone for an unknown
location: **Unasserted:** no test pins this.

### 25. Every Router page scopes a semantics route, and a labelled route names it

**Choice:** a Router page is an addressable screen, so the
Router wraps each one in `Semantics::scopes_route(true)
.explicit_child_nodes(true)`, and adds `names_route(true)` with the label when
`Routable::semantics_label` returns one. An assistive technology then hears a
route change on every navigation, with no app bar required.

**Unasserted:** no test pins this.

### 26. Global widgets localizations live in the catalog, not in a separate package

**Choice:** `GlobalWidgetsLocalizations`, `GlobalWidgetsLocalizationsDelegate`
and `RTL_LANGUAGES` live in `flui_widgets::localization`, next to the
`WidgetsLocalizations` contract they implement.

**Why not a separate package.** A separate package pays off when it carries
translated strings. Here there are none: every string
forwards to `DefaultWidgetsLocalizations`, and the only behavior is the RTL
language table and the delegate. A separate crate held that one table in a
layer of its own, which ADR-0081 deleted; the table moved down into the
crate whose contract it implements.

**Consequences:**

- The delegate's `is_supported` is always `true`: a gate on "this locale has
  translated strings" would, with no translations for any locale, only
  produce false negatives.
- Translated string catalogs, when they arrive, are a separate decision
  about where FLUI sources translations; they do not reopen a crate here.

**Tests:**
`flui-widgets::tests::localizations::the_global_delegate_makes_an_rtl_locale_subtree_rtl`
mounts `Localizations` with the global delegate for `ar` and reads
`Directionality::of` from inside the subtree; the unit tests in
`localization/global_widgets_localizations.rs` pin the table, the `iw`
alias, and `is_supported`.

### 22. A form field validates at the event, not in `build`

**Choice:** `build(&self)` cannot mutate, so the autovalidate switch runs at the
moments that would schedule a build: `did_change` (a user edit),
`init_state` (mount), `did_update_view` (reconfiguration), a field joining an
`Always` form, the field's focus wrapper losing focus, and the form's
`validate()`/`reset()`. A field whose shown error changed schedules its own
rebuild through the `RebuildHandle` it took in `init_state`. A form `reset()`
defers the form-level autovalidation to the end of its loop, so it runs once
after `reset`. A field is always wrapped in its unfocus
`Focus` (not focusable, so
never a traversal stop, and no semantics) and checks the modes at focus loss, so a
mode change never remounts the field's content. Tab order between form fields
and `OnUnfocus` validating as Tab leaves a field: **Unasserted:** no test pins
this.

**Tests** (`tests/form.rs`): `validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it`,
`reset_restores_initial_values_and_clears_errors_and_interaction` (an edit
after a reset validates again under `OnUserInteraction`). Frames are driven with
`tick`, which does not dirty the root, so a field that stored its error
without scheduling a rebuild fails the first one. The remaining autovalidate
modes — `OnUserInteraction` validating only after the first edit, the same
mode set on the form validating every field after any edit,
`OnUserInteractionIfError`, and `Always` at mount and on every change:
**Unasserted:** no test pins this.

### 23. `FormHandle` and `FormFieldHandle` replace `GlobalKey<FormState>`

The event methods `FormHandle::save`/`reset` and
`FormFieldHandle::did_change`/`reset` forward the caller's `EventCx` to their
callbacks (ADR-0086). Validators remain queries with no writer. Form reset
restores its validation-suppression flag on unwind, so a panicking user callback
cannot disable validation for later edits; the partial field mutations are not
rolled back. A field schedules its rebuild immediately after committing its
reset state, before the controller sink and `on_reset`, so an unwind cannot hide
that partial commit behind stale UI. `a_panicking_reset_callback_does_not_disable_later_form_validation`
pins recovery and visibility. Context forwarding: **Unasserted:** no test pins
this.

**Choice:** The caller creates a `FormHandle`/`FormFieldHandle` and passes it
to the widget (`Form::handle`, `FormField::handle`), or reads `Form::of`. A
handle is a cheap `Rc` clone that owns the state, so it outlives the build
that created it and needs no key registry. A mounted field rebuilt with a
different handle moves onto it: the new handle takes the field's value,
error, interaction and registration slot, and the old handle is detached.
Each mounted form or field holds an exclusive generation-stamped attachment
lease. A simultaneous duplicate is reported and isolated behind a fresh
internal handle before configuration mutates the requested handle; conditional
lease cleanup prevents a refused or stale owner from detaching the live one.
Acquiring a replacement precedes releasing the current lease, so a busy target
cannot leave a mounted state detached. This is a local recovery contract rather
than a panic because duplicate attachment is caller-triggerable and lifecycle
admission is infallible. The tracing error is paired with the typed
`take_attachment_error` drain on the requested handle, matching the framework's
duplicate-`GlobalKey` diagnostic shape instead of making a mount-time error look
like an event-time `Result`.
A handle is not the element's identity, so a field rebuilt with a different
handle keeps its element and state. A text form field rebuilt without the
caller's controller moves its text into a controller it owns.
**Tests:** every `tests/form.rs` case drives the form through a handle;
`a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first`.
A new handle on rebuild taking the mounted field over, a dropped caller's
controller moving its text into a field-owned one, the same duplicate-mount
refusal for a `FormFieldHandle`, a rebind onto a busy handle, and unmounting a
text form field releasing its callbacks and controller binding:
**Unasserted:** no test pins this.

### 24. A field registers with its form in lifecycle hooks

**Choice:** `init_state` registers, `did_change_dependencies` moves the
registration when the enclosing form changed, and `dispose` unregisters.
Registration order is kept; the
form holds each field strongly and each field holds the form weakly, so a
`FormHandle` captured in a field callback is a cycle only until that field's
`dispose`. **Unasserted:** no test pins this.

### 25. A text field's error line is a live region instead of an announcement

**Choice:** there is no widget-facing announce API, so `RawTextFormField`'s
error line is a `Semantics(live_region: true)` container, which assistive
technology reads when it appears. **Unasserted:** no test pins this.

### 26. `Form` carries the form semantics role

**Choice:** `Form` is a semantics container with `SemanticsRole::Form`
(AccessKit `Role::Form`), so a screen reader can name the group.
**Unasserted:** no test pins this.

### 27. Clipboard bindings come from `DefaultFocusTraversal`

**Choice:** `DefaultFocusTraversal`, which every `FocusRoot` builds, binds the
three chords (Cmd on macOS and iOS, Control elsewhere — a pure table resolved
once per state from `TargetPlatform::current()`). The chord resolves at the
primary focus like the traversal keys: an `EditableText` answers it on its own
node, and with no text field focused nothing does, so the chord keeps
bubbling. **Tests:** `tests/editable_text_clipboard.rs`
(`copy_then_paste_round_trips_text_in_an_editable_text`). The platform table
for every target, and a chord left unconsumed with no text field focused:
**Unasserted:** no test pins this.

### 28. `EditableText`'s clipboard actions win over ancestor bindings

**Choice:** `EditableText` layers its actions over the chain at its position
and records the result on its node, so they are the nearest declaration of
the two intent types and an ancestor mapping never replaces them.
**Unasserted:** no test pins this.

### 29. Paste drops `\r` as well as `\n`

**Choice:** a paste into the single-line field removes both, since a stray
carriage return is never text the user meant (denying only `\n` would leave a
`\r` behind from a Windows `\r\n`). **Unasserted:** no test pins this.

### 30. `RawTextFormField`, not `TextFormField`

**Choice:** the theme-free form field is named `RawTextFormField` for the
facade-additivity reason `RawTextField` is: with the `material` feature on,
`flui::prelude::TextFormField` means exactly the Material type, and enabling a
feature never changes what an existing name resolves to.
`flui_material::TextFormField` is the Material type.

### 31. `SingleActivator` compares ASCII letters without case

**Choice:** `Key::Character` carries what the key produced, so Caps
Lock turns Ctrl+C into a `"C"` event. A single ASCII letter trigger therefore
matches either case; the exact Shift comparison still tells Ctrl+Shift+C
apart. `character_ignoring_shift` is a separate character-only constructor:
Shift may be held or released, while Control, Alt and Meta remain exact. A
named key cannot acquire that policy. `Slider` consumes logical `+` and `-`
through this constructor, including characters produced by native Shift key
translation. ADR-0156 supersedes the old exact-Shift contract for this constructor.
**Asserted:** `a_shift_produced_character_matches_when_shift_is_ignored` and
`slider_focus_keys_and_semantic_actions_share_controlled_proposals`.

### 32. `EditableText::on_changed` reports only the user's edits, and a text form field reads its controller

Platform accessibility Focus and SetValue requests use the current field's
lifecycle-acquired writer, controller and exact generation-checked focus
attachment. SetValue reports a user edit through the same observer as keyboard
input and drains accepted IME grants first. Delivery revalidates ownership after
that drain because callbacks may adopt the node into a later attachment. An
enabled but unfocusable document remains writable; Focus separately checks
eligibility and is advertised only while eligible. Disposal withdraws authority
before cleanup, so queued work cannot edit or detach a later owner.

The `native_actions` rows in `tests/editable_text.rs`, registered in
`text_editing`, exercise queued Focus and SetValue through the headless host,
current controller replacement, Unicode and empty edits, disabled and retired
targets, deferred grant ordering and same-parent attachment takeover before and
during delivery. These are producer and UI runtime-routing contracts; native UIA
TextPattern, selection, password handling and screen-reader execution require
separate verification.

**Choice:** controller listeners are `Send + Sync` and cannot reach
owner-thread form state, so a text form field takes the user's edits from
`EditableText::on_changed` (typing, deletion, IME commit, cut, paste — not
`set_text`, and not the edit an `on_submitted` callback makes), and reads the
controller's text before it validates or saves. A caller's own controller
edit is therefore validated and saved but does not mark the field interacted,
and a reset's write-back needs no equality guard. Because the controller is
the value, `FormFieldHandle::set_value` on a text form field writes the text
into the controller too, so the field's value and its controller never
disagree. **Tests:**
`reset_restores_initial_values_and_clears_errors_and_interaction` (the reset's
write-back does not mark the field interacted). `on_changed` reporting the user's
edits but not the caller's own, and `set_value` on a text form field seen by
`value`, `validate` and `save`: **Unasserted:** no test pins this.

### 33. `RawButton` is a widgets-layer button whose press writes through `EventCx`

**Choice:** `RawButton` is a theme-free press target, a `GestureDetector`
with a `Semantics(button: true)` around it, as one widget, so an application
that uses no design system has a button, and its `on_press` takes
`Fn(&mut EventCx<'_>)`, the typed write capability of ADR-0086. It builds
`Semantics::new().container(true).button(true).enabled(on_press.is_some())`
around an opaque `GestureDetector` and forwards its event context to the press
callback. The detector owns the lifecycle-acquired writer source; `RawButton`
is stateless. The gesture arena's lower-level callback aliases do not change
(ADR-0086 §4). Without
`on_press` the node is disabled and advertises no click, and a tap does
nothing. A press may return a write's
`Result`; a refused write is logged on `flui::signals`. Keyboard activation
(Enter and Space through `ButtonActivateIntent`) and pressed and hovered
state are not implemented yet. **Tests:** `tests/raw_button.rs`
(`raw_button_press_is_reachable_through_a_platform_click`,
`a_refused_write_in_a_press_is_reported_not_panicked`). A button without
`on_press` disabled and advertising no click: **Unasserted:** no test pins this.

### 34. `EditableText` answers an input method's pulls; one platform session is one change

**Choice:** `EditableText` is a `flui_platform_api::TextStore` (ADR-0090). The
input method reads the text, selection, composition and geometry in UTF-16
offsets and edits under a lock; a push `ImeEvent` is projected onto the same
store. A read-write session is written back to the controller once, at the
end of the grant: one listener notification and at most one `on_changed`,
however many edits the session made (a TSF conversion replaces, re-marks and
moves the caret in one session). Both run in the arbiter's `settle`, after the
lock is released, so `on_changed` may request a lock: `on_changed` first, then
the listeners. The write-back records what it owes, with the committed text
it produced, and `settle` takes every obligation before any owner code runs,
so a session that code opens settles its own after the owner heard of this
one, in commit order with each session's own text
(`a_listener_session_inside_settle_is_its_own_on_changed`). `on_changed` receives the
committed text (`TextEditingController::committed_text`, the composition left
out) and runs only when that changed, so a session that only composes is no
owner change; a text form field reads its value the same way. Every call into
owner code, and every snapshot of it, goes through `OwnerCalls`
(`owner_code_is_contained_at_every_point`). The write-back
compares the controller's generation, in the same critical section, with the
one the session opened at, and the controller's identity: an application edit
or a swapped controller wins and the session is dropped. A panicking
`on_changed` is parked in the presentation's gate, after the observer heard of
the session, and resumed by the owner's next dispatch or anchor (ADR-0142 items 1–3). A lock asked for inside the frame
transaction (the whole frame drive, post-frame callbacks included, in the
harness's `tick` as in `flui-app`'s `UiRuntime::drive_frame`) runs after the
frame; a key press first runs those queued grants, so it lands after an IME
commit. **Tests:** `tests/text_store_kit.rs`
(`editable_text_conforms_to_kit_v1`,
`obscured_editable_text_conforms_to_kit_v1`; among the kit's cases,
`tsf_style_conversion_script` counts one owner notification for a whole
conversion session and `async_request_inside_a_transaction_waits_for_the_next_anchor`
defers a lock asked for inside the frame transaction to the next frame),
`tests/editable_text.rs`'s
`text_store::typing_after_a_deferred_commit_lands_after_the_commit`,
`text_store::on_changed_runs_after_the_lock_is_released` and
`text_store::an_app_edit_during_a_lock_is_not_overwritten`,
`text_store::swapping_the_controller_during_a_grant_drops_the_session` and
`text_store::a_panicking_on_changed_is_reported_once_and_the_field_keeps_working`, and `tests/form.rs`'s
`a_text_form_field_validates_and_saves_the_committed_text`. A lock
requested from a post-frame callback specifically: **Unasserted:** no test pins
this.

### 35. Platform selection is exact; user selection snaps

**Choice:** a selection set through the text store
is kept at any scalar boundary, including inside a grapheme cluster (offset 4
of `"a😀e\u{301}…"` is between the `e` and its combining mark), because TSF and
AppKit address scalars and a snapped answer would disagree with what they set.
A tap, a drag and the arrow keys keep snapping to extended grapheme clusters
through the controller (its "Character unit"); a tap and a drag land where
`TextPainter::get_position_for_offset` answers, which snaps to an ICU4X
grapheme boundary while a caret query stays per scalar (flui-painting mapping
decision 15). The arrow keys step the same ICU4X graphemes a hit snaps to
(decision 19), so an arrow key lands only on offsets a hit can answer.
**Tests:** the kit's
`selection_inside_a_grapheme_is_kept_exactly`, for the platform half; for the
tap half, the painter's `a_combining_mark_is_one_hit_target` and
`a_zwj_family_is_one_hit_target` rows of flui-painting's `caret_contract`;
for the arrow keys, `the_editor_steps_the_graphemes_the_painter_snaps_to`.

### 36. `WidgetsApp::router`: a bare Router as the routing subtree, and a form without navigator builders

**Choice:** `WidgetsApp::router(Router<R>)` keeps the Router as a
`BoxedView` and mounts it bare, under the same `builder`, `DefaultTextStyle`
and `Localizations` bands; each of its pages has its route's focus scope. The
app has no navigator of its own: the Router's is its root navigator, and its
facade refuses a stray page pushed through `NavigatorHandle::maybe_of_root`.
A rebuilt app updates the boxed Router in place (same view type), so the
stack and page state survive. The routing form is a type
parameter, not a run-time assertion. `router` returns a
`WidgetsApp<RouterForm>` and `new`/`with_builder` a
`WidgetsApp<NavigatorForm>` (the default, so `WidgetsApp` alone still names
it); only the navigator form has `navigator` and `observer`, so the
configuration of a router app with a navigator key or observers does not compile. The two forms are two
view types, so switching one to the other remounts the shell, and the
navigator form's `dispose` releases its navigator and observers. Owning the
presentation's URL waits for ADR-0093 step 3's `RouterScope`. **Tests:**
`tests/widgets_app_router.rs`
(`switching_widgets_app_from_home_to_router_releases_the_navigator`,
`widgets_app_router_navigates_by_handle_and_the_url_follows`);
`tests/routable_ui/fail/router_app_takes_no_navigator.rs`. The Router's
navigator as the app's root navigator, refusing a stray page; no focus scope
above the Router; and a rebuilt app keeping its stack: **Unasserted:** no test
pins this.

### 37. `PageView` reports page changes after the frame, in order

**Choice:** the controller's listener is `Send + Sync` (a foundation
`ListenerCallback`) and cannot hold the owner-local callback or its writer. It
keeps the synchronous `round(page)` dedupe, records the page and schedules the
page view's rebuild. `build` hands every recorded page to the local post-frame
lane, one entry per page. Each entry holds only a weak reference to the
state's delivery target and runs the callback current at that moment inside
a write the state's `WriterSource` opens. So the callback runs after the
frame that next rebuilds the page view (one frame later for a
change seen during input; two for one seen during layout, whose rebuild
lands in the next frame), never inside a build, with every page a frame
recorded in order. A page recorded before a rebuild reaches the rebuilt
callback. A page view unmounted before its rebuild records nothing to the
lane, and one unmounted after its rebuild queued a page fails the upgrade,
because `finalize_tree` drops the state before the lane runs; either way it
delivers nothing. Without a post-frame lane the pages are dropped with a
warning. A callback that panics loses only its own page, and the panic
leaves the frame on the post-frame lane rather than from inside a scroll
listener; the pages after it run on the next frame. The
same accepted latency as `AnimatedSize` and `Dismissible`. **Tests:**
`tests/page_view_events.rs`.

### 38. `on_draggable_canceled` takes one `DraggableCanceledDetails`

**Choice:** the callback is `Fn(&mut EventCx<'_>, DraggableCanceledDetails)`,
a `Copy` value with `velocity` and `offset`. The catalog's event
callbacks take `cx` and at most one value, which is the shape
`callback_with` fixes for a `let`-bound closure; two value arguments would
need a closure annotation there. **Tests:** `tests/draggable_events.rs`
(`a_let_bound_drag_callback_compiles_through_callback_with`).

### 39. Actions are invoked with the key event's `EventCx`; `maybe_invoke` has no counterpart

**Choice:** `Action::invoke(&self, cx: &mut EventCx<'_>, intent: &T)`; the
`Shortcuts` key handler passes its own `cx`, as `CallbackShortcuts` passes it
to its bindings (ADR-0086, amending ADR-0023). `Actions::maybe_invoke` is
removed: its only caller could hold a `BuildContext` only in `build`, which
has no event context, and a write there is refused. An invoker resolved at
build time and called from an event is deferred
until a consumer needs one. `is_enabled` and `to_key_event_result` stay
queries. **Tests:** `tests/actions.rs` (resolution through key dispatch),
`tests/shortcuts.rs`'s `event_cx_tests`.

### Refresh pulls accumulate outside the clamped scroll position

`RefreshIndicator` retains the distance pulled beyond the top separately from
its scroll position, which stays at the minimum extent. Each pointer delta adds
to that distance; reversed motion consumes it before advancing ordinary scroll
content. Releasing at the threshold starts one refresh. Gesture updates while
refreshing do not alter its pull or content, and releasing a gesture does not
start a ballistic run; `finish` permits a new operation.
`incremental_pulls_refresh_once_and_finish_allows_the_next_gesture` and
`reversing_a_pull_consumes_it_before_scrolling_content` exercise this through
pointer dispatch in `scroll_physics_and_activity`. The row
`a_fast_gesture_while_refreshing_does_not_start_a_fling` advances virtual frames
after a fast upward gesture to distinguish ignored direct motion from an
erroneously started fling, then checks post-finish scrolling and coasting.
Ballistic motion requires an ambient `VsyncScope`; the internal controller has
no ticker or wall-clock fallback.

Content drags publish scroll activity and direction, retaining activity through
ballistic motion until completion. Cancellation without a settling run and an
accepted refresh end activity. `refresh_motion_notifies_activity_through_release_and_recovery`
observes actual activity listeners alongside pixels; the phase/activity failure
and recovery row `a_failed_refresh_notification_releases_activity_and_recovers`
pins shared failure custody through phase publication, terminal activity and
the accepted refresh callback. Observer failure cannot skip either delivery;
competing failures preserve the first panic before a later healthy refresh.

A changed `ScrollPosition` identity stops the simulation based on the retired
position's metrics and replaces the fling listener's target. Reconfiguration
with the same position preserves the active run. The listener is removed and
replaced outside any controller or position guard; its captured handles can
retire without holding those locks. The rows
`a_refresh_controller_swap_retires_the_old_fling_and_drives_the_new_position`
and `rebuilding_refresh_content_with_the_same_position_preserves_its_fling`
use virtual frames to observe actual position changes.

The design follows this widget's synchronous completion and logical-pixel
threshold contract. As a comparison after choosing it, Flutter's
[refresh notification handler](https://github.com/flutter/flutter/blob/main/packages/flutter/lib/src/material/refresh_indicator.dart)
also accumulates updates and overscroll into its drag offset. FLUI does not
adopt its notification-based routing, viewport-relative threshold or futures.


### Scroll resistance and controller attachment ownership

`BouncingScrollPhysics` resists additional outward displacement from the current
overscrolled position, or from the boundary when first crossing it. It does not
reapply resistance to accumulated overscroll: zero input preserves the position,
and movement toward the valid range is unrestricted. Crossing the whole range
uses the newly crossed edge. Public rows `bouncing_lower_edge_preserves_outward_direction`,
`bouncing_upper_edge_preserves_outward_direction`,
`bouncing_stationary_input_preserves_overscroll` and
`bouncing_inward_motion_and_crossing_respect_the_new_edge` pin these decisions in
`scroll_physics_and_activity`.

Changing a `Scrollable` controller's position identity stops the old trajectory
before changing its value/status listeners, then detaches the old command
listener and cancellation hook. A stopped trajectory cannot transfer its old
metrics to the new position. Rebuilding with the same identity preserves the
run. Cancellation hooks have an owning attachment: detaching or disposing an
older attachment removes its hook and pending command only if that hook is
still installed. Hook comparison/take is under the private lock; retired hooks,
command values and animation callbacks retire or execute after it is released.
This does not introduce multi-position commands or arbitration between several
scrollables driving one position. The public rows
`a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook`,
`a_same_position_scrollable_rebuild_preserves_motion` and
`retiring_one_scrollable_preserves_a_later_owners_jump_hook` exercise virtual
frames, retired-controller commands and a shared-controller detach.

An actual position change in `Scrollable` or `RefreshIndicator` also replaces
the private detector ownership identity. Incoming recognizers and mounted weak
targets commit before outgoing contact cancellation; the old Move or terminal
cannot drive the replacement position. Ordinary same-position rebuild retains
the contact. `replacing_a_scroll_position_cancels_its_contact_and_recovers`
checks both widgets, Up/Cancel and fresh-contact recovery.
Its bouncing-overscroll cases also check that cancellation of an outgoing
refresh contact cannot start a simulation through retargeted value listeners.
Refresh callback authority retires before listeners change; accepted terminals
from that owner cannot restart the shared fling controller. Without an ambient
Vsync registration, a current refresh contact can still drag and refresh, but
its terminal declines ballistic motion and ends scroll activity.
`refresh_without_vsync_ends_activity_after_release_and_cancel` mounts through
the public embedder bootstrap without a VsyncScope and checks Up/Cancel,
unchanged pixels through later frames and fresh-contact recovery.

The three gesture inertia drivers in `Scrollable`, `RefreshIndicator` and
`InteractiveViewer` depend on the ambient `VsyncScope` during lifecycle hooks.
Same registry identity preserves motion. A changed registry commits the new
identity and stops the old trajectory at its sampled position before rebinding
the `DrivenController`, which owns registration and retirement. Stopping first
prevents clock removal from settling an old finite run into its endpoint.
Admitted contacts keep their gesture profile. The public row
`replacing_vsync_retires_old_motion_and_drives_fresh_contacts` observes retired
clock immobility and fresh motion on the new clock. Viewer stops at a boundary
only when containment refuses proposed displacement; a repeated accepted frame
sample preserves the trajectory. `a_repeated_frame_does_not_cancel_viewer_inertia`
checks zero elapsed time followed by real progression and recovery.

Viewer focal inertia requires a live bound driver. Without one, release retains
the current scene transform and still reports the measured interaction velocity;
it does not submit an unbound simulation that would settle synchronously.
Clock replacement withdraws release authority before stopping and rebinding,
then publishes the driver's actual binding state for retained callbacks.
`viewer_focal_fling_advances_then_stops_on_new_input` covers initial detachment,
detachment during coast, frame-work drainage and fresh motion after reattachment.


### Wheel precision selects local animation policy

`Scrollable` applies `Precise` and `Unknown` wheel packets immediately, stopping
the previous synthetic trajectory once. A `Notched` packet uses the existing
animation controller for a 150 ms ease-out. Repeated accepted ticks add to the
committed outstanding destination, then animate from the displayed pixels;
restarting does not discard unfinished distance. Accepted motion identity is
committed before activity callbacks, so reentry cannot let an older handler
overwrite newer work. A current drag, controller replacement or unmount retires
that owner's motion. Independent presentations use their own clock and state.

This is an authored local default. There is no wired `SystemPreferences` or
duration-scale producer supplying this scroll policy. Ambient vsync remains
required for animated progress. Public rows
`notched_wheel_accumulates_distance_and_eases_out_in_150ms`,
`precise_and_unknown_wheels_interrupt_synthetic_motion_once`,
`replacing_or_unmounting_a_scrollable_retires_its_notched_motion` and
`dragging_interrupts_notched_motion_and_windows_progress_independently` pin
distance, interruption, retirement and independent-clock behavior.

Win32 packet distance alone does not establish a device's physical notch
capability. Microsoft documents smaller-than-120 wheel messages for finer
resolution in [WM_MOUSEWHEEL](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel).
The native adapter marks observed nonzero fractional wheel units `Precise`;
integral and zero packets remain `Unknown`. Known backend classification can
produce `Notched`, including Winit's `LineDelta`. The platform contract row
`fractional_native_wheel_packets_preserve_observed_precision_and_source` drives
actual queued messages through an owned hidden HWND, preserving source metadata
and signed units with fractional packets and an integral unknown control.

### Remaining touch contacts retain scroll drag ownership

`Scrollable` explicitly selects `DragPointerStrategy::ContinueWithRemaining`
on its actual `GestureDetector`. Lifting the current touch while another
admitted touch remains rebaselines the drag rather than emitting an intermediate
release or fling. The final release uses that surviving contact's measured
motion. Other `GestureDetector` consumers keep their selected policy. The public
row `a_remaining_touch_continues_scroll_without_an_intermediate_fling` observes
continued pixels before final release and subsequent ballistic progress.

### Nested ballistic transfer belongs to the accepted run

[ADR-0169](../../docs/adr/ADR-0169-nested-scroll-ballistic-handoff.md) records the
physics, animation and mounted-owner boundary. Clamping physics obtains the
remaining hard-edge velocity from the existing friction simulation. The exact
run continuation visits the nearest live same-axis willing parent, preserving
reversal and each parent's actual physics. A bouncing parent can accept motion
at its extent; a saturated clamping parent is traversed. Replacement, unmount,
supersession and accepted controller commands retire old delivery authority.
Owner-local default physics retains identity across ordinary configuration
rebuilds; explicit physics retains the caller's identity contract.

The nested fling rows in `scroll_physics_and_activity` cover both axes, reversal,
local bounce, nearest willing parent, same-controller rebuild, replacement,
equal-edge cancellation, custom physics callback/retirement competition and
fresh gesture recovery. Pixel listener failures follow the notifier's existing
diagnostic contract rather than implying frame-pump propagation.

### Terminal inertia uses admitted gesture policy

`Scrollable`, `RefreshIndicator` and `Dismissible` consume the admitted terminal
impulse from `DragEndDetails::fling_velocity()` (ADR-0172). Axis selection and
scroll reversal preserve its sign. The recognizer applies the contact's captured
minimum and maximum before release; these consumers do not replace that policy
with a framework default. Scroll physics and dismissal direction/threshold
rules remain independently authored. Raw measured velocity remains available to
application callbacks.

`terminal_scroll_motion_uses_the_admitted_fling_profile` and
`terminal_refresh_motion_uses_the_admitted_fling_profile` observe bounded coast,
below-minimum rest and fresh-contact recovery after a live profile change.
They also admit a maximum above the framework baseline and observe the larger
coasting distance; no second default ceiling constrains the resolved impulse.
`dismissal_release_uses_its_captured_fling_profile` and
`vertical_dismissal_release_uses_its_captured_fling_profile` preserve dismissal
thresholds and the active contact's impulse on both axes.

`InteractiveViewer` seeds focal friction from
`ScaleEndDetails::focal_fling_velocity()`. Contact Down and native Begin capture
the release policy; replacing the source before End affects the next sequence.
Its public interaction callback retains the raw focal measurement in logical
pixels per second and the separate dimensionless scale velocity.
`viewer_touch_focal_inertia_uses_the_admitted_profile` and
`viewer_native_focal_inertia_uses_the_admitted_profile` observe below-minimum
rest, the captured maximum and fresh-sequence recovery through actual frame
motion.

### Accepted gesture cancellation does not commit a release action

Accepted drag cancellation still reaches `on_end`, carrying
`GestureEndReason::Cancelled`; normal release carries `Completed` (ADR-0112).
Pre-acceptance rejection remains `on_cancel`. Recognizers clear their contact
before either terminal callback and invoke it without a state lock.
`horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector`
observes both reasons and a subsequent contact.

`Scrollable` excludes measured velocity on cancellation. In-range cancellation
ends activity and resets direction to idle; overscroll uses zero-impulse physics
recovery, with the existing ambient-vsync requirement. Public rows
`cancelling_an_in_range_scroll_ends_activity_without_coasting` and
`cancelling_bouncing_overscroll_settles_without_release_velocity` advance virtual
frames and then perform a new gesture. `RefreshIndicator` resets a cancelled
pull without starting refresh, even past its threshold, and permits only the
same zero-impulse boundary recovery; an already-active refresh remains active.
`cancelling_a_threshold_refresh_pull_does_not_refresh` distinguishes cancellation
from the next completed pull.

`Dismissible` cancels drag ownership before reversing to its original location,
including a drag at its completed move bound. It discards transient completion
rather than applying dismissal thresholds or release velocity. Public horizontal
and vertical rows `a_cancelled_horizontal_dismiss_restores_the_card` and
`a_cancelled_vertical_dismiss_restores_the_card` check no dismissal, restored
hit location and the next completed dismissal in `animation_and_visibility`.
The row `cancelling_a_fully_slid_card_restores_it_without_dismissal` covers the
completed-bound bypass through an actual out-of-bounds pointer move.
A cancelled back swipe restores the still-current route regardless of its
position or velocity; a route already navigated away retains the existing
active-route settling policy. The public PageRoute row
`cancelling_a_back_swipe_past_halfway_keeps_the_route` checks the route, gesture
counter and next completed swipe.

`InteractiveViewer` clears pan bookkeeping and forwards the reason to
`InteractionEndDetails`. Discrete wheel and panzoom updates synthesize
`Completed`, without claiming a physical pointer release.
`viewer_reports_cancelled_then_completed_interactions` checks what the public
callback observes. Cancellation stops focal inertia without a release impulse.


### Text-store exact points use source scalar intervals in either direction

`TextStoreRead::index_at_point(Exact)` tests the interval between consecutive
source-scalar carets using its minimum and maximum x coordinate. An RTL pair
therefore names its source scalar rather than being treated as outside the text.
This retains the per-scalar caret contract; it does not introduce a visual-run
hit topology for discontinuous mixed-bidi ranges. `Nearest` remains a boundary
query. **Test:** `rtl_scalar_rect_midpoints_resolve_to_the_source_scalar`.

### Selection dragging belongs to the mounted field and its contact

`EditableText` retains the source anchor and typed contact in its state. A
same-controller configuration rebuild preserves that drag; only the first active
contact can move or terminate it. Disablement, actual controller replacement and
disposal retire it. Double-tap word selection deliberately takes over and clears
ordinary drag tracking. Admission precedes focus callbacks; a callback that
retires the contact prevents the subsequent caret write.
**Tests:** `selection_drag_survives_a_same_controller_rebuild`,
`foreign_release_preserves_the_selection_contact`,
`foreign_cancel_preserves_the_selection_contact`,
`disabling_the_field_retires_its_selection_contact`,
`replacing_the_controller_retires_the_old_selection_contact`.

### Non-IME splices collapse after the resulting grapheme

`TextEditingController::insert_str` moves its collapsed caret forward to the
next ICU extended-grapheme boundary of the resulting document. Inserted bytes
can join a following combining mark, and deleting a selected separator can join
regional indicators; neither leaves Backspace starting inside the new cluster.
Raw text-store/IME scalar selection remains exact under ADR-0090.
**Tests:** `insertion_keeps_the_caret_after_the_joined_combining_cluster`,
`deleting_a_separator_keeps_the_caret_after_the_joined_flag`.

### Decoded cache admission rechecks completed images and owns eviction

`AssetImage::resolve_async` and `NetworkImage::resolve_async` probe the completed
LRU while holding the pending-load admission guard. A completion between the
widget's earlier miss and its subscription therefore does not start another
load. Completed hits use `get` to refresh recency; a still-pending slot shares
its existing future. Neither hit invokes a replacement loader. Incoming unused
loader captures are retired after releasing both infrastructure guards.

LRU insertion uses `push`, which returns a replaced or evicted entry, rather
than `put`, which destroys capacity evictions internally. The entry is retired
after unlocking so a last pixel buffer is not freed while other cache probes
wait. Current image/key types have no user destructor callback; this is an
ownership and lock-duration decision. Nonzero capacity is represented by the
type. The bound counts cached entries, and eviction cannot invalidate a pixel
handle held by an already displayed image.

**Tests:**
`asset_image_async_reuses_completed_decodes_after_cold_failure_recovery`
removes the actual source after decoding and repeats async resolution, including
cold failure followed by recovery.
`decoded_cache_promotes_hits_and_preserves_displayed_pixels_after_eviction`
uses a private local cache because production capacity is not a consumer
contract. `decode_cache_coalescing_contracts` exercises unused capture reentry
through the public asset provider on completed and pending hits.

## Controlled catalog inputs

`Slider` and `Disclosure` implement ADR-0124. Input proposes values through
`EventCx`; only parent configuration commits them. The slider paints and maps
pointers against allocated bounds, honors inherited or explicit direction, and
keeps extreme interpolation finite. Disclosure unmounts its collapsed body and
keeps a real focusable header. Both recheck mounted writer and focus authority
after reentry, and guard independently owned callback and view retirement tails.
`controlled_slider_input_and_geometry` and
`controlled_disclosure_state_and_geometry` pin these mapping decisions through
actual input, frame geometry, semantics and retirement producers.

Captured gesture groups share their owning state's mounted admission. Disposal
revokes admission before recognizer retirement, and delivery checks it between
recognizers so an earlier callback can unmount the group safely. A retained
Listener route still receives its terminal event; its disposed composite
recognizers are no longer invoked. The controlled slider retirement row pins
mid-contact disablement, stale Move/Up delivery and a fresh mounted contact.
Disclosure explicitly merges its named Focus/action header semantics while its
body remains a separate subtree; indicator direction uses the header allocation.

## Mounted gesture policy ownership

`GestureDetector`, `Draggable` and the navigator's edge-swipe detector acquire
the resolved `GestureSettingsProvider` in lifecycle hooks. Live host publication
updates that same provider; recognizers snapshot policy at admission and retain
it through the contact or candidate's terminal event. An authored fixed profile
or a different source identity replaces the owning recognizers instead. Equal
profiles and the same live source preserve active work.

Mounted listeners retain private weak attachments. Replacement commits every
incoming owner and target before cancelling outgoing owners, so cached routes
cannot reach a retired actor. Disposal revokes admission before cleanup.
`authored_settings_replace_active_owners_and_preserve_equal_profiles`,
`authored_settings_retire_tap_candidates_and_deadlines` and
`authored_settings_retire_native_scale_session_before_fresh_admission` pin the
six detector families. `draggable_reads_admission_profiles_and_retires_authored_owners`
and `replacing_authored_back_swipe_policy_cancels_the_outgoing_contact` cover
the direct consumers and their next healthy operation.

The back-swipe settle converts the admitted fling vector's signed horizontal
component to route widths per second, then applies directionality. Raw measured
velocity remains available for reporting; `Draggable` reports it without an
internal inertial animation. `mounted_back_swipe_settle_uses_the_admitted_fling_bound`
pins retained policy and the subsequent contact's fresh bound against actual
mounted route transitions.

Native scale Begin stages the actor's immutable profile without claiming the
stream or delivering recognized callbacks. The actor's typed disposition
distinguishes dormant admission from refusal and recognized handling. A Begin
refused while touch contacts are active remains refused until its terminal
event; a later Update cannot revive that session after touch release. Admitted
dormant End and cancellation retire the actor before an independent Update can
start. `mounted_native_begin_retains_estimator_before_first_claim` pins the
estimator at Begin, identity updates and unclaimed terminal cleanup;
`mounted_native_begin_refused_by_touch_cannot_claim_after_touch_terminal` pins
refusal, standalone updates and fresh-session recovery.
