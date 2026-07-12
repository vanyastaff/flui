# ADR-0027 Step 2b: owner-routed interaction routes

Status: Approved architecture, implementation pending
Scope: `Listener`/`GestureDetector`, `MouseRegion`, cached routes, and the
owner-local callback boundary with `Send + Sync` render data.

## Decision

Render objects and `HitTestEntry` become data-only. Executable interaction
callbacks live in a realm-owned, structurally owner-local registry. A new hit
result resolves typed target IDs to owner-local callback cells before dispatch.

```text
Render tree / HitTestEntry                 UiRealm interaction lane
--------------------------                 ------------------------
PointerTarget (Send + Sync)        --->    registry Rc<HandlerCell>
transform / cursor / region ID             |
                                            +--> ResolvedHitRoute
                                                 (strong Rc cells)
```

An active Down route retains strong callback cells until Up/Cancel and continues
delivery after target unmount, matching Flutter's cached `HitTestEntry.target`.
Missing targets in a newly resolved data-only hit result become ordered
`RouteResolutionMiss` values; direct operations on a missing target still return
the typed `InteractionDispatchError::TargetGone` error.

Gesture execution is also owner-localized in its delivery stage:

- `GestureBinding`, arena state, recognizers, and recognized detail values are
  owner-thread control-plane state and become `!Send + !Sync`;
- recognizer/user callback aliases lose `Send + Sync`;
- `Arc<Mutex<callback>>` slots become plain owner-local state or
  `Rc<RefCell<...>>` where shared identity is required;
- raw pointer targets are the only render-to-owner protocol. Recognized gesture
  details never cross a token protocol, `Any`, serialization, or a queue.

`RenderObject: Send + Sync` remains an ADR-0027 invariant.

## Alternatives

### Queue callbacks to Idle

Rejected. Down handlers must join recognizers before arena close and Up handlers
must run before arena sweep. Queuing changes gesture semantics, clones payloads,
loses synchronous delivery, and adds latency. Owner inboxes remain correct for
cross-thread ingress, not for the event already executing on the owner.

### Store `Rc<dyn Fn>` in render objects

Rejected. It makes heterogeneous render storage owner-local and breaks future
raster transfer and immutable hit-test movement.

### Lookup a target token on every event

Rejected as the cached-route design. Unmount between Down and Up would remove
the target and suppress Flutter-required delivery; every Move would also repeat
the registry lookup.

### Data-only entries plus resolved routes

Selected. Down resolves once to strong cells; Move/Up/Cancel reuse them. Rebuild
mutates the same cell, so the active route observes the current handler like
Flutter's retained render target.

## Ownership and dependencies

- `flui-interaction` owns target/route IDs, callback cells, owner-local lane,
  resolution/invocation, gesture-local state, mouse annotations, and errors.
- `flui-rendering` stores/re-exports data-only targets in `HitTestEntry`.
- `flui-objects` stores targets in `RenderListener`/`RenderMouseRegion`.
- `flui-view` exposes a narrow render-object mount/update context for handler
  registration and replacement.
- `flui-widgets` owns user callback configuration and adapters.
- `UiRealm` and `HeadlessBinding` own and activate one interaction lane.
- A future `PresentationRuntime` owns gesture/hit-test state and uses its parent
  realm's lane; multiple presentations still execute serially at that owner.

No lower crate depends on `flui-app`; `rendering -> interaction` stays acyclic.

## Types and structural invariants

```rust,ignore
InteractionLane             // Rc-backed; !Send + !Sync
InteractionDispatchHandle   // ticket only; Clone + Send + Sync
PointerTarget               // lane identity + generational/non-reused target ID
MouseRegionTarget           // same data-plane contract
ResolvedRouteToken          // lane identity + generational/non-reused route ID
RouteResolution             // valid route token + ordered resolution misses
RouteResolutionMiss         // TargetGone { path_index }
ResolvedHitRoute            // owner-local Vec<ResolvedHitEntry>
HandlerCell                 // Rc cell containing the current local handler
```

1. Target and route identities cannot ABA-alias after slot reuse or realm
   recreation. `ResolvedRouteToken` validates both its lane identity and route
   generation/non-reused ID.
2. Route resolution is partial. `[live, gone, live]` produces a valid route
   containing both live entries in their original relative order plus an
   ordered `RouteResolutionMiss::TargetGone { path_index: 1 }`. An all-missing
   path produces a valid empty route plus one ordered miss per path entry.
3. Direct target mutation/removal still reports the typed `TargetGone` error;
   boundary failures remain typed as `WrongThread`, `InactiveRealm`,
   `WrongRealm`, and `OwnerGone`.
4. Cached-route failures distinguish `StaleRoute` from `OwnerGone`; neither may
   resolve to a newer route occupying the same slot.
5. Resolution clones strong `Rc<HandlerCell>` values. Every registry/route-table
   borrow ends before callback invocation.
6. Rebuild replaces the handler inside the existing cell. Unmount removes new
   lookup but active routes keep their cells alive.
7. Up/Cancel releases its route after delivery; lifecycle pause/dispose drains
   every route. Nested realm activation is LIFO and unwind-safe.
8. Locally transformed `&PointerEvent` values are borrowed synchronously; no
   queued path extends the borrow or clones merely for dispatch.
