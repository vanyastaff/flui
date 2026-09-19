# macOS / iOS feature completeness — design record

**Date:** 2026-09-18
**Status:** design only; build + hot-reload are implemented (see "Landed" below)
**Scope:** the macOS and iOS platform backends, the build/hot-reload tooling
around them, and the platform features named but not yet implemented.

This is a *shape* record, not a plan-of-record with dates. Its job is to fix the
seams large features will land on, so that later work does not have to reshape
code that already shipped. Nothing here is implemented unless marked **Landed**.

---

## Landed (2026-09-18)

These are done and verified; recorded here so the design below builds on them.

- **`flui-build` selects the cargo unit.** `BuilderContext.target:
  BuildUnit::{DefaultBinary, Package, Example}` replaced the hard-coded
  `crates/flui_app/Cargo.toml` in `DesktopBuilder`/`IOSBuilder`. `flui build`
  now produces the *user's* binary/library.
- **`flui build` enters a tokio runtime.** The builders drive `tokio::process`;
  `pollster` installs no reactor, so every build path panicked "there is no
  reactor running". `commands/build.rs::execute` now enters a multi-thread
  runtime before driving any builder.
- **macOS `.app` staging.** `AppBundle` on the context; `flui build macos`
  reads identity from `flui.toml` and stages a launchable `.app`.
- **`flui build macos --universal`.** Two darwin slices fused with `lipo`.
- **`flui build ios`** drives `IOSBuilder` (no longer the "not yet supported"
  message).
- **`flui create --hot-reload`** emits the three-crate host/worker/types
  workspace + `[hot_reload]` in `flui.toml`; gated by a real `cargo check
  --workspace` in `tests/cli_create.rs`.
- **Hot reload actually rebuilds.** `BuildOwner::reassemble` queued dirty
  entries without setting the element's own dirty flag, and the drain skips a
  clean element — so a reload rebuilt nothing. Fixed; mutation-verified by
  `crates/flui-widgets/tests/hot_reload_state.rs`.
- **Degradation is surfaced.** `poll_and_apply` no longer drops
  `Degraded`/`ReloadFailed` outcomes.
- **macOS worker-dylib freshness.** `flui-cli` stages worker builds at a
  content-addressed path (`{stem}-hot-{fnv1a-hash}{ext}`) instead of fixed
  `staging-a`/`staging-b` slots, and best-effort ad-hoc `codesign`s each staged
  dylib on macOS. The old pair stopped changing path once both slots existed,
  so a rebuild re-opened the same path and dyld (a deferred-unmap runtime) could
  serve the retained image; the content-addressed name changes whenever the
  bytes do. A distinct-path freshness test now holds on macOS. See the
  *worker-dylib freshness* entry under the reserved seams for the full shape.
- **iOS worker hot reload works in the Simulator.** `bootstrap_ios` now runs the
  same host/worker split as desktop: it loads the worker, starts the
  artifact watcher, and reassembles on change. The one platform-shaped blocker
  — a worker `cdylib` statically links its own `flui-painting` (and therefore
  its own `FONT_SYSTEM`) but never `flui-engine`, whose renderer installs the
  empty-database font fallback, so the worker's first shaped run panicked with
  cosmic-text's `no default font found` on iOS, where no system face is
  discoverable — is closed structurally: `flui-painting` now installs the
  embedded `Roboto-Regular` when host discovery finds no Latin-capable face
  (ADR-0016's "lowest owner loads the baseline", completed; see that ADR's
  2026-09-18 amendment). Verified end-to-end on an iOS-Simulator: the counter
  renders its first frame, and two consecutive worker edits each re-staged the
  dylib and reassembled the realm with zero panics. Still Simulator-only: a
  production App Store build has no mutable dylib to load, so the worker field
  stays `None` and the capability is inert.

---

## Deferred large features — seams reserved

Each entry names what exists today, the seam to extend, and what the extension
must not break. The point is that a later implementer extends a named seam
rather than discovering the shape is wrong.

### 1. In-process `HotRestart` root remount

**Today.** `PresentationState::apply_hot_reload(HotRestart)` logs "not
implemented" and degrades to `reassemble`.

**What it needs.** The realm does not retain the root view it was mounted with
(`UiRealm::attach_root_widget*` consumes it), so there is nothing to remount.
A remount must: `detach_root_widget()` (running every `State::dispose`), then
re-attach the SAME root configuration with fresh state.

**Seam.** Store the root view (type-erased `Box<dyn View>` plus its sizing) on
the `PresentationState` at attach time, and add
`PresentationState::remount_root()`. `attach_root_widget` already has the
`attach_root_widget_with_size` split to reuse.

**Why deferred.** It changes what the realm retains for the whole process's
life, and the retention interacts with `GlobalKeyScope` reclamation
(ADR-0043). It is a design item, not a wiring one.

**Must not break.** The hot-reload ADR's state-preservation contract:
`HotReload` preserves `State`, `HotRestart` deliberately drops it. The two
tiers must stay distinguishable in `HotReloadOutcome`.

### 2. macOS worker-dylib freshness (`H5`) — **Landed** (number kept for stable references)

Landed 2026-09-18; the full shape and evidence are in the *Landed* section
above. In short: `flui-cli` stages each worker build at a content-addressed path
(`{stem}-hot-{fnv1a-hash}{ext}`) and best-effort ad-hoc `codesign`s it on macOS,
the sidecar manifest carries that path on every platform, and a distinct-path
freshness test (`dlclose_then_reload_distinct_paths_serve_fresh_images`) holds on
macOS where the same-path test cannot. No driver trait changed.

