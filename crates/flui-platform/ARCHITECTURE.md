# flui-platform Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate, as required by [`docs/PORT.md`](../../docs/PORT.md) §Per-crate
`ARCHITECTURE.md` template. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

---

## Mapping decisions

### The winit backend delegates the whole keyboard event to `ui-events-winit`; Win32/AppKit keep hand-written tables

**Rule:** every native keyboard event this crate receives must be normalized
into the canonical `ui_events`/`keyboard-types` vocabulary (`Code`, `Key`,
`Location`) at the platform boundary — see `traits/input.rs`'s module doc.
`Code::Unidentified` must mean the backend genuinely could not identify the
physical key, never that a conversion table was incomplete (issue #1092).

**Choice:** the winit backend's `platforms/winit/events.rs::keyboard_event`
converts the whole `winit::event::KeyEvent` (code, logical key, location,
down/up state, `repeat`, and modifiers) through
`ui_events_winit::keyboard::from_winit_keyboard_event` (crates.io
`ui-events-winit`, the same bridge Masonry and Xilem use) instead of
hand-assembling any `KeyboardEvent` field itself; `platform.rs`'s
`ModifiersChanged` handler stores winit's raw `ModifiersState` rather than a
pre-converted `keyboard_types::Modifiers`, and each read site (pointer paths
and the keyboard path alike) converts through the crate's
`from_winit_modifier_state` at the point of use. The Win32 (`shared/keys.rs`)
and AppKit (`shared/keys_macos.rs`) backends keep their own hand-written
`Code`/`Key` tables, because no equivalent bridge crate exists for
`WM_KEYDOWN` scancodes or `NSEvent.keyCode` — those two native id spaces are
FLUI-specific translation work with no upstream crate to delegate to.

Location is also sourced differently than before this issue: winit's own
`KeyEvent.location: KeyLocation` field feeds `from_winit_keyboard_event`
directly, rather than being re-derived from the physical `Code` (the
pre-#1092 code matched a hand-picked list of `KeyCode` variants against
`Location::Numpad`/`Left`/`Right` — exactly the incomplete-table failure
mode this issue fixed for `code` too). There is no "does `Code::X` imply
`Location::Numpad`" property left to hold; winit already computes location
per-platform and exposes it, so nothing here re-derives it.

This asymmetry between backends is deliberate, not inconsistent: the rule is
"delegate to an audited bridge when one exists for this native surface," not
"every backend must look the same." `platforms/winit/events/keyboard_tests.rs`'s
`cross_backend_physical_key_agreement` test cross-checks a curated set of
physical keys against both hand-written tables so the two authored
translations and the delegated one cannot silently drift apart.

**Alternatives considered:**

- Complete the crate's own hand-written `winit::KeyCode → Code` table (kept
  as a dev-dependency oracle test against `ui-events-winit`, never shipped).
  Rejected: it is a second, unaudited copy of exactly the table the bridge
  crate already maintains and tests upstream — an ownership cost with no
  behavioral upside, and the class of bug this issue exists to fix (a
  hand-rolled table quietly falling behind the enum it mirrors).
- Delegate only the three field-level conversions (`from_winit_code`,
  `from_winit_key`, `from_winit_location`) and keep hand-assembling
  `KeyboardEvent`'s remaining fields (down/up state, `repeat`,
  `is_composing`, and a separately hand-written modifiers conversion) in
  `keyboard_event`. Rejected: `from_winit_keyboard_event` and
  `from_winit_modifier_state` already perform exactly that assembly, so a
  hand-written half is byte-identical duplicate logic — the same
  re-derived-seam risk the issue exists to close, just smaller.
