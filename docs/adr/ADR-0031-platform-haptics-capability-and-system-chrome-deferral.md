# ADR-0031: Platform haptics capability, and system chrome deferral

*Haptic feedback becomes a fallible per-window platform capability (`PlatformHaptics`, reached through `PlatformWindow::haptics()`), carrying a Flutter-mirrored vocabulary (`flui_types::HapticFeedback`) homed in `flui-types`; `PlatformSystemChrome` is deferred in full, with no target date, because none of its six upstream methods has an honorable desktop surface today.*

---

- **Status:** Accepted
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-types/src/haptics.rs`; `crates/flui-platform/src/traits/{haptics.rs,text_input.rs,window.rs}`, `crates/flui-platform/src/platforms/headless/{mod.rs,platform.rs}`; `crates/flui-app/src/app/binding.rs` (`AppBinding::perform_haptic_feedback`)
- **Related:** ADR-0030 (`PlatformTextInput` — the template this capability follows); `docs/FOUNDATIONS.md` ("No `flui-services`" — IME/text-input, system chrome, and haptics become capability traits on `flui-platform`, not a standalone crate); `docs/ROADMAP.md` App.1

---

## Context

ADR-0030 landed `PlatformTextInput` and explicitly named `PlatformSystemChrome`/`PlatformHaptics` as siblings expected to follow the same template. `docs/ROADMAP.md` App.1's "Remains" list carried both forward as the last two dissolved-`services` capability traits. This PR delivers `PlatformHaptics` in full (vocabulary, trait, both backends, the `flui-app` bridge) and defers `PlatformSystemChrome` in full, rather than doing a partial pass on both.

## Decision

### 1. `flui_types::HapticFeedback` — Flutter's vocabulary, mirrored 1:1

`HapticFeedback` (`Vibrate` / `LightImpact` / `MediumImpact` / `HeavyImpact` / `SelectionClick` / `SuccessNotification` / `WarningNotification` / `ErrorNotification`) mirrors Flutter's `HapticFeedback` static method set field-for-field (`packages/flutter/lib/src/services/haptic_feedback.dart` @ 3.44.0: `vibrate()`, `lightImpact()`, `mediumImpact()`, `heavyImpact()`, `selectionClick()`, and the three later-added `successNotification()`/`warningNotification()`/`errorNotification()` statics — each its own plain static method, not variants of a `notification(type)` call; the `'HapticFeedbackType.*Notification'` string each sends is only the platform-channel payload, not a Dart-level type). It is `#[non_exhaustive]` because the upstream vocabulary already grew once — the three notification statics postdate the original five — so a future addition here is an additive enum variant, not a breaking change to callers matching on it.

Every variant carries the same fire-and-forget, best-effort semantics: a silent no-op on a platform/OS-version/device combination without support. This is Flutter's own degradation contract (the Dart API returns `Future<void>` and never surfaces "unsupported" as an error), and there is deliberately no availability-discovery API upstream or here — a caller cannot ask "can this device vibrate?" before calling, matching Flutter exactly.

### 2. Homed in `flui-types`, not `flui-platform` — the `ImeEvent` precedent

`HapticFeedback` lives in `crates/flui-types/src/haptics.rs`, re-exported at the crate root, following the same reasoning `ImeEvent` established: the payload vocabulary a platform capability carries is homed below `flui-platform` in the dependency graph so a future consumer that also sits below it — a Material `InkWell` or `Switch` in `flui-widgets`/`flui-material` firing haptic feedback on tap/toggle — can name the type without depending on the platform layer itself. Only the trait that actually *performs* feedback (`PlatformHaptics`) needs to live where it can see `flui-platform`'s `Arc<dyn _>` capability-discovery machinery.

### 3. `PlatformHaptics`: one `perform(enum)` method, not eight discrete methods

`PlatformHaptics { perform(&self, HapticFeedback), as_any() }` lives in `flui-platform/src/traits/haptics.rs`. This is a deliberate divergence from `PlatformTextInput`'s shape, which exposes one method per control (`set_ime_allowed`, `set_ime_cursor_area`) because those are semantically distinct operations with different argument shapes. Haptics has no such distinction — every `HapticFeedback` variant is the *same* operation ("perform this feedback kind") differing only in which kind. Eight discrete methods (`vibrate()`, `light_impact()`, …) would make adding a ninth kind a breaking change to the trait; a single `perform(HapticFeedback)` makes the identical addition a non-breaking enum variant instead, which matters given the vocabulary has already grown once upstream (see §1). The two capabilities have different growth profiles, so they get different trait shapes on purpose, not by inconsistency.

### 4. Per-window, not device-global on `Platform`

