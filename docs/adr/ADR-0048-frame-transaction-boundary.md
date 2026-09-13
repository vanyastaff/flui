# ADR-0048: Presentation-local frame-transaction boundary

*FLUI contains failures at two nested scales. Three bounded build-side seams
repair one failed child or removal hook locally and preserve the rest of the
element tree. A panic or structured pipeline error that escapes those seams is
contained by the realm's per-presentation frame boundary: that frame is
dropped, sibling presentations continue in the same pump, other realms are
untouched, and the last successfully presented frame stays on screen. This is
local repair plus frame isolation, not a global tree transaction or rollback;
the residuals below define the remaining blast radii.*

---

- **Status:** Accepted (2026-08-18)
- **Date:** 2026-08-18
- **Deciders:** @vanyastaff
- **Scope:** bounded build-side recovery in `flui-view`, the
  per-presentation frame boundary in `UiRealm::draw_frame_entered` /
  `draw_frame_for_presentation` / `render_frame_entered`, the typed report
  surface in `crates/flui-app/src/app/frame_failure.rs`, and
  `WidgetsBinding::draw_frame`'s unwind-consistency guard
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

---

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

Each presentation's build+layout+paint segment
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
unreported count probes. A host may drain it after the build segment; otherwise
the next frame entry discards the stale records with one aggregate warning.
Every produced presentation segment enters `WidgetsBinding::draw_frame` for
this expiry even when the
widget tree has no pending builds; the build drain itself stays conditional.
Thus a record produced by late lazy-child service is still bounded when the
next frame carries only pipeline work, even when no host forwarding consumer
is installed.

### Failure classification and the typed route

`crates/flui-app/src/app/frame_failure.rs`:

- `FrameFailureKind::Pipeline { error: RenderError }` — structured failures
  (caller validation, recoverable subtree, backend), keeping their typed shape.
- `FrameFailureKind::SegmentPanic { message, internal_invariant }` — escaped
  panics caught at the boundary. `internal_invariant` is true when the payload
  carries the panic policy's `BUG:` prefix: a framework invariant violation is
  *classified and named loudly* (distinct error-level message field), but still
  contained — an end user's sibling windows surviving is worth more than an
  abort, and the report is the loud part.
- `FrameFailureReport { address: PresentationAddress, kind,
  consecutive_failures }` — ownership identity via the generational address
  (#552), plus a per-presentation consecutive-failure streak (reset by the next
  cleanly completed segment) an embedder can key escalation off.

Both kinds route through one `UiRealm::report_frame_failure`: streak bump →
structured `tracing::error!` → `FrameFailureHandler` delivery (registered via
`AppConfig::with_frame_failure_handler`, wired realm-scoped by each backend's
bootstrap — never a process-global hook). The handler runs synchronously
mid-pump with no realm borrow held; its contract (lightweight, no re-entry
into FLUI APIs) is on its rustdoc.

**Privacy:** the panic message is emitted as a plain string `tracing` field
(`panic_message`), which FLUI's device sinks redact by default (`flui-log`'s
private-by-default classification) — so a message that interpolated user data
does not reach OS log stores unredacted, while the developer console still
shows it. The typed report carries it verbatim; registering a handler is the
embedder's opt-in.

### Retry and last-good retention

- A failed frame **submits nothing**: `render_scene` is reached only by a
  `Painted` outcome, so the surface keeps presenting the last successfully
  submitted frame. There is no zero-size, empty, or placeholder scene on the
  failure path — retention is structural, not synthesized.
- `draw_frame_entered` now returns an explicit **`any_failed`** bit covering
  every segment in the pump, and `render_frame_entered` arms the retry
  (`wake_frame()`, no `mark_rendered()`) off that bit — not off the last
  producer's outcome. The retry re-attempts from the pipeline's retained dirty
  marks. A retry whose failure consumed its build-dirty state may find nothing
  dirty and park with the last-good frame on screen; the failure was already
  surfaced, and that quiescent ending is documented rather than papered over.
