# flui-view Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.
[`UNIFIED_ELEMENT.md`](UNIFIED_ELEMENT.md) is the crate's existing element
behaviour taxonomy and remains a sibling appendix.

---

## Mapping decisions

### Clean widget frames report no builds

The binding's draw-frame entry clears build telemetry even when no build work is pending.
Dirty frames reset the same report through `BuildOwner::build_scope`; lazy child service
adds its builds to that frame's report. A realm pump producing no draw frame retains the
most recent actual frame report. Clearing telemetry does not route build work or change
the scheduler's drain budget. Pinned by
`tests/build_owner_tests.rs::clean_binding_frames_report_no_builds`.

### Owner and key envelopes retire after authority is withdrawn

**Rule:** the build owner's reactive graph, tree observer and scheduled-build
callback are independent ownership envelopes, and so are the boxed keys held by
the registry, scope, reservation, displacement and verification maps. Healthy
destruction keeps field and container order. Once a destructor failure has
propagated, or while the thread is already unwinding, the remaining independent
envelopes are retained instead of dropped
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)).

**Rule:** key authority is withdrawn before the key is retired. Registration
prepares the local key before claiming scope authority; scope hashing, equality
and cloning run outside the scope borrow. A comparison pins the complete hash
bucket (owner tags and allocation markers) and revalidates it afterwards; one
fresh retry is allowed, and a second mismatch is refused as an unstable
comparison ([ADR-0126](../../docs/adr/ADR-0126-reentrant-scoped-key-comparisons.md)).
A failed local insertion rolls back by cached hash, owner and allocation marker
without calling key hashing or equality. Release removes local and scoped
authority before either key is dropped, so a key destructor may inspect the
scope or claim the key again. A scope snapshot may defer physical key
destruction until its pins retire; logical authority is already gone by then.

**Limits:** competing destructors inside one opaque graph or user value are not
contained, and local registry callbacks and other raw build-owner fields are
outside this boundary.

**Pinned by:** the `owner_key_retirement` and `owner_key_lookup_reentry` rows of
`lifecycle_panic_containment_matrix`, `rollback_invokes_no_key_callbacks`,
`scope_clone_read_reentry`, `scope_clone_competing_owner_reentry`,
`local_clone_competing_owner_reentry` and
`key_collision_same_owner_and_stale_release`.

### Exhausted owner identities refuse admission permanently

An owner tag is claim authority, so its allocator never wraps or reissues a
retired tag. The final nonzero tag is admitted once; after that, owner
construction panics permanently. Existing scoped claims stay usable, and a
stale release cannot withdraw a later owner's claim.

### Binding observers and returned futures retire independently

**Rule:** observer registries and each notification snapshot own separately
guarded observer envelopes. Healthy destruction keeps container order; after
the first propagating destructor failure, or during an incoming unwind, the
remaining envelopes are retained (ADR-0127). Snapshot callbacks run after the
binding guard is released. A registry change affects the next notification;
a running dispatch keeps its snapshot, and a callback panic stops that
notification. A legacy lifecycle failure precedes the scoped lifecycle drain,
and its payload stays the first failure.

**Rule:** suspended pop, push and application-exit notifications guard the
returned response future, the current observer and the remaining iterator
separately, so cancelling a healthy future retires it normally while a later
independent destructor never competes with an existing failure. Pop and push
stop at the first handled response; exit consults every observer.

**Limits:** competing destructors inside one opaque observer or future
aggregate are not contained, and whole-binding destruction is not covered.
Predictive-back list clearing and replacement still retire entries under the
write guard.

**Pinned by:** the `binding_observer_ownership_and_notifications` rows of
`lifecycle_panic_containment_matrix`.

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
missed the retargeting build's own drain by one absorb call. The inbox-based
external scheduling had no per-iteration re-entry point.

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