`PlatformHaptics` is reached through `PlatformWindow::haptics(&self) -> Option<Arc<dyn PlatformHaptics>>`, defaulting to `None` — not through a device-global accessor on `Platform`. Three reasons, in order of weight:

1. **Template consistency.** `PlatformTextInput`'s own module doc (ADR-0030) committed `PlatformSystemChrome`/`PlatformHaptics` to "the same template": a fallible per-window accessor matching `PlatformWindow::display()` and `Platform::primary_display()`, not a method bolted directly onto `PlatformWindow` with a panicking or silently-no-op default.
2. **The richest target is per-`View`.** Of FLUI's eventual backend targets, Android has the most granular haptics contract, and it is per-`View` (`android.view.View.performHapticFeedback(int)`), not a device-global service. Per-window is the FLUI shape closest to that reality; a device-global trait would have to be artificially widened again once an Android backend lands.
3. **It is the only scope `flui-app` can reach today.** `AppBinding` retains `active_window` as its one live platform handle — `Box<dyn Platform>` is consumed by `run()` and not kept around after startup — so a device-global accessor on `Platform` would be unreachable from the one production call site this capability needs (`AppBinding::perform_haptic_feedback`). Per-window costs nothing extra: a desktop backend with a single device-global haptics engine (if one ever exists) can trivially return the same `Arc` from every window's accessor, since per-window here means per-window-*reachability*, not per-window-*state*.

   Clipboard reachability (ADR-0034) later closed this exact gap for a device-global `Platform` capability without adding a platform handle: `Platform::clipboard()` is resolved once, before `run()` takes ownership of the `Box<dyn Platform>`, and stashed in a plain `AppBinding` slot — no new trait surface needed.

### 5. Backends

**winit:** no override. `WinitWindow`'s `PlatformWindow` impl inherits the trait
default (`None`) with a comment stating this is the permanent correct answer —
desktop winit targets have no haptic hardware to drive, not a stub awaiting a
backend. `open_window()` returns the exact stored `Arc<WinitWindow>` erased to
`Arc<dyn PlatformWindow>`; the former delegating window wrapper was deleted, so
`text_input()`, `display()`, cursor state, callbacks, and raw handles cannot
diverge across two window objects.

**Headless:** `FakeHaptics` (`platforms/headless/platform.rs`, re-exported at the crate root) is a recording fake mirroring `FakeTextInput`'s exact shape: `Mutex<Vec<HapticFeedback>>` history, `new()`, `calls() -> Vec<HapticFeedback>` (delivery order), `last() -> Option<HapticFeedback>`. `MockWindow` stores one `Arc<FakeHaptics>` field, and `haptics()` returns a clone of the *same* `Arc` on every call (a dedicated test proves this, matching the existing `text_input_reaches_the_same_fake_across_calls…` test) — so a test driving the binding and a test asserting on the fake directly observe the same recorded history.

### 6. `flui-app`: `AppBinding::perform_haptic_feedback`

`AppBinding::perform_haptic_feedback(&self, feedback: HapticFeedback)` reads the binding's existing `active_window` slot through `with_window()` (the same accessor `attach_text_input`/`detach_text_input` use), and if the window has a `PlatformHaptics` capability, calls `perform(feedback)`. No active window, or an active window with no haptics capability, is a silent no-op — no panic, nothing to report — which IS Flutter's own degradation contract (§1), not a gap. This unit deliberately does **not** add a `BuildContext` capability handle (the shape `text_input_handle()` established for IME in ADR-0030 PR2) — that lands with the first widget consumer that actually needs to call it from build/event-handler code, a named deferral. `scripts/check-frame-capability-scope.sh` is untouched by this PR.

## `PlatformSystemChrome` — full deferral

Flutter's `SystemChrome` static class exposes six methods. Auditing each against FLUI's current backend set (winit desktop, headless) finds none with an honorable desktop surface:

| Upstream method | Why it has no honorable desktop surface today |
|---|---|
| `setPreferredOrientations` | Orientation is a mobile-only axis (portrait/landscape lock). Desktop windows have no orientation to lock — there is nothing for this to do on winit, and stubbing it as a silent no-op the way haptics degrades would be misleading here, since a caller has no reason to expect orientation locking to be *conditionally* unsupported the way haptic hardware is. |
| `setApplicationSwitcherDescription` | Subsumed by `PlatformWindow::set_title()`, which already exists and is wired to the real OS window title/taskbar entry on every current backend. A second, narrower API for the same concept is a name collision waiting to happen, not a gap. |
| `setEnabledSystemUIMode` | Android/iOS system-bar visibility modes (edge-to-edge, immersive, …). No desktop backend has a "system UI" to hide — this is mobile chrome with no desktop analogue at all, not a partially-supported feature. |
| `restoreSystemUIOverlays` | The undo half of `setEnabledSystemUIMode`; deferred for the identical reason. |
| `setSystemUIChangeCallback` | A callback for system-UI-mode transitions the platform itself initiates (e.g. the user swiping to reveal a hidden status bar). No desktop backend has this transition to notify about. |
| `setSystemUIOverlayStyle` | Android/iOS status-bar and navigation-bar tinting (icon brightness, background color). No desktop backend has a system status bar to tint. See the porting note below — this one is not a bare trait method even when a real Android/iOS backend eventually lands. |

