# flui-view Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.
[`UNIFIED_ELEMENT.md`](UNIFIED_ELEMENT.md) is the crate's existing element
behaviour taxonomy and remains a sibling appendix.

---

## Mapping decisions

### Same-drain absorption of a mid-drain external schedule (issue #1180)

**Rule:** `BuildOwner::drain_build_scope`'s heap loop absorbs the
out-of-frame external inbox (`ExternalBuildScheduler` /
`RebuildHandle::schedule`) at the top of *every* pop, not only once before
the first one. An id landing in the inbox while its own drain is still
running joins that same `build_scope` call: `Occupied` in `dirty_reasons`
merges the new causes into whatever still-live entry holds the id (on the
heap, or sitting in a deferred layout-builder scope bucket); `Vacant` inserts
it fresh, keyed by the tree's *authoritative* depth (never the depth
captured at `schedule` time), and routes it exactly as any other
newly-dirtied id would be for the current drain target — a Global id landing
during a `LayoutBuilder(scope)` drain defers to the root bucket for the next
Global pass, and the mirror lands a scope-local id in `isolated[scope]`
during a Global drain.

**Conflict:** issue #1180 — a build that calls another element's
`RebuildHandle` synchronously (a `Listenable` notifying a subscriber it
owns, the common `AnimatedBuilder`/`AnimatedView` shape) lands in the
inbox, but the OLD `drain_build_scope` only ever emptied that inbox once,
before its loop started. A schedule arriving mid-loop therefore sat until
the *next* `build_scope` call — a whole extra frame — even though the
notifying build and the notified rebuild belong to the same
user-observable action. Concretely: a `Duration::ZERO` implicit-animation
retarget snaps the controller's value synchronously in `did_update_view`,
and the dependent `AnimatedBuilder`'s listener fires in that same instant —
but used to need a second pump to actually rebuild, because its schedule
missed the retargeting build's own drain by one absorb call. This is also
Flutter's contract, precisely at 3.44.0: `BuildScope._dirtyElementIndexAfter`
re-sorts `_dirtyElements` only when a mid-flush `_scheduleBuildFor` set
`_dirtyElementsNeedsResorting` (not on every iteration unconditionally),
and a `markNeedsBuild` mid-build is absorbed by `Element`'s own
`if (dirty) return` guard — FLUI's inbox-based external scheduling had no
equivalent per-iteration re-entry point.

**Choice:** absorb per pop, bounded by a per-*frame* budget
(`BuildOwner::mid_drain_absorbs_left`, reset to `MAX_MID_DRAIN_ABSORBS = 16`
at every `build_scope` entry). The budget is genuinely per FRAME, not per
`build_scope` CALL: `build_scope` factors into the reset plus
`build_scope_impl`, and every mid-frame re-entrant caller — the
layout-builder fixpoint's own `drain_prepared_build_target` calls, and
`service_child_requests_impl`'s lazy-sliver "second build_scope" pass —
reaches `build_scope_impl` directly instead of `build_scope`, so none of
them re-runs the reset. The budget charges only a RE-ENTRY — a `Vacant`
absorb for an id already recorded in `BuildOwner::built_this_frame` (it
already completed a build in this `build_scope` call, so this landing is
some element being notified again after that build — the common case is
the SAME element rescheduling itself, but a child notifying its
already-built parent, or an A↔B ping-pong, charges identically) — never a
first-time absorb, however many independent elements get notified in one
frame: each id can be a first-time absorb at most once, so that case is
inherently finite (a page whose N unrelated parents each retarget an
implicit animation in one frame performs N legitimate first-time absorbs,
none of them charged). On exhaustion, the id is left in the inbox for the
next `build_scope` (the existing `has_dirty_elements` gate already
schedules that frame) and a `tracing::warn!` fires once per streak,
re-arming only after a frame that ends with budget left — there is no
frame-complete hook on `BuildOwner`, so the re-arm check runs retroactively
at the *next* `build_scope` entry.

