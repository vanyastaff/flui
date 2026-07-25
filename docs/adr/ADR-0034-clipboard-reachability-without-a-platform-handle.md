# ADR-0034: Clipboard reachability without a platform handle

*Clipboard text access becomes reachable from `AppBinding` by resolving `Platform::clipboard()` — an existing required method every backend already implements — before `Platform::run()` consumes ownership of the platform value, and stashing the resulting `Arc<dyn Clipboard>` in a plain `AppBinding` slot. No new trait, no new `Platform` surface.*

---

- **Status:** Accepted
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-app/src/app/binding.rs` (`AppBinding::{set_platform_clipboard,clear_platform_clipboard,clipboard}`); `crates/flui-app/src/app/runner.rs` (`bootstrap_desktop`, `run_android`, `run_web`, `teardown_platform_realm`)
- **Related:** ADR-0031 (`PlatformHaptics` — named the same "`Box<dyn Platform>` is consumed by `run()`" gap this ADR closes for a device-global capability); ADR-0030 (`PlatformTextInput` — the install/teardown-symmetry template this ADR follows); `docs/FOUNDATIONS.md` ("No `flui-services`")

---

## Context

`Platform::clipboard(&self) -> Arc<dyn Clipboard>` has existed on the `Platform` trait since before this ADR, as a required method — every one of the eight `Platform` implementations (Windows, macOS, Linux native stub, winit, Android, iOS, web, headless) already has a body for it. Nothing reachable from `flui-app` called it, though: `AppBinding` had no clipboard slot, and no bootstrap path ever called `.clipboard()` on the platform value it was handed.

The obvious-looking blocker is the same one ADR-0031 named for haptics: `Platform::run(self: Box<Self>, on_ready)` takes ownership of the platform value, and none of the three bootstrap shapes (`run_desktop`, `run_android`, `run_web`) keeps a live `Platform` handle around after `run()` returns — `AppBinding` only ever retained `active_window` past startup, never the platform itself. A capability that needs the platform-shaped object *after* startup looks unreachable by construction.

It is not, for clipboard specifically: `Platform::clipboard()` can be called **before** `run()` is invoked, or (on desktop) from inside `on_ready`'s `&dyn Platform` parameter, which is live at that point. Nothing about `run()` consuming the box prevents resolving a capability from the platform value first and holding onto the *result*, rather than the platform value itself.

## Decision

### 1. No new `PlatformHandle` trait

The reachability problem could be "solved" by introducing a `PlatformHandle` (or similarly named) capability object that `Platform::run()` hands back, or a `platform.handle()` method returning `Option<Arc<dyn PlatformHandle>>` for post-`run()` access. This ADR rejects that shape.

`Platform::clipboard()` is already exactly the right shape: a **required** trait method returning `Arc<dyn Clipboard>`, which the compiler enforces on every backend. A `PlatformHandle::clipboard() -> Option<Arc<dyn Clipboard>>` alternative would have to default to `None` (a handle abstraction spanning eight backends cannot require every one of them to opt in), which converts a compiler-enforced capability into a silently-losable one: a backend that has a perfectly real clipboard implementation (macOS's `NSPasteboard`-backed one, for instance) could ship a `PlatformHandle` impl that forgets to wire `clipboard()` through, and nothing red-flags it — the trait default quietly returns `None` and the bug ships. Calling `Platform::clipboard()` directly, at the one point the platform value is still intact, keeps the existing required-method guarantee intact instead of laundering it through an optional pass-through.

### 2. A plain `AppBinding` slot, not a bridge type

`flui-app` already has a template for "install a platform capability once, read it from many call sites" in `TextInputPlatformBridge` and `HotReloadBridge` (`binding.rs`) — small `Clone` handle structs holding an `Arc`-cloned slot, installed into `UiRealm::bind_to_app` so realm-bound consumers target the correct `AppBinding` instance rather than resolving `AppBinding::instance()` fresh. Clipboard access has no such per-realm consumer yet — nothing outside `AppBinding` itself needs to read it — so this ADR does not add a bridge type for it. The unit is exactly:

- `platform_clipboard: Arc<Mutex<Option<Arc<dyn Clipboard>>>>` — a field on `AppBinding`.
- `set_platform_clipboard`/`clear_platform_clipboard` (`pub(crate)`) — install/teardown.
- `clipboard(&self) -> Option<Arc<dyn Clipboard>>` (`pub`) — the read path.

If a future widget-tree consumer needs clipboard access from build/event-handler code, that lands as its own seam (a `BuildContext` capability, following the `text_input_handle()` precedent) built on top of this slot — not a reason to add a bridge type preemptively today.

`clipboard()` clones the `Arc` out of the lock and lets the temporary guard drop before returning, matching `TextInputPlatformBridge`/`perform_haptic_feedback`'s clone-then-call discipline: a caller's `read_text()`/`write_text()` call, even one that re-enters `AppBinding::clipboard()`, never finds `platform_clipboard`'s lock still held. A `None` return (no platform installed yet, or torn down) is logged at `tracing::debug!` rather than left silent, since a caller that gets `None` back has no other way to learn whether that is "too early" or "permanently absent" for this run.

### 3. The winit coherence fact

Resolving `.clipboard()` once, before or early inside bootstrap, and holding onto that single `Arc` for the rest of the process's life is only correct if the returned clipboard's state stays coherent with whatever the running platform does with clipboard access afterward — i.e., if nothing else, later, independently, drifts out of sync with the `Arc` this ADR stashed.

This holds for winit (the desktop backend in current use for Linux via the `winit-backend` feature flag, and structurally for Windows/macOS as configured) because `WinitPlatform`'s clipboard state is not constructed fresh per call: `WinitPlatform::clipboard()` returns `state.clipboard.clone()`, where `state: Arc<Mutex<WinitPlatformState>>` is shared across every `WinitPlatform` method call for the life of that one platform instance. The `ArboardClipboard` inside it owns a live `arboard::Clipboard` — on X11 this holds an actual X11 connection used to serve clipboard-selection requests — so the coherence property that matters is "the Arc resolved before `run()` is the *same* `ArboardClipboard`/connection the running event loop would also resolve," not merely "clipboard reads/writes eventually reach the OS clipboard somehow." Because `state` is an `Arc`, it is: `platform.clipboard()` called from `run_desktop` before `platform.run(...)` and any hypothetical later call on the same `WinitPlatform` instance clone the identical inner `Arc<Mutex<WinitPlatformState>>`, so there is exactly one `ArboardClipboard`/connection for the process, not one that this ADR's early resolution silently orphans.

Android and web mirror this shape (`AndroidPlatform`/`WebPlatform` also return a clone of an `Arc`-held clipboard field, not a fresh construction). macOS's `MacOSClipboard::new()` constructs a fresh wrapper per call, but that is safe for a different reason — it wraps `+[NSPasteboard generalPasteboard]`, itself a process-wide OS singleton, so any number of independently constructed `MacOSClipboard` wrappers already observe the same underlying pasteboard; there is no owned connection state to fall out of sync.

**Contract for a future backend:** if a backend's `Clipboard` impl ever owns real connection state (a socket, a file descriptor, a session token) rather than wrapping an OS-level singleton, that state must live behind an `Arc` the platform struct clones out on every `clipboard()` call — the same shape winit/Android/web already use — not be reconstructed per call the way `MacOSClipboard` currently gets away with. Reconstructing per call is only safe when the thing being reconstructed is a stateless handle onto an OS singleton.

### 4. Install/teardown symmetry

`set_platform_clipboard` is called once per bootstrap; `clear_platform_clipboard` is called once from `teardown_platform_realm`, after the event loop has exited (this function runs from both `run_desktop` and `run_android`; the web bootstrap has no teardown call at all today — the runner stays owner-TLS resident for the page's lifetime, same as its window). Without the clear half, a torn-down realm would leave a live platform resource pinned behind `AppBinding`'s `'static` singleton indefinitely: on Linux, `ArboardClipboard` owns a live X11 connection, and that connection would otherwise outlive the winit event loop it belongs to. This mirrors the same install/teardown discipline `PlatformTextInput`'s `ImeBackend::attach`/`detach` (ADR-0030) already established for this crate.