**Explicit revisit trigger:** this deferral is reopened the moment a real Android or iOS backend moves past its current stub state (`docs/ROADMAP.md` Cross.P) — at that point at least `setEnabledSystemUIMode`/`restoreSystemUIOverlays`/`setSystemUIOverlayStyle` gain a genuine target, and `setPreferredOrientations` gains one on any mobile backend regardless of OS.

**Non-obvious porting fact for that future work:** `setSystemUIOverlayStyle` is **stateful** in the Flutter framework, not a bare fire-and-forget platform call the way every `HapticFeedback` variant is (verified against `packages/flutter/lib/src/services/system_chrome.dart` @ 3.44.0). Its Dart implementation holds `SystemChrome._pendingStyle`/`_latestStyle`: a call sets `_pendingStyle` and, if no microtask is already queued, schedules one via `scheduleMicrotask`; every `setSystemUIOverlayStyle` call arriving before that microtask runs just overwrites `_pendingStyle`, so N calls in the same microtask window collapse into at most one platform-channel invocation, and a call whose style already equals `_latestStyle` is skipped entirely as a trivial no-op. Separately, `SystemChrome.handleAppLifecycleStateChanged` — called by the binding on every app lifecycle transition — reacts only to `AppLifecycleState.detached`: it clears `_latestStyle` to `null` (via its own microtask), so that the *next* `setSystemUIOverlayStyle` call after the app reattaches is guaranteed to bypass the "already equals `_latestStyle`" skip and actually resend, rather than staying silently stale against a host that may have reset its own system-bar tinting while detached. Neither mechanism proactively re-pushes the style on a timer or on `resumed` — the actual per-frame reassertion Flutter apps rely on (e.g. an `AppBar` continuing to tint the status bar as the widget tree rebuilds) comes from the **rendering layer**, where `AnnotatedRegion<SystemUiOverlayStyle>` re-calls `setSystemUIOverlayStyle` each time its region repaints, not from `SystemChrome` itself. A future FLUI split must therefore NOT be a single `PlatformSystemChrome::set_overlay_style(&self, Style)` trait method mirroring `PlatformHaptics::perform`'s shape — it needs a trait method **plus** a framework-side coalescer that reproduces the pending/latest-style dedup and the detach-clears-cache behavior (`AnnotatedRegion`'s repaint-driven resend is a separate, rendering-layer concern), or the naive port silently drops both the call-coalescing and the stale-after-reattach fix Flutter apps depend on. This is named now so that future work does not rediscover it by porting the naive shape first and finding the bug in production.

## Evidence

- `cargo nextest run --workspace --exclude flui-platform`: 7392 passed, 4 skipped.
- `cargo test -p flui-platform --lib`: 57 passed. `cargo test -p flui-platform --lib --features winit-backend`: 67 passed (flui-platform's own suite is excluded from the workspace nextest gate per `AGENTS.md`'s "Testing quirks" — STATUS_HEAP_CORRUPTION investigation in progress — so it is verified directly instead).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace --exclude flui-platform --doc`: 626 passed, 370 ignored.
- `cargo test -p flui-types -p flui-platform -p flui-app --doc`: 129 passed, 29 ignored.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-types -p flui-platform -p flui-app --no-deps --document-private-items`: clean.
- `just fmt-check port-check inventory-check`, `taplo fmt --check`, `typos`: all clean.
- Red→green evidence: `AppBinding::perform_haptic_feedback` reduced to a no-op made `perform_haptic_feedback_reaches_the_active_windows_platform_capability` fail (`left: [] right: [SelectionClick]`); restoring the real body turned it green.

## What is deferred

- `PlatformSystemChrome` in full — see the table above; no target date, reopened when a real mobile backend exists.
- A `BuildContext` haptics capability handle for widget consumers (Material `InkWell`/`Switch` and similar) — `AppBinding::perform_haptic_feedback` is the seam; no widget calls it yet.
- Real-device haptic hardware verification — only the headless `FakeHaptics` path and the winit permanent-`None` path are machine-verified; no Android/iOS backend exists yet to verify actual vibration against.