- Vendor `ui-events-winit`'s tables directly into this crate (a `// PORT
  NOTE`-style copy). Rejected for the same reason as the first bullet, plus
  it would need re-syncing by hand on every winit/`ui-events` bump instead
  of `cargo update` doing it.

**Trade-off:** this crate's winit backend now tracks `ui-events-winit`'s
release cadence for keyboard fidelity — a regression or gap introduced
upstream reaches FLUI without a code review here. The `.github/dependabot.yml`
`ui-events-cohort` group (with `ui-events` itself) and the
`every_winit_keycode_maps_to_a_canonical_code` regression test are the
mitigations: a bump that drops coverage for a `KeyCode` variant fails CI
immediately rather than shipping silently — though that test can only pin
the dependency's own completeness, not a regression inside `keyboard_event`
itself, since `winit::event::KeyEvent` cannot be constructed outside winit
(`platform_specific` is `pub(crate)`) and so `keyboard_event` can never be
called directly from a test; see that test's doc for the full reasoning. If
the workspace's `winit` pin ever needs to move ahead of what
`ui-events-winit` supports (it pins `winit ^0.30`), the escape hatch is a
`[patch.crates-io]` pin at the upstream `ui-events-winit` git repo (plus
adding that repo to `deny.toml`'s `sources.allow-git`), not reviving a hand
table — see the dependency comment in `Cargo.toml`.

**One observable behavior change for already-working input:** a winit
`Key::Dead(_)` (a dead-key composition in progress) now reports
`Key::Named(NamedKey::Dead)` instead of `Key::Named(NamedKey::Unidentified)`
— `from_winit_key` distinguishes the two where the deleted hand table
collapsed both to `Unidentified`. Verified inert today:
`crates/flui-widgets/src/text/editable_text.rs`'s key handler matches
specific `NamedKey` variants and falls through everything else, `Dead`
included, to `Key::Named(_) => KeyEventResult::Ignored` — no consumer in the
workspace currently branches on `NamedKey::Dead` specifically (`rg
'NamedKey::Dead'` across `crates/` has no hits outside this record). Named
here so a future consumer that starts caring about dead-key state — an IME
composition indicator, for instance — knows this signal already exists on
the winit backend and does not need a new one.

**Further reading:** [`.rust-studio/specs/1092-winit-physical-key-map/survey.md`](../../.rust-studio/specs/1092-winit-physical-key-map/survey.md)
is the market/reference survey that motivated this decision (`ui-events-winit`
coverage measurement, Flutter's generated `PhysicalKeyboardKey` catalog as
the completeness precedent, and the three options evaluated before choosing
production delegation) — read it for context, not as the source of a fact;
figures cited in this entry are measured directly against this repository
and `ui-events-winit`'s own source, not against the survey.

**Replacement coverage:** `platforms/winit/events/keyboard_tests.rs`'s
`keyboard_conversion_tests` module (`every_winit_keycode_maps_to_a_canonical_code`
over all 194 winit 0.30.13 `KeyCode` variants — length- and
duplicate-checked against the winit source — plus the issue's
acceptance-criteria spot pairs and the location/logical-key spot checks) and
`cross_backend_physical_key_agreement` (winit vs. Win32 vs. AppKit on a
shared physical-key set). Before this change, `convert_physical_key`'s hand
table mapped 71 of winit 0.30.13's 194 `KeyCode` variants to `Code` (the
count measured at issue #1092's baseline) and `convert_winit_key` mapped 37 of 306
`NamedKey` variants to `Key`, falling through to `Unidentified` for the rest
in both cases.

### The surface-availability signal is a per-window `bool` callback, not a polled query

**Rule:** a backend that learns the GPU surface's native backing is about to go
away must say so through `PlatformWindow::on_surface_status_change` **before** the
handle behind any surface built from it dies, so the presentation can drop that
surface at the right moment. A backend that never emits the signal is harmless:
the surface is never released, which is the pre-#1146 behavior and the same shape
the visibility callback already documents. A backend that emits `false` and never
`true` is not harmless, because the presentation stays released, every later frame
is a skipped one, and the window paints nothing forever.

**Choice:** `on_surface_status_change(Box<dyn FnMut(bool) + Send>)`, a defaulted
no-op, following the `on_active_status_change`/`on_visibility_status_change`
family's shape: stored in `shared::handlers::WindowCallbacks`, dispatched through
the same FIFO and `CallbackLease`. `false` means "release the surface before this
callback returns"; `true` means "a surface valid for the handle available now must
exist", which the runner acts on unconditionally rather than by comparing it
against what it already holds — that statelessness is the point, since a `false`
can be lost and an "already present, do nothing" rule would then keep a surface
built from a dead window for the rest of the process's life. The parameter is a
`bool`, not an enum: a third state of the *surface* belongs in its own callback,
per ADR-0035's split of this family by signal rather than by arity. Android is the
only backend that emits it — `Pause` and `TerminateWindow` send `false`, `Resume`
and `InitWindow` send `true` — and `platforms/android/mod.rs`'s module doc carries
why that pair, and not `Pause` alone, is the correct mapping.

**Alternatives considered:**

- A polled query on the window (a `surface_is_available()` the runner reads each
  iteration). Rejected: a poll cannot carry this deadline. The drop has to happen
  while the native handle is still valid, which on Android means inside the
  `TerminateWindow` callback; a query consulted at the next loop iteration reports
  a window that is already gone.
- A `#[non_exhaustive] enum WindowSurfaceStatus` parameter instead of `bool`.
  Rejected for the parameter, not for the concept: it is the shape this crate uses
  for input and owner status, but every consumer of this signal branches on two
  states, and a callback's parameter type is a one-way door that changing later
  breaks for every registrant.