**Decision 1 (no descendant-only debug assert):** a mid-build schedule is
not asserted to name a descendant of the element currently building.
The inbox is the only route for a listener-driven or cross-thread
rebuild, and a `RebuildHandle::schedule` call carries no structural
guarantee that it targets the building element's subtree (`RebuildHandle` is
`Send + Sync`, callable from a worker thread or from an unrelated element's
build with no relationship to whichever element the drain happens to be
building), so there is no cheap invariant to assert; an assert would either
never fire (too weak to catch anything) or reject a legitimate
cross-subtree listener (too strong).

**Decision 2 (a re-entered element keeps rebuilding):**
FLUI cannot tell "during my own build" from "after it,
before the next pop" apart from a synchronous `schedule` call alone: by the
time the drain gets back around to absorbing the inbox, the building
element's build has already returned and its `dirty_reasons` entry has
already been removed (ordinary post-build cleanup, not something this
change added), so a re-entry — a `Vacant` landing for an id that already
completed a build in this `build_scope` call — looks identical whether it
is that same element rescheduling itself, a child notifying its
already-built parent, or one half of an A↔B ping-pong. FLUI therefore
rebuilds a re-entered element once per re-entry, up to
`MAX_MID_DRAIN_ABSORBS`. Coalescing them would need
`RebuildHandle`'s inbox entry to also record "was this scheduled during a
build the current drain has not yet reconciled," which is out of scope.

The re-entries are visible in `BuildOwner::last_frame_build_report`:
`elements_built` counts distinct elements (the size of `built_this_frame`),
so a re-entered element counts once there, while `builds_run` counts every
completed build, so it counts once per build. The two differ exactly by the
frame's re-entries, which is why the perf baseline records both.
**Unasserted:** no test pins this.

**Decision 3 (`on_build_scheduled` fires mid-drain):**
`ExternalBuildScheduler::schedule` fires `on_build_scheduled` on every
newly-queued id regardless of whether a drain is already running.
**Unasserted:** no test pins this. Latching the frame request while a drain runs is the
alternative not built: the redundant frame request this can cause is discarded downstream
by the ordinary dirty-state gate a wake-with-nothing-new-to-do already hits, so adding the
latch(es) would trade a real per-callsite invariant (every fresh inbox
entry asks for a frame) for a saving with no measured cost — take it up
only if a wake-count oracle ever shows the cost is real.

### Render children adopt themselves at mount

**Rule:** a render child enters the render tree by adopting ITSELF at mount
time. There is no element-side child-mutation seam (insert / move / remove /
attach / detach of a child render object on the parent element): a seam
of that shape had zero production callers across the workspace and was
deleted. The audit table behind this decision is recorded in issue #1203; it
mapped every consumer family of such a seam, including the hard cases
(multi-child reorder, GlobalKey reparent, parent-data attach).

**The live paths, family by family:**

- *Adopt/insert* — the freshly mounted element adopts itself:
  `RenderBehavior::on_mount` reads the `parent_render_id` propagated
  BEFORE mount (`ElementTree::insert` reads the parent's
  `child_render_id()` off the node and hands it to the child via
  `set_parent_render_id` before `mount` runs; the pass-through that
  forwards the nearest render ancestor through component elements is
  `ElementBase::child_render_id` / `ElementCore::child_parent_render_id`,
  and the root passes its own render id for its child), then calls
  `PipelineOwner::adopt_render_child`, which writes both link directions
  in one call. The sliver-slot half of adoption rides the
  same propagation and is stamped at adoption time.
- *Remove* — `RenderBehavior::on_unmount` → `remove_render_object_from_tree`
  (the dispose cascade), with keyed soft-remove relocation tokens for
  children that are merely leaving view rather than dying.
- *Move/reorder* — no per-child mutation at all: a post-build batch pass,
  `ElementTree::reorder_render_children_after_build`, settles render
  children into slot order after a build.
- *Attach/detach* — pipeline-owner wiring at mount/unmount (`on_mount` /
  `on_unmount`) and, for GlobalKey reparent, the render-relocation tokens
  (`PipelineOwner::detach_render_subtrees` / `attach_render_subtrees`,
  carried through the inactive-element record).

