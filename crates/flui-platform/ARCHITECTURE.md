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

### AppKit's frame source stays `drawRect:`, and its re-arm crosses one owner-lane turn

**Rule:** a display-driven backend's produce signal must be able to re-arm itself
from inside the frame it just produced. AppKit discards a `setNeedsDisplay:`
issued while it is displaying the view, so a backend whose only frame source is
`drawRect:` and whose re-arm happens there is circular — one frame, no wake, no
frame. A re-arm AppKit silently drops is worse than a missing feature: the pump
stops with no error, no log line, and a perfectly healthy-looking request behind
it.

**Choice:** `drawRect:` stays the frame source. It is the signal AppKit actually
delivers — the view is live and the frame carries its own dirty rect — and the
produce contract the engine consumes is dispatched from it (`view.rs::draw_rect`
→ `WindowCallbacks::dispatch_request_frame`). What changed is the re-arm.
`platforms/macos/display_pass.rs` marks the thread for the duration of the frame
AppKit asked for (an RAII guard, nesting-safe, thread-local), and
`window.rs::dispatch_redraw_request` — the single decision point `request_redraw`
calls — sends the `setNeedsDisplay:` inline at any other moment, but hands it to
the next owner-lane turn when the caller is inside that pass. This is ADR-0039
§4(c) realised on AppKit: the frame transaction is a region the drain gate is
closed for, and a wake arriving while it is closed defers rather than draining.
The deferred turn re-checks the marker before messaging AppKit and re-defers,
bounded at `MAX_DEFER_HOPS`, because a display pass that opens a nested run loop
services the same lane from inside itself — which the deferral would otherwise
reproduce silently.

Measured on macOS 15.7 (Darwin 24.6), a 100 Hz panel, isolated probe arms of 4 s
each, every arm with the window fully visible (`occlusionState` carries the
visible bit):

| re-arm issued from inside `drawRect:` | draws over 4 s |
|---|---|
| `[view setNeedsDisplay:YES]` | 1 |
| nothing at all (control) | 1 |
| `[view.layer setNeedsDisplay]` | 1 |
| `setNeedsDisplay:` deferred to the next main-queue turn | 396 |

**Alternatives considered:**

- Re-arm through the backing layer (`[view.layer setNeedsDisplay]`). Rejected by
  measurement, not by taste: the layer's `needsDisplay` reads `YES` right after
  the call while the view is still never redisplayed — the flag that survives is
  not the flag AppKit acts on, so it is not a usable signal to test against
  either.
- Move the frame off the display pass entirely (`CADisplayLink`, i.e.
  `NSView.displayLink(target:selector:)` on macOS 14+, or `CVDisplayLink`).
  Deferred, not rejected: ADR-0044 names `CADisplayLink` as macOS's intended
  produce signal, and ADR-0045 (Proposed) keeps macOS on `RasterMode::Inline`
  until the locked `wgpu-hal`'s Metal path stops messaging `NSView`/`NSWindow`
  from the calling thread. Until that lands, the frame has to be able to re-arm
  from where it runs.
- A diagnostic redraw tick in the backend (an `NSTimer`/thread poke behind a
  `FLUI_MACOS_REDRAW_POKE_MS`-style knob, which was implemented while diagnosing
  this). Deleted, not deferred: it contradicts ADR-0058 — the platform paces
  production, a sleep never does — and it pre-empts the `PlatformProxy` redraw
  verb ADR-0045 decision 5 scopes to #559 by shipping a second, private frame
  source. The primer that starts a measurement belongs to the probe, which is
  where it now lives (`examples/frame_pump_probe.rs`).

