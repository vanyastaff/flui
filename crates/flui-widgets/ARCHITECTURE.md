# flui-widgets architecture

The user-facing widget catalog: configuration objects over the `flui-objects`
render catalog, plus the stateful widgets that own gesture, focus, routing and
overlay behavior. Layer rules, dependency direction, and the crate's place in
the workspace DAG live in [`docs/FOUNDATIONS.md`](../../docs/FOUNDATIONS.md)
and [`docs/workspace-layers.toml`](../../docs/workspace-layers.toml); this file
records the per-widget decisions that diverge from the Flutter reference and
would otherwise read as drift.

Cross-crate protocol decisions belong in an ADR (`docs/adr/`). What belongs
here is a decision local to this crate: a widget's internal shape, a callback
bound, a payload type.

## Mapping decisions

### 1. `DragTarget` publishes a shared `DragTargetSlot`, not its `State`

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — behavior is the
floor, structure is designed for Rust; every divergence names what is better,
replaces the oracle's test, and drops no edge case by accident.

**Oracle:** `widgets/drag_target.dart`. `_DragTargetState.build` wraps its
child in `MetaData(metaData: this)`, and `_DragAvatar._getDragTargets` walks
the hit path for a `RenderMetaData` whose `metaData` *is* a
`_DragTargetState`, then calls `didEnter`/`didMove`/`didLeave`/`didDrop` on
that object directly. Dart can do this because the payload is a GC'd reference
to the live `State`, and because a `State` can reach `widget` for the
callbacks.

**Choice:** the payload is an `Arc<DragTargetSlot>` — a non-generic object that
owns the entered list (keyed by `PointerId`), holds the current build's
callbacks type-erased behind a private `TargetCallbacks` trait, and carries the
target element's `RebuildHandle` so a transition can schedule the rebuild the
oracle gets from `setState`. `DragTargetState<T>` keeps only the slot and reads
its candidate/rejected lists back out of it; `build` refreshes the callbacks
into the slot so a rebuilt view's closures are the ones a later transition
invokes.

**Why the oracle's shape does not transcribe.** Two independent reasons, both
structural:

- A hit-test payload is `Arc<dyn Any + Send + Sync>` (`HitTestEntry::metadata`).
  A `DragTargetState` holding `Rc<dyn Fn …>` callbacks could not be one.
- FLUI's callbacks live on the *view*, which the state does not own. A payload
  reaching only the state could not invoke them at all.

**Consequences:**

- **`DragTarget`'s four transition callbacks change from `Rc<dyn Fn …>` to
  `Arc<dyn Fn … + Send + Sync>`** — a breaking public-API change. It makes
  `DragTarget` consistent with `Draggable`, whose callbacks already carried
  those bounds, and it puts the target's cross-thread contract in the type
  system instead of resting on GC. The *builder* stays `Rc`: it produces a
  `BoxedView`, which is owner-local by construction, and it is only ever called
  from `build`.
- The veto (`on_will_accept`) stays **synchronous**, which a deferred
  drain-on-next-build queue would have lost — a drag has to know at move time
  which targets are candidates.
- The slot outlives its element by `Arc`. A target that leaves the tree
  mid-drag is `retire`d in `dispose`, and every later transition is a no-op —
  the oracle's `if (!mounted) return;` in a form that cannot be forgotten at one
  call site. One deliberate improvement rides on this: `did_drop` on a retired
  slot returns `false`, so the drag reports the drop as *not* accepted, where
  the oracle's `finishDrag` records `wasAccepted = true` even though its
  `didDrop` returned early. Covered by
  `a_target_removed_mid_drag_receives_nothing_further_and_accepts_nothing`.
- `DragTarget` tags itself `HitTestBehavior::Translucent`, the oracle's own
  default `hitTestBehavior`. Making it configurable stays a named deferral.

**Replacement tests:** the whole `DragTargetSlot` protocol group in
`tests/parity/draggable_test.rs` (group 2), plus the live-discovery group
(group 4) which pins the enter/move/leave/drop ordering for nested and
overlapping targets against `_DragAvatar.updateDrag`'s own rules.

### 2. `DragTargetDetails` carries a target-local position as well as a global one

**Oracle:** `DragTargetDetails.offset` is a global position and nothing else. A
Dart target that wants a local one calls `globalToLocal` on its own render
object.

**Choice:** `offset` keeps the oracle's global meaning; `local_offset` adds the
same point mapped through the hit entry's own global-to-local transform.

**Why:** FLUI callback code cannot reach a render object, so the oracle's
escape hatch does not exist here — a target would have had *no* way to learn
where the drag is in its own space. Taking the value from
`HitTestEntry::transform` composes the entire ancestor chain, so it is exact
under scale and rotation, where subtracting a remembered origin is not.

**Replacement test:**
`a_transformed_target_is_told_the_drag_position_in_its_own_space`, whose
expected value is reachable only by composing the real transform.

### 3. `Draggable` recovers its global position through a private origin probe

**Oracle:** `_DragAvatar.updateDrag` hit-tests at `globalPosition +
feedbackOffset` on every move. Flutter's `PointerEvent` carries `position`
(global) *and* `localPosition`, so a widget always has both.

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
Flutter's answer is `OffsetPair` (`gestures/events.dart`), which
`DragGestureRecognizer` threads through `_initialPosition` and `_lastPosition`
(`gestures/monodrag.dart:417`, `:686`) and hands to every detail struct; the
velocity tracker samples the *local* half (`:664`) and deltas are mapped to
global via `PointerEvent.transformDeltaViaPositions`. Porting that means the
trait signatures, `RecognizerBase`'s tracked position, and each of the ten
recognizers' internal position plumbing — a change of its own size, with its
own per-recognizer parity evidence.

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

- A drag carrying no data discovers nothing. The oracle's null-data drag enters
  every target; `ErasedDragData` erases a concrete value, not an `Option`, so
  that state has no representation here.
- `axis` restriction applies to deltas in the `Listener`'s space rather than the
  root's, which differs from the oracle only under a rotating ancestor.
- **A transform that changes mid-contact is still converted inconsistently.**
  Pointer dispatch localizes with the `HitTestEntry` transform captured in the
  route resolved at `PointerDown`, while `local_to_global` converts with the
  tree's *current* transform. A frame that moves or scales the `Listener`
  between two moves therefore has the drag convert a stale local point through
  a fresh matrix, and the probe lands off the pointer until the contact ends.
  The value that fixes this now exists and is proven correct at the dispatch
  boundary — `a_mid_contact_transform_change_does_not_move_the_reported_global_position`
  in `tests/parity/pointer_local_position_test.rs` pins it — but it stops at
  the `Listener`, one layer above where this widget reads its position. Not
  worked around.

**Replacement tests:** group 4 of `tests/parity/draggable_test.rs` drives real
pointer input across a tree where the draggable and the targets are at
different offsets, so a local-position implementation enters targets the
pointer was never over.

### 4. Named routes split into six untyped entry points and two typed ones, and a request that cannot be served is a typed error

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — behavior is the
floor, and a divergence names what is better, replaces the oracle's test, and
drops no edge case by accident. Gated as [ADR-0024](../../docs/adr/ADR-0024-named-routes-seam.md) §7.3.

**Oracle:** `widgets/navigator.dart`, `NavigatorState._routeNamed` and the four
`*Named` methods. Flutter asserts (debug-only) when `onGenerateRoute` is absent
or returns null with no `onUnknownRoute`, and re-types the generated route
through an unchecked `as Route<T?>?` — so in a release build an unresolvable
name is a null dereference and a *wrong* `T` is never detected at all.
`pushNamed<T>` returns `Future<T?>`, which cannot express either failure.

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
observed, pinned by
`an_unresolvable_name_after_a_navigating_factory_adds_nothing_of_its_own`. Those are the factory's own
mutations, deliberate on its part, and they are not rolled back for the same
reason §5 does not undo a factory's nested push: it is not this operation's to
undo. What the guarantee covers is that **the failing operation adds nothing of
its own**.

