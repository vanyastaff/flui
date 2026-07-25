# ADR-0032: IME cursor-area single-rect reduction

*The platform IME candidate window follows the caret: `EditableText` runs a self-rescheduling post-frame loop that reads the caret's current global rect and forwards it through `TextInputHandle::set_cursor_area` whenever it changes — a winit single-rect reduction of Flutter's transform+local-rect protocol.*

---

- **Status:** Accepted (amended by [ADR-0033](ADR-0033-composing-region-rendering.md) — upgrades the caret-only fallback to prefer the composing region's rect)
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-objects/src/text/editable.rs` (`RenderEditable::caret_local_rect`); `crates/flui-widgets/src/text/editable_text.rs` (`CursorAreaLoop`, the inner `SubtreeAnchor`); `crates/flui-interaction/src/text_input.rs` (`TextInputHandle::set_cursor_area`); `crates/flui-app/src/app/{binding.rs,ui_realm.rs}` (`AppBinding::set_ime_cursor_area`, `TextInputPlatformBridge::set_cursor_area`); `crates/flui-platform/src/traits/text_input.rs` (doc only — the trait method already existed)
- **Related:** ADR-0030 (`PlatformTextInput` — the capability this loop finally drives); ADR-0021 (`post_frame_handle` — the frame-capability rule this loop's acquisition follows); ADR-0027 (`UiRealm`/realm-scoped ownership — per-window routing deferral below)

---

> **Ownership clarified (2026-07-23):** [ADR-0037](ADR-0037-presentation-ownership-domains.md) supersedes this ADR's `AppBinding`/`TextInputPlatformBridge` plumbing and single-current-window routing. The single global rect reduction, per-attach tracking loop, coordinate-space contract, and composing/caret geometry remain accepted.

## Context

ADR-0030 landed `PlatformTextInput::set_ime_cursor_area` and the headless `FakeTextInput` recorder, but named the caret-to-global-coordinates path a named deferral: nothing in the widget tree called it. `EditableText`'s own module doc stated plainly: *"nothing in this widget tree calls it yet, so the platform IME candidate window does not follow the caret."* This ADR closes that gap.

Flutter's reference (`widgets/editable_text.dart` @ 3.44.0) drives this with three cooperating pieces: `_updateSizeAndTransform` (recomputes the field's transform to the view root), `_updateCaretRectIfNeeded` (recomputes the local caret rect, falling back to it when no composing-range rect exists — see `_updateComposingRectIfNeeded`), and `_schedulePeriodicPostFrameCallbacks` (the self-rescheduling post-frame loop that ties the two together and forwards both channels — transform and local rect — to the platform separately via `TextInputConnection.setEditableSizeAndTransform`/`setCaretRect`).

## Decision

### 1. Winit single-rect reduction (documented divergence)

Flutter's platform channel carries **two** separate quantities: the field's transform-to-view-root, and the caret's local rect; the native embedder composes them. Winit's `Window::set_ime_cursor_area` accepts **one** already-composed rect in window-root-space logical pixels. FLUI therefore composes `transform × local_caret_rect` on the **framework** side, in `CursorAreaLoop::global_caret_rect`, and sends the single resulting rect. This is a deliberate, documented reduction, not an accidental simplification: the *observable* contract (the candidate window tracks the caret through scrolling, transforms, and ancestor translation) is preserved; only the wire shape (one rect vs. two channels) differs, because that is the surface winit itself exposes. A native Android/iOS embedder that wants the two-channel protocol back is free to re-decompose the single rect (it already has the platform's own transform-to-root) — nothing here forecloses that.

### 2. The post-frame self-rescheduling loop, per attach

`CursorAreaLoop` is a small `Clone` bundle (`PostFrameHandle`, `Option<Arc<RwLock<PipelineOwner>>>`, the inner `SubtreeAnchor`, `TextInputHandle`, and two per-attach cells — see §3) whose `fire` method: checks `alive`; if alive, reads the caret's current global rect and sends it through `TextInputHandle::set_cursor_area` when it differs from the last send; then unconditionally reschedules itself via `PostFrameHandle::schedule_local`. One instance starts on IME attach (focus gain) and dies on blur or dispose.

This mirrors Flutter's `_schedulePeriodicPostFrameCallbacks` cadence exactly: it runs once per completed frame, and is dormant (schedules nothing further) once nothing reschedules it. `PostFrameHandle` is acquired in `init_state`, beside the existing `text_input_handle()` acquisition — the same lifecycle-only frame-capability rule ADR-0021 established for `rebuild_handle()`/`post_frame_handle()`: never acquired inside `build`/`perform_layout`/`paint`, always in `init_state`/`did_change_dependencies`, then stored and fired later. `scripts/check-frame-capability-scope.sh` passes against the acquisition site unchanged — `init_state` is already the sanctioned place; no new token needed.

### 3. Per-attach alive-flag and per-attach last-sent cache

Two invariants, both load-bearing, both caught by adversarial review before this shipped:

**A shared alive-flag across attaches double-loops.** If one `Rc<Cell<bool>>` lived on the field for its whole lifetime (rather than a fresh one per attach), a blur immediately followed by a refocus — both inside the same active-lane window, with no intervening frame — would leave the *old* attach's already-queued firing still `alive == true` when the new attach's fresh loop also starts. Both loops would then fire on the same next frame: the stale one because nothing told it to stop, the new one because it just started. A fresh `Rc<Cell<bool>>` per attach fixes this structurally: detach/dispose flips *that specific attach's* cell false and drops the reference; a stale queued firing from a previous attach reads `false` and dies silently, no matter how a later attach's own state looks.

**A shared last-sent cache across attaches suppresses the first send of a new session.** If `last_sent` lived on the field permanently, a blur→refocus at an *unchanged* caret position (the common case — focus moved elsewhere and came right back with no edit in between) would see the cache still holding the old rect, conclude "nothing changed", and never resend — leaving a new IME session with no cursor-area rect until the caret actually moves. A fresh `Rc<Cell<Option<Bounds<Pixels>>>>` per attach fixes this: a brand-new IME session always gets its first rect, even at a caret position identical to the one the previous session last reported.

Stated as the design invariant, not the review history that found it: **every IME attach is a fresh tracking session, structurally independent of whatever session preceded it** — no shared mutable state crosses an attach boundary.

`ImeEvent::Enabled` additionally clears the *current* attach's `last_sent` (wired into the existing IME event callback, alongside `apply_ime_event`): the backend may restart the IME session without a focus change, and the same "new session, first send is never suppressed" guarantee should hold there too.

### 4. Visibility-independent caret geometry

`RenderEditable::caret_local_rect()` composes from `caret_offset` (computed in `perform_layout`) + caret width/height, regardless of `show_caret`. This is deliberate: composition is exactly the state in which the candidate window must track the caret, and it can coincide with a hidden-caret-while-composing state this substrate does not yet model (see `RenderEditable`'s "Deferred" section — the controller collapses the caret to the composing region's end instead of hiding it). `paint` composes its own painted caret rect from the *same* accessor, keeping its own `show_caret` gate — one fact (the caret's geometry), one place it is computed, two independent consumers (paint decides whether to draw it; the cursor-area loop always reads it).

### 5. Coordinate-space contract

`Bounds<Pixels>` at every seam in this path — `TextInputHandle::set_cursor_area`, `AppBinding::set_ime_cursor_area`, `PlatformTextInput::set_ime_cursor_area` — is **window-root-space logical pixels**, matching `PlatformWindow::bounds`'s own convention. DPI conversion is the backend's job, not the framework's: the winit implementation (`WinitTextInput::set_ime_cursor_area`, pre-existing since ADR-0030) already converts to `LogicalPosition`/`LogicalSize` at the boundary, which winit itself scales by the OS-reported scale factor. `Bounds<Pixels>` never carries device pixels through this path.

`flui_types::geometry::{Bounds, Pixels}` (re-exported from `flui-geometry`) crosses into `flui-interaction`'s `TextInputHandle` without a new crate edge: `flui-interaction` already depends on `flui-types`, which re-exports `flui_geometry as geometry` — the same pattern `flui-platform`'s `PlatformTextInput` trait already uses.

### 6. The inner anchor: a second, structural `SubtreeAnchor` at the editable

Before this change, the chain from the field's outer `RenderSubtreeAnchor` (published for reading-order traversal, unrelated to IME) down to `RenderEditable` passed through an `AnimatedBuilder` — zero-offset by convention, never structurally guaranteed. Reusing the outer anchor for cursor-area tracking would mean `transform_to` walks through that whole intervening subtree on every frame, correct today only because nothing in between happens to apply an offset.

This ADR adds a **second, inner** `SubtreeAnchor`/`AnchoredBox` wrapped directly around `EditableTextRenderView` in `build_field_view` — the transform this loop reads starts exactly at the editable's own anchor, structurally, not by convention. `CursorAreaLoop::global_caret_rect` then descends one level (`RenderTree::children(anchor_id).first()`) to reach the concrete `RenderEditable` and read its local caret rect, before applying `transform_to(anchor_id, root_id)`.

### 7. Transient-`None` is a skip, never a stop

`global_caret_rect` returns `Option<Bounds<Pixels>>`; every internal `?` (anchor unmounted mid-rebuild, no pipeline owner, no root, transform unavailable) collapses to `None`. `CursorAreaLoop::fire` treats `None` as "nothing to send this firing" and reschedules anyway — only `alive == false` ever stops the loop. A frame where the anchor is transiently unreachable (a rebuild boundary) must not be mistaken for "this field is gone"; the loop must still be running on the frame after that, when the anchor is reachable again.

### 8. Diagnostics: every scheduling failure is a `tracing::warn!`

`CursorAreaLoop::schedule`'s `PostFrameHandle::schedule_local` call can fail (`InactiveLane`/`WrongThread`/`LaneClosed`/`NoLocalLane`) — every failure is logged via `tracing::warn!`, including the very first schedule at attach time. A loop that silently never starts is a candidate window permanently stuck at `(0, 0)` with no signal that anything is wrong; this is the one failure mode this feature cannot afford to leave undiagnosed.

## Named deferrals

- **Composing-range cursor-area rect.** The loop always reports the collapsed caret's rect, never a per-glyph composing-range rect. Flutter's own `_updateComposingRectIfNeeded` falls back to the caret rect when no composing-range geometry is available (`editable_text.dart` @ 3.44.0) — this substrate uses that same fallback unconditionally, since it has no composing-range geometry to fall back *from* yet (see `RenderEditable`'s "Deferred" section on the hidden-caret-while-composing gap this shares a root cause with).
- **Per-window routing.** `TextInputRegistry` already carries the attaching window's opaque handle but routes every dispatched event to the single global active client regardless of origin window (ADR-0030 §6, unchanged by this ADR). The cursor-area send follows the same single-window assumption; real multi-window routing is deferred to when `UiRealm`'s realm-scoped ownership (ADR-0027) reaches this registry.
- **Internal editable scrolling.** `RenderEditable::caret_local_rect` is viewport-relative by construction (its own module doc states this); when internal scrolling lands, the accessor stays viewport-relative rather than reporting full-text-content-space geometry — a caller that needs the latter will need a separate accessor at that point.
- **A dedicated transient-`None` unit test.** Forcing `global_caret_rect` to observe the inner anchor mid-unmount deterministically would require reaching into the pipeline mid-rebuild, which the widget-level harness has no cheap hook for. The *branch* (`if let Some(rect) = ... { send }; self.schedule()` — the reschedule is unconditional, not gated on the `Some` arm) is exercised structurally by every other test's very first frame (before the tree has settled), and its shape is simple enough that the blur/dispose tests (which *do* stop the reschedule, via `alive`) are the meaningful contrast case.
- **A `flui-app`-level, full-widget-tree end-to-end geometry test.** *Resolved after this ADR.* `AppBinding::attach_root_widget`'s `RootRenderElement` bootstrap previously failed to connect a mounted subtree's own render root back to its synthetic `RenderViewAdapter` node — the parent-link direction only, confirmed independent of this feature (a bare `LeafView` showed the identical disconnect) — because `RootRenderElement`'s `ElementBase::render_id` fell through to the trait default (`None`) instead of its own render id, which corrupted `ElementTree::reorder_render_children_after_build`'s parent-sync pass for the root's first render child. Fixed by giving `RootRenderElement` a real `render_id()` trait override and consolidating the render-tree's set-parent-and-add-child call sites behind one `RenderTree::adopt_child` primitive. `transform_to` from a real mounted `EditableText` up to `root_id()` now succeeds through the standard `AppBinding` bootstrap; see `transform_to_resolves_through_the_root_hop_after_standard_bootstrap` in `flui-app/src/app/binding.rs`. The `flui-app`-level test already in this change (`set_ime_cursor_area_reaches_the_active_windows_platform_capability`) still deliberately exercises the `AppBinding -> TextInputPlatformBridge -> PlatformTextInput` plumbing directly rather than a real widget tree — not because the connectivity gap forced it to, but because that's what the test is about; the geometry/dedupe/lifecycle coverage stays in `flui-widgets`' own test harness.

## Evidence

- Scheduler pre-test (built first, per this ADR's own design gate): `cargo test -p flui-binding --test self_rescheduling_local_post_frame` — a self-rescheduling `PostFrameHandle::schedule_local` callback, driven through `HeadlessBinding::pump_frame` (not a bare `lane.enter(|| scheduler.execute_frame())`), fires exactly once per pumped frame. Passed on first construction — the design's foundation held.
- `cargo nextest run -p flui-objects -p flui-interaction -p flui-app -p flui-widgets -p flui-binding` (flui-platform excluded per `AGENTS.md`'s documented gap): 2432 passed, 0 failed.
- `cargo clippy -p flui-objects -p flui-interaction -p flui-platform -p flui-app -p flui-widgets -p flui-binding --all-targets -- -D warnings` and `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `just fmt-check port-check inventory-check`, `taplo fmt --check`, `typos`: all clean.
- `bash scripts/check-frame-capability-scope.sh crates/flui-widgets/src/text/editable_text.rs`: clean — the `post_frame_handle()` acquisition site in `init_state` is within the sanctioned scope.
- Red-checks run (not merely asserted, mutant applied then reverted for each): dropping the alive-flag guard in the blur branch fails `loop_stops_sending_after_blur`; skipping `transform_rect` (sending the untransformed local rect) fails the ancestor-translation assertion in `focusing_sends_the_exact_caret_rect_including_ancestor_padding`; sharing `last_sent` across attaches fails the refocus-at-same-position half of `dedupes_unchanged_frames_and_resends_after_a_refocus_at_the_same_position`; dropping the `Enabled`-clears-`last_sent` line fails `ime_enabled_event_clears_the_dedupe_cache_and_forces_a_resend` (`left: 1, right: 2` — the resend never happens); the visibility-independence contract is red-checked directly at the `RenderEditable` harness level (`harness_editable_caret_local_rect_is_visibility_independent`, paired with `harness_editable_hidden_caret_paints_no_caret_rect` as the paint-gating contrast).
- **The dispose-stop red-check needed a second pass.** The first version of `loop_stops_rescheduling_after_dispose_while_still_focused` asserted only "no new `cursor_area_calls` after unmount" — dropping the dispose kill-switch does NOT fail that assertion, because `RenderSubtreeAnchor::detach` clears `inner_anchor` on unmount regardless, so `global_caret_rect` returns `None` and a zombie loop (one that kept rescheduling itself forever without ever stopping) looks exactly as send-silent as a correctly-stopped one. The fix adds a test-only thread-local counter (`cursor_area_reschedule_count`) that counts every `CursorAreaLoop::schedule` registration, and asserts THAT holds steady after the dispose frame settles. Re-run with the dispose kill-switch commented out: `left: 5, right: 3` — the count kept climbing across the two post-dispose ticks instead of holding steady, confirming the strengthened test actually distinguishes a stopped loop from a leaking one.

## What is deferred

- Composing-range cursor-area rect (caret fallback used unconditionally — see above).
- Per-window IME event/cursor-area routing (ADR-0030 §6, ADR-0027 territory).
- Internal editable scrolling's effect on `caret_local_rect`'s coordinate space.
- Real-IME visual confirmation on a live winit window — this change is verified headlessly (exact `Bounds` assertions against a recording harness); no manual on-screen check of a real platform candidate window has been performed. Same manual-verification caveat the IME exit criterion (`docs/ROADMAP.md` App.1) already carries.