**Market lineage:** nobody shipping a Rust macOS UI builds its frame loop this
way. gpui-ce (`crates/gpui_macos/src/display_link.rs`, `main` @ `8e36ac0`,
2026-09-15) and upstream Zed (the same crate, `main` @ `c24e309`, 2026-09-17)
both pace on `CVDisplayLink` — one immortal link per `CGDirectDisplayID` in a
`static Mutex<Registry>`, windows subscribing rather than owning, driven by
`NSWindowOcclusionState::Visible` as the master start/stop switch and re-armed on
screen change — and render **directly** from the link's callback on the main
queue, with `displayLayer:` as the second, AppKit-initiated entry; both paths
stop the link, render, and restart it. `CADisplayLink` /
`NSView.displayLink(target:selector:)` appears nowhere in either repository, and
**neither calls `setNeedsDisplay:` at all** (the sole hit is
`setNeedsDisplayOnBoundsChange(true)` on the `CAMetalLayer`), nor has any
`drawRect` path (`0` hits). That last point is the one worth keeping straight:
it is evidence that the path this backend was on is a path the market left, not
evidence about AppKit's behaviour — the discarded in-pass `setNeedsDisplay:` is
this entry's own four-arm measurement, and the market never reaches the
question. Their link is also leaked by design, because `CVDisplayLinkStop` is
asynchronous and releasing raced its io thread (upstream segfaults #32116 /
ZED-7XR) — the wart `CADisplayLink` does not have, which reads as support for
ADR-0044's choice with the caveat that the market sits on the API macOS 15
deprecates. Their deferral is a different one and not to be confused with the
one above: upstream Zed PR #3592 (`f12510b8`, 2023-12-11) defers *drawing* until
CoreAnimation calls `displayLayer:`, i.e. defers to a platform signal rather than
around a discarded request; core gpui's `deferring re-entrant window draw
request` is its own comment's description of a Windows-only re-entrancy case.
One behaviour of theirs this backend still lacks is the fix for the stall that
keeps a cold start from beginning: a single synchronous frame on re-activation,
gated on `activated_least_once`.

Basis for the above: source reading at the two commits named (clone grep plus
`gh search code`, which agree), not by running either project — the clones are
shallow, so an older revision containing a `setNeedsDisplay:` call is not ruled
out, and no Apple documentation was consulted for the API-deprecation claim.

**Trade-off:** this deferral is AppKit-local, and the same defect has a second
instance on another backend with the opposite sign. Web's `request_redraw`
dispatches the frame immediately and `WindowCallbacks::drain_events` drains
synchronously until the queue is empty, so a frame callback that re-arms recurses
*within* the originating call rather than reaching a browser frame turn — measured
at 4 frames inside one JS-to-Rust call, with no RAF turn, GPU setup or renderer
involved (issue #1047, open). AppKit drops the re-arm that never crosses out of
the display pass; web honours one that never crosses into a future turn. Both are
the same contract failing: **a redraw request issued from inside a frame request
must be deferred to the platform's next frame turn, not serviced in place.** The
macOS half is correct now and the web half is not, which is the cost this entry
books — the concept lives in `platforms/macos/display_pass.rs` rather than in a
shared place both backends could hold, so a third backend writing a frame loop
has to rediscover it. Lifting it to the `WindowCallbacks`/`request_redraw` layer
is the obvious shape and is not done here: it would edit the web backend, which
no gate on this host executes, and §Definition-of-Done forbids claiming a fix on
a path nothing runs.

**Replacement coverage:** three tiers, because no single one reaches the claim.
`display_pass.rs`'s four tests pin the marker (set inside a pass, restored on
drop, nesting, thread grain). Two always-run window tests pin the decision
without an NSWindow
(`a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline` and its
complement — deleting the deferral branch fails the first, checked by mutation:
with `dispatch_redraw_request`'s body replaced by an unconditional inline send,
that test fails and the other five in scope still pass), because a bare test
process cannot construct an NSWindow at all. The behavioural end-to-end pin is
`examples/frame_pump_probe.rs` (`just macos-frame-pump`): a real visible window
on the real AppKit run loop, whose frame callback re-arms the way the engine's
frame does, behind a primer that stops at the first frame so the measurement
cannot be explained by it. Measured 2026-09-17: 301 frames in 3.010 s (100.0 fps
on the 100 Hz panel) with the deferral, against 1 frame total and 0 in the
measurement window with the deferral branch removed. Both runs' marker lines are
recorded in the plan beside their probe
(`.rust-studio/specs/macos-native-vsync-pacing-evidence/plan.md` §4); the raw
run logs are not tracked, since `.gitignore` excludes `*.log` repo-wide.

### macOS reports its display period as the current mode's rate, cached on the window

**Rule:** a backend that can tell an embedder the display's refresh period should
report the rate of the mode that display is *in* — not a capability of the panel
and not a generic default. The value is a floor for pacing ("never produce faster
than this", `PlatformWindow::refresh_period`), so a rate that is too high paces
against a cadence the display is not running, and a rate that cannot be
determined must be reported as unknown rather than guessed at.

**Choice:** `MacOSWindow::refresh_period` answers from
`CGDisplayModeGetRefreshRate` over the display's *current* mode
(`platforms/macos/display.rs::refresh_period_for_screen`), reached from the
window's screen via `NSScreenNumber` — the same display id `display.rs` already
reads for its bounds. A rate of 0 is `None`: CoreGraphics reports 0 for modes it
cannot describe and some displays do, and a 0 turned into a period is infinite.
The read is **cached** in `MacOSWindowState` beside `scale_factor`, refreshed in
the handler that already updates the scale
(`handle_backing_properties_changed`, reached from `handle_screen_changed`) and
seeded at construction, which is safe there because the window is created at the
origin — a point on a screen — so `-[NSWindow screen]` answers rather than
returning nil. Caching is the one place this diverges from the winit
implementation it otherwise mirrors: winit queries the monitor per call, but this
backend's AppKit traffic belongs to the owner lane while `refresh_period` is a
plain `&self` accessor any thread may call, so the query happens on the lane and
the accessor reads the lock.

**Alternatives considered:**
- `NSScreen.maximumFramesPerSecond` — rejected twice over. It is macOS 12+, above
  this crate's 11.0 deployment floor, so an unguarded call would crash on 11;
  and it reports the panel's hardware *maximum*, so it over-reports on a
  ProMotion display running a 60 Hz mode — exactly the too-high rate the rule
  above forbids. winit does not use it.
- Querying per call, the way winit does — rejected here for lane safety, above.
- A `CVDisplayLink` fallback for displays whose mode reports 0 (winit has one) —
  **deferred, not dropped**: it needs a `core-video` dependency, and CVDisplayLink
  is this backend's intended produce signal anyway (ADR-0044), so it belongs with
  that tick source rather than ahead of it. Until then `None` is honest and
  observable: `flui-app`'s runner logs `reported=false` and paces against its
  documented default.

**Market lineage:** winit 0.30.13's macOS `MonitorHandle::refresh_rate_millihertz`
(`src/platform_impl/macos/monitor.rs:260`) reads `CGDisplayCopyDisplayMode` →
`CGDisplayModeGetRefreshRate`, returns it when it is `> 0.0`, and only then falls
back to `CVDisplayLinkGetNominalOutputVideoRefreshPeriod`. That is the source
read here and the reason for the `> 0` guard. Read at the version pinned in this
workspace's `Cargo.lock`, not recalled.

**Trade-off:** the cached value follows the window across screens and backing
changes, and is re-read whenever a resize reaches the runner. It does **not**
follow a mode change that leaves the window where it is (a user switching refresh
rate in System Settings): this backend observes no screen-parameters
notification, so such a change is picked up on the next move or resize rather
than immediately. Named rather than assumed away — it is unobservable on the
single fixed-mode display this was measured on.

**Replacement coverage:** the arithmetic is free-standing and AppKit-free
(`period_from_refresh_hz`), with three always-run tests: the 100 Hz → 10 ms
conversion, a fractional rate that must not round to its neighbour, and the
non-cadence inputs (zero, negative, NaN, ±infinity) that must be unknown rather
than an infinite or panicking period — removing the guard makes that third test
fail inside `Duration::from_secs_f64`, checked by mutation. The live path is
pinned behaviourally by the same bundled probe that pins the frame pump
(`just macos-frame-pump`), which reports `FRAME_PUMP_PROBE_REFRESH_PERIOD` from
the real backend on the real display: measured 2026-09-17 as `period_us=10000
hz=100.0`, matching the independent AppKit probe's `CGDisplayModeGetRefreshRate`
of 100.000 Hz on the same panel. Like every macOS-gated test here, these run
locally only — CI has no macOS test job and `cross-typecheck` is lint-only.

### AppKit's input method is a route for a `keyDown:`, not a second producer of one

**Decision.** `FLUIContentView` conforms to `NSTextInputClient` (all 11 methods plus
`add_protocol`), and `keyDown:` becomes a gate rather than a direct conversion:
while `TextInputState.ime_allowed` is **false** it calls `handle_input_event`
exactly as before; while it is **true** it hands the event to
`interpretKeyEvents:` and emits nothing itself, with `doCommandBySelector:`
re-dispatching the events the input method declines. `PlatformTextInput` is
implemented by `MacOSTextInput`, whose two setters route through the owner lane.

**Why.** AppKit's composition pipeline is entered only by `interpretKeyEvents:`,
which calls *back* into the same view (`setMarkedText:` while composing,
`insertText:` on commit). Calling it alongside the existing conversion would make
one physical press reach the application twice — a `Key::Character` **and** an
`ImeEvent::Commit`. That is ADR-0044 §3's double-producer defect class, and
ADR-0069 states the contract it violates. The gate makes attachment the *only*
variable: the pre-existing path is byte-identical at the default, so the
conformance adds no regression surface to a window that never attaches a text
input.

**Market lineage.** winit 0.30.13 documents the same rule in the same terms
(`Window::set_ime_allowed`): with IME allowed the window receives `Ime` events
and, during the preedit phase, no `KeyboardInput`; with it disallowed the window
receives no `Ime` events and a `KeyboardInput` for every keypress; and IME is
**not** allowed by default. The winit backend inherits this by delegation
(`WinitTextInput`); the native backend owns the pipeline instead of delegating,
so it implements the protocol rather than wrapping one. The documentation covers
the *press*; reading winit's macOS implementation shows the release is gated too
and on a different variable — `key_up` queues a `KeyboardInput` only from its
`Ground` and `Disabled` states, so an open preedit suppresses the release while a
committed character's release still arrives (`ImeState::Committed` is reset to
`Ground` inside `keyDown:`). `key_up` here gates on the same condition expressed
in this backend's own state (`TextInputState::reports_key_release`), so a release
inside an open composition is suppressed for the same reason the press was: the
application never saw that key go down.

**Divergences from Flutter, both deliberate.** Flutter's framework closes a
connection *without discarding the composed characters*:
`EditableTextState.connectionClosed` (`editable_text.dart:4138`, checked at the
pinned 3.44.0) nulls the connection, drops `_lastKnownRemoteTextEditingValue`
and unfocuses; the unfocus routes through `_openOrCloseInputConnectionIfNeeded`
to `controller.clearComposing()`, and `clearComposing`
(`editable_text.dart:378`) assigns only `composing: TextRange.empty` — the
controller's `text` is untouched. The characters the user was composing are
therefore still there afterwards, as ordinary text. What Flutter does *not* do
on teardown is announce a commit: the text simply remains, and the commit the
application finally observes is the input method's own last
`updateEditingValue` before the close. `set_ime_allowed(false)` here **drops**
the composition (ADR-0069) and `unmarkText` emits
`ImeEvent::Preedit { text: String::new(), cursor: None }` rather than nothing, so
the second divergence is a *third* answer to the same situation rather than the
inverse of Flutter's. It is grounded in the client-side bug class
`flui-types/src/ime.rs` records — a client left holding composition state it was
never told ended suppresses `Key::Character` for the rest of the focus session
and keeps the cancelled slice in its buffer — and in winit's own macOS
implementation, which drops the marked text on `set_ime_allowed(false)`, so
ADR-0069 cites that implementation for the drop-vs-commit half rather than the
Flutter contrast. AppKit's header does not say whether an empty
`setMarkedText:` always precedes `unmarkText`, so both paths are covered; the
event is inert when nothing is composing, which is why the callback carries no
`hasMarkedText` check.

**Rejected.** Unconditional `interpretKeyEvents:` (the two-producer defect).
Gating on `-[NSTextInputContext handleEvent:]`'s `BOOL` — the routing decision is
already correct before the call, and consulting AppKit first would take the
keyboard path in exactly the states where both producers are live. A pre-call
predicate on the key, which is undecidable before the input method runs, and
wrong per layout: on a U.S. layout a plain letter *does* reach `insertText:`.

**Marked-range convention, a choice a document-less view cannot do better than.**
`markedRange` answers `(0, utf16_len)` while composing and `{NSNotFound, 0}`
otherwise; `selectedRange` echoes what `setMarkedText:` delivered. `NSRange` is
UTF-16 and `ImeEvent::Preedit.cursor` is a **byte** range, so the conversion is
`utf16_range_to_byte_range` — AppKit-free and tested.

**Trade-off, named rather than assumed.** The class-registration block cannot run
under a bare `cargo test` (an unbundled `NSWindow` throws
`_CFBundleGetValueForInfoKey`), so every encoding and selector here was
hand-checked against `objc-0.2.7`'s `add_method` preconditions until the bundled
probe executed them — which it now has, on a real Mac, with all five assertions
passing. What the probe measures about the rect is that it is non-zero and
screen-relative; the **direction** of the `firstRectForCharacterRange:` Y-flip is
still designed rather than measured, because a flip of the wrong sign produces a
non-zero screen rect too. Measuring which side of the caret the candidate window
would land on needs a real input method to ask, which is the same gap the probe's
own module doc states.

**Replacement coverage.** The conversion carries seven always-run tests
(ASCII identity, multi-byte divergence from UTF-16 offsets, CJK unit-vs-byte, a
range splitting a surrogate pair, past-the-end, empty text, and AppKit's real
`NSNotFound` location), and an eighth pins the `keyUp:` gate across all four
states it distinguishes — unattached, attached-but-idle, composing, and a
composition that has just ended. It is mutation-checked against the gate a
reader would first reach for (`!ime_allowed` alone fails it on the
attached-but-idle case). `just macos-ime` drives the live path from a bundled
`.app` and reports `IME_PROBE_RESULT=PASS` on a real Mac: the inverse pair
(attached → one `Commit` and no key event; detached → one key event and no
`ImeEvent`), the `Preedit` → `Commit` sequence with the byte cursor asserted, the
empty-`Preedit` that ends a composition, a non-zero cursor rect whose
`actualRange:` is answered rather than left not-found, and the `keyUp:` gate
end-to-end — a release reported while a text input is attached with nothing
composing, and the same call suppressed while a composition is open. The gate's
arm is mutation-checked against the ungated `key_up`: removing the check leaks a
`KeyboardEvent { state: Up, key: Character("a") }` into that arm and fails the
probe. Its own module doc states that a synthesized `NSEvent` proves the
*routing* and is not a genuine input method, so real composition (press-and-hold
or a CJK source) remains **not driven**.

### iOS binds UIKit through `objc2`, and its loop-exit signal is `applicationWillTerminate:`

**Decision.** The iOS backend (`platforms/ios/`) binds UIKit through `objc2`
0.6 / `objc2-ui-kit` 0.3 / `objc2-quartz-core` / `objc2-metal` / `block2` /
`dispatch2`, and takes its framework loop-exit signal from
`applicationWillTerminate:`. ADR-0070 carries the full record.

**Why `objc2` and not the macOS backend's `cocoa`/`objc`.** The choice is not
consistency-versus-modernity: `objc` has not released since 2019 and `cocoa`
has **no UIKit surface at all**, so the macOS stack cannot express this platform
even in principle. `icrate`, which this module's stub doc once named, is a
deprecated alias split into the `objc2-*` crates. Every shipping Rust Apple
stack is on `objc2` (winit's iOS backend since 0.30, wgpu-hal, egui, slint,
gpui), and the macOS module's own header already commits to migrating there.

**Why `applicationWillTerminate:`.** `UIApplicationMain` never returns, so
there is no "after `Platform::run`" for the runner to use — the desktop and
Android runners call `teardown_platform_realm()` there. Without a deliberate
choice the framework never receives its loop-exit signal on iOS and leaks every
realm, service pool and the clipboard for the process's life. That delegate
method is the only pre-exit notification iOS sends, so the platform fires its
quit handler from it and the runner runs the teardown.

**Alternatives considered.**
- *A process-global for the delegate's session state* — rejected: it would add
  a new entry to the ambient-reach ratchet (`docs/runtime-contract.toml`), and
  the state is main-thread-only anyway. A thread-local is the owner-affine
  scope ADR-0027 prefers and matches winit's own `ACTIVE_EVENT_LOOP`.
- *`UIScene` adoption* — deferred, not dropped. It is the multi-window iPadOS
  feature and would make `UIScreen.mainScreen` (deprecated in the scene era)
  scene-relative; this backend presents one full-screen window, so the app-wide
  accessor is the honest spelling and the module carries an
  `expect(deprecated)` with that reason.
- *`objc2-ui-kit` with default features* — rejected: it enables all ~456
  header features, compiling the whole framework's bindings for a handful of
  classes. The backend names the classes it messages, as `winit-uikit` does.

**Trade-off.** Two `objc2` generations are in the tree: this backend uses
0.6.4/0.3.2 (already there via `wgpu-hal`), while `winit` 0.30 and
`accesskit_macos` 0.27 still pull 0.5.2/0.2.2. The duplicate resolves when
those move (H10 / winit 0.31), not by anything here. A real device is not
covered — the simulator slice is, via `just ios-sim` — and no CI job boots a
simulator; `cross-typecheck` gained an `aarch64-apple-ios` clippy line so a
broken iOS build is at least loud.

**Replacement coverage.** `just ios-sim` (executing; asserts a Metal device
and a rendered frame from the app's own log, plus a screenshot) for the
end-to-end path, and three host-run unit tests on `display.rs`'s bounds
arithmetic. The engine-side portability fix this uncovered —
`required_limits` clamped to the adapter's own, because the simulator's Metal
adapter caps `max_inter_stage_shader_variables` at 15 where
`wgpu::Limits::default()` asks for 16 — carries a comment at the clamp.

### macOS and iOS share the `objc2` binding stack; the `cocoa`/`objc` pair is gone

**Decision.** Both Apple backends bind their platform frameworks through the
`objc2` family (`objc2`, `objc2-app-kit` for macOS/AppKit, `objc2-ui-kit` for
iOS/UIKit, `objc2-foundation`, `objc2-quartz-core`, `objc2-metal`) at the
versions `wgpu-hal` already pins. The `cocoa` 0.27 / `objc` 0.2 dependency pair
and the `build.rs` that existed only for its `cfg` macros are removed from the
crate; neither appears in `Cargo.lock` any more. ADR-0071 carries the full
record (ADR-0070 chose the stack for iOS first).

**Why.** `objc` has not released since 2019 and points at `objc2` as its
successor; `cocoa` deprecated its whole surface in the same direction and has no
UIKit bindings at all. Every shipping Rust Apple stack is on `objc2` (winit since
0.30, `wgpu-hal`, egui, slint, gpui). Carrying both stacks was a two-generation
wart with a `build.rs` to feed.

**The raw `msg_send!` shape is kept on purpose, not left un-migrated.** objc2's
macro accepts a raw `*mut AnyObject` receiver, `Bool` arguments and a manual
`release`, so `window.rs`'s already-reviewed safety shape survives. The concrete
reason: `NSWindow`/`NSView` are `MainThreadOnly` in objc2's typed API, while the
backend constructs test windows on a caller-supplied off-main serial lane
(`MacOSWindow::for_test`), which a `MainThreadMarker`-gated method would refuse.
`ClassBuilder` replaces `objc` 0.2's `ClassDecl` for the two runtime classes
(`FLUIContentView`, `FLUIWindowDelegate`).

**Two defects the stricter macro caught.** objc2's `msg_send!` verifies return
types against the selector encoding at run time and found `makeFirstResponder:`
declared `void` where AppKit returns `BOOL` — silently accepted by objc 0.2. It
also made the cursor-icon match's explicit `arrowCursor` arm list
`clippy::match_same_arms`-visible (kept as documentation under a scoped allow).
The newer macro is a stricter oracle; that is part of the migration's value.

**Trade-off.** A second `objc2` generation is still in the lock via
`accesskit_macos` 0.27 and winit 0.30 (both on 0.5); resolved when those move
(H10), not by anything here.

**Replacement coverage.** The four bundled macOS probes are unchanged in what
they assert and all PASS on `objc2`, driving the migrated `msg_send!` sites
through the production launch path; the five real-`NSPasteboard` tests and the
`display.rs` arithmetic run in the normal suite.