**Replacement guarantee:** the loud half-state gate on the LIVE adoption
path — `RenderBehavior::on_mount`'s diagnostic when an element-tree parent
with an active `PipelineOwner` leaves the chain with no render ancestor,
plus its debug-build refusal test
(`crates/flui-view/tests/orphaned_render_mount.rs`).
That gate and its tests arrived with the #1198 fix and are untouched here;
the else-arm diagnostics the same fix added to the six (now deleted) seam
methods vanish with them, as intended.


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

**Scope.** The context is only reachable inside build/lifecycle calls, so it cannot be
stashed and used after its element is defunct. Capability acquisition is split out by type (ADR-0078:
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

**Typing.** Aspects are compile-time field masks and every public read depends. There is no blanket `Data: PartialEq` bound — the diff
comes from the opt-in derive. The dependent registry is the same reader registry signals use
(ADR-0074 §5.5).

### Signal reads subscribe through a private sink

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

A local invariant (ADR-0086).

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

The unwind half of ADR-0074's write-to-dirty contract.

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
authoritative: the value and reader set are no longer live, so no commit
survives to invalidate. Loan finalization returns a still-live value before any
retirement. When a callback has already failed, a released loan's opaque value
is retained instead of destroyed: aggregate drop glue can abort before an outer
catch observes a secondary failure, and nested release obligations must not run
while that first failure has priority. Successful callbacks still retire released
values normally, outside the graph borrow. Restoration keeps the value owned by
the loan until the slot check completes; a restoration failure likewise retains
that value before resuming its payload.

Explicit release and element-owned release commit the slot's terminal state,
reader removal and reusable index before retiring its value outside the graph
borrow. Element teardown commits every owned slot before running the first
destructor. A destructor can reenter the graph and allocate a replacement; a
stale release cannot remove that replacement. The departing element itself admits
no new owned slot while its release runs: a destructor that creates one is refused
(`signal_owned_by` panics holding the refused value, so the unwind retains it;
`try_signal_owned_by` drops it after the graph borrow ends). A refusal made while
another refused value's destructor runs retains its value instead of dropping it,
so a value whose destructor recreates itself through the fallible call runs that
destructor once rather than recursing until the stack overflows. Teardown is one
pass even for a value that recreates itself from its destructor
(`owner_release_refuses_signals_its_destructors_reintroduce`). Once a retirement fails, remaining
opaque values are retained and the first payload resumes. Release during active
unwind likewise retains its opaque value. The public
`explicit_release_allows_destructor_reentry_and_slot_reuse` and
`owner_release_commits_the_batch_before_the_first_destructor_failure` rows pin
these graph invariants through explicit release and production element removal;
they do not promise recovery of the surrounding element-tree teardown.
`release_during_unwind_preserves_the_primary_failure` exercises active-unwind
retention in a subprocess, since its negative control double-panics.

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
Its `released_update_retains_aggregate_before_resuming_failure`,
`released_read_retains_aggregate_before_resuming_failure`,
`released_update_retains_nested_release_obligations` and
`released_read_retains_nested_release_obligations` rows join
`signal_read_and_write_matrix` and run in child processes. They pin the primary
payload, released-slot behavior, retained destructors and the next live update.
The companion `released_update_reports_ordinary_retirement_failure` and
`released_read_reports_ordinary_retirement_failure` rows preserve normal released
value retirement: its first destructor failure propagates while callback captures
are retained. The subprocess boundary makes an old-code aggregate abort a
table-row failure.

### Reconciliation emits typed events on the live path

**Rule.** `reconcile_children_by_id` emits one `ReconcileEvent` per child disposition (`Mount`,
`Reuse`, `Reorder`, `Unmount`, `Reparent`) through the `flui::reconcile` tracing target, so the
reconciliation stream observed by tools is the production one, not a test-only reconciler's.

The stream is typed and zero-cost when nothing subscribes.

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
mounted nowhere reports the same skip, and a warning there would fire every frame.
**Unasserted:** no test pins this.

Closing a presentation uses the same composite as every other realm entry, the closing
presentation included. Its keys resolve until its tree teardown takes the binding lock (a
lifecycle observer told the presentation is detaching sees them), and resolve to nothing during
the teardown, where `dispose` runs. **Unasserted:** no test pins this.

**Limitation.** A read returns nothing for keys of the presentation whose frame is
running. The exit is to serve those reads from the frame's own tree once the realm owns the
binding by value (ADR-0083). Pinned by
`global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking` (`binding.rs`),
and through the realm by `state_read_across_presentations_during_a_segment_resolves`
(`ui_realm/tests/global_key_lookup_during_frame.rs` in `flui-runtime`), which reads a key held
by another presentation while the reader's own frame lock is held. For a read from `dispose`
during detach: **Unasserted:** no test pins this.

### The development-reload hook lives here, not in the runtime

`dev_reload::DevReloadHook` (ADR-0094 §1) is the only seam between a host and a reload tool.
It sits in this crate, below the runtime, so the host (`flui-app`) and the tool
(`flui-hot-reload`) each name it without naming each other, and a package reaches it through
`flui-sdk`'s `view` glob re-export with no new SDK item. The trait is the driver half — `attach`, `detach`,
`poll` and `scene_frame`; the per-call seam a code patcher needs arrives with its first
producer. Its bound is `Send + 'static` because the instance travels in the application's
configuration; it is only ever called on the owner thread. `scene_frame` lends the scene to a
callback so a scene built by a plugin image cannot outlive it. Pinned by
`scene_frame_default_never_calls_render` and by the host's tests in `flui-app`
(`app/hot_reload/tests.rs`).

### The development-agent hook lives here too

`dev_agent::DevAgentHook` (ADR-0095 §3) is the seam between a host and a tool that serves the
application's semantics tree to an agent (`flui-devtools`' `agent` server). It sits here for the
reason `DevReloadHook` does: the host and the tool each name it without naming each other, and a
package reaches it through `flui-sdk`'s `view` glob. Its calls speak `flui-protocol`'s schema,
which is why this crate depends on that contract crate. An `AgentWindow` is built only through
the hidden `__runtime::agent_window`, over a `Weak<dyn __runtime::AgentPort>` the runtime
implements, so a package can hold and use one but cannot forge one, and a handle never keeps a
closed window alive. Flutter has no counterpart. Pinned by
`a_window_answers_through_its_port_until_the_port_is_gone` and the runtime's
`dev_agent_host_contains_its_hook`.

