# AGENTS.md — flui-testing

> The workspace's **test-support package**. Read this before writing a test that
> drives a frame, and before adding a test-only API anywhere else.

## What this crate is

The deterministic, non-singleton headless runtime: `HeadlessBinding` owns a
virtual `ManualClock`, a clock-bound `GestureBinding`, an animation-controller
registry, its own `UpdateScheduler`, and optionally a mounted tree. It is the
FLUI equivalent of Flutter's `TestWidgetsFlutterBinding` — with the crucial
difference that it is an ordinary value, not a process global, so any number
can exist and run in parallel.

Three surfaces, each owning one thing:

| Module | Owns |
|---|---|
| root (`lib.rs`) | `HeadlessBinding`: `pump_frame`, `dispatch_pointer`, `swap_root_view`, the multi-presentation clock registry (`install_presentation_clock` / `pump_presentation` / `pump_all`) |
| `bootstrap` | `mount_root` — the one canonical way to get from a root `View` to mounted, rooted, laid-out owners |
| `replay` | `PointerScript` + `HeadlessBinding::replay` — scripted gestures replayed on the virtual clock |
| `a11y` | `A11yTree` / `A11yQuery` — query the assembled semantics tree by role, through the same `flui_semantics::tree_to_update` a platform adapter uses |
| `log_capture` | `capture` — race-free `tracing` capture for tests that assert on what was logged |

## The rule this crate exists to enforce

**A test-only API belongs here, not behind a `testing` feature on a shipped
crate.** Where that is impossible, the reason is layering, and it must be
written down:

- `flui_widgets::testing::LaidOut` (the canonical *widget* harness) stays in
  `flui-widgets` because the presentation scopes it mounts — `FocusRoot`,
  `VsyncScope`, `GestureArenaScope` — are widgets, and **flui-testing must
  never depend on the widget catalog** (a forbidden edge in
  `docs/workspace-layers.toml`). It is built on `mount_root`, so the ordering
  is still owned here.
- `flui-rendering` / `flui-layer` / `flui-painting` / `flui-interaction` keep
  their own `testing` features: those harnesses drive their own crate's
  machinery with no frame, no tree, and no binding.

## `mount_root` — do not hand-roll a bootstrap

`HeadlessBinding::with_tree` deliberately does no bootstrap; it takes owners
already mounted, rooted, and laid out. Getting them there is an eight-step
sequence whose ordering is load-bearing at nearly every step. It used to be
copied per harness — eight times — and the copies drifted in ways a green suite could not see
— one bootstrapped with a bare `PipelineOwner::run_frame` instead of the
layout↔build fixpoint, so every `SliverAppBar` delegate child it captured was
unbuilt; none ran the lazy-sliver service pass.

`mount_root`'s contract is the property those copies kept losing:

> **The bootstrap frame is the same frame `pump_frame` runs** — same
> `run_frame_with_layout_builders` fixpoint, same `service_child_requests`,
> same owner scope.

So: mount through it. If a harness needs something it does not offer, extend
`MountOptions` rather than re-deriving the sequence beside it.

## Gotchas

- **The binding is `!Send + !Sync` and owner-affine.** Create, drive, and drop
  it on one thread. Lifecycle work done through raw owner access must be
  wrapped in `enter_owner_scope` so callbacks see the same active interaction
  lane they see during a frame.
- **Install build capabilities before the mount, never after.** A
  `ViewState::init_state` runs inside the mount `build_scope` and already asks
  for them; a `FutureBuilder`/`StreamBuilder` that subscribes there silently
  never polls if the async driver arrives late. `mount_root` does this for you.
- **Replay spends virtual time.** `binding.replay(&script)` pumps a frame per
  gap, so the clock advances by the script's own duration. That is the point —
  a script's timing is what decides a deadline-driven recognizer's verdict.
  Never reintroduce a wall clock here: a recording stamped `Instant::now()` is
  unreproducible by construction, which is exactly why the predecessor
  infrastructure in `flui-interaction` was deleted.
- **A contact's route is captured once, on its Down.** `replay`'s hit-test hook
  fires once per *contact*, not once per event — the real `GestureBinding`
  protocol. Give each contact its own `PointerId`; a platform never recycles an
  id into a still-tracked gesture.
- **Never capture `tracing` with `subscriber::with_default`.** `tracing`
  computes a callsite's interest **once**, on whichever thread reaches it
  first, and caches it process-globally
  (`tracing_core::callsite::Rebuilder::JustOne` → `dispatcher::get_default`).
  A thread-local subscriber therefore loses every event from a callsite some
  other test reached first — cached as `Interest::never()` and silent for the
  rest of the process. That is not a hypothesis: it made a `flui-widgets`
  parity test fail 4 times in 25 runs of its binary while passing 60/60 in
  isolation. Use [`log_capture::capture`], which disarms that cache with
  registered-but-never-default sentinel dispatchers and then installs its own
  subscriber only thread-locally — so it does **not** take the process-global
  default slot, and a binary's own logging subscriber keeps working. Crates at
  or below `flui-interaction` cannot depend on this one and keep their own
  technique — and their own caveat.
- **`#![deny(missing_docs)]`.** Every public item, including test-facing ones.
- **No process-global state, with one named exception.** Every test constructs
  its own binding, so nothing in this crate needs a test lock. If you find
  yourself wanting one, check `docs/runtime-contract.toml`'s ambient-reach
  ratchet — the resource you are racing on is named there, and the lock belongs
  beside that test. The exception is `log_capture`'s two sentinel dispatchers,
  which are process-wide by necessity (`tracing`'s interest cache is), but hold
  no state, receive no events, and never occupy the global default slot.

## Dependency rule

Runtime and framework crates may take a **development** edge into this crate
and nothing more; a normal edge would link the test driver into production
binaries. One sanctioned exception, recorded in
`docs/workspace-layers.toml` and enforced by `just inventory-check`:
`flui-widgets` declares an *optional* edge activated solely by its `testing`
feature. The reverse edge is forbidden outright.

This crate's own `flui-interaction` dependency carries `features = ["testing"]`
on the normal edge so `replay` can build events through the canonical
`make_*_event_for_id` constructors. That travels nowhere new — flui-testing is
itself test support, and flui-widgets' `testing` feature already enables it.

## See also

- [`docs/testing.md`](../../docs/testing.md) — map of the whole testing layer, tier by tier
- [`crates/flui-rendering/docs/TESTING.md`](../flui-rendering/docs/TESTING.md) — `RenderTester` / `Probe`, the tier below this one
- [`README.md`](README.md) — crate overview