**Divergence 1 (no descendant-only debug assert):** Flutter's
`Element.markNeedsBuild` debug-asserts, from inside `buildScope`, that a
mid-build `markNeedsBuild` names a descendant of the element currently
building (`_debugCurrentBuildTarget`). FLUI has no equivalent assert here.
The inbox is FLUI's only route for a listener-driven or cross-thread
rebuild — Flutter routes the identical shape through `setState`, which is
always issued by the element's own `State` object and therefore always
structurally a self-notification. A `RebuildHandle::schedule` call carries
no such structural guarantee (`RebuildHandle` is `Send + Sync`, callable
from a worker thread or from an unrelated element's build with no
relationship to whichever element the drain happens to be building), so
there is no cheap invariant to assert here; recording the gap is the honest
choice over a debug assert that would either never fire (too weak to catch
anything) or reject a legitimate cross-subtree listener (too strong).

**Divergence 2 (a re-entered element keeps rebuilding; Flutter drops it):**
Flutter's `if (dirty) return` in `markNeedsBuild` silently drops a
self-`setState` issued *during* the element's own build — the dirty flag is
already set, so the second call is a no-op, and the element builds once for
both causes combined. FLUI cannot tell "during my own build" from "after it,
before the next pop" apart from a synchronous `schedule` call alone: by the
time the drain gets back around to absorbing the inbox, the building
element's build has already returned and its `dirty_reasons` entry has
already been removed (ordinary post-build cleanup, not something this
change added), so a re-entry — a `Vacant` landing for an id that already
completed a build in this `build_scope` call — looks identical whether it
is that same element rescheduling itself, a child notifying its
already-built parent, or one half of an A↔B ping-pong. FLUI therefore
rebuilds a re-entered element once per re-entry, up to
`MAX_MID_DRAIN_ABSORBS` — Flutter's tighter contract would need
`RebuildHandle`'s inbox entry to also record "was this scheduled during a
build the current drain has not yet reconciled," which is out of scope for
this change.

The re-entries are visible in `BuildOwner::last_frame_build_report`:
`elements_built` counts distinct elements (the size of `built_this_frame`),
so a re-entered element counts once there, while `builds_run` counts every
completed build, so it counts once per build. The two differ exactly by the
frame's re-entries (pinned by
`a_re_entered_element_counts_once_in_elements_built_and_twice_in_builds_run`),
which is why the perf baseline records both.

**Divergence 3 (`on_build_scheduled` fires mid-drain; Flutter latches its
frame request through TWO nested guards, one at each level FLUI's
`schedule` conflates):** at 3.44.0, `BuildOwner.scheduleBuildFor` guards its
own frame-request callback with `if (!_scheduledFlushDirtyElements &&
onBuildScheduled != null)` (`framework.dart`), then calls into
`BuildScope._scheduleBuildFor`, which separately guards ITS OWN per-scope
`scheduleRebuild?.call()` with `if (!_buildScheduled && !_building)`. Every
Flutter schedule — `setState`, a `Listenable` firing, a `BuildOwner`-level
reassemble — passes through BOTH guards uniformly, since there is only one
`scheduleBuildFor` entry point. FLUI's `ExternalBuildScheduler::schedule`
has no equivalent at either level: it fires `on_build_scheduled` on every
newly-queued id regardless of whether a drain is already running (pinned by
`mid_drain_schedule_still_requests_a_frame_like_an_out_of_frame_schedule`).
Recorded as the deliberate alternative rather than built: the redundant
frame request this can cause is discarded downstream by the ordinary
dirty-state gate a wake-with-nothing-new-to-do already hits, so adding the
latch(es) would trade a real per-callsite invariant (every fresh inbox
entry asks for a frame) for a saving with no measured cost — take it up
only if a wake-count oracle ever shows the cost is real.

### Flutter: parent-inserts-child → FLUI: child-adopts-itself

**Rule:** a render child enters the render tree by adopting ITSELF at mount
time. FLUI has no element-side child-mutation seam — the port of Flutter's
`RenderObjectElement` seam (`insertRenderObjectChild` /
`moveRenderObjectChild` / `removeRenderObjectChild` /
`attachRenderObject` / `detachRenderObject`, `framework.dart`, pinned tag
3.44.0) was deleted as dead code: it had zero production callers across the
workspace, and every Flutter consumer family of that seam has a live FLUI
equivalent reached by a different direction. The audit table behind this
decision is recorded in issue #1203; it mapped all ten Flutter consumer
families, including the hard cases (multi-child reorder, GlobalKey
reparent, parent-data attach).