### 5. Bootstrap wiring differs by platform, on purpose

The three desktop/mobile/web bootstrap shapes extract the clipboard at different points because they hand the platform value to the caller differently:

- **Desktop** (`bootstrap_desktop`): runs *inside* `on_ready`, with a live `platform: &dyn Platform` parameter — `platform.clipboard()` is called as the function's first statement, on that reference, before window creation.
- **Android/web** (`run_android`, `run_web`): construct/obtain a `Box<dyn Platform>` and do most of their setup *before* calling `platform.run(...)`, whose `on_ready` closure discards its own `platform` parameter (`|_platform| { ... }`). `.clipboard()` is called on the `Box` immediately after it is constructed — the last point every one of these bootstraps still owns it uncontested, before `.run()` takes it by value.
- **iOS** (`run_ios`): a config-only stub (no UIKit-backed `flui-platform` implementation exists yet — see `docs/ROADMAP.md`'s Cross.P section). It has no `Platform` value to resolve a clipboard from at all, so this ADR does not touch it.

## Evidence

- `cargo nextest run --workspace --exclude flui-platform`: all tests pass, including the four new `platform_clipboard_reachability` tests.
- `cargo test -p flui-platform --lib`: passes (flui-platform's own suite is excluded from the workspace nextest gate per `AGENTS.md`'s "Testing quirks").
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo check -p flui-app --target wasm32-unknown-unknown`: compiles (the `run_web` bootstrap edit).
- `cargo test -p flui-app --doc`: passes.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-app --no-deps --document-private-items`: clean.
- `just fmt-check port-check inventory-check`, `taplo fmt --check`, `typos`: all clean.
- Red→green evidence: reducing `set_platform_clipboard` to a no-op made `app_binding_clipboard_reaches_the_installed_platform_clipboard`, `clear_platform_clipboard_removes_the_installed_clipboard`, and `clipboard_reentrant_read_does_not_deadlock` fail; restoring the real body turned all three green again.

## What is deferred

- **`open_url`.** No `flui-platform` backend has a body for it (winit has none), and there is no consumer waiting on it. Plumbing an `AppBinding`-level seam now, ahead of both a real implementation and a caller, would ship stub API with nothing behind it. Ships together with its first real consumer and a real winit body, not before.
- **`reveal_path`/`open_path`/save-and-open prompts.** No `flui-platform` backend exposes these today and nothing calls them. Same reasoning as `open_url` — no plumbing ahead of a real consumer.
- **macOS main-thread pasteboard affinity — a doc'd hazard, not a fix.** `clipboard()` makes the platform clipboard reachable from `AppBinding`'s `'static`-per-thread singleton, which in practice is read from whichever thread owns the running realm — today, always the platform's main/owner thread, since that is the only thread that calls `bootstrap_desktop`/`run_android`/`run_web`. `NSPasteboard` access is documented by Apple as safe off the main thread for reads, but `MacOSClipboard` currently makes no assertion either way. A future macOS `Clipboard` implementation, or a future caller that reaches `AppBinding::clipboard()` from a background thread (a worker completing an async paste operation, for instance), should `debug_assert!` main-thread affinity at the point `MacOSClipboard`'s methods run, rather than silently relying on every future call site staying on the owner thread by convention. No such assertion exists yet; this is named so it is not rediscovered as a production crash.

## Resolved follow-up

- **Window-scoped cursor ownership (2026-07-23).** The parallel
  `flui_platform::CursorStyle` type and process-wide
  `Platform::set_cursor_style` method were deleted. The render/interaction,
  platform, winit, and CSS boundaries now use the same
  `cursor_icon::CursorIcon`. `MouseTracker` applies a cursor through its
  presentation's exact `PlatformWindow`; each native/headless window stores or
  restores its own selection. There is no compatibility conversion or
  forwarding API.
