# ADR-0048: Presentation-local frame-transaction boundary

*A frame that fails — a structured pipeline error or a panic that escaped every
inner recovery layer — is contained to the one presentation whose frame it was.
The realm's per-presentation `catch_unwind` seam converts the escape into a
dropped frame plus a typed `FrameFailureReport`; sibling presentations in the
same pump keep framing, other realms are untouched, the process survives, and
the last successfully presented frame stays on screen because a failed frame
submits nothing (never a zero/blank scene). Full transactionality of
mid-segment tree mutations is NOT claimed; the consistency audit below names
exactly what unwinding does and does not re-establish.*

---

- **Status:** Accepted (2026-08-18)
- **Date:** 2026-08-18
- **Deciders:** @vanyastaff
- **Scope:** frame failure containment — `UiRealm::draw_frame_entered` /
  `draw_frame_for_presentation` / `render_frame_entered`
  (`crates/flui-app/src/app/ui_realm.rs`), the typed report surface
  (`crates/flui-app/src/app/frame_failure.rs`, `AppConfig::frame_failure_handler`),
  and `WidgetsBinding::draw_frame`'s unwind-consistency guard
  (`crates/flui-view/src/binding.rs`)
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

FLUI already had two inner containment layers, both ported from Flutter's
behavior:

1. **Build phase:** a panicking user `build()` is caught per element and the
   broken widget is replaced by an `ErrorView`
   (`crates/flui-view/src/element/behavior_commons.rs`; Flutter's
   `ComponentElement.performRebuild` → `ErrorWidget.builder`).
2. **Layout/paint:** the pipeline wraps `perform_layout_raw`/`paint` in
   `catch_unwind` and surfaces panics as `RenderError::Poisoned`; geometry is
   validated **before** it is committed to node state, and a failed node keeps
   its `NEEDS_LAYOUT` mark for the next frame
   (`crates/flui-rendering/src/error.rs`, `pipeline/owner/subtree_arena.rs`).

What was missing was the layer above them, and it had two concrete holes:

- **An escaped panic was process-fatal.** Segment phases covered by neither
  inner layer — `ViewState::dispose` during `finalize_tree`, lazy-sliver child
  servicing, overlay attachment — unwound through the realm's per-presentation
  loop (aborting every later sibling's segment in the same pump) and then
  through `dispatch_platform_realm`'s restore-then-`resume_unwind`, killing the
  process. One window's teardown bug took down every window in every realm.
- **A failure was a silent skip.** A pipeline error was logged and dropped;
  nothing typed reached the embedder, no per-presentation failure accounting
  existed, and with more than one presentation mounted the retry logic keyed
  off only the LAST presentation's outcome — an earlier presentation's failure
  followed by a clean sibling `Painted` ended the pump in `mark_rendered()`,
  clearing the very wake the failed presentation's retry needed.

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
  call itself. Part of #561's remaining work.
- The post-frame stationary-device re-hit-test is skipped on any pump with a
  failed segment: hover state holds the last cleanly committed version instead
  of actively probing a mid-commit tree.

### Consistency audit — what unwinding does and does not re-establish

Honestly named, per the issue's "transactional" acceptance criterion:

**Re-established (retained premises are consistent):**
- Render geometry: committed only after validation; a failed node keeps its
  old geometry AND its `NEEDS_LAYOUT` mark (pipeline discipline, pre-existing).
- Locks/borrows: parking_lot guards and `RefCell` borrows release during
  unwind; no poisoning, no deadlock on retry.
- `WidgetsBinding`'s `debug_building_dirty_elements` flag now resets via RAII
  on unwind (this change) — previously a caught mid-`draw_frame` panic wedged
  it `true`, turning every later debug-build frame into a bogus
  "recursive draw_frame" assert.
- First-frame latch, segment-span telemetry, submit gating: all keyed off the
  `Errored` outcome exactly as the pipeline-error path always was.

**NOT re-established (named residual gaps, all Part of #561):**
- **Mid-segment tree mutations are not rolled back.** A build that panicked
  after rebuilding half its dirty list leaves the element tree mixed-version;
  a finalize that panicked mid-sweep leaves some inactive elements
  slab-resident with disposed state. The next frame proceeds from that state;
  it is contained, reported, and usually recoverable (the dispose-panic test
  proves a clean recovery pump), but it is not a transaction rollback.
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

- One window's frame bug (including a `dispose` panic) no longer kills a
  multi-window process; siblings keep framing in the same pump. Pinned by
  born-red tests in `ui_realm.rs`'s `frame_failure_containment` module, each
  verified to fail with the specific seam disabled.
- Embedders get a typed, addressed failure feed with streak accounting;
  `tracing` alone is no longer the only witness.
- A permanently failing presentation costs a contained panic per fallback-pace
  wake until the embedder acts or the dirty state parks — accepted for this
  slice, escalation policy deferred as above.
- `FramePaintOutcome` stays a unit-variant enum; failure detail travels the
  report route, not the outcome value, so submit/telemetry matching is
  untouched.