9. Render objects, hit entries, targets, and route tokens stay `Send + Sync`;
   lanes, gesture state, cells, and resolved routes do not.

During singleton retirement the lane owns resolved routes and `GestureBinding`
holds `ResolvedRouteToken`. This is a complete capability contract, not an
executable closure hidden in Send storage.

## Normative pointer semantics

FLUI adopts Flutter parity for ordinary pointer hit dispatch:

- dispatch returns `()`;
- every hit target receives its locally transformed event in leaf-first order;
- pointer delivery has no `EventPropagation::Stop` and never stops bubbling;
- pointer-signal/scroll arbitration remains a separate resolver with its own
  claiming result and is not generalized back into ordinary pointer dispatch.

Removing `EventPropagation::Stop` from `PointerEventHandler`, `HitTestEntry`, and
the ordinary pointer path is an intentional public pre-1.0 break.

Both callers use one resolver/invoker:

1. `GestureBinding`: Down resolves, invokes, caches the route token, then closes
   the arena. Move reuses it. Up/Cancel invokes, sweeps, and releases it.
2. Direct `HitTestResult::dispatch`: resolve an ephemeral route, invoke it, and
   release it. Tests/headless helpers may not carry a second callback loop.

Mouse tracking resolves `MouseRegionTarget` to strong annotations and retains
the previous annotation long enough to emit Exit after it leaves the new result.

## Panic, reentrancy, and destruction

The shared invoker catches unwind per target, records the first panic payload,
and continues later entries, matching Flutter's per-entry isolation. It returns
that payload internally; public pointer dispatch still returns `()`.

The owner boundary performs mandatory cleanup before resuming the first panic:

- Down closes the arena;
- Up/Cancel sweeps and releases the route;
- ephemeral direct dispatch releases its route;
- scope guards restore the prior active realm.

Then `resume_unwind` propagates the first panic under the repository panic
policy. Later panics are traced without replacing it. No callback, old handler,
route, or lane-owned capture is dropped while TLS, registry, route-table, or
`RefCell` borrows are held: take/move values out first, end borrows, then invoke
or drop on the owner thread. A nested platform event remains governed by the
ADR-0027 FIFO; callbacks for the current event are never deferred to Idle.

## Public API consequences

- Replace closure-valued `PointerEventHandler`/`HitTestEntry.handler` with a
  concrete target field.
- Replace `RenderObject::pointer_event_handler` with a target-returning method.
- Change `RenderListener`/`RenderMouseRegion` configuration to targets.
- Add narrow `RenderObjectContext` parameters to `RenderView` create/update.
- Remove ordinary pointer propagation results.
- Remove gesture callback `Send + Sync` bounds in the gesture-local/bound wave.

Do not add a public generic UI-thread executor. Target types are documented
framework-author APIs but absent from the application prelude; lane composition
surfaces may remain `#[doc(hidden)]`.

## Coherent delivery stages

1. Land inert lane/context ownership, IDs, errors, and auto-trait assertions;
   no production dispatch path changes.
2. Atomically migrate the pointer slice: registry, data-only `HitTestEntry`,
   `RenderListener`, shared invoker, and both dispatch callers. Never land a
   data-only entry without a working resolver.
3. Atomically owner-localize `GestureBinding`, arena, recognizers, and callback
   storage. Include the minimum View/ViewState bound ripple needed to compile;
   do not bridge local details through tokens or retain `Arc<Mutex>` temporarily.
4. Atomically migrate `MouseRegion`, tracker annotations, enter/exit/hover, and
   both current/new annotation lifetime behavior.
5. Complete the remaining workspace View/State/callback bound wave, including
   public `Rc<Cell<_>>` authoring proofs.

Every landing is behaviorally complete and green; no intermediate closure path,
broken caller, or data-only overclaim is permitted.

## Verification and performance

- Auto traits for every lane/target/route/render/gesture type.
- Down-before-close, Up-before-sweep, every-target delivery, transforms, and
  parity between direct and cached callers.
- Active route fires after unmount; fresh resolution reports `TargetGone`.
- Rebuild changes the handler observed by an active route.
- `StaleRoute` after slot reuse, route release, and realm recreation; an old
  token never invokes the new route. `OwnerGone` is distinct.
- Self-update/unregister, nested realm restoration, wrong thread/realm, and two
  independent realms/presentations.
- Per-target panic continuation plus close/sweep/release before resumed unwind.
- `DropProbe` coverage for replaced handlers, target unregister, route release,
  lane drop, panic, and realm teardown; every drop occurs owner-thread and
  outside TLS/`RefCell` borrows.
- Long-press/double-tap through real `HeadlessBinding`; Mouse Exit after removal.
- Criterion 1/4/16-target baselines and common Move allocation checks. Resolve
  once on Down; no performance claim before measurement.

## Flutter references

- `gestures/binding.dart`: cached route, every-target synchronous dispatch,
  per-entry exception isolation, and arena close/sweep after dispatch.
- `rendering/proxy_box.dart`: `RenderPointerListener.handleEvent` reads the
  retained target's current callback and returns `void`.
- `gestures/mouse_tracker.dart`: previous annotations survive for Exit diffing.
- `widgets/gesture_detector.dart`: recognizers resolve inside the current pointer
  event/arena transaction.