**Why not type every entry point.** That was the first design, and it was worse
than the bug it fixed. `push_named::<()>("/settings")` against a
`PageRoute<i32>` generator would return `Err` and *not navigate* — a caller who
just wants to go to a screen has no reason to know what that screen completes
with, and Flutter's `pushNamed<void>` pushes it without complaint. Trading a
silent-`None` for a refusal-to-navigate in the common case is a regression. The
split keeps the strong guarantee exactly where the caller has asserted a type.
Typed siblings for replacement and remove-until are deliberately not offered:
no consumer yet, and each is purely additive later.

**Why the oracle's shape does not transcribe.** Two reasons, one per variant:

- A route name is *caller input*, not a framework invariant, and
  [`PANIC-POLICY`](../../docs/PANIC-POLICY.md) puts caller input on the `Result`
  side. Flutter's assert is also debug-only, so its release behavior is worse
  than either option here.
- The type mismatch is genuinely detectable in Rust and genuinely undetectable
  in Dart: FLUI's registration stays typed, so the generated route knows its own
  `Output`. Reporting it as an indistinguishable `None` at delivery — this ADR's
  own first answer — would have thrown that information away for nothing.

**Consequence, named rather than left to be discovered:** the *result* a `_with`
variant delivers to the departing route keeps the ordinary delivery-time
contract (`pop_with`'s), because the navigator still cannot know the departing
route's `Output`. Only `push_named_typed`'s `T` is checked early. So
`push_replacement_named_with(name, Wrong)` still completes the replaced route
with `None` and a log line, exactly as `push_replacement_with` does.

**Replacement tests:** in `tests/navigator_public.rs` —
`a_route_whose_output_the_caller_never_names_is_still_navigable_by_name`
(a registered `PageRoute<i32>` reached through `push_named` with no `T` in
sight: the regression the split prevents) and its sibling
`the_other_five_untyped_operations_also_navigate_an_unnamed_output_route`;
`push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing`
for the guarantee the typed entry point keeps; and
`an_unresolvable_name_errors_without_touching_the_stack_or_the_observers`. The
error cases assert the error's own fields *and* that the stack and the observer
stream are unchanged.

The two families have opposite polarity under one mutation, which is what makes
them a pair rather than a duplication: deleting the `TypeId` comparison in
`GeneratedRoute::checked` fails the typed test and leaves both untyped tests
green, while routing `push_named` through `checked::<()>` fails the untyped ones
and leaves the typed one green.

### 5. Named operations capture their target, then resolve, then act on the captured route

**Rule:** as §4 above; same ADR.

**Oracle:** `NavigatorState.popAndPushNamed` is literally
`pop<TO>(result); return pushNamed<T>(routeName, arguments: arguments);` — the pop
is committed before `_routeNamed` is called. `pushReplacementNamed` instead
evaluates `_routeNamed(..)` in *argument* position and then replaces whatever is
on top.

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

**Three divergences from the oracle, all real, all only for a re-entrant factory:**

1. **Ordering.** The nested `didPush` precedes the dismissal of the caller's
   route. Flutter's `popAndPushNamed` pops *first* and cannot produce this, so
   **this exposure is created by the divergence, not inherited.** An earlier
   revision of this entry claimed the success-path stream was "identical to the
   oracle's" and that "nothing is lost". Both were false; a divergence's cost is
   not visible until something else changes, and `RouteRequest::navigator()` was
   what changed.

   **That accessor is now withdrawn (§9), and these two divergences change
   meaning rather than disappearing.** They were the price of a capability the
   crate offered. They are now what still happens if a factory mutates the stack
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
   and the observation reads it. Note this is invisible to a kind-only oracle:
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
  then a no-op — so divergence 2 shows only in the *buried* case, not in every
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
`false` (a public trait whose default is `true`, and ADR-0024 §7.4 sanctions user
routes), `maybe_pop_with` under a `PopScope` veto — which reports *handled* while
discarding, so it is worse than the case below — and **`maybe_pop` on a lone
route**.

A tenth condition, "a result displaced from an entry that already carried one",
is armed for in `arm_pop` and is believed **unreachable**: every arming site
flushes inside the same locked section, and every flush arm takes the pending
result. It is left armed rather than removed, because reachability here is a
property of the current call graph and this feature has already watched that
graph change four times.

That last one is not an edge case at all. `Route::pop_disposition` is Flutter's
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
deliberately worded to the drop case rather than generalised. The pinning test
is named for the drop case too; the prose is what had to be narrowed to match it. `apply` deliberately runs
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

**Where this beats the reference.** `pushReplacementNamed` has the identical
defect in Flutter: the generator runs in argument position, and a Dart factory
can navigate through `Navigator.of(context)` just as ours can, after which the
replacement targets the wrong route. FLUI's capture removes it — completion and
observation both, per point 3 above.

**Replacement tests** (`tests/navigator_public.rs`), each with the mutation it
detects:

- `pop_and_push_named_with_an_unresolvable_name_pops_nothing` and
  `…_delivers_its_result_to_nobody` — move `self.pop()` back above
  `resolve_named` and only these two fail; the success path cannot see the
  ordering, because both orderings pop and then push.
- The three re-entrant-factory cases — drop the capture and act on the current
  top, and they fail with `left: [1,2,4] right: [1,4]` and the misdelivered
  result value.
- `pop_and_push_named_observes_a_pop_where_push_replacement_named_does_not` pins
  that this is a pop-and-push rather than a replacement wearing its name. Stack
  shape and result delivery cannot tell those apart — a `PushMode::Replace` body
  keeps every other named-route test green, the parity leg included — so the
  discriminator is the observer stream: a pop-and-push emits `didPop` + `didPush`,
  a replacement emits `didReplace` and neither.
- `a_re_entrant_replacement_reports_the_route_it_actually_replaced` pins point 3
  on the `didReplace` **payload**; deriving the reported id positionally again
  makes it name the factory's route. Its sibling
  `a_re_entrant_replacements_observer_stream_is_pinned` records the stream, and
  is deliberately *not* the oracle for point 3 — the stream does not change when
  the identity is wrong.

### 6. Named-route registration lives on the handle, and the app builder will replace the table wholesale

**Rule:** as §4 above; ADR-0024 §3.1, amended by §7.

**Oracle:** Flutter splits registration across two widgets. `Navigator` owns
`onGenerateRoute`/`onUnknownRoute`; `WidgetsApp` owns `routes: Map<String,
WidgetBuilder>` and `home`, and `WidgetsApp._onGenerateRoute` folds all three
into the single hook `Navigator` reads.

**Choice:** all three register on `NavigatorHandle` — `route(name, factory)`,
`on_generate_route`, `on_unknown_route` — and one private `RouteRegistry`
resolves them in Flutter's order (table → generator → unknown). FLUI's
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
registry: an app that registers once against a handle it retains — the flow
`widgets_app.rs`'s
`unmount_and_remount_over_a_retained_handle_does_not_duplicate_observers`
exercises — would otherwise get `Unresolved` from every `push_named` after its
first unmount. Clearing on dispose was tried in this slice and reverted for
exactly that reason; `route_registrations_survive_an_unmount_and_remount_over_a_retained_handle`
is the pin.

**Consequence, named rather than left to be discovered:** a factory that clones
its own `NavigatorHandle` in still closes an `Arc` cycle through the registry,
and nothing reclaims it implicitly. §9 removed the *reason* to capture one —
`RouteRequest` hands the factory its navigator — so this is no longer what the
natural code does, and `NavigatorHandle::clear_routes` is the explicit escape for
a caller who captured anyway. Caller-controlled by necessity: only the caller
knows whether it intends to register again.

**Replacement tests:**
`route_registrations_survive_an_unmount_and_remount_over_a_retained_handle` and
`clear_routes_drops_every_registration_including_the_generator_hooks`
(`navigator_tests.rs`) for the lifecycle contract above — restoring the dispose
clear fails the first;
`a_table_entry_wins_and_the_generate_hook_is_never_consulted`,
`on_unknown_route_runs_only_after_the_generator_declined_and_sees_the_callers_payload`,
`two_handles_resolve_the_same_name_through_their_own_registries`, and
`a_factory_that_pushes_re_entrantly_does_not_deadlock` in
`tests/navigator_public.rs`.

### 7. A generated route that is never pushed still runs `dispose`

**Rule:** as §4 above — "never lose an edge case by accident".

**Oracle:** `Route.dispose` is an explicit lifecycle method, and
`Navigator.defaultGenerateInitialRoutes`' failure branch walks the routes it had
already generated calling `route?.dispose()` before giving up. Discarding a
generated route without disposing it is a leak in Flutter too; Flutter simply
never leaves one undisposed.

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

**Replacement test:**
`push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing`
(`tests/navigator_public.rs`) counts `dispose()` calls on a probe route and
asserts exactly one. Deleting `impl Drop for GeneratedRoute` fails it.

### 8. `RouteSettings::with_arguments_shared` relays a payload without changing its identity

**Rule:** as §4 above.

**Oracle:** Dart's `RouteSettings.arguments` is an `Object?` reference, and the
upstream `'arguments for named routes'` tests assert it with `same(...)` —
pointer identity, not value equality. Relaying one settings object's arguments
onto another is free there.

**Choice:** `with_arguments<T>(value)` keeps taking the value and minting a
fresh `Arc` (it is the construction case), and a second constructor
`with_arguments_shared(RouteArguments)` forwards an existing payload untouched.

**Why:** `RouteSettings`' own `PartialEq` compares the payload by `Arc::ptr_eq`,
so a relay through `with_arguments` silently changes the answer to the exact
question the oracle asks — and the two constructors are one character apart at
the call site with no type error between them.

**What it does *not* claim.** It is not the only way to move a payload: a caller
holding a `RouteSettings` can already `clone()` the `Arc` out of
[`arguments`](RouteSettings::arguments) and carry it by hand, and the test that
motivated this constructor could have done exactly that. What the constructor
buys is that the identity-preserving relay is *expressible as a builder call*,
so the safe form is as short as the unsafe one — rather than a fact about
`with_arguments` that every relay site has to remember. That is a real but
modest gain, and it is stated here as such.

**Replacement tests:**
`route_key_with_arguments_shared_relays_a_payload_without_changing_its_identity` covers
the keyed counterpart `RouteKey::with_arguments_shared`, which exists because
`RouteKey::with_arguments` takes its payload by value and would wrap an `Arc` in
another `Arc` — making the factory's `argument::<OriginalType>()` answer `None`
silently. And
`with_arguments_shared_relays_a_payload_without_changing_its_identity` (which
also asserts the contrast: `with_arguments` on an identical value is *not*
`ptr_eq`), and the identity half of
`on_unknown_route_runs_only_after_the_generator_declined_and_sees_the_callers_payload`,
which compares against an `Arc` the caller constructed rather than against the
settings object it was handed.

### 9. A route factory is handed the request only — the navigator accessor is withdrawn

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — the reference's
observable behavior is the floor; where a contract can be improved, improve it
and record what is better.

**Oracle:** `widgets/navigator.dart`, the `RouteFactory` typedef —
`Route<dynamic>? Function(RouteSettings)`. A Dart factory that needs to navigate
closes over `Navigator.of(context)` and the resulting reference cycle is
collected.

**Choice:** the factory takes a `RouteRequest<'_>` carrying the request only —
`settings()`, `name()`, `argument::<T>()`. A redirect is expressed by *returning a
different route*, which is what a factory is for.

**Superseded, kept visible with its correction.** This entry originally added a
`navigator()` accessor handing the factory its own handle, on the argument below.
Both are withdrawn, for two reasons:

- **The argument was circular.** It existed to remove the *reason* to capture a
  handle. But a route's content never needed one either — a `RouteContentBuilder`
  receives `&dyn BuildContext` and `NavigatorHandle::maybe_of(ctx)` resolves from
  it, exactly as `Navigator.of(context)` does. With no need to capture there was
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

**Why the oracle's shape does not transcribe.** Rust does not collect cycles, and
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
resolves from it, exactly as `Navigator.of(context)` does. So after this change
there is no remaining case where capturing is the right answer.

**Consequences, named rather than left to be discovered.** Two, and the second
is the one that cost a review round:

- It is a breaking change to every registration call site, deliberately so — a
  compile error at each, which is the cheapest it will ever be.
- **It made re-entrant navigation *advertised*, which is what surfaced §5's
  hazard.** Handing the factory a navigator turned "a factory could conceivably
  navigate" into a documented, ergonomic shape with a passing test. §5's
  captured-target fix, its three observable divergences, and §4's qualifier about a
  declining factory's own mutations were all written because of that. Withdrawing
  the accessor un-advertises the shape; it does not un-reach it, so all three
  survive the withdrawal — see the measurement above. What the accessor really
  cost, then, was not the defences (those defend an invariant that was always
  worth defending) but the six rounds it took to notice they were needed.

**Replacement tests:** `a_factory_is_handed_the_callers_name_and_arguments` pins
what the request delivers. Its predecessor also asserted that `navigator()`
returned *this* navigator rather than any navigator; that claim's subject no
longer exists, so nothing pins it and nothing needs to.
`a_factory_that_pushes_re_entrantly_does_not_deadlock` now obtains its handle by
an ordinary capture, through an `Rc<RefCell<Option<NavigatorHandle>>>` cell,
since the accessor it used to call no longer exists. It still pins that the
registry guard is released before the factory runs — holding it deadlocks the
owner thread — and its claim is now the weaker *survivable*, not *supported*.

### 10. `RouteKey<T>` moves the result-type check from run time to the registration site

**Rule:** as §9 above.

**Oracle:** Flutter routes by `String` and re-types through an unchecked
`as Route<T?>?`. Nothing connects `'/details'` to the `MaterialPageRoute<Order>`
behind it, in either direction, at any time.

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

**Replacement test:** `keyed_settings_untyped_carries_a_keys_arguments_onto_an_untyped_operation`
— returning `RouteSettings::named(name)` without the payload fails it
(`left: [Some(1776), None]`).

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
table cannot express and Flutter's `onGenerateRoute` fallback would defeat
anyway.

**What the `TypeId` guard actually buys: a pre-mutation, non-panicking
failure.** Not type safety — the `RouteResult` downcast underneath is checked, so
a silently wrong result was never reachable. Removing the guard does not produce
a `RouteResult<u32>` fed by a `String` route; it makes `TypedPush::push` land the
push and *then* fail its `BUG:` `expect`, panicking mid-operation with the stack
already mutated. That is the stronger and more honest claim, and it is what the
mutation below actually demonstrates.

**Replacement tests:**
`a_route_key_carries_its_result_type_from_registration_to_delivery` (dropping
`push_keyed`'s result handle fails it),
`route_key_identity_is_its_name_and_costs_its_output_type_no_bounds` (deriving
the impls instead of writing them stops it compiling, since its `Output` type
implements nothing), and two collision oracles —
`a_name_registered_by_both_paths_with_different_outputs_is_reported_not_silently_wrong`
(typed-vs-untyped) and
`two_route_keys_sharing_a_name_collide_even_though_both_registrations_compile`
(keyed-vs-keyed, the case that falsified this entry's original claim). Dropping
the `TypeId` comparison fails both — by **panicking** inside `TypedPush::push`
after the push has landed, which is the failure mode the guard converts into a
clean `Err`.

### 11. A route's `settings` are write-only, so the factory relays values instead — recorded, with its trigger

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — a behavior the
reference handles is dropped only by decision, recorded where a reader will find
it.

**Oracle:** every Flutter route factory ends
`MaterialPageRoute(settings: settings)`, relaying the request's name *and*
`arguments` onto the route it builds, so the pushed screen can read
`ModalRoute.of(context)!.settings.arguments` back ambiently.

**Choice:** not ported. No FLUI route builder accepts a `RouteSettings` —
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
capability the oracle's relay serves is served *better* here: moving the value
into the builder is a typed capture, where Dart needs `settings:` only because
its screens read arguments back ambiently.

**The trigger — this is the part that makes recording correct rather than a
trap.** The first read path added — an observer reporting route names, a
`ModalRoute::of`, a name-based route finder — makes the empty `settings`
silently wrong, and `with_settings` on `SimpleRoute` / `PageRoute` / `PopupRoute`
must land **in that same change**, not after it. `RouteSettings::with_arguments_shared`
(§8) already exists as the identity-preserving half such a builder needs.

**Replacement test:** none, and deliberately — there is no behavior to pin, only
an absent capability. What is pinned is the *documentation*: `RouteRequest::settings`
no longer claims a relay that does not exist, which is what its doc said before.

### 12. A name re-registered with a different `Output` is reported at the registration site

**Rule:** as §11 above. The house rule for caller error in this repo is
repair-and-warn, not refuse.

**Oracle:** none. Flutter's table is `Map<String, WidgetBuilder>` and its routes
are `Route<dynamic>`; there is no type to disagree about, so the reference has
nothing to say here. This is a hazard FLUI's own `RouteKey<T>` creates by making
a promise the name-keyed table cannot keep.

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
would otherwise emit one warning per pass; conflicts are still counted in full,
which is what lets a test distinguish "warned once" from "stopped noticing". The
latch is per **registry**, not a process-global `static`, so one navigator's
conflict cannot silence another's.

**The decision and the latch commit together, under the lock.** A review raised
that latching *after* the emit would let a `tracing` subscriber re-entering
registration during the warn see a stale flag and emit a second warning. Measured:
that half is not reachable — `tracing` suppresses re-entrant event dispatch on the
same thread, so the inner `warn!` never reaches a subscriber and the event count
is 1 either way. What *is* reachable is the counter drifting from the emission
(`warns_emitted` reads 2 for one event), and since that counter is the oracle the
warn-once test asserts on, a counter that can over-report is a counter that cannot
pin anything. Committing both in one locked step is what keeps it honest. Recorded
so the guard is not later removed as dead: it is not protecting the warn, it is
protecting the counter.

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

**Replacement tests:**
`a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry`
covers all four displacement paths; reverting any one of them to drop under the
guard makes it **hang** rather than fail, which is why it asserts progress
counters as it goes rather than only at the end.
`a_route_re_registered_with_a_different_output_type_warns_once_per_navigator`
(`navigator_tests.rs`) asserts six conflicts produce one warning, that a
same-type replacement produces neither, and that a second navigator warns for
itself — dropping the latch reads 6 instead of 1, dropping the type guard makes
the same-type replacement warn.
`the_conflict_warning_is_emitted_once_and_names_both_result_types` captures the
real `tracing` events through `flui_testing::log_capture` and asserts on them, so
deleting the `tracing::warn!` fails it while every counter assertion still passes.
`a_subscriber_that_re_registers_while_handling_the_warning_cannot_skew_the_latch`
pins the re-entrancy above.
`a_keyed_entry_that_declines_falls_through_to_a_generator_whose_type_is_still_checked`
(`navigator_public.rs`) pins the shape the warning cannot reach.

### 13. We keep the oracle's callback-before-observers order and drop its refusal, so an effect can be observed before its cause

**Rule:** as §4 above; same ADR.

**Oracle:** `Route.onPopInvokedWithResult` is called from `_RouteEntry.handlePop`,
inside `_flushHistoryUpdates`; the pop observation is only *queued* there, and
observers are notified afterwards by `NavigatorState._flushObserverNotifications`.
So a `PopScope` callback runs **before** `NavigatorObserver.didPop` in the
reference too.

**Choice:** keep that order. `apply` runs step 0 — everything the flush owes user
code, including deferred `PopScope` effects — before step 1 delivers to observers.
This is parity, and reordering would *create* a divergence rather than remove one.

**Where we diverge, and it is not the ordering.** `handlePop` runs under
`assert(navigator._debugLocked)`, and every imperative entry point on
`NavigatorState` asserts `!_debugLocked` — 13 of them. So in the reference a
synchronous navigation from `onPopInvokedWithResult` **aborts a debug build**.
Flutter's answer to "an effect observed before its cause" is not an ordering rule:
it is that you cannot get there.

FLUI permits it, deliberately — `pop_scope_callbacks_may_call_back_into_the_navigator`
guarantees it, and the permission exists because refusing re-entrancy is what
produced a fan-out deadlock here. **So we kept the reference's ordering and removed
its refusal, and the inversion is the price of that.** A `PopScope` callback that
navigates is observed before the pop that invoked it:

```
["push(RouteId(3), prev=Some(RouteId(1)))",
 "pop(RouteId(2),  prev=Some(RouteId(1)))"]
```

**Why not restore the refusal.** A `_debugLocked` equivalent would revert a
recorded improvement to buy back a restriction removed on purpose. Flutter can
afford the refusal because it never had to order the interleaving — refusing
re-entrancy means never having to sequence it. Having solved the harder problem,
adopting the easier prohibition would be a regression wearing a parity badge.

**What was missing until now** is exactly this entry: the *permission* was recorded
as an improvement and its *ordering consequence* was not. An improvement's cost is
not visible until something else changes, and the something else was already in the
tree.

**Replacement test:**
`a_pop_scope_callback_that_navigates_is_observed_before_the_pop_that_caused_it`
(`navigator_tests.rs`), red-checked by swapping step 0 and step 1 — which yields
`[pop, push]`, i.e. **the divergence, not the fix**.

### 14. `ParentDataView` ancestry is checked at attach, with catalog diagnostic labels

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — framework-user
composition errors must not surface as internal render-protocol panics.

**Oracle:** Flutter's `ParentDataWidget` / `_updateParentData` rejects misuse
(e.g. `Expanded` under `Stack`) with an "Incorrect use of ParentDataWidget"
diagnostic naming the widget, typical ancestor, and ownership chain. Debug and
profile/release are meant to agree on the contract (see flutter/flutter#108186).

**Choice:** keep Flutter's *observable* early-reject contract, expressed in Rust
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

**Replacement tests:** `parent_data_ancestry.rs` (`expanded_under_stack_…`,
`positioned_under_row_…`) — assert the attach-seam diagnostic and that the
message is not `BoxLayoutCtx::from_erased`. Happy paths remain in
`flex_parent_data.rs` / `stack_positioned.rs`.

### 15. `Container` is one render object, not a conditional widget stack

**Rule:** Prime Directive #1 — a convenience widget's implementation shape must
not make the caller's unkeyed child state depend on which cosmetic options are
set.

**Oracle:** `widgets/container.dart` builds `Align` / `Padding` / `ColoredBox` /
`DecoratedBox` / `ConstrainedBox` / margin `Padding` / `Transform` only when the
matching field is set. Toggling a field inserts or removes a level between the
parent and the child, so reconciliation diverges there and an unkeyed stateful
child below is rebuilt from scratch (flutter/flutter#161698). That issue is
still open, and the thread is worth reading before touching this decision:
maintainers weighed GlobalKey-like reparenting (goderbauer — concluded it
duplicates the GlobalKey mechanism and its cost, so a caller may as well key
the child), a "compressed element" holding the intermediate widgets (chunhtai),
a local deactivated-element map, and render-level composition. Hixie's position
is to fix the docs rather than the widget, and to steer people away from
`Container` entirely.

**Choice:** take the render-level composition — the option loic-sharma proposed
upstream (2025-05-13) and later prototyped as `Container2` in
`loic-sharma/flutter_playground` (2025-12-26). `Container` is a `RenderView`
over one `RenderContainer` (`flui-objects`) that carries margin, additional
constraints, padding, alignment, color, decoration and transform as *fields*.
The child's slot is therefore structurally fixed and no option can move it.

**Where we diverge from that prototype, and what it costs.** The upstream
sketch keeps composition in the render layer: its `RenderContainer` extends a
`RenderComposedBox` that builds a real render-object subtree
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
chunhtai's objection to render-level composition applies to us unchanged —
it fixes `Container` and not the general class, so any other conditional-layer
widget in this catalog keeps the same hazard.

Two properties follow, and both are the reason for the divergence:

* **State survives every toggle** with no `GlobalKey`, no retake, and no
  lifecycle churn — nothing for the caller to opt into, and no reparenting
  semantics leaking into an unmoved subtree.
* **The element tree is where the win is, not the render tree.** A conditional
  stack inflates and deflates an *element* per toggled option, and elements are
  not free: knopp reports on the upstream issue (2026-04-12) that element
  inflation/deflation is a measured bottleneck during fast scrolling — "a
  thousand cuts problem" — and names constraining `Container` to a single
  element as a direct improvement. That is the load-bearing argument for this
  divergence. The render-node accounting below is an honest cost statement, not
  the justification; read it as "what this costs", not "why we did it". FLUI has
  no equivalent measurement of its own yet, so this rests on an upstream
  maintainer's profiling, not ours.
* **Node count depends on whether there is a child.** With a child, identity
  (no options at all) is the widget passing the child straight through —
  zero extra nodes — so Flutter is cheaper there. At exactly one option,
  Flutter's stack is also exactly one extra node (a single `padding` builds
  one `RenderPadding`), so node count ties. `RenderContainer` only wins on
  count from two options up, where Flutter would otherwise stack one level
  per option (up to seven if every option is set). **Childless, Flutter is
  never free**: `build` reaches for a two-node placeholder (`LimitedBox` +
  `ConstrainedBox`) even with no option set at all (`Container()`), so
  `RenderContainer` already wins there. The only childless tie is a *tight*
  effective constraint — both `width` and `height` set, or an explicit tight
  `constraints` — which suppresses the placeholder and leaves Flutter a
  single `ConstrainedBox` against one node here; a lone `width` does not
  qualify, since `BoxConstraints::is_tight` requires both axes, so that case
  still takes the placeholder and costs three. Every
  other childless option (color, padding, decoration, an alignment paired
  with a fixed size) only grows Flutter's node count further, never brings
  it back below one. **What node count never buys, in either regime, is
  node weight**: `RenderContainer` carries every field — alignment, padding,
  margin, color, decoration, additional constraints, transform, plus the
  committed child offset/size/baselines — whether or not that option is
  set, so it is heavier than whichever single-purpose object the stack
  would have used, in every configuration including identity. The reason
  for the divergence is the stable slot, not a cheaper or lighter
  `Container`.

**Intrinsics:** a tight additional width or height answers before the child
is queried, matching `RenderConstrainedBox`. Without that short-circuit a
`LayoutBuilder` (or any child that rejects speculative intrinsic queries)
would be asked even though the result is discarded.

**Parent-data transparency:** Flutter's identity `Container` (every option
absent) builds to the child itself, so `Row → Container → Expanded` and
`Stack → Container → Positioned` attach the parent-data widget directly to
Flex/Stack. A `RenderView` always inserts `RenderContainer`
(`ParentData = BoxParentData`) between them, so those trees panic at
`apply_ancestor_parent_data`. That is a named consequence of the stable-slot
choice, not an accidental drop: restoring identity passthrough would recreate
flutter/flutter#161698 the moment any option is toggled on. The supported
shape is `Row → Expanded → Container` / `Stack → Positioned → Container`.
Covered by
`identity_container_between_flex_and_expanded_is_not_parent_data_transparent`.

Parent data is not the only consequence of that always-a-node choice.
Hit-testing has the same shape one level up: `RenderContainer` always bounds
the incoming position against its own box before doing anything else, while
Flutter's identity `Container` is not a node at all and so bounds nothing. A
child whose own `hit_test` deliberately does not bound itself — `RenderTransform`
is the documented case — is therefore reachable outside the container's box in
Flutter and not here, whenever *no* margin and *none* of the five gated
properties are set. Measured under a shared `RenderPadding` parent with a
scaled child, three of four probe points outside the box hit in Flutter's tree
and miss here. Unlike the margin-band gate below, this one is **not** closed:
the gated-level reasoning that fixes that case does not extend to the outer
gate, because at identity there is no level to reason about — the node itself
is the divergence. Tracked in issue #1143.

**Collapsed branch:** Flutter's three childless shapes — the placeholder
`LimitedBox(0, 0, child: ConstrainedBox(expand))`, an empty `Align`, and no
inner widget at all — all resolve to the same box, so `RenderContainer` has no
childless branch. The equality is proven, not assumed, by
`harness_container_childless_matches_each_flutter_shape_it_replaces`, which
diffs each real shape against `RenderContainer` under the configuration
Flutter would pick it for, and additionally forces the placeholder shape
under the tight additional constraints branches two and three use, so all
three shapes are diffed against EACH OTHER too, not only each against
`RenderContainer`.

**Not carried over:** `foregroundDecoration`, `clipBehavior`, `isAntiAlias` and
`transformAlignment` have no FLUI `Container` setter. These are four
different kinds of gap, not one undifferentiated "not yet":

- **`clipBehavior` does not fit this shape at all.** [`PaintEffects`] gives a
  node exactly one clip slot, wrapping everything the node's `paint` records
  as one fragment. Flutter's `ClipPath` (the `clipBehavior != Clip.none`
  branch in `Container.build`) sits between `ColoredBox` and `DecoratedBox`:
  it clips the padding, color and child, and explicitly does **not** clip
  the decoration (`DecoratedBox` wraps the already-clipped `current`
  afterward, unclipped). `RenderContainer::paint` records decoration, color
  and child as one fragment, so a `PaintEffects.clip` here would clip the
  decoration too — wrong. Adding this needs a paint-level re-split (a second
  recorded fragment, or a clip scoped to a sub-range of one), not a new
  `Option` field.
- **`foregroundDecoration` is additive.** A second decoration field, a
  `paint_box_decoration` call after the child (Flutter's `DecorationPosition
  .foreground`, painted on top rather than behind), and a hit arm —
  `DecoratedBox`'s own doc states a foreground decoration participates in
  `hitTestSelf` exactly like the background one does.
- **`transformAlignment` is additive but not local.** It needs an
  `alignment: Option<Alignment>` field folded into the pivot the way
  [`RenderTransform::effective_transform`] already combines one with its own
  base matrix, and that combined value would have to move together through
  `apply_paint_transform`, `hit_test`'s inverse, `paint_translation`,
  `skip_paint` and `owns_effect_layer` — every site that reads `self.transform`
  today.
- **`isAntiAlias` is additive and narrow.** In Flutter it is a `ColoredBox`-
  only flag (`Container.build`'s `ColoredBox(color:, isAntiAlias:, …)` call);
  nothing else in the stack reads it. FLUI's own color fill
  (`ctx.canvas().draw_rect(rect, &Paint::fill(color))`) always anti-aliases
  (`Paint::fill`'s default), with `Paint::with_anti_alias` already available
  to turn it off — adding the setter is one field plus one call-site change,
  not a structural gap.

A `BoxDecoration` border's thickness is separately still not folded into the
effective padding (`_paddingIncludingDecoration`) because `flui-types`'
`BoxDecoration` exposes no border insets. Flutter also `assert`s that `color`
and `decoration` are mutually exclusive; FLUI accepts both and paints color
over the decoration — the order the widget stack would have produced
(`DecoratedBox` enclosing `ColoredBox`) — rather than panicking.

**Replacement tests:** the geometry the collapsed stack owes is pinned against
the stack itself by `harness_container_matches_the_widget_stack_it_collapses`
(size, child size, absolute child position and hit path, over eight
configurations spanning both wet layout and hit-testing, each making a
different level decide), plus
`harness_container_paints_its_chrome_inside_the_margin` for the decorated box's
own rect — the level Flutter's `paints..rect(...)` oracle pins and the one a
single node no longer exposes as a separate render object. State stability is
covered by `container.rs`'s `container_optional_*_preserves_unkeyed_child_state`
family and `animated_container_optional_color_preserves_unkeyed_child_state`;
all five fail against the conditional stack and pass against this node.
Tight additional constraints answering an intrinsic without querying a
`LayoutBuilder` child are covered by
`container_tight_width_does_not_query_layout_builder_intrinsics` and
`container_tight_height_does_not_query_layout_builder_intrinsics`.
Chrome self-hit uses the same half-open gate as the stacked `DecoratedBox`
(`harness_container_decoration_misses_the_exclusive_chrome_max_edge`);
baselines add the child's offset
(`harness_container_baseline_adds_child_offset`).

**Hit-testing gates the child behind the SAME boxes the stack does — only
when a level exists to gate on, and none always does.** `RenderContainer::
hit_test` tests the child before the decoration/color path (a child hittable
in a cut-out the decoration's rounded corners exclude must stay reachable),
but ordering is not the only thing that has to match the stack: `Container.
build` inserts `Padding`/`ColoredBox`/`DecoratedBox`/`ConstrainedBox`/`Align`
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

Pinned in both directions: `harness_container_margin_alone_does_not_gate_an_
overflowing_child` (nothing set — the tap DOES hit) against
`harness_container_color_gates_an_overflowing_child_in_the_margin_band` (one
property added — the identical tap does NOT), and
`harness_container_padding_does_not_expose_an_overflowing_aligned_child`
(the narrower content-box gate, alignment set) against
`harness_container_padding_without_alignment_does_not_narrow_the_gate` (the
same padding, no alignment — the content-box gate must not bind on its
own). The differential carries the matching pair of cases too, and
`padding` is `Option<EdgeInsets>` on `ContainerStackCase` for the same
reason it is on `RenderContainer` — a composed tree that always inserted a
zero-inset `Padding` level would silently endorse the divergence instead of
detecting it.

### 16. A push's entrance-transition future is awaited outside the flush that installed it

**Rule:** Prime Directive #1 — a Flutter contract carried over a flush-timing
constraint the reference never has, so the mapping decision belongs here
beside the local placement it governs; [ADR-0064](../../docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md)
records the cross-crate design this decision consumes.

**Oracle:** `handlePush` (`navigator.dart:3273-3290`) parks an entry in
`pushing` and attaches `routeFuture.whenCompleteOrCancel(...)`, which always
arrives on a later microtask — Flutter can never observe that callback firing
while `_flushHistoryUpdates` itself is still on the stack.

**Choice:** `PushCompletion::Animating(TickerFuture)` carries the future
`AnimationController::forward()` (or an equivalent run-starting call) returns,
but the continuation that awaits it is registered from `NavigatorShared::apply`
— after the flush that produced the entry has released the history lock —
never from inside `RouteEntry::handle_push` itself. `RouteHistory::flush()`
re-drains any `RouteCommand`s a route raised between its own passes, so a
continuation registered mid-flush on an already-resolved future (a
zero-duration push, or one canceled before the flush even returns) would
settle within that same flush rather than on the next one — a timing FLUI can
reach and Flutter's microtask model cannot. The continuation itself may only
push `RouteCommand::PushCompleted(id)` onto the `Send` route-command queue and
schedule the Navigator's rebuild through `NavigatorShared::settle_wake`, read
at the moment the continuation fires rather than captured at registration:
`NavigatorHandle::push` flushes immediately, so a route pushed before the
Navigator mounts registers its continuation while that slot is still empty,
and only a later mount fills it. Cancellation settles the entry exactly like
completion — there is no separate "the push was canceled" state at this
layer, only whichever lifecycle state the entry has moved to by the time the
queued command is drained.

### 17. `Semantics` action builders take `Send + Sync` handlers, so the caller hoists the `Arc` where the reference's closure captures a `State` field

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — the reference's
observable behavior is the floor; where a contract is better, improve it and
record what is. This file's own scope note puts a callback bound here rather
than in an ADR: it is local to this crate.

**Oracle:** `Semantics(onTap: …, onSetText: …)` (`src/semantics/semantics.dart`,
tag `3.44.0`) relays Dart closures into the `SemanticsConfiguration` setters,
which store them and hand them to the engine. Dart's type system has no thread
affinity, so the reference states no bound anywhere. The nearest FLUI analogue
of that configuration object is `SemanticsConfiguration`, and the widget-level
builders are new ergonomics over it rather than a transcription of it — the
reference's own widget does not surface `onShowOnScreen` or `onScrollToOffset`
at all, though its configuration does.

**Choice:** eleven payload-free builders — `on_tap`, `on_long_press`,
`on_scroll_{left,right,up,down}`, `on_increase`, `on_decrease`,
`on_show_on_screen`, `on_focus`, `on_blur` — take
`impl Fn() + Send + Sync + 'static`; `on_set_text` takes `impl Fn(&str) + …`
and `on_scroll_to_offset` takes `impl Fn(f64, f64) + …`. `on_action` registers
any `SemanticsAction` verbatim for the two things the typed set deliberately
does not cover. All of them go through one private `add_action_handler`.

**Why the oracle's shape does not transcribe.** The bound is already at the
storage, so it has to be met somewhere, and this is where it is met:

- `SemanticsActionHandler` is
  `Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>`
  (`crates/flui-semantics/src/action.rs`). **The bounds come from storage, not
  from a calling convention:** the handler is stored in a
  `SemanticsConfiguration`, which rides in the annotation render object, whose
  `RenderView::RenderObject` associated type is pinned `Send + Sync + 'static`.
  The handler is *not* invoked across a thread — resolution is owner-local and
  commits only at the pipeline's `Idle` point
  (`PipelineOwner::resolve_semantics_action`, whose module doc says exactly
  this), which is why the invocation it returns holds a cloned handler rather
  than a borrow. The genuinely cross-thread seam in this story is one layer out,
  in the platform's action *listener*. An earlier draft of this entry gave the
  thread-crossing reason; it sounded right and was wrong.
- `RenderView::RenderObject` is bounded
  `RenderObject<_> + Send + Sync + 'static`
  (`crates/flui-view/src/view/render.rs`), so the annotation render object the
  widget wraps cannot hold a `!Send` closure either.
- The catalog's **dominant** callback convention is `Rc<dyn Fn(..)>`, owner-thread-local: 56 such
  type aliases across `flui-widgets/src` (44) and `flui-material/src` (12), counted as
  `type <name> = Rc<dyn Fn…>` declarations *including* those whose `Rc<dyn Fn` sits on a
  continuation line (`pub trait`-style wrapping is common on these signatures, so a same-line read
  under-counts them: 34/11). This bound is stricter than that convention, and a
  caller meets it on the first handler they write.
- It is **not unprecedented**, and the precedent is worth reading rather than rediscovering. Ten
  public builders already take `impl Fn(..) + Send + Sync + 'static`: `interaction/draggable.rs`
  (5), `interaction/drag_target.rs` (4), `scroll/page_view.rs` (1). `draggable.rs` names the
  rationale outright, calling those `Arc` bounds a "legacy storage shape, not a cross-thread callback
  contract" — which is precisely an action handler's situation. Each of those ten stores the
  callback as `Arc::new(<the caller's closure>)`; none of them shows the hoist pattern a caller
  needs when the closure must be shared with something else, so `on_action`'s own docs carry that
  example instead of pointing at them.

**Consequences, named rather than left to be discovered:**

- **The ordinary "activation toggles this control's own state" closure does not
  compile.** A widget's state is `Rc<RefCell<_>>`, which is neither `Send` nor
  `Sync`. `Arc<Mutex<_>>`, or a shared store, is the way through — the same
  trade `CustomPainter` already makes, which is `Send + Sync` and
  widget-facing. `on_action`'s own docs show the hoist.
- **This publishes actions; it does not make any shipped control
  activatable.** No Material or Cupertino widget gains a semantics action here,
  so nothing in the catalog can be activated through the semantics tree yet.
  Flutter's `InkResponse` publishes `Semantics(onTap: …)` itself
  (`material/ink_well.dart`), which is why the reference's nodes carry
  `SemanticsAction.tap` while FLUI's do not — the gap is on FLUI's side, not a
  shape the two share. `Button`, `Checkbox` and `ListTile` publish no tap
  semantics of their own in either framework, so the wiring belongs at
  `InkResponse`'s layer when it lands. Bridging a widget's *existing* gesture
  callback into an action is a separate change, and what blocks it is storage and
  threading — not this surface.
- **The likelier long-term shape is the opposite one, and it is rejected here
  only for want of a design.** No surveyed framework puts the bound on the
  handler: GPUI's listener carries none at all, and Bevy, Iced, Slint, and
  Dioxus/Blitz each put `Send` on a sender the widget owns rather than on the
  callback. What FLUI would need to reach that shape is the `!Send` closure
  carried beside an owner-local handle — a handle type and its own design
  record, so it is not this widget's change; until then the bound is the
  documented contract rather than an accident a later reader has to guess at.

**A handler allocated on every build costs a `SEMANTICS` impact on every
rebuild — the price of comparing handlers by identity, pinned rather than
assumed.** The configuration stores the handler it is handed and compares two
configurations' handlers with `Arc::ptr_eq`
(`crates/flui-semantics/src/configuration.rs`), so a handler built on the spot
inside `build` is a fresh `Arc` each time, the mounted configuration compares
unequal, and `RenderSemanticsAnnotations::set_configuration` answers
`RenderUpdateImpact::SEMANTICS` even when nothing semantic changed — every typed
builder allocates one through `add_action_handler` (`src/semantics/mod.rs`). The
escape is the hoist `on_action`'s own rustdoc demonstrates: build the handler
once where the widget's state lives and let each build take an `Arc::clone`.
What holds that consequence in place is
`a_handler_allocated_per_build_costs_a_semantics_impact_per_rebuild`, a unit
test in `src/semantics/mod.rs` beside the identity pin: a later change that
dedupes the builders, caches the handler, or relaxes the comparison has to flip
that test deliberately rather than move the re-publish rate of every
action-bearing node in silence.

**Builder inventory, and what has no builder.** FLUI's action vocabulary
(`crates/flui-semantics/src/action.rs`) has 24 `SemanticsAction` variants, of
which **9 have no inbound route at all** — the eight the translation table drops
deliberately (the four cursor moves, copy, cut, paste, and dismiss) plus
`DidGainAccessibilityFocus`, which is advertised outbound and unreachable
inbound. Of the 15 routable ones, 13 have a typed builder and `SetSelection` /
`CustomAction` are reachable through `on_action` only. `expand` / `collapse`
have no `SemanticsAction` variant to map to, so the reference's `onExpand` /
`onCollapse` are not merely unwired here. `on_blur` is deliberately *not* the
mirror of `on_focus`: the platform reports losing accessibility focus as a
notification about something that already happened, whereas a focus request is a
command the node may refuse.

**A payload that did not cross the seam is dropped, not defaulted.**
`on_set_text` and `on_scroll_to_offset` trace a `warn!` and do nothing when
`ActionArgs` arrives without the matching payload. `""` and `(0.0, 0.0)` are
both legitimate values a platform can mean, so substituting either turns a lost
payload into a silent edit or a scroll to the origin — a wrong result that reads
as a right one.

**Replacement tests** (`tests/semantics.rs`), each with what it can fail on:

- `a_tap_handler_round_trips_from_a_platform_click_to_the_callback` — the
  acceptance test, and it asserts both halves: the node *tells* the platform the
  action exists (`supports_action(Action::Click)`) and pressing it *runs* the
  callback exactly once. A node passing only the first half is the dead control
  this surface exists to rule out.
- `a_semantics_node_with_no_tap_handler_advertises_no_click` — the negative
  control, so the advertise half is not satisfied by a node that advertises
  everything.
- `a_set_text_request_carries_its_payload_into_the_handler` — the payload path,
  which no payload-free test reaches.
- `the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set` —
  derives the unreachable set from the live translation table and asserts it
  equals the documented nine, so a table change that closes one is loud rather
  than a silent improvement nobody notices.
- `the_exhaustive_routing_list_agrees_with_the_translation_table` — the routing
  predicate is written out exhaustively and then checked against the production
  table, so it cannot drift into a second copy of the answer. Because
  `accesskit::Action` is not `#[non_exhaustive]`, an upstream release that adds
  a variant stops this file compiling rather than silently dropping it.
- `block_user_actions_refuses_a_click_the_node_still_holds_a_handler_for` — both
  halves, now measured rather than implied. The refusal half: the request errors
  *and* the handler did not run. The advertise half: the blocked node does **not**
  advertise the click, which is asserted as the measured value rather than
  assumed — `blocks_user_actions` narrows the effective action set that snapshot
  export and input dispatch both consult, so the node is invisible to assistive
  technology instead of a control it can see and press to no effect.

**Red→green, measured rather than asserted.** Both halves of the acceptance test
were shown to fail against a mutated builder and then pass against the restored
one: replacing the handler invocation with a discarded binding turns
`a_tap_handler_round_trips_…` red on *its own* delivery assertion
(`left: 0, right: 1` — the advertise assertion above it stays green, so the two
halves are independently pinned), and the same mutation of `on_set_text` turns
`a_set_text_request_carries_its_payload_into_the_handler` red with
`left: [], right: ["hello"]`.

### 18. Replacing a `HeroController` retires its in-flight flights, restoring both heroes

**Rule:** Prime Directive #1 — the reference's observable behavior is the floor;
where the reference has no behaviour (no analogue exists), FLUI names the rule,
justifies it, and pins it with a test. This entry is local to the crate, so it
lives here rather than in an ADR.

**Oracle:** Flutter's `HeroController` is owned by its `NavigatorState` for the
navigator's whole life (`widgets/navigator.dart`), so there is no "replaced
controller" state to define. The two reference seams that do touch flight
cleanup are `_HeroFlight.dispose` (`heroes.dart:654-665`) and
`HeroController.dispose` (`heroes.dart:1112-1116`): dispose removes the overlay
entry and un-links the proxy, but it does **not** call `endFlight` — both
heroes' placeholders stay frozen, which is fine in Flutter only because the
whole tree is being torn down with the navigator, so the blank placeholder is
about to be destroyed anyway.

**Choice:** when a `HeroController` is detached — replaced by
`NavigatorHandle::add_observer` (which takes the auto-installed default),
removed by `remove_observer`, or its navigator unmounts —
`HeroController::did_detach` calls `FlightManager::finish_all`, which **aborts**
every flight still in the air: it removes each overlay entry and calls
`end_flight(false)` on **both** heroes. Distinguishing an abort from the
ordinary `finish` is load-bearing: a normal `finish` ends one hero hidden and
the other revealed, chosen by the terminal animation status (the
`heroes.dart:608-614` comment), whereas an abort has no status to choose with
and must leave both pages — which stay alive and in the stack — showing their
real children rather than a blank placeholder.

**Why the reference's shape does not transcribe.** Flutter's controller is never
replaced in place, so its cleanup is a full-tree teardown that tolerates frozen
placeholders. FLUI's controller is a swappable observer, and the flight it
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

**Replacement test**
(`navigator::hero_gesture_tests::replacing_the_auto_hero_observer_retires_its_in_flight_flight`):
pushes two same-tagged hero pages so the auto observer launches a real
programmatic flight, then installs a manual controller and asserts (a) the
overlay count returns to its pre-flight value, (b) the replacement controller
inherited no flight, and (c) both heroes' placeholders are cleared. Red-check:
deleting the `finish_all` call from `did_detach` leaves the overlay count one
entry high (`left: 4, right: 3`) and both placeholders set.

### 19. Word-boundary movement uses `unicode-segmentation` (UAX #29), not ICU dictionary segmentation

**Rule:** Prime Directive #2 — search the market/existing dependency graph
before adding one, and cite what an unmatched reference actually needs.

**Oracle:** Flutter's `TextPainter.getWordBoundary` (which
`RenderEditable`'s Ctrl+Arrow word-jump and double-tap word selection both
call) wraps `dart:ui`'s `Paragraph.getWordBoundary`, backed by ICU's
`UBreakIterator` in word mode. ICU's word breaking is **dictionary-based**
for two distinct groups: Thai, Lao, Khmer, and Myanmar (scripts with no
spaces between words at all, where the Unicode Standard Annex #29
default algorithm — rule-based, no lexicon — cannot find a linguistically
correct boundary on its own), and separately Chinese/Japanese, via ICU's
`cjdict` word-frequency dictionary (these scripts DO have UAX
#29-recognized character-class boundaries, so the rule-based default
does not fail outright the way it does for the first group — it just
segments per character/script-run rather than per linguistic word).

**Choice:** `unicode-segmentation`'s `split_word_bound_indices` — pure UAX
#29, no dictionary data. It was already a workspace dependency (pulled in
for extended-grapheme-cluster caret/Backspace/Delete, landed before this
change) and pure Rust, so wiring it into word-boundary movement too adds
no new dependency, no C binding, and no data-table download — unlike an
ICU binding (`rust_icu`, `icu4x`), which would be a materially heavier
addition for the one feature this touches.

**Why the reference's shape does not transcribe.** ICU's dictionary data
— both the Thai/Lao/Khmer/Myanmar lexicons and `cjdict` — is the expensive
part of the reference's behavior, megabytes of data, not an algorithm,
and nothing else in this workspace needs it. Bringing in a full ICU
dependency to correct word-jump behavior for a handful of scripts, when
every script with UAX #29-recognized boundaries (Latin, Cyrillic, Greek,
Arabic, Hebrew, Hangul, and more) already works correctly for free, is
the wrong trade for what this feature is worth today.

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
  needed for it). A known, tested limitation
  (`get_word_boundary_splits_cjk_per_character_not_per_word`), not a
  silent bug.