- A deterministic failure that keeps re-dirtying therefore retries at the
  runner's fallback pace with a caught, reported failure each time — the same
  accepted steady state as a permanently failing surface submit (see
  `render_frame_entered`'s `SurfaceValidation` arm). Automatic suspension
  ("halt this presentation after N consecutive failures") is deliberately NOT
  in this slice: it needs its own wake-predicate exclusions to avoid a
  busy-spin, and the embedder already gets `consecutive_failures` to make that
  call itself. Automatic suspension is a separate scheduling-policy decision.
- The post-frame stationary-device re-hit-test is skipped on any pump with a
  failed segment: hover state holds the last cleanly committed version instead
  of actively probing a mid-commit tree.

### Measured cost of the build-side seams

The intervals below are the recorded Criterion 95% confidence intervals from
the production reconcile harnesses. On success, dense update returns inline;
recovery is a private `#[cold] #[inline(never)]` path. The containment column
uses that structure and is compared with the recorded pre-containment
baseline.

| Benchmark | Baseline | Containment | Midpoint change |
|---|---:|---:|---:|
| dense same-slot /32 | `[8.0031, 8.0627, 8.1142] µs` | `[7.6674, 7.7051, 7.7496] µs` | −4.4% |
| dense same-slot /256 | `[58.436, 58.927, 59.298] µs` | `[61.434, 61.784, 62.041] µs` | +4.8% |
| dense same-slot /1024 | `[228.27, 229.97, 232.14] µs` | `[237.69, 240.46, 244.60] µs` | +4.6% |
| dense reordered /32 | `[15.603, 15.724, 15.875] µs` | `[14.185, 14.218, 14.243] µs` | −9.6% |
| dense reordered /256 | `[105.21, 105.63, 106.09] µs` | `[108.33, 109.38, 110.39] µs` | +3.6% |
| dense reordered /1024 | `[426.92, 430.03, 433.33] µs` | `[440.20, 445.24, 449.51] µs` | +3.5% |

The insert-side production run also recorded `scoped_build_drain` at
`[8.4986, 8.6204, 8.7228] µs` (no scopes/256),
`[14.796, 15.032, 15.205] µs` (sibling scopes/32),
`[135.36, 136.92, 138.33] µs` (sibling scopes/256), and
`[5.7700, 5.8920, 5.9761] µs` (nested published scopes/13), plus
`global_key_reparent_latency` at `[4.7009, 4.7399, 4.7832] µs` (/32),
`[23.176, 23.348, 23.542] µs` (/256), and
`[86.144, 86.895, 87.775] µs` (/1024). Against their recorded baselines no
statistically supported regression exceeded 5%.

### Consistency audit — local repair, not global rollback

Honestly named, per the issue's "transactional" acceptance criterion:

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
- **Pointer events arriving before the retry still hit-test the live tree** —
  only the pump's own ambient re-probe is gated. One committed hit-test
  version per frame needs snapshotted/versioned geometry, out of scope here.
- **The realm-level pre-phase is outside the boundary:** vsync ticker
  callbacks (which can run user animation listeners) and gesture-deadline
  ticks run before the per-presentation loop; a panic there still escapes to
  the runner.
- **Semantics:** flushed inside the segment, so a semantics panic is contained,
  but a failed frame's semantics are simply not published (last published
  version stands) — no candidate/validate/publish transaction yet.
- **Secondary-realm windows** (`open_secondary_window` under
  `SeparateRealms`) get containment and tracing but no handler wiring — the
  deferred-completion plumbing does not carry the primary `AppConfig`.

## Consequences

- One window's frame bug no longer kills a multi-window process. The bounded
  build-side seams keep a recoverable child or removal-hook failure inside its
  element tree; an escape still leaves sibling presentations framing in the
  same pump, and other realms are untouched.
- Embedders get a typed, addressed failure feed with streak accounting;
  `tracing` alone is no longer the only witness.
- A permanently failing presentation costs a contained panic per fallback-pace
  wake until the embedder acts or the dirty state parks — accepted for this
  slice, escalation policy deferred as above.
- `FramePaintOutcome` stays a unit-variant enum; failure detail travels the
  report route, not the outcome value, so submit/telemetry matching is
  untouched.
