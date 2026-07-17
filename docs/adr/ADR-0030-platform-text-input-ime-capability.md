# ADR-0030: Platform text input (IME) capability

*IME composition becomes a fallible platform capability (`PlatformTextInput`, reached through `PlatformWindow::text_input()`), a winit-shaped event vocabulary (`flui_types::ImeEvent`), and a token-guarded single-client registry (`flui_interaction::TextInputRegistry`) split across crates by the dependency graph, not by convenience.*

---

- **Status:** Accepted
- **Date:** 2026-07-17
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-types/src/ime.rs`; `crates/flui-platform/src/traits/{text_input.rs,input.rs,window.rs}`, `crates/flui-platform/src/platforms/{winit/{events.rs,platform.rs},headless/platform.rs}`; `crates/flui-interaction/src/text_input.rs`; `crates/flui-app/src/app/binding.rs` (`ImeBackend`, `AppBinding::attach_text_input`/`detach_text_input`, the `PlatformInput::Ime` arm in `handle_input`)
- **Related:** `docs/FOUNDATIONS.md` ("No `flui-services`" — IME/text-input, system chrome, and haptics become capability traits on `flui-platform`, not a standalone crate); the target layer graph (`flui-interaction` and `flui-platform` are both L2 substrate, with no `interaction --> platform` edge drawn); ADR-0022 (Focus widget seam — the same ambient-singleton-registry shape `TextInputRegistry` reuses); ADR-0027 (owner-affine `UiRealm` — the per-window/per-realm routing this PR defers into)

---

## Context

FLUI's roadmap (`docs/ROADMAP.md`, App.1) named IME as a remaining gap after the frame-pacing work (ADR-0029) landed. Flutter's own `services` package bundles IME/text-input, system chrome, and haptics behind one `TextInputConnection`/`MethodChannel` layer; `docs/FOUNDATIONS.md` already decided FLUI does **not** port that as a standalone crate — the residue becomes capability traits directly on `flui-platform` (`PlatformTextInput`, and future `PlatformSystemChrome`/`PlatformHaptics` siblings).

This PR (PR1 of the IME feature) delivers the vocabulary, the platform capability, and the client-registry routing — not the widget-side controller/composing model or `EditableText` adoption, which is PR2.

## Decision

### 1. `flui_types::ImeEvent` — winit-shaped, not W3C- or Android-shaped

`ImeEvent` (`Enabled` / `Preedit { text, cursor }` / `Commit(String)` / `Disabled`) mirrors winit 0.30.13's own `Ime` enum almost field-for-field (verified against the vendored source, `winit-0.30.13/src/event.rs:774-803`). This is a deliberate choice of *winit's* shape as the reference, not the W3C composition-event triad (`compositionstart`/`compositionupdate`/`compositionend`) or Android's pull-model `InputConnection` — winit is FLUI's lead desktop backend today, and its four-variant push model maps imperfectly onto both alternatives (documented on the type itself, not just here, so a future web/Android backend author sees the divergence at the point they need to bridge it). `Preedit.cursor` is a byte-offset `(start, end)` range **into the preedit string itself**, not a surrounding document; `cursor: None` means hide the caret (winit's own semantics for that case) — the widget-side hidden-caret rendering state that honors it is a named PR2 deferral.

### 2. `flui-platform`: `PlatformTextInput`, reached via `PlatformWindow::text_input()`

`PlatformTextInput { set_ime_allowed(bool), set_ime_cursor_area(Bounds<Pixels>), as_any() }` lives in `flui-platform/src/traits/text_input.rs`. It is reached through `PlatformWindow::text_input(&self) -> Option<Arc<dyn PlatformTextInput>>`, defaulting to `None` — the same capability-discovery template `PlatformWindow::display()` and `Platform::primary_display()` already established: a fallible accessor returning `Option<Arc<dyn _>>`, not a method bolted directly onto `PlatformWindow` with a panicking or silently-no-op default. A backend that cannot honor IME (a future minimal embedder; headless without wiring a fake) simply returns `None` instead of every `PlatformWindow` implementor inheriting methods it cannot make work. `PlatformSystemChrome`/`PlatformHaptics` are expected to follow the identical template when they land.

`PlatformInput` gains an additive `Ime(ImeEvent)` arm (plus `as_ime()`, matching `as_pointer()`/`as_keyboard()`). This is a breaking change to any exhaustive match over `PlatformInput` — `examples/web_demo/src/lib.rs` was the one production call site with such a match; it now logs the event instead of failing to compile.

**winit backend:** the `WindowEvent::Ime` arm — previously silently dropped in the `_ => {}` catch-all at `platforms/winit/platform.rs:623` — now converts via a new pure, unit-tested function `winit_events::ime_event` (`platforms/winit/events.rs`) and dispatches through the same `WindowCallbacks::dispatch_input` path every other input kind uses. `WinitWindow::text_input()` returns a small `WinitTextInput` wrapper around a cloned `Arc<winit::window::Window>` (not an impl directly on `WinitWindow`, which is typically boxed — `Platform::open_window` returns `Box<dyn PlatformWindow>` — so there is no `Arc<Self>` to hand out; cloning the `Arc<Window>` winit already holds is cheap).

**Headless backend:** `FakeTextInput` (`platforms/headless/platform.rs`, re-exported at the crate root) is a recording fake — every `set_ime_allowed`/`set_ime_cursor_area` call is appended to an in-memory history, not just accepted silently. `MockWindow::text_input()` returns the *same* `Arc<FakeTextInput>` on every call (verified by a dedicated test), so a test can call `.text_input()` independently of whatever code path attached a client and still observe the same recorded history.

### 3. `flui-interaction::TextInputRegistry` — the dep-forced trait split

`TextInputRegistry` lives in `flui-interaction`, matching the ambient-singleton shape of its sibling registries (`FocusManager`, `MouseTracker`): `global()` for production call sites via a thread-local (mirroring `FocusManager::global()` exactly), `new_for_test()` for isolated unit tests.

It does **not** depend on `flui-platform` — cannot, by the target layer graph in `docs/FOUNDATIONS.md`: both crates sit at L2 (substrate), and `interaction --> platform` is not a drawn edge. Adding one is a new lateral dependency between same-layer siblings, which is exactly the kind of decision this studio's `architecture` standard reserves for an explicit ADR, not a side effect of wiring one feature through the path of least resistance. So the trait that actually *does* something platform-specific (`PlatformTextInput`) lives on `flui-platform` per FOUNDATIONS.md's explicit call-out (§2 above), while the registry that only tracks *which client is currently active* lives on `flui-interaction` and never names `PlatformWindow`/`PlatformTextInput` at all.

The registry still needs to carry a window identity (see §4), so it accepts an `OpaqueWindowHandle` — `Arc<dyn Any + Send + Sync>` wrapped behind a typed `new`/`downcast_ref` pair. `flui-app` (which depends on both crates) is the only production caller; it wraps `Arc<dyn PlatformWindow>` on the way in and downcasts back to it wherever it needs the concrete window (`ImeBackend`, below). This is the "trait split": the capability trait is homed where it can see the platform; the registry is homed where its ambient-singleton siblings already live, and the two meet only at `flui-app`, through an opaque handle instead of a new crate edge.

**Single active client, attach-replaces, identity-token detach.** Only one client receives IME events at a time (Flutter parity: one current `TextInputConnection`). `attach()` always replaces whatever was previously active with no coordination required from the caller. `detach(token)` is a no-op unless `token` is still the *active* client's token — this closes the stale-detach race: field A attaches, field B gains focus and attaches (replacing A), and A's now-stale blur/dispose handler calling `detach(token_a)` must not disable IME while B still has focus. This exact interleaving is a dedicated test (`a_stale_detach_from_a_replaced_token_does_not_disturb_the_new_active_client`, `flui-interaction`) and its end-to-end counterpart at the platform boundary (`a_stale_detach_records_nothing_on_the_platform`, `flui-app`).

### 4. `flui-app`: the `ImeBackend` bridge

`ImeBackend` (private to `crates/flui-app/src/app/binding.rs`) bridges `TextInputRegistry` attach/detach to the window's `PlatformTextInput` capability: `attach()` registers with the registry *and* calls `set_ime_allowed(true)` if the active window supports IME; `detach()` only calls `set_ime_allowed(false)` when `TextInputRegistry::detach` actually reports the token was active (propagating the stale-detach guard from §3 all the way to the platform side, so a replaced field's dispose path cannot disable IME out from under the field that replaced it). `AppBinding::attach_text_input`/`detach_text_input` are the public entry points PR2's `EditableText` will call, reading the current window from `AppBinding`'s existing `active_window` slot. `handle_input`'s `PlatformInput::Ime(event)` arm dispatches straight to `TextInputRegistry::global().dispatch(&event)`, mirroring the existing `Keyboard` → `FocusManager` and `Pointer` → `GestureBinding` arms.

### 5. The suppression contract — documented, not implemented, in this PR

`ImeEvent`'s type doc and `TextInputRegistry`'s module doc both spell out the client contract PR2's `EditableText` must implement: suppress `Key::Character` insertion **only** while a composition is non-empty — winit already withholds `KeyboardInput` during composition and immediately after a commit (confirmed against `winit-0.30.13/src/window.rs`'s `set_ime_allowed` docs and the macOS `view.rs` `key_down` path in the vendored source), so suppressing *all* typing after `Enabled` would silently kill plain (non-IME) keyboard input for the rest of the session. `Disabled` delivered mid-composition means the client must strip the in-progress composing slice — winit's own semantics, and a **documented divergence** from Flutter's `TextInputConnection.connectionClosed`, which instead *keeps* the uncommitted text. Detach-on-dispose is part of the same client contract (this workspace's recurring bound-drop-cascade knot class, named so PR2 does not rediscover it from scratch).

### 6. Window routing — deferred, not implemented

`TextInputRegistry` records the attaching window's `OpaqueWindowHandle` but V1 dispatches every event to the single global active client regardless of which window it came from. Real per-window/per-realm routing is deferred until `UiRealm`'s realm-scoped ownership (ADR-0027) reaches this registry; the handle is captured now specifically so that migration is a routing-logic change, not a breaking signature change to `attach()`.

## Evidence

- `cargo nextest run --workspace --exclude flui-platform`: 7359 passed, 4 skipped.
- `cargo test -p flui-platform --features winit-backend --lib`: 65 passed (flui-platform's own suite is excluded from the workspace nextest gate per `AGENTS.md`'s "Testing quirks" — STATUS_HEAP_CORRUPTION investigation in progress — so it is verified directly instead).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace --exclude flui-platform --doc`: 626 passed, 370 ignored.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-types -p flui-platform -p flui-interaction -p flui-app --no-deps --document-private-items`: clean.
- `just fmt-check`, `just port-check`, `just inventory-check`, `taplo fmt --check`, `typos`: all clean.
- Red→green evidence for the two load-bearing behaviors this PR introduces:
  - `winit_events::ime_event` — four dedicated conversion tests (`Enabled`/`Disabled`/`Commit`/both `Preedit` cursor states), each asserting the exact delivered `ImeEvent`, not `is_ok()`.
  - `TextInputRegistry`'s stale-detach guard and `flui-app`'s end-to-end `ImeBackend` bridge — both written against the exact interleaving named in §3/§4 (attach A, attach B replacing A, stale detach of A, active detach of B), asserting the platform-side `set_ime_allowed` call history at each step, not just that no panic occurred.

## What is deferred (PR2)

- The controller/composing text model and `EditableText` adoption.
- Enforcing the suppression contract (§5) — currently documented only.
- Cursor-area wiring (`set_ime_cursor_area`) from real widget geometry — the platform method exists and is tested at the fake/mock level, but nothing in the widget tree calls it yet.
- Per-window/per-realm event routing (§6).