**Flutter's model:** the PARENT acts. `attachRenderObject` walks up to the
nearest `RenderObjectElement` ancestor, which then calls
`insertRenderObjectChild(child, slot)` to slot the child into its own
render object; `move`/`remove` go through the same parent-driven surface;
the root overrides `attachRenderObject` to set
`pipelineOwner.rootNode` instead (`RenderTreeRootElement`).

**FLUI's live equivalents, family by family:**

- *Adopt/insert* — the freshly mounted element adopts itself:
  `RenderBehavior::on_mount` reads the `parent_render_id` propagated
  BEFORE mount (`ElementTree::insert` reads the parent's
  `child_render_id()` off the node and hands it to the child via
  `set_parent_render_id` before `mount` runs; the pass-through that
  forwards the nearest render ancestor through component elements is
  `ElementBase::child_render_id` / `ElementCore::child_parent_render_id`,
  and the root passes its own render id for its child), then calls
  `PipelineOwner::adopt_render_child`, which writes both link directions
  in one call. The sliver-slot half of Flutter's `didAdoptChild` rides the
  same propagation and is stamped at adoption time.
- *Remove* — `RenderBehavior::on_unmount` → `remove_render_object_from_tree`
  (the dispose cascade), with keyed soft-remove relocation tokens for
  children that are merely leaving view rather than dying.
- *Move/reorder* — no per-child mutation at all: a post-build batch pass,
  `ElementTree::reorder_render_children_after_build`, settles render
  children into slot order after a build; its own doc calls it "the arena
  analogue of Flutter slotting each child via `insertRenderObjectChild`".
- *Attach/detach* — pipeline-owner wiring at mount/unmount (`on_mount` /
  `on_unmount`) and, for GlobalKey reparent, the render-relocation tokens
  (`PipelineOwner::detach_render_subtrees` / `attach_render_subtrees`,
  carried through the inactive-element record).

The slab-resident architecture superseded the old box-graph propagation
this trait ported (the `element_tree.rs` "E3 atomic box→arena swap"
comment records that supersession).

**Replacement guarantee:** the loud half-state gate on the LIVE adoption
path — `RenderBehavior::on_mount`'s diagnostic when an element-tree parent
with an active `PipelineOwner` leaves the chain with no render ancestor,
plus its `orphaned_render_mount` test family
(`crates/flui-view/tests/orphaned_render_mount.rs` and
`crates/flui-view/src/tree/element_tree/orphaned_render_mount_tests.rs`).
That gate and its tests arrived with the #1198 fix and are untouched here;
the else-arm diagnostics the same fix added to the six (now deleted) seam
methods vanish with them, as intended.