- **Every other script this crate's tests cover — Arabic (clusters-only,
  see its own test's caveat below), Latin with apostrophes, emoji
  clusters — segments correctly** per UAX #29's own rules, which is the
  ceiling this crate claims for them; no claim is made about Hebrew,
  which this crate does not currently test.
- **Grapheme-cluster correctness is unaffected.** The dictionary gap is
  specific to WORD boundaries; cluster boundaries (caret, Backspace,
  Delete) use `GraphemeCursor`, a different UAX #29 mode with no
  dictionary dependency in the reference either, and are correct for
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

**Replacement tests**
(`flui-widgets::controller::tests::{word_right_lands_on_the_next_words_start_skipping_trailing_whitespace,
word_left_returns_to_the_current_words_own_start_without_skipping_it,
a_run_of_whitespace_is_skipped_as_one_stop_not_a_stop_per_space,
an_apostrophe_inside_a_word_does_not_split_it,
cjk_text_splits_per_character_not_per_word,
arabic_text_word_jump_never_lands_inside_a_char}`,
`flui-widgets::text::editable_text::tests::word_jump_modifier_maps_every_platform`
(the platform table itself, every `TargetPlatform` variant),
`flui-painting::tests::text_layout_unit::{get_word_boundary_selects_a_whole_whitespace_run,
get_word_boundary_does_not_split_on_an_apostrophe,
get_word_boundary_splits_cjk_per_character_not_per_word,
get_word_boundary_prefers_the_word_side_of_a_boundary_over_the_whitespace_side,
get_word_boundary_prefers_the_following_segment_between_two_word_segments,
get_word_boundary_word_vs_punctuation_boundary,
get_word_boundary_two_space_run_boundary_matrix,
get_word_boundary_at_the_buffers_own_leading_and_trailing_whitespace,
get_word_boundary_never_splits_inside_a_zwj_emoji_cluster}`): each asserts
a specific, pinned boundary — including the CJK per-character split and
the boundary tie-break matrices (`"foo bar"` at 0/3/4/7; `"(foo"` at 1;
`"日本語"` at 3; `"foo, bar"` at 3/4; `"foo  bar"` at 3/4/5; leading/
trailing whitespace at a buffer's own edges) — rather than a loose "some
boundary was found" check. Arabic is the one exception, asserted only as
"never lands inside a char, always makes progress",
because this port does not claim cluster-vs-word segmentation parity for
that script specifically, only that it is never corrupted.

### 20. `GestureDetector` composes AROUND `Listener`, not inside it, for double-tap word selection

**Rule:** Prime Directive #2 — reuse tested framework machinery
(`flui_interaction::DoubleTapGestureRecognizer` via
`flui_widgets::GestureDetector`) rather than hand-rolling tap-count/slop/
timeout tracking a second time inside `EditableText`'s own pointer
handlers.

**Oracle:** Flutter's `EditableText` composes its own
`TextSelectionGestureDetector` (a `RawGestureDetector` subclass) around
the text span, wiring `onDoubleTapDown` to
`_handleDoubleTapDown` → `renderEditable.selectWord`
(`widgets/editable_text.dart`, `widgets/text_selection.dart`).

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
[`TextLayout::get_word_boundary`](#19-word-boundary-movement-uses-unicode-segmentation-uax-29-not-icu-dictionary-segmentation)
— the same `unicode-segmentation`/UAX #29 machinery Ctrl/Alt+Arrow
word-jump uses one layer down, but NOT the same function: the keyboard
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

**Why the reference's shape does not transcribe.** Flutter's
`TextSelectionGestureDetector` is a text-specific subtype that also owns
triple-tap and drag-selection-handle gestures this port does not have
yet; building an equivalent specialized subtype for one callback would be
premature machinery for what `GestureDetector`'s existing generic API,
widened by one method, already covers.

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

**Replacement tests**
(`flui-interaction::recognizers::double_tap::tests::on_double_tap_down_fires_at_the_second_contacts_own_down_not_its_up`,
`flui-widgets::tests::gesture_detector_advanced::{double_tap_down_fires_before_the_second_contact_lifts,
on_double_tap_down_alone_with_no_on_double_tap_still_participates}`,
`flui-widgets::text::editable_text::tests::a_double_tap_selects_the_word_under_it`):
the recognizer-level tests pin the DOWN-not-UP timing and the
participation-gating fix in isolation; the widget-level test is the
end-to-end proof that a double-tap on a mounted `EditableText` actually
selects the word, not just that the underlying callback fires. Red-check
for the last one: skip wrapping `install_pointer_handlers`'s return value
in `wrap_double_tap_word_select` — the selection stays collapsed after
the second tap.

