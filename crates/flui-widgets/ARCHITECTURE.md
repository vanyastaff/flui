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

### 4. Named routes split into six untyped entry points and one typed one, and a request that cannot be served is a typed error

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

Both failures are total: **nothing is pushed, popped, replaced, or removed, no
observer is notified, and the generated route is disposed** (see §7 below).

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

### 5. `pop_and_push_named` resolves before it pops

**Rule:** as §4 above; same ADR.

**Oracle:** `NavigatorState.popAndPushNamed`, which is literally
`pop<TO>(result); return pushNamed<T>(routeName, arguments: arguments);` — the
pop is committed before `_routeNamed` is called.

**Choice:** resolve the name first, then pop, then push. The two mutations are
the same two calls in the same order, so on the success path the observer stream
is identical to the oracle's.

**Why:** Flutter's order makes a failed generation leave the stack already
mutated — the app is one route shallower and the exception surfaces from the
middle of a two-step operation, with no route to show for it. Reversing the two
makes the failure total, which is what lets §4's "nothing was pushed" guarantee
cover this entry point too. Nothing is lost: the generator cannot observe the
stack it is about to be pushed onto in either ordering, because Flutter calls it
after a pop that has not flushed.

**Replacement tests:** `pop_and_push_named_with_an_unresolvable_name_pops_nothing`
and `pop_and_push_named_with_an_unresolvable_name_delivers_its_result_to_nobody`
(`tests/navigator_public.rs`). Those two are the *only* tests that cover this
divergence, and the distinction matters: the success path cannot see it, because
both orderings pop and then push. Moving `self.pop()` back above
`self.resolve_named(..)` leaves the parity leg of
`named_route_arguments_reach_the_generator_for_every_named_entry_point` and
`pop_and_push_named_with_delivers_its_result_to_the_popped_route` green and
fails only these two.

A separate oracle, `pop_and_push_named_observes_a_pop_where_push_replacement_named_does_not`,
pins that this really *is* a pop-then-push and not a replacement wearing its
name. Stack shape and result delivery cannot tell those apart — a
`PushMode::Replace` body keeps every other named-route test green, the parity
leg included — so the discriminator is the observer stream: a pop-and-push emits
`didPop` + `didPush`, a replacement emits `didReplace` and neither.

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

**Consequence, named rather than left to be discovered:** a factory that clones
its own `NavigatorHandle` in — the natural way to navigate from inside one —
closes an `Arc` cycle through the registry, and the navigator's storage is never
reclaimed. Observers escape this by attaching only while mounted; registrations
have no such lifecycle. Documented on `on_generate_route`; a `Weak` handle would
fix it and is not in this slice.

**Replacement tests:**
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
`with_arguments_shared_relays_a_payload_without_changing_its_identity` (which
also asserts the contrast: `with_arguments` on an identical value is *not*
`ptr_eq`), and the identity half of
`on_unknown_route_runs_only_after_the_generator_declined_and_sees_the_callers_payload`,
which compares against an `Arc` the caller constructed rather than against the
settings object it was handed.

### 9. A route factory is handed its navigator, so it never needs to capture one

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — the reference's
observable behavior is the floor; where a contract can be improved, improve it
and record what is better.

**Oracle:** `widgets/navigator.dart`, the `RouteFactory` typedef —
`Route<dynamic>? Function(RouteSettings)`. A Dart factory that needs to navigate
closes over `Navigator.of(context)` and the resulting reference cycle is
collected.

**Choice:** the factory takes a `RouteRequest<'_>` — the settings *and* the
`&NavigatorHandle` resolving them — with `settings()`, `name()`, `argument::<T>()`
and `navigator()` on it.

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

**Consequence, named rather than left to be discovered:** this is a breaking
change to every registration call site, and deliberately so — it is a compile
error at each one, which is the cheapest it will ever be.

**Replacement tests:** `a_factory_is_handed_its_own_navigator_and_the_callers_request`
asserts the handle is *this* navigator (`is_same`) and not merely some navigator,
against a second handle registering the same name — returning a fresh
`NavigatorHandle` from `RouteRequest::navigator` fails it.
`a_factory_that_pushes_re_entrantly_does_not_deadlock` now pushes through
`request.navigator()` with no cell, and still pins that the registry guard is
released before the factory runs — holding it deadlocks the owner thread.

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

Arguments ride on `RouteKey::request(args)`, producing a `KeyedRequest<T>` that
`push_keyed` takes via `impl Into<_>` — mirroring the string path's
`impl Into<RouteSettings>`, so `push_keyed(ORDER)` and
`push_keyed(ORDER.request(id))` are one method. Deliberately **not**
`push_keyed_with(key, args)`: on this handle `_with` means "a result delivered to
the departing route" (`pop_with`, `push_replacement_with`, `remove_route_with`,
`maybe_pop_with`), and spending that suffix on arguments would make one word mean
two things.

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

**What this does not close.** The erased fall-through: a table entry that
declines and an `on_generate_route` / `on_unknown_route` that answers with a
differently-typed route. Nothing at those hooks' registration sites knows what
`Output` they will produce, so `GeneratedRoute::checked`'s runtime comparison
stays — with its documented remit shrunk to exactly that one shape for keyed
callers, instead of implying it is the general guard.

**Replacement tests:**
`a_route_re_registered_with_a_different_output_type_warns_once_per_navigator`
(`navigator_tests.rs`) asserts six conflicts produce one warning, that a
same-type replacement produces neither, and that a second navigator warns for
itself — dropping the latch reads 6 instead of 1, dropping the type guard makes
the same-type replacement warn.
`a_keyed_entry_that_declines_falls_through_to_a_generator_whose_type_is_still_checked`
(`navigator_public.rs`) pins the shape the warning cannot reach.