**Reference-tag caveat:** the 3.44.0 pin for the Flutter citations above
was verified via the `.flutter` clone's `.git` refs as part of the #1203
audit (the clone is a local gitignored checkout, so the tag's version file
is gitignored/absent and the tag is read from the clone's refs); a fresh
clone must re-run `git describe --tags` inside `.flutter` before citing
further.


### Lifecycle capability types are nameable through the facade

`BuildContext` returns async and post-frame handles whose canonical public paths
are also exported by `flui-view`, and therefore by `flui::view`. This includes
`TaskToken`, `BoxedTask`, local scheduling errors, and the timing value types
used by post-frame callbacks, including the cross-platform `Instant`. Widget
authors can store explicitly typed capabilities acquired in `init_state` without
adding scheduler dependencies. These are reexports of the existing types, not
new wrappers or duplicated scheduler state: task-token cancellation and weak
post-frame ownership keep their existing contracts.

The external facade-extension fixture acquires the handles from a live binding,
polls a task to completion, cancels a pending task, and executes local and shared
post-frame callbacks using named timing types. It runs with both `flui` and a
renamed dependency. Scheduler construction and local lane implementation remain
outside this view-level capability vocabulary.

The facade's `flui::interaction` likewise names the focus, hit-test, and text-input
capabilities returned by `BuildContext`, together with their callback/result and
keyboard value vocabulary. The external fixture retains a focus node, mounts
`Focus`, and observes focus and unfocus notifications. Its headless IME check
only verifies typed capability access with no native owner; it does not claim
platform IME behavior. Runtime owners and adapter constructors are not exported.

### Build contexts are live during build; there is no detached fallback context

**Rule.** During a `BuildOwner::build_scope` drain, the element being built is taken out of its
slot and built against a borrowed, read-only view of the real tree (`BuildCtx`), then put back.
Node fields that survive the take (parent, depth, inherited scope, children) stay readable; only
the element itself is a hole. A component build outside a drain is a framework bug and panics
with a `BUG:` message — there is no inert "minimal" context to fall back to.

**Why.** An earlier shape built every context over a shared empty dummy tree, so
`depend_on`/`find_ancestor_*` silently returned nothing in production. By-value extraction gives
a live tree without re-locking the tree the drain already holds.

**Divergence.** Flutter's `BuildContext` *is* the element and can be stashed and used after the
element is defunct (caught only by a debug assert). FLUI's context is only reachable inside
build/lifecycle calls. Capability acquisition is split out by type (ADR-0078:
`LifecycleContext` in `init_state`/`did_change_dependencies`, `BuildContext` in `build`).

### Inherited reads are O(1) and field-precise; reading is depending

**Rule.** Each element node carries its resolved inherited scope — an `Arc<HashMap>` built at
mount, shared by refcount down runs of non-providers, re-inserted by each provider so nested
providers of the same type shadow nearest-first. `find_inherited_provider` is one map lookup.
`#[derive(InheritedData)]` generates `FieldMask` constants per field; `InheritedView::changed_fields`
diffs old and new data per field; `depend_on_field(mask, …)` records a masked dependency, and a
provider update rebuilds only dependents whose mask intersects the change. The whole-type read
is the all-bits mask; an empty mask is promoted to a whole-provider dependency rather than
silently opting out. There is no read path that does not record a dependency.

**Divergence.** Flutter's `InheritedModel` aspects are untyped objects and reading without
depending (`getInheritedWidgetOfExactType`) is allowed; FLUI's aspects are compile-time field
masks and every public read depends. There is no blanket `Data: PartialEq` bound — the diff
comes from the opt-in derive. The dependent registry is the same reader registry signals use
(ADR-0074 §5.5).

### Signal reads subscribe through a private sink

Signals have no Flutter counterpart, so this is a local invariant rather than a divergence.

**Rule.** The reactive graph (`reactive/mod.rs`, one `Reactive` per `BuildOwner`) implements
`flui_foundation::read_scope::ReadGraph` — pure reads, enough for `Signal::peek` — and never
`ReaderSink`. `BuildContext` has `ReadScope` as a supertrait, so `sig.get(cx)` resolves through
the context's scope. The scope pairs the graph with `ElementReads`, a crate-private sink bound
to one element: `make_build_ctx` mints it for the element about to build (`BuildCtx` always
subscribes), and `ElementBuildContext` captures one for its own element and subscribes only while
marked as building. `begin_element_build`/`end_element_build` bracket every build in
`build_or_recover` and `release_element` runs on every unmount, in every build: there is no
`signals` feature. Writes are the sealed `SignalWriteExt` trait over a sealed `WriteTarget`
(next section). The routing test is
`tests/signal_reads.rs::a_read_in_build_subscribes_through_the_production_context`; the
`Reactive`/`ElementReads` sink pair is pinned by `static_assertions` in the module's tests.

### Writes open through a WriterSource

Signals have no Flutter counterpart; this is a local invariant (ADR-0086).

**Rule.** `SignalWriteExt::set`/`update`/`set_if_changed` take `&W` where `W: WriteTarget`, a
sealed trait whose graph accessor takes a token only this crate can make
(`reactive/writer.rs`). Application code meets one target: the `&mut EventCx<'_>` an event
callback receives, which derefs to a `Writer`. `Writer` has no public constructor and is neither
`Clone` nor `Send`; an `EventCx` exists only inside `WriterSource::write`. Stateful widgets acquire
their `WriterSource` through `LifecycleContext::writer_source`; render views acquire it through
`RenderObjectContext::writer_source` while registering owner-local interaction handlers.
Neither capability is exposed by `build`'s `&dyn BuildContext`. A detached render context
returns `None`, rather than manufacturing a graph unrelated to a presentation.
The contexts hand out a source over the graph their element reads through
(`ElementReads::graph`), so a source writes into its own presentation's graph and refuses another
graph's handles with `ForeignGraph`. The run-time guard is unchanged and stays authoritative: a
write a widget opens inside its own `build` is refused with `WrittenDuringBuild`. `Reactive` is
still a `WriteTarget` so tests, the `signals_rebuilds` bench and `UiCommand::SignalWrite` keep
compiling; ADR-0086 §8 step 3 removes it with both `reactive()` accessors. An event callback may
return `()` or a write's `Result`; `EventOutcome::report` logs a refused write on
`flui::signals` rather than dropping it silently. A `Signal::default()` handle names graph 0,
which `Reactive::new` never mints, and is refused with `Unbound`.

The borrowed `Writer` and `EventCx` also implement pure `ReadGraph`: an event's
`peek` observes current values without creating a build subscription. Owner-bound
non-signal mutations use `WriterSource::check_context` before changing state;
it refuses foreign presentation contexts and a build in progress. It cannot prove
the target widget is mounted: the target clears its stored source when detached.
This preflight is not rollback of effects a user callback has already performed.

Pinned by the `reactive/writer.rs` unit tests and `static_assertions`,
`tests/writer_source.rs` (the production context's source rebuilds the reader, the test
context's writes the owner's graph, a write opened in `build` is refused), the
`compile_fail` doctests on `WriterSource` and `LifecycleContext::writer_source`, and the
`tests/ui/signal_write_*`, `unit_closure_*`, `let_bound_*` and `writer_escapes_*` snapshots.

### Signal mutation is commit-on-unwind, not transactional

Signals have no Flutter counterpart; this is the unwind half of ADR-0074's
write-to-dirty contract.

**Rule.** An `update` closure that mutates its value and then panics leaves the
partial value committed while the slot remains live. Before the original panic resumes, every registered
reader is inserted into the external rebuild inbox as one durable batch. The
batch releases its lock before requesting one frame; a panicking wake therefore
cannot expose only a prefix of the reader set. Signal telemetry runs only after
that enqueue. A failed wake leaves debt on the shared inbox; the next hooked
scheduler call retries it even when every id is already queued. Concurrent
callers never wait behind the external hook: they may race delivery, and a
successful hook acknowledges only the identity token captured before that
hook began. Fresh work replaces the token, so an older success cannot erase a
newer failure and there is no finite counter to exhaust on 32-bit targets. A
reentrant schedule cannot call the hook recursively and receives
one compensating attempt from its outer call. The same debt also covers direct
`BuildOwner`/`ElementOwner` scheduling. If invalidation or
loan finalization panics while an updater panic is already being handled, the
updater's original payload keeps priority.

`set` commits its replacement without running either value's destructor, then
returns the loan and invalidates readers before retiring the old value. A
panicking old-value destructor therefore observes an already-readable
replacement and cannot prevent its readers from being scheduled.
`set_if_changed` likewise keeps its proposed value outside the equality
comparison's unwind boundary, so a panicking `PartialEq` remains the primary
failure and commits nothing even when the proposed value's destructor panics.

The updater is `FnMut`, although the graph calls it exactly once. Keeping the
  closure owned outside the caught invocation lets the graph retain its opaque
  capture bundle when the updater panics; consuming an `FnOnce` would instead run
  capture destructors during the updater's unwind, where a second panic aborts the
  process before the graph can finalize the loan or invalidate readers. The same
  ownership boundary covers pre-invocation preparation: a panicking refusal
  telemetry subscriber retains the still-uninvoked updater. This is an
  exceptional-path leak: aggregate closure drop glue cannot be decomposed or made
  safe by an outer `catch_unwind`; successful callbacks still destroy captures,
  but only after loan restoration and reader invalidation are durable.

A valid typed read releases the graph's value loan, then subscribes before a
panic from its user closure resumes. A recovered first build that panics in
`Signal::with` therefore retains the dependency needed for a later write to
retry it without requiring a reentrant `ReadGraph`. Foreign, stale, unbound and
type-mismatched reads still subscribe nobody. Before any caught panic resumes,
  every still-owned opaque callback, generic result and cleanup payload is
  deliberately retained, so generated aggregate drop glue cannot replace the
  chronologically first panic or turn recovery into a double-panic abort.
Typed reader closures follow the same `FnMut`-called-once rule so their captures
remain available to that cleanup boundary.

**Not promised.** `update(&mut T)` is not a transaction and cannot roll back an
arbitrary `T` or external effects. Code requiring atomic domain changes prepares
and validates a replacement value before `set`; a future transactional primitive
needs its own consumer and contract. Notification stays deferred through the
existing rebuild inbox and never invokes signal readers inline. An explicit
`Reactive::release` of that same slot from inside its closure remains
authoritative: it destroys the loaned value and reader set, so no commit
survives to invalidate.

Rust also provides no generic way to recover from aggregate drop glue when two
fields both panic: the second panic occurs while the first is unwinding and the
process aborts before an outer `catch_unwind` can observe either payload. FLUI
therefore completes its own loan/invalidation/wake protocol before destroying a
successful callback or retired value, and retains opaque values once another
panic already has priority. It does not promise process recovery from two
simultaneously panicking destructors inside one user-owned aggregate.

Pinned by the reactive graph unit tests for replacement/equality/destructor and
updater/wake/telemetry panics,
`flui-foundation`'s subscribe-before-unwind test, and
`tests/signal_reads.rs` for mounted partial-commit and first-build recovery.

### Reconciliation emits typed events on the live path

**Rule.** `reconcile_children_by_id` emits one `ReconcileEvent` per child disposition (`Mount`,
`Reuse`, `Reorder`, `Unmount`, `Reparent`) through the `flui::reconcile` tracing target, so the
reconciliation stream observed by tools is the production one, not a test-only reconciler's.

**Divergence.** Flutter exposes rebuild tracking only in debug mode through the devtools
protocol; FLUI's stream is typed and zero-cost when nothing subscribes.

### A GlobalKey read inside its own presentation's frame resolves to nothing

**Rule.** `GlobalKey::current_element` and `with_current_state` return `None` when called
from inside the frame of the binding that hosts the key: from `build`, a lifecycle hook,
`dispose`, or a layout-builder build, all of which run while `WidgetsBinding` holds its own state
lock. The registry closures take that lock with a non-blocking recursive read and report
`RegistryBusy` when it is held; the realm composite skips a busy member and keeps trying the
others, so keys held by other presentations of the realm resolve normally. The binding is
`!Send` (pinned by a static assertion), so a held lock can only mean re-entry on the owner
thread. Before this rule such a read blocked on its own thread forever. The skip is logged at
`debug`, not `warn`: the running presentation is busy for every read in its frame, so a key
mounted nowhere reports the same skip, and a warning there would fire every frame
(`unmounted_global_key_read_during_a_frame_does_not_warn`).

Closing a presentation uses the same composite as every other realm entry, the closing
presentation included. Its keys resolve until its tree teardown takes the binding lock (a
lifecycle observer told the presentation is detaching sees them), and resolve to nothing during
the teardown, where `dispose` runs (`closing_presentations_own_key_resolves_while_it_detaches`
in the realm tests; `dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter` in
`flui-app`'s dispatch tests).

**Divergence.** Flutter's `GlobalKey.currentElement`/`currentState` (`framework.dart:3163-3170`)
return the element during build. FLUI returns nothing for keys of the presentation whose frame is
running. The exit is to serve those reads from the frame's own tree once the realm owns the
binding by value (ADR-0083). Pinned by
`global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking`,
`global_key_in_a_sibling_binding_resolves_during_this_bindings_frame` and
`global_key_lookup_from_dispose_during_detach_returns_instead_of_deadlocking` (`binding.rs`), and
through the realm by `ui_realm/tests/global_key_lookup_during_frame.rs` in `flui-runtime`.

### Not adopted

A branded `Cx<'build>` token (its role is taken by the `BuildContext`/`LifecycleContext` split,
ADR-0078); a `Mounted<'_>` re-entry token with RAII effect scopes (async and listener re-entry go
through `RebuildHandle` and the realm inbox, ADR-0027 §3; derived state and effects are
ADR-0075's subject); shrinking `ElementBase` into a capability-typed `Element<V, P>`.
