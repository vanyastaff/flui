# ADR-0048: Presentation-local frame-transaction boundary

- **Status:** Accepted
- **Date:** 2026-08-18
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (runtime topology
  is a sanctioned leapfrog zone); [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md)
  (`PresentationState` bundles the trees this boundary scopes);
  [`docs/PANIC-POLICY.md`](../PANIC-POLICY.md) (the `BUG:` convention this
  boundary classifies rather than overrides); the Win32 wndproc panic boundary
  (`crates/flui-platform/src/shared/panic_boundary.rs`) — a DIFFERENT seam with
  the opposite verdict, compared below; [Runtime Architecture Execution
  Plan](../research/2026-08-01-runtime-architecture-execution-plan.md) ("Make
  frame failure recovery transactional")
- **Issue:** [#561](https://github.com/vanyastaff/flui/issues/561)

*FLUI contains failures at two nested scales. Three bounded build-side seams
repair one failed child or removal hook locally and preserve the rest of the
element tree. A panic or structured pipeline error that escapes those seams is
contained by the realm's per-presentation frame boundary: that frame is
dropped, sibling presentations continue in the same pump, other realms are
untouched, and the last successfully presented frame stays on screen. This is
local repair plus frame isolation, not a global tree transaction or rollback;
the residuals below define the remaining blast radii.*

## Context

The failure model has two narrower containment layers below the frame seam:

1. **Build phase:** a panicking user `build()` is caught per element and the
   broken widget is replaced by an `ErrorView`
   (`crates/flui-view/src/element/behavior_commons.rs`; Flutter's
   `ComponentElement.performRebuild` → `ErrorWidget.builder`).
2. **Layout/paint:** the pipeline wraps `perform_layout_raw`/`paint` in
   `catch_unwind` and surfaces panics as `RenderError::Poisoned`; geometry is
   validated **before** it is committed to node state, and a failed node keeps
   its `NEEDS_LAYOUT` mark for the next frame
   (`crates/flui-rendering/src/error.rs`, `pipeline/owner/subtree_arena.rs`).

Two outer-layer holes require the frame seam:

- **An escaped panic was process-fatal.** Segment phases covered by neither
  inner layer — lifecycle teardown, child mounting outside a substitution
  window, and overlay attachment — unwound through the realm's per-presentation
  loop (aborting every later sibling's segment in the same pump) and then
  through `dispatch_platform_realm`'s restore-then-`resume_unwind`, killing the
  process. One window's teardown bug took down every window in every realm.
- **A failure was a silent skip.** A pipeline error was logged and dropped;
  nothing typed reached the embedder, no per-presentation failure accounting
  existed, and with more than one presentation mounted the retry logic keyed
  off only the LAST presentation's outcome — an earlier presentation's failure
  followed by a clean sibling `Painted` ended the pump in `mark_rendered()`,
  clearing the very wake the failed presentation's retry needed.

The outer boundary alone is too coarse for build-side failures. A child
lifecycle hook could leave a parent's slot occupied by a failed element, a
dense reconcile failure could abandon the
remainder of the sibling list, and a removal-hook failure could stop teardown.
The decision therefore also defines the smaller regions that can repair one
child without claiming that arbitrary framework panics are recoverable.

## Decision

### The seam: per-presentation `catch_unwind` in `draw_frame_entered`

Each presentation's complete segment
(`draw_frame_for_presentation`) runs inside
`catch_unwind(AssertUnwindSafe(..))`. A caught payload becomes
`FramePaintOutcome::Errored` for that presentation only; the loop continues to
the next sibling. This is a **deliberate architectural seam**, sanctioned as
the frame-boundary exception to the panic policy's "a panic is a bug report"
rule: the panic IS still treated as a bug report (error-level tracing, typed
report, dropped frame) — what the seam removes is only the process-global
blast radius, which was never part of the report's value.

Why this is the right seam and not the wndproc one's verdict: the Win32
boundary (#598) chose log-then-abort because unwinding across an FFI frame is
undefined behavior — there, containment is impossible and loud death is the
only sound option. Here the unwind crosses only Rust frames inside one
presentation's segment; parking_lot guards release during unwind (no lock
poisoning), `RefCell` borrows drop, and the pipeline's own failure discipline
(validate-before-commit, retained dirty marks) means the retained premises are
consistent even when the frame's partial work is not.

### Build-side containment: three bounded seams

The outer frame boundary is the fallback. `flui-view` uses three smaller seams
when it can identify both the failed child and a valid local repair:

1. **Stateful phase one.** `init_state` and
   `did_change_dependencies` are each caught at the behavior call, attributed
   to that exact element as
   `RecoveredAt::Element { element, parent: None }`, and rethrown with an
   owner-local staged token. The token captures behavior attribution only;
   it does not publish a record or claim that replacement committed. The
   immediate `BuildOwner` catch first restores the element's slab slot, then
   takes ownership of the token. A
   parented element is finalized and replaced by an `ErrorView` at its own
   parent slot; topology and the later substitute mount establish the parent
   and outcome separately. Dirty and inherited-dependency state for the failed
   generation is removed; a scoped drain is repartitioned from a fresh
   live-scope snapshot and continues. If recovery-view construction panics,
   the original element is preserved and requeued, and this attempt's owned
   token is dropped; earlier committed records remain. A root has no parent
   slot to repair, so it is restored, requeued, and rethrown without a
   `RecoveredPanic` record. Ordinary `build` panics retain the existing
   behavior-level `ErrorView` recovery.
2. **Child creation, retake, and update.** A fresh element has one literal
   catch from `view.create_element()` through the end of its `mount`. A
   stateful element's `init_state` is not in this catch: FLUI runs it later in
   the first build-scope drain. Post-catch `GlobalKey` registration, the mount
   event and observer notification, ancestor `ParentDataView` application, and
   render reordering remain outside. A `GlobalKey` retake has two separately
   labelled literal catches: `Activate`
   wraps only `activate_subtree`, and `Update` wraps only the retaken element's
   `update`. `InsertedChild::Retaken` is committed before activation so a
   failure has an element to remove. Retake preflight, duplicate-key and
   registry checks, ancestry repair, render relocation, parent-data reset, and
   every other retake framework tail stay outside or between those catches.
   Sparse and dense reconciliation use these same primitives and substitute only the
   failed final slot before continuing siblings.
3. **Removal.** `deactivate`, `dispose`, and
   `did_unmount_render_object` are caught individually, attributed to the
   element, and followed by the framework-owned state transition or teardown.
   A state whose `init_state` did not finish receives none of `activate`,
   `deactivate`, or `dispose`. `AnimatedBehavior` closes the separate
   `listenable()` cleanup seam by caching the subscribed `Arc` and listener id:
   unmount removes through that handle without calling user `listenable()`
   again, and update reads the new handle once and compares identity with
   `Arc::ptr_eq`.

The rule is deliberately literal: **the code region decides whether a panic is
contained; the payload only classifies it.** A `BUG:` payload inside one of the
listed regions is contained and recorded with `internal_invariant: true`; the
same payload outside remains an escape. No prefix widens a catch or chooses a
different recovery path.

A production `LayoutBuilder` scope root cannot itself enter the stateful
phase-one seam: its behavior is `LayoutBuilderBehavior`, not
`StatefulBehavior`. A stateful descendant built while that scope is serviced
can fail there; recovery replaces the descendant, resumes the scoped drain,
and leaves the still-live `LayoutBuilder` registry entry intact. Normal scope
unmount owns deregistration, while the registry's existing stale-entry pruning
remains the defensive fallback.

Lazy delegates are bounded separately. A mounted-item builder records the host
and logical index; `find_index_by_key` records the host with no index and
declines the move. `probe_item_count` and `regrown_item_count` catch, construct
an unmounted recovery-view value, and discard it without recording. A later
mounted-item call may mount and record the same index, or the probed index may
never become production content.

### Flutter 3.44.0 mapping

The reference is Flutter 3.44.0, checked at the pinned tag. FLUI preserves its
observable recovery floor and narrows several blast radii:

- `ComponentElement.performRebuild` first catches `build` and substitutes
  `ErrorWidget`; its second catch surrounds `updateChild`, attempts to
  deactivate the old child, and retries with an `ErrorWidget`. FLUI keeps the
  build recovery and moves child create/retake/update recovery down to the
  actual failing child's final slot, so dense reconciliation can continue its
  healthy siblings.
- `BuildScope._tryRebuild` catches an exception from each dirty element and
  reports it, but does not itself install a substitute. FLUI's stateful
  phase-one recovery replaces a parented element at its own slot. For an
  `init_state` failure this is narrower than relying on the nearest enclosing
  `ComponentElement.performRebuild` recovery; a root still propagates because
  it has no repairable parent slot.
- `_InactiveElements._deactivateRecursively` catches a throwing `deactivate`,
  marks the failed subtree, and rethrows. FLUI records the exact element,
  completes its `Inactive` transition, and permits same-frame retake or later
  finalization.
- `BuildOwner.finalizeTree` catches around the inactive drain as a whole.
  FLUI catches `dispose` per element, so the remaining inactive elements still
  finalize.
- `RenderObjectElement.unmount` calls `super.unmount()` before
  `didUnmountRenderObject`; if that hook throws, render-object `dispose` is
  skipped. FLUI's ordering differs: the element behavior runs before
  `ElementCore::unmount`. The local catch guarantees that a hook panic cannot
  stop the subsequent framework tree-side and render-tree removal; cleanup
  owned by the hook itself can still be skipped, as recorded below.

Every recovery described above that has an attributable element, substitute,
or lazy host enters a frame-scoped `RecoveredPanic` queue, except the explicitly
unreported count probes. `UiRealm` takes that queue exactly once after the
entire presentation attempt and outside its `catch_unwind`. The timing includes
the layout fixpoint and late post-pipeline lazy service, and still drains
recoveries if later Tail or Scene work unwinds. Records are delivered in queue
order before any terminal report for that attempt. Every produced presentation
segment enters `WidgetsBinding::draw_frame` even when the widget tree has no
pending builds; if no host drains a stale record, the next frame entry discards
it with one aggregate warning.

### Failure classification and the typed route

`crates/flui-runtime/src/frame_failure.rs` (moved from `flui-app` by ADR-0083; `flui_app`
re-exports the types):

- `FrameFailureKind::Pipeline { error: RenderError }` is a terminal
  `FrameDropped` report for a structured pipeline failure. The handler keeps
  the typed error.
- `FrameFailureKind::SegmentPanic { message, phase, internal_invariant }` is a
  terminal `FrameDropped` report for a panic escaping Build, Finalize,
  Pipeline, Tail, or Scene. `phase` is written before the matching work.
- `FrameFailureKind::RecoveredPanic { at, view_type_id, hook, message,
  internal_invariant }` is a `Contained` report for a narrower lifecycle
  recovery. It does not by itself drop the surrounding frame or arm a retry.
- `FrameFailureReport { address, kind, disposition, consecutive_failures }`
  carries the generational presentation address (#552). The streak counts
  consecutive `FrameDropped` reports only. A `Contained` report exposes the
  current streak but neither increments nor resets it; a terminally clean
  attempt resets the streak after that attempt's contained reports have been
  delivered.

Disposition is derived privately from `FrameFailureKind`, so an impossible
kind/disposition pair cannot enter the delivery path. Each report is traced
before synchronous `FrameFailureHandler` delivery. The handler is cloned out
before invocation, no realm borrow is held while embedder code runs, and a
handler panic is caught at the delivery site. It neither escapes the frame
boundary nor unregisters the handler, and the same report is not retried.

**Privacy:** `FrameFailureDetail` defaults to `Verbatim` in debug builds and
`Redacted` in release builds; an explicit `AppConfig` override is independent
of `DiagnosticsProfile`. The raw panic payload is classified for the `BUG:`
invariant prefix before policy materialization. A recovered record separately
stores `payload_text: Option<Box<str>>`: `Some` only preserves an actual string
payload, while its display-facing `FlutterError` may keep the existing
synthetic fallback for a non-string payload. The app consumes that provenance,
never the diagnostic fallback. Under `Redacted`, panic text is
neither retained nor formatted; non-string payloads remain redacted even under
`Verbatim`. Segment-panic and recovered-panic reports therefore carry the
policy-filtered `PanicText`. For pipeline failures, the policy controls only
the text FLUI itself formats into its tracing event: a handler still receives
the original typed `RenderError`, and formatting that error or the report's
`Debug` representation may expose sensitive text. Handler authors own that
boundary.

This guarantee covers FLUI-owned app tracing, not arbitrary custom subscriber
formatting and not lower-level `flui-view` recovery traces. It is not a global
sanitizer. Desktop, web, and Android bootstrap apply both handler and detail
policy to their initial realm. A `SeparateRealms` secondary carries its detail
policy through both the Ready and Pending completion paths; its handler remains
unset while secondary windows have no production content/render path. A
`SharedRealm` secondary inherits the existing realm's detail policy and handler;
the supplied secondary config does not override either for existing siblings.

### Retry and last-good retention

- A failed segment **submits nothing**: `render_scene` is reached only by a
  `Painted` outcome, so the surface keeps presenting the last successfully
  submitted frame. There is no zero-size, empty, or placeholder scene on the
  failure path — retention is structural, not synthesized.
- A Tail or Scene panic occurs after the pipeline consumed paint dirtiness.
  Its catch re-dirties the exact failed presentation before a later sibling can
  become the pump's producer. Build, Finalize, and Pipeline failures do not use
  this full-repaint arm; they rely on their owning recovery/dirty state and may
  park cleanly if no such work remains.
- Paint commits the candidate layer tree and leader/follower link registry as
  one pair before semantics runs. A semantics error retains that matched pair,
  so retry cannot combine one tree with an empty or newer registry; the last
  published semantics tree remains in effect.
- `draw_frame_entered` returns an explicit **`any_failed`** bit covering every
  presentation segment in the pump. It only keeps the later wake decision from
  settling: it does not choose a repaint target. `render_frame_entered` arms a
  retry (`wake_frame()`, no `mark_rendered()`) from that bit rather than from
  the last producer's outcome.
- `TreeRevision` advances on each terminal `Painted` or `Errored` segment, but
  not on `Idle`. `FrameCommitState` becomes committed only when a `Painted`
  revision receives `Presented` or `NoPresent`; both verdicts acknowledge every
  revision through the current one. A deferred painted frame, `Errored`,
  `SurfaceStale`, `DeviceLost`, and `Failed` remain uncommitted. `Idle` neither
  advances nor acknowledges a revision.
- Submit verdicts have distinct retry effects. `SurfaceStale` and `DeviceLost`
  retain input telemetry, arm a wake, and request a full repaint of the actual
  producer. `Failed` drains telemetry and does not retry. `Presented` drains
  telemetry and commits; `NoPresent` commits without draining pending input
  epochs because no backend present occurred. Segment failures arm only the
  wake, except for the exact Tail/Scene repaint rule above.
- A retry whose terminal failure consumed its dirty state may run a clean
  `Idle`, reset the failure streak, and clear the wake. It does **not** repair
  the absent commit: the presentation remains `Uncommitted` with the last-good
  frame on screen until a later painted revision receives an accepted submit
  verdict.
- A deterministic failure that keeps re-dirtying therefore retries at the
  runner's fallback pace with a caught, reported failure each time — the same
  accepted steady state as a permanently failing surface submit (see
  `render_frame_entered`'s `SurfaceValidation` arm). Automatic suspension
  ("halt this presentation after N consecutive failures") is deliberately
  absent: it needs its own wake-predicate exclusions to avoid a
  busy-spin, and the embedder already gets `consecutive_failures` to make that
  call itself. Automatic suspension is a separate scheduling-policy decision.
- The primary presentation's stationary-device re-hit-test runs after submit
  classification only when that primary's own `FrameCommitState` is
  `Committed`. A secondary failure therefore does not freeze a committed
  primary, while an uncommitted primary holds its previous hover derivation.

### Cost of the build-side seams

On success, dense update returns inline; recovery is a private
`#[cold] #[inline(never)]` path. Measured with Criterion against the
pre-containment baseline on the production reconcile harnesses, dense same-slot
and reordered reconciliation moved within about ±5% (small lists faster, large
lists up to ~5% slower), and the scoped-build-drain and GlobalKey-reparent
benchmarks showed no statistically supported regression above 5%.

### Consistency audit — local repair, not global rollback

**Re-established (retained premises are consistent):**
- Render geometry: committed only after validation; a failed node keeps its
  old geometry AND its `NEEDS_LAYOUT` mark (pipeline discipline, pre-existing).
- Locks/borrows: parking_lot guards and `RefCell` borrows release during
  unwind; no poisoning, no deadlock on retry.
- `WidgetsBinding`'s `debug_building_dirty_elements` flag resets via RAII on
  unwind; a caught mid-`draw_frame` panic cannot wedge it `true` and turn every
  later debug-build frame into a bogus
  "recursive draw_frame" assert.
- First-frame latch, segment-span telemetry, submit gating: all keyed off the
  `Errored` outcome exactly as the pipeline-error path always was.
- Recovered records are drained once after the complete attempt, in production
  queue order, before a terminal report. Contained recovery does not alter the
  dropped-frame streak, retry bit, or sibling failure accounting.
- Paint's layer tree and leader/follower link registry remain a matched pair
  across a semantics failure, and Tail/Scene failure re-dirties the exact
  affected presentation.
- `TreeRevision`/`FrameCommitState` distinguish a terminal tree attempt from a
  submit acknowledgement; ambient primary hover refreshes only after the
  submit verdict leaves that primary committed.
- Addressed pointer input now follows the same commit-state boundary. While
  a presentation is `Uncommitted`, or while an earlier pointer event is already
  held for that presentation, `UiRealm` queues pointer events before hit
  testing, input-epoch stamping, or gesture dispatch. The queue replays only
  after a `Painted` frame is committed by `Presented` or `NoPresent`, through
  the same dispatch path live pointer input uses. This is the local analogue
  of Flutter's event-locking queue, but applied to FLUI's frame-commit state:
  Flutter may surface a mixed retained tree after a partial frame failure,
  whereas FLUI treats "not committed to the screen" as not eligible for new
  pointer hit tests.

**Not covered by these local repairs:**

- A root `init_state` or `did_change_dependencies` panic has no parent slot for
  an `ErrorView`; it propagates after restoring and requeuing the root.
- Direct activation outside a bounded retake is not recovered. Duplicate-key
  rejection, retake preflight, registry collision checks, and the framework
  relocation tails also remain outside the child windows.
- For stateful phase-one recovery, a panic while constructing the configured
  recovery view preserves and requeues the original element. It drops this
  attempt's owned staged token and retains all earlier committed records.
- In mount/update recovery the failed child has already been discarded or
  finalized before the recovery-view factory runs. Replacement creation,
  replacement mount, and any failure after destructive replacement begins
  have no rollback.
- An `ErrorView` is a box-protocol substitute. Installing it under a
  sliver-protocol parent can itself fail the protocol adoption, as Flutter's
  `ErrorWidget` can in the equivalent mismatch.
- Lazy `probe_item_count` and `regrown_item_count` failures are caught but do
  not create `RecoveredPanic` records by design.
- A `did_unmount_render_object` panic can leave a registration owned by that
  hook's own lane behind; framework tree removal still continues.
- A mounted state whose initialization never completed skips `dispose`;
  `create_state` therefore must not acquire lifecycle resources.
- `ParentDataView` validation and parent-data application straddle the bounded
  child regions and are not yet locally recovered.
- A deterministic update panic can oscillate between the failing view and its
  substitute on successive parent rebuilds, producing one recovery per pass.
- Mid-segment work outside these seams is not rolled back globally. The next
  frame proceeds from the locally repaired or last retained state.
- Held pointer replay preserves target correctness and event order, not
  velocity reconstruction. Gesture recognizers currently sample from the arena
  clock at delivery (`SystemClock` in production), and `PointerEventData`'s
  original `time_stamp` is not plumbed into those recognizer samples. A burst
  of held `Down`/`Move`/`Up` events therefore replays at the retry-delivery
  instant and may degrade fling velocity after a failed frame. That is an
  accepted cost of this local repair because the screen was frozen during the
  failed interval; passing event timestamps into `flui-interaction` remains a
  future compatibility improvement if replayed kinetic fidelity becomes a
  user-visible requirement.
- **The realm-level pre-phase is outside the boundary:** vsync ticker
  callbacks (which can run user animation listeners) and gesture-deadline
  ticks run before the per-presentation loop; a panic there still escapes to
  the runner.
- **Semantics:** a failed frame does not publish a new semantics update; the
  last published version stands. Layer-tree/link-registry pair retention is
  narrower than a full candidate/validate/publish semantics transaction.
- **Production multi-paintable routing:** `render_frame_entered` still owns one
  constraints set, one sink, and only the last produced scene. Secondary
  windows remain contentless until #559 adds per-presentation constraints,
  sinks, and submit routing.
- **Secondary handler wiring:** a new `SeparateRealms` realm receives the
  secondary config's detail policy through Ready and Pending completion but no
  failure handler. That handler decision is blocked on the same #559
  production secondary rendering contract. `SharedRealm` already uses the
  existing realm's handler and intentionally refuses a per-window override.

## Consequences

- One window's terminal frame bug no longer kills a multi-window process. An
  escape drops and retries only that presentation's frame; sibling
  presentations continue in the same pump and other realms are untouched.
- A locally recovered lifecycle panic produces a `Contained` report without
  dropping the frame or arming retry. A deterministic recovery can therefore
  produce a report storm even while frames keep presenting; embedders that
  forward reports own throttling or deduplication.
- Embedders get a typed, addressed failure feed with streak accounting;
  `tracing` alone is no longer the only witness.
- A permanently failing terminal path costs a caught, reported failure per
  fallback-paced retry until the embedder acts or the dirty state parks.
  Automatic suspension remains a separate scheduling-policy decision.
- `FramePaintOutcome::Errored` represents failure anywhere in the complete
  Build/Finalize/Pipeline/Tail/Scene segment. Failure detail travels the typed
  report route rather than the outcome value, keeping submit/telemetry
  matching independent of diagnostic payloads.