### The composition-root seam is a hidden module, not a feature

What the realm-owning crates (`flui-runtime`, `flui-app`, `flui-testing`, `flui-hot-reload`)
need from a binding lives in `#[doc(hidden)] pub mod __runtime` (ADR-0081 §4): the
`GlobalKey` registry activation, the frame-phase stamp at the build-to-finalize boundary
(`FramePhaseMarker`), the multi-presentation `GlobalKeyRegistryComposite`, and the terminal
lifecycle ladder (`LifecycleSource`). The module is always
compiled, so no build configuration changes what `WidgetsBinding` holds, and it has no semver
promise. The methods it adds to `WidgetsBinding` are on the sealed `BindingRuntime` trait
rather than inherent, so they resolve only where the trait is imported and are not part of the
binding's surface as `flui::view` and `flui_sdk::view` expose it. Those two re-export this
crate as a glob module with a private `mod __runtime {}` that shadows the glob's, so the seam
is not reachable through them; a `compile_fail,E0603` doctest on each pins it. Pinned from
outside the crate by `tests/runtime_seam.rs`.

### Not adopted

A branded `Cx<'build>` token (its role is taken by the `BuildContext`/`LifecycleContext` split,
ADR-0078); a `Mounted<'_>` re-entry token with RAII effect scopes (async and listener re-entry go
through `RebuildHandle` and the realm inbox, ADR-0027 §3; derived state and effects are
ADR-0075's subject); shrinking `ElementBase` into a capability-typed `Element<V, P>`.

### A scene-plugin rendering callback has an explicit unsafe lifetime contract

`DevReloadHook::scene_frame` is unsafe: a scene borrow allows cloning layers
and shared annotations whose code belongs to a plugin image. Its caller must
retain none of those image-dependent payloads after the callback boundary,
including an unwinding call. The hook may then unload on the next frame or
on drop. A `compile_fail,E0133` doctest pins mandatory acknowledgement at the
public invocation. Ordinary worker polling remains safe and unchanged.

The scene callback also receives a pending font namespace reset and returns a
rendering verdict. The host uses the dedicated plugin renderer entry point;
only a true callback result acknowledges that reset. Plugin image replacement
can reuse font IDs for different bytes, so this boundary carries the image
transition independently of an ordinary font cache lookup (ADR-0108).

### Exhausted element identities refuse removal before teardown

An element slot's next nonzero generation is checked before eager removal or
finalized retirement unregisters dependencies, invokes unmount or frees storage.
The checked generation is committed only after slot removal. A caught exhaustion
panic therefore leaves the original occupant live rather than freeing a slot
that could reuse its identity. Subtree removal preflights every live slot it
will finalize before detaching keyed descendants or retiring any node. Under
`DeactivateKeyed`, the snapshot excludes keyed boundaries and their surviving
descendants; those slots require no generation advance. Under `Finalize`, every
node is admitted before deepest-first teardown. Keyed soft removal does not
free storage and does not advance the generation.

The three exhaustion rows of `element_tree_contract_matrix` inject the otherwise
unreachable maximum counter, then use the tree's removal, lookup and insertion
surface. Each refuses repeated retirement while preserving the active occupant,
and proves a subsequent ordinary sibling can be removed and replaced without
reviving its stale ID. Unannounced retirement shares the finalized primitive.
Six subtree rows inject exhaustion at the root, intermediate node or leaf for
both removal modes and assert that repeated refusal leaves every occupant,
parent/child link and active lifecycle intact, with no unmount observation.
An independent sibling still retires and remints afterward. A separate exhausted
wrapper row preserves its keyed descendant's active lifecycle and registration
before any soft detach. The terminal keyed subtree control checks that soft
removal preserves the keyed boundary and its exhausted descendant instead of
imposing a generation advance on retained slots.


### Object keys own their identity

`ObjectKey` stores the erased owning Arc alone. `Arc::ptr_eq` compares its
allocation identity, while hash and debug formatting use its data address.
Cloned keys keep that allocation live; equal values in separate allocations
remain different keys. Automatic Send/Sync follow the stored Arc's bounds,
without duplicate pointer state or manual unsafe implementations.

The public constructor doctest distinguishes shared and separate allocations.
`object_keys_follow_retained_allocations_through_reorder` in
`dense_and_production_reconcile_matrix` checks that real keyed reconciliation
moves the original elements for equal-valued separate objects and keeps the
source allocation alive after its original owner is dropped.

### Observer containment retains exceptional ownership before reporting

**Rule.** Tree emission and outgoing `detached()` callbacks borrow their owned
observer envelopes inside the unwind boundary (ADR-0040). A caught failure
retains its opaque payload and the failed observer Arc before any diagnostics
run. Emission clears the slot without invoking `detached()`; replacement keeps
its newly installed observer. Failure reporting has its own unwind boundary and
retains a competing subscriber payload. No opaque destructor is attempted after
an observer callback has already failed.

Successful `detached()` calls retain ordinary Arc retirement semantics. This
boundary does not recover from an abort inside callback code, or from several
panicking capture destructors during otherwise successful retirement. It neither
changes the observer's non-reentrancy rule nor covers `replay_mounts`, whose
install failure policy remains ADR-0040's.

The public `lifecycle_panic_containment_matrix` includes isolated observer
children for aggregate payloads, final capture envelopes, competing failures,
and a hostile scoped tracing subscriber. Each child verifies that the real tree
mount completes, the failed observer is disarmed, and a healthy observer receives
the following builds. Replacement also checks that its new observer survives an
outgoing `detached()` failure.

### Build recovery retains opaque failure ownership before substitution

Build failure classification borrows the original payload, then retains that
opaque payload before reactive build finalization, the recovery factory or
diagnostics can fail. Its destructor is never invoked on this exceptional path.
The recovery factory keeps its existing failure authority: a factory unwind
publishes no successful build-recovery record.

Committed recovery records are stored before reporting. A subscriber failure
has a separate unwind boundary and its opaque payload is retained, so it cannot
erase attribution or unwind a returned recovery view. Staged lifecycle reporting
is likewise contained, while its owned diagnostic token is committed only after
the containing replacement succeeds. This does not make recovery factory code
or simultaneous panicking destructors inside otherwise ordinary user-owned
aggregates recoverable.

Six isolated rows in `lifecycle_panic_containment_matrix`, defined in
`tests/support/build_payload_recovery.rs`, exercise original aggregate payloads,
subscriber failure alone and in competition, recovery-view capture ownership,
factory failure priority and staged init-state attribution. Each child checks
actual ErrorView or configured-view substitution where recovery commits, the
original hook and element record exactly once, and following healthy builds.

### Lifecycle retirement follows the existing first failure

**Rule.** Lifecycle delivery keeps the first callback or retirement panic
through cancellation and terminal cleanup (ADR-0035). A cancelled callback
envelope is retained after a caught failure. Terminal release shares the active
drain's failure accumulator and retains later envelopes once the first ordinary
retirement fails. Subscription and source destruction during an independent
unwind retain callbacks without entering their opaque drop glue. Cancellation
and close still publish their state before retirement, and healthy eligible
listeners receive queued FIFO events before the original panic resumes.
The active drain publishes its caught-failure status before invoking another
callback. Nested cancellation and source retirement consult that same status,
so cancelling a pending callback cannot retire a hostile capture aggregate
after an earlier callback failed. The status is cleared when drain ownership
ends; later successful cancellation keeps ordinary capture destruction.
`caught_failure_protects_nested_pending_subscription_retirement` covers an
earlier callback panic, a later callback cancelling a pending aggregate, queued
FIFO delivery and the next healthy operation in a bounded child.

Rejected lifecycle admission owns its incoming generic callback even though no
listener was registered. Closing/closed-source rejection releases the state
borrow before retirement. A caught failure in the active source drain, or an
independent unwind including a dead weak source, retains that callback without
calling its body or destructor. Ordinary rejection still destroys captures and
allows their destructors to query the source without a borrow conflict.
`caught_failure_retains_rejected_lifecycle_callback`,
`live_source_rejection_during_unwind_retains_captures` and
`dead_source_rejection_during_unwind_retains_captures` join the public containment
family. They preserve the original string failure, prove queued FIFO tail/terminal
delivery or independent unwind, then check ordinary rejected retirement and a
fresh source's next operation.

With no prior failure and no active unwind, callback retirement keeps ordinary
Rust destruction semantics. A first envelope with two panicking fields can
abort before `catch_unwind` returns; this boundary cannot recover it. Live
callbacks that panic without cancelling remain registered, as before.

The public `lifecycle_panic_containment_matrix` exercises self-cancellation,
competing payload/capture aggregates, terminal cleanup, first retirement failure,
and subscription/source Drop during independent unwind in bounded children.
Healthy FIFO delivery and the next source operation distinguish retention from
lost work; `successful_lifecycle_cancellation_retires_captures_and_keeps_fifo`
ensures ordinary successful cancellation still releases captures.

### Async snapshot publication precedes generic retirement

**Rule.** Future and stream completions construct their incoming snapshot and
publish it under the subscription-generation fence. Old snapshot retirement
and the matching rebuild request run after the slot Mutex is released. If old
retirement panics, rebuild scheduling is still attempted; the first panic then
resumes, and a competing wake payload is retained. The new snapshot already
belongs to the slot, so unwinding the old value cannot destroy it. Inline future
completion still suppresses a redundant rebuild and keeps its `Done` guard.
Connection-state-only transitions preserve owned data/error without retirement.
Stale incoming values retire outside the slot lock and schedule no rebuild.

The task that panicked remains failed under the driver's policy. Recovery means
that the queued next build can read the published snapshot and a fresh keyed
subscription can publish again. This does not promise continued polling of the
failed producer, arbitrary user-builder containment, or recovery from two
panicking fields within the first ordinary retired aggregate.

The public `lifecycle_panic_containment_matrix` includes FutureBuilder and
StreamBuilder children for old retirement alone, competing old/new values, and
old retirement plus a failed rebuild wake. Each observes the committed incoming
value on a real build without test-induced dirtiness, then changes the key and
observes a second completion.

The borrowed caller Arc stays live through retirement and wake catches. After
an accepted-update failure, one owning slot Arc is cloned and retained before
resuming, so disposal of the failed producer or eagerly initialized element
cannot retire the published incoming value in competition. Healthy updates
perform no additional Arc clone/drop. The guarantee retains the slot's lifetime,
not an immutable historical value across later writes.

The accepted-update guard costs one Arc clone only on a caught failure path. The private
`accepted_publication_guard_retains_incoming_after_caller_disposal` row joins
`future_builder_matrix`: it calls the same production `apply_update` with no
driver-owned Arc, catches old retirement, and disposes the final caller owner.
The public eager-disposal row separately checks a real builder; retained failed
task ownership can also pin its snapshot, so that row is not a guard-only oracle.

### GlobalKey identity exhaustion is permanent

`GlobalKey::new` issues every identity in `1..=u64::MAX` once, then refuses
all further allocations. Zero is an internal exhaustion sentinel, never a key.
The atomic transition publishes the last identity and sentinel together, so
catching a refusal cannot wrap the allocator into an earlier identity. This
changes identity admission only; GlobalKey registries remain realm-owned.

`exhausted_global_key_counter_never_reissues_an_identity` joins the existing
private `element_tree_contract_matrix`. It drives the production mint helper
with a local terminal counter because a consumer cannot exhaust the real
identity space. It admits both final identities, catches repeated refusal and
checks subsequent ordinary allocation from an independent local counter.

### Closed presentation authority

Closing a presentation withdraws its signal graph, external build inbox,
rebuild handles and local and realm GlobalKey lookup before any optional
terminal callback runs
([ADR-0123](../../docs/adr/ADR-0123-exceptional-presentation-close.md)). The
closed graph refuses reads, writes and new signals with
`SignalError::OwnerClosed`, and a saved writer reports its owner detached. Key
owners are removed from their registries before they are dropped. A
`LifecycleSource` drain runs every eligible callback before propagating its
first failure; in preserving mode the host skips the optional rounds after it
while the required terminal commits still happen. The value or closure a closed
graph rejects is dropped normally unless the thread is already panicking
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)).

### Independent child and contained payload retirement

**Rule:** `StaticChildren` withdraws its independently owned views and its
mapper before retiring any of them. The first failure propagates and the
untouched tail is retained (ADR-0127); healthy destruction is ordinary, and
aliases keep their owners.

**Rule:** a contained child create, mount or update failure, and a swallowed
dispose, deactivate or render-unmount failure, take custody of the opaque
payload before diagnostics or recovery run. The payload stays retained through
competing factory or reporting failures, and healthy siblings and the next
operation proceed.

**Limits:** a user aggregate whose own destructors double-panic still aborts
(ADR-0127, Consequences).

**Pinned by:** the `static_children_retirement`, `child_payload_recovery` and
`lifecycle_hook_payload_retirement` rows of
`lifecycle_panic_containment_matrix`.