- Delegating the surface to the platform, the shape Leptos reaches on Android by
  hosting a Tauri v2 WebView. Rejected as unavailable rather than inferior: it
  requires giving up the adapter, the swapchain, and any ability to composite
  native-rendered content into the framework's own UI. It would not escape the
  problem either, since a wgpu surface inside a WebView is canvas-backed and
  wgpu's wasm backend does not recover from `webglcontextlost` (gfx-rs/wgpu#3679).

**Market lineage:** [bevy#6830](https://github.com/bevyengine/bevy/pull/6830), where
an `android-activity` maintainer states that only the top-level render target needs
recreating, not the wgpu stack; [bevy#9937](https://github.com/bevyengine/bevy/pull/9937),
which first despawned the window on suspend and was revised to "keep Bevy window,
only recreate Winit window and wgpu surface"; and
[winit#3786](https://github.com/rust-windowing/winit/pull/3786), which forwards
Android's `suspended()`/`resumed()` and is a named second emitter for this same
`false`/`true` pair, since winit is already a workspace dependency with a backend
here. Flutter asks for the same contract under another name:
[flutter#160933](https://github.com/flutter/flutter/issues/160933) requested an
`onSurfaceDestroying` that precedes the engine's `cleanup()`, having found the
after-the-fact callback fires once the native surface is gone. The spelling that
landed upstream is not that one: the API that exists is
`SurfaceProducer.onSurfaceCleanup`, the before-signal that replaced
`onSurfaceDestroyed` because another thread could still touch the surface after
the fact, so a grep for the requested name finds the request rather than the
implementation (ADR-0063 names both spellings). One divergence from
winit and bevy is deliberate: winit 0.30.13 keys on the window events alone —
`src/platform_impl/android/mod.rs` maps `MainEvent::InitWindow` to `Resumed`
and `MainEvent::TerminateWindow` to `Suspended`, while its `Start` and `Stop`
arms are `warn!("TODO: forward …")` stubs that forward nothing — and bevy
consumes those same two events. This backend keys on `Pause`/`Resume` *as
well as* `TerminateWindow`/`InitWindow`: the window pair is where the handle
actually dies and returns, and the lifecycle pair is where the framework is
told the app is going away, so emitting on both is the more conservative
choice, at the cost the trade-off below books.

**Trade-off:** releasing on `Pause` unconditionally makes the cost per-edge rather
than per-defect. Android maps `false` to both `Pause` and `TerminateWindow` and
`true` to both `Resume` and `InitWindow`, and the seam attempts a recreation on
every `true`, so every `Pause`/`Resume` pair pays a release, a create/configure,
a lane generation mint and a full repaint — including the pauses where the native
window was never destroyed, such as a dialog over the activity or a multi-window
deactivation. A second `true` over a surface still held on the same window is not
paid twice but refused: Vulkan allows one `VkSurfaceKHR` per `ANativeWindow`, so
the second create fails, the held surface stays, and no second mint or repaint
follows (ADR-0063 decision 6 books what that refusal costs under the vendored
`wgpu-hal`, and marks the ordering that could produce it as unverified). Dropping
a configured `wgpu::Surface` is
not cheap either: it reaches `vkDeviceWaitIdle` before destroying anything, which is
an unbounded wait on whichever thread runs the callback. That is why the release is
a synchronous, surface-only operation that performs no lane handoff, no probe and no
submit, and why the `TerminateWindow` release that follows `Pause` in the ordinary
cycle is an idempotent no-op — while its caller holds the raster lane's guard across
it by design, so the wait does happen with the lane held.

**Replacement coverage:** `platforms/headless/platform.rs`'s
`test_on_surface_status_change` is the wire test — registration goes through the
`PlatformWindow` trait method, the `simulate_surface_status` affordance drives the
platform's own dispatch, and the closure asserts both edges — because a backend left
out of `impl_window_callback_setters!` still compiles the registration and silently
drops it, which a direct `dispatch_surface_status_change` test cannot see.
`shared/handlers.rs`'s `surface_status_change_reaches_its_callback_with_the_parameter`
and `surface_status_change_cleared_from_inside_is_not_resurrected` cover the slot's
FIFO and lease behavior. The Android arms themselves are **type-checked by
`just cross-typecheck` and executed by nothing**: no gate on this host runs the
Android backend, so their mapping is an inference from `android-activity`'s
documented contract, recorded rather than measured.