### 3. Appearance (dark/light) on macOS + iOS

**Today.** Neither Apple backend overrides `PlatformWindow::appearance`, so it
returns the trait default `Light` forever, and `on_appearance_changed` never
fires. The consumer exists: `runner/desktop.rs` forwards it to
`MediaQueryData::platform_brightness`, so the Material dark theme is dead on
both.

**Seam.** `WindowAppearance` and `on_appearance_changed` already exist on the
trait — this is an implementation, not a contract change. macOS:
`NSWindow.effectiveAppearance.name` → the four `NSAppearanceName*` values plus
a `viewDidChangeEffectiveAppearance:` hook on `FLUIContentView`. iOS:
`UITraitCollection.userInterfaceStyle` plus `traitCollectionDidChange:`.

**Must not break.** `desktop.rs`'s existing appearance wire and
`WindowAppearance`'s four-variant vocabulary (Light/Dark/VibrantLight/
VibrantDark — Flutter's names).

### 4. `WindowInsets` / safe area

**Today.** No `WindowInsets` type exists anywhere. iOS ignores the notch and
home indicator; the Material `SafeArea`/scaffold machinery has nothing to read.

**Seam.** A new value type + `PlatformWindow::insets()` (default zero) +
`on_insets_changed`, mirroring gpui-ce's `WindowInsets`
(`platform.rs:831`). iOS: `UIView.safeAreaInsets` +
`safeAreaInsetsDidChange`. macOS: `NSView.safeAreaInsets` (usually zero — the
honest desktop value).

**Must not break.** The value must be a plain `Edges<Pixels>` equivalent, not a
platform handle; `MediaQuery` is where it becomes user-visible.

### 5. iOS text input / soft keyboard

**Today.** `PlatformTextInput` has no iOS implementor; there is no
show/hide-soft-keyboard or IME-position surface. iOS cannot host a text field.

**Seam.** New `ios/text_input.rs` over `UITextInput`/`UIKeyInput`, reached by
`PlatformWindow::text_input()` (already exists, macOS-only). The trait may need
`show_soft_keyboard`/`hide_soft_keyboard`/`set_ime_position`; those are additive
defaults, so no existing backend breaks.

**Must not break.** The macOS `NSTextInputClient` conformance is the model:
one producer per press (ADR-0066), composition state in `RefCell` on the view.

### 6. Haptics implementations

**Today.** `PlatformHaptics` exists with only a headless `FakeHaptics`; the
per-window accessor `PlatformWindow::haptics()` returns `None` on both Apple
backends. `UiRealm::perform_haptic_feedback` is the wire, currently uncalled.

**Seam.** macOS `NSHapticFeedbackManager`, iOS `UIImpactFeedbackGenerator`,
each returned from `haptics()`. No trait change.

**Must not break.** The `perform(HapticFeedback)` single-method shape (see
`traits/haptics.rs`) — do not add discrete methods.

### 7. System drag-and-drop (ADR-0038 slice 4)

**Today.** The widget layer (`Draggable`/`DragTarget`), the event vocabulary
(`PlatformInput::DragDrop`), and the winit file-drop transport all exist.
macOS/iOS `data_transfer()` return `NullDataTransferSource`, with the comment
naming `NSDraggingDestination`/`NSPasteboard` as the future work.

**Seam.** ADR-0038 already froze the transport trait and the synchronous
hover-feedback stage; the Apple backends implement `DataTransferSource` on top.
macOS: `NSDraggingDestination` on the content view + `NSPasteboard`. iOS:
`UIDropInteraction` + `NSItemProvider`.

**Must not break.** The ADR's one-source-per-platform-instance contract
(`Platform::data_transfer`'s doc) and the `DragDropEvent` shape.

### 8. Native gestures (macOS trackpad done; iOS absent)

**Today.** macOS converts `NSEventTypeMagnify`/`Rotate` into the shared
`PointerGesture` vocabulary. iOS has only raw touches; no pinch/rotate/long-
press recognizers.

**Seam.** `UIPinchGestureRecognizer`/`UIRotationGestureRecognizer` feeding the
same `shared::gestures` conversions macOS uses, so the two backends cannot
drift.

**Must not break.** `shared::gestures`' synthetic pointer identity
(`GESTURE_POINTER_ID`) and the pan-zoom lane's existing consumption.

### 9. Menus and file dialogs

**Today.** Neither exists on macOS. ADR-0039 explicitly deferred dialogs to
their own ADR (async shape + main-thread-panel affinity); menus need a
`Menu`/`MenuItem` vocabulary FLUI does not have.

**Seam.** Menus: a new trait vocabulary + `Platform::set_menus`, mirroring
gpui-ce's `app_menu.rs`. Dialogs: the follow-up ADR ADR-0039 names.

**Why deferred.** Menus are a new public vocabulary and dialogs are an
async-shape decision; both are large and neither blocks a mobile app.

---

## Not planned

Named so they are not mistaken for gaps: screen capture, system
notifications, credentials/keychain, thermal state, prevent-idle-sleep,
cursor-visibility control, and the system bell. gpui-ce has some of these; none
serves the mobile/first-release target, and each is additive if it is ever
wanted.
