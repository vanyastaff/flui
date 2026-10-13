# Interaction input completeness: dependency selection

- **Date:** 2026-10-13. Base: this branch on top of `main` @ `4da87df0b`, toolchain from
  `rust-toolchain.toml`.
- **For:** the input-completeness spec ([requirements](../plans/specs/flui-interaction/completeness/requirements.md),
  [design](../plans/specs/flui-interaction/completeness/design.md)) and its research
  ([best practices](2026-10-12-interaction-input-best-practices.md) "BP",
  [follow-ups](2026-10-12-interaction-input-followups.md) "FU").
- **Question:** for each external need of the spec, which crate (or own code), at which version
  and features, and why not the alternatives.

## Method and sources

- **Workspace version** is the requirement in the root `Cargo.toml` `[workspace.dependencies]`
  ("ws") or in a member manifest when the workspace has no entry ("member"), followed by the
  version `Cargo.lock` resolves ("lock").
- **Latest version, date, license** come from the crates.io API
  (`https://crates.io/api/v1/crates/<name>`, `.../versions`, `.../owners`), queried 2026-10-13.
  "Owners" are the crates.io owners: people, or a GitHub team written as `team:org`.
- **Advisories** come from `rustsec/advisory-db` (`crates/<name>/`), read through `gh api`.
- **Feature names** come from the crates.io feature list of the exact version
  (`.../crates/<name>/<version>`). Where the claim is about the source, the file and git ref
  are given.
- **deny.toml** names no crate individually, except in three places: `[bans] deny` (`atty`,
  `openssl`, `native-tls`, and `unicode-segmentation` with `winit` allowed as a wrapper), the
  advisory `ignore` list (RUSTSEC-2025-0057 `fxhash`, RUSTSEC-2026-0192 `ttf-parser` via
  winit's decorations), and the license `exceptions` (`webpki-root-certs`). The "deny.toml"
  column therefore records whether a crate is named there and whether its license is in
  `allow`. Two settings matter for new crates. `unmaintained = "all"` turns any unmaintained
  advisory, even a transitive one, into a failure. `[graph]` sets no `targets`, so
  dependencies declared for exotic targets count too.
- `[U]` marks an item not verified here.

## Summary

| Need | Decision |
|---|---|
| Win32 input, DnD, haptics, DirectManipulation | `windows` 0.62 (already), more features. **`InputHapticsManager` is not in any published `windows`:** windows-rs release 74 shipped `0.100.0` for the small crates but held `windows`/`windows-sys` back (crates.io still 0.62.2 / 0.61.2). Report it as `Unavailable` and generate a narrow local binding only if a device needs it. |
| Apple | `objc2` 0.6 / `objc2-*` 0.3.2 cohort (already); add `objc2-core-haptics` 0.3.2 for iOS. |
| Android | `android-activity` 0.6.1 + `jni` 0.22.4 (already); no direct `ndk`. |
| Linux | `winit` 0.30 now, 0.31 once stable. On Linux, 0.31's pen support is Wayland-only and X11 cannot start an outgoing drag. `ui-events-winit` 0.3 still pins winit 0.30 and blocks the move. No `smithay-client-toolkit`. |
| Web | `web-sys`/`wasm-bindgen`/`js-sys` (already). EditContext, `PointerEvent.altitudeAngle`/`azimuthAngle`/`getPredictedEvents` are **not bound**; write private `extern` blocks. `GestureEvent` is behind `web_sys_unstable_apis`. |
| Accessibility | `accesskit` 0.25.1 cohort (already). `accesskit_ios` 0.2.1 and `accesskit_android` 0.9.0 exist, but they pull older `objc2` 0.5 and `jni` 0.21 generations. |
| Recordings | `serde` + `serde_json` (already), own JSON Lines framing. **No binary codec in this spec.** `bincode` is out (RUSTSEC-2025-0141, unmaintained). If a binary encoding becomes necessary, use `rmp-serde` (self-describing). `postcard` is acceptable only with `default-features = false`. |
| Sets | `bitflags` 2 (already in ws) for `PointerKindSet` and `TransferActions`. `enumset` does not fit `PointerKind`. |
| Collections | `smallvec` 1 and `slab` (already). A reused `Vec` for the scroll-chain snapshot. No `arrayvec`. |
| Spatial index for `InputRegions` | Own code (a linear scan). No `rstar`. |
| Testing | `proptest`, `trybuild`, `criterion` (already); `insta` only for decision-trace snapshots. |

No new crate enters a shipped library except `objc2-core-haptics`, which is iOS-only and in the
cohort already locked. `bitflags` becomes a new normal edge of `flui-platform-api`.

---

## 1. Windows

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Win32 + WinRT bindings: pointer, DirectManipulation, OLE DnD, haptics, QPC, injection | `windows` | ws `0.62` / lock 0.62.2 (also 0.61.3 transitively) | 0.62.2 (2025-10-06); no newer `windows` on crates.io | owner `kennykerr` (Microsoft); MIT OR Apache-2.0; repo release 74 on 2026-09-03 | not named; license allowed | add `Win32_Graphics_DirectManipulation`, `Devices_Haptics`, `Devices_Input` (`PenDevice`), `UI_Input` (`PointerPoint`), `Win32_System_Performance` (QPC), `Win32_System_Com_StructuredStorage` and `Win32_UI_Shell_Common` if the OLE data object needs them. Already on: `Win32_UI_Input_Pointer`, `Win32_UI_Controls` (synthetic pointer injection), `Win32_System_Ole`, `Win32_System_Com`, `Win32_System_DataExchange`, `Win32_UI_Shell` (`IDragSourceHelper`) | `windows-sys`: has no COM/WinRT, and DirectManipulation, OLE and haptics are COM/WinRT. Hand-written COM vtables: unsafe and redundant. `winit`'s Win32 backend has no DirectManipulation, gestures or DnD source on 0.30 |
| COM objects FLUI implements (`IDirectManipulationViewportEventHandler`, `IDropTarget`, `IDropSource`, `IDataObject`) | `windows-core` `#[implement]` | ws `0.62` / lock 0.62.2 | 0.100.0 (2026-09-03), a different generation | same | not named | none (the macro is always on) | `windows-core` 0.100: mixing it with `windows` 0.62 types makes two incompatible COM runtimes. Move only together with `windows` |
| Raw Win32 for the devtools pipe | `windows-sys` | ws `0.61` / lock 0.61.2 (+0.52, 0.59, 0.60 transitively) | 0.61.2 (2025-10-06) | same | not named | unchanged | not used for input; listed because the brief asked |
| `InputHapticsManager` (touchpad/mouse haptics, Windows 11 SDK 10.0.28000) | **Own narrow binding, deferred**; `Availability::Unavailable` until then (design §7 already says so) | — | — | — | — | — | Bump `windows`: impossible, because no published `windows` has it. 0.62.2 (tag `71`) has `SimpleHapticsController` but 0 matches for `InputHapticsManager` in `Windows/Devices/Haptics/mod.rs`. Tag `74`, the source at version 0.100.0, has 26, but `windows` 0.100.0 was not published. The release newsletter ([windows-rs#4867](https://github.com/microsoft/windows-rs/issues/4867)) says the umbrella crates "may initially publish ... without `windows` and `windows-sys`" and tells libraries to "generate any other required APIs locally with `windows-bindgen`". Git dependency on windows-rs: `deny.toml` `allow-git = []`, and 0.100 renames many Win32 APIs (new header-derived metadata) |

**`InputHapticsManager` path.** Generate with the `windows-bindgen` release paired with the
`windows-core` the workspace links: 0.65.0 (2025-10-06) pairs with 0.62. Do not use bindgen
0.100.0 (2026-09-03): it emits code for `windows-core` 0.100. Run it against the Windows SDK
10.0.28000 `Windows.winmd`, filtered to `Windows.Devices.Haptics.InputHapticsManager` and the
`KnownSimpleHapticsControllerWaveforms` it needs, and check the output into `flui-platform`'s
Win32 backend as a private module. [U] Whether bindgen 0.65 accepts the 10.0.28000 winmd has
not been tried. If it does not, the haptics row stays `Unavailable` until windows-rs publishes
a `windows` with it. This is the same "typed `Unavailable`, blocked-external" rule as E4.

**Migration warning.** When `windows` 0.100 does publish, it is a large break. Win32 APIs are
grouped by header, enums and ownership types disappear, and raw `HRESULT`s and pointer/count
pairs appear (newsletter, "A new Windows API foundation"). `accesskit_windows` 0.35 pins
`windows` 0.62, so the platform backend and the a11y adapter must move together. Plan it as its
own task, not inside an input PR.

## 2. macOS and iOS

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Objective-C runtime | `objc2` | ws `0.6.4` (`std`); the iOS section repeats it as a literal with `exception` / lock 0.6.4 (+0.5.2 transitively) | 0.6.5 (2026-10-05) | owners `madsmtm`, `simlay`; MIT | not named | unchanged | `objc`/`cocoa`: already removed (the `flui-platform` manifest records why). `objc2` 0.5: the old generation |
| AppKit: tablet/pen events, magnify/rotate/swipe, `NSDragging*`, promised files, `NSHapticFeedbackManager` | `objc2-app-kit` | ws `0.3.2` / lock 0.3.2 | 0.3.2 (2025-10-04) | same owners; Zlib OR Apache-2.0 OR MIT | not named | add `NSHapticFeedback`, `NSDragging`, `NSDraggingItem`, `NSDraggingSession`, `NSFilePromiseProvider`, `NSFilePromiseReceiver`, `NSPasteboardItem`, `NSPressureConfiguration` (`NSEvent` and `NSPasteboard` already on) | none viable |
| UIKit: trackpad pinch/rotate/pan recognizers, coalesced/predicted touches, Pencil, screen edges, drag/drop interactions, feedback generators, context menus | `objc2-ui-kit` | member `0.3.2` (iOS section) / lock 0.3.2 | 0.3.2 (2025-10-04) | same | not named | add `UIPinchGestureRecognizer`, `UIRotationGestureRecognizer`, `UIPanGestureRecognizer`, `UIScreenEdgePanGestureRecognizer`, `UIPencilInteraction`, `UIDragInteraction`, `UIDropInteraction`, `UIDragItem`, `UIDragSession`, `UIDragPreview`, `UITargetedDragPreview`, `UISpringLoadedInteraction`, `UIFeedbackGenerator`, `UIImpactFeedbackGenerator`, `UISelectionFeedbackGenerator`, `UINotificationFeedbackGenerator`, `UICanvasFeedbackGenerator`, `UIContextMenuInteraction`, `UIContextMenuConfiguration` | none viable |
| Core Haptics (pattern tier, iOS) | **`objc2-core-haptics`** (new) | — / not in lock | 0.3.2 (2025-10-04) | owners `madsmtm`, `simlay`; Zlib OR Apache-2.0 OR MIT; 3,737 recent downloads (generated, low use) | not named; license allowed | `default-features = false`, `std`, `block2`, `CHHapticEngine`, `CHHapticPattern`, `CHHapticEvent`, `CHHapticParameter`, `CHHapticPatternPlayer`, `CHHapticDeviceCapability`, `CHHapticErrors`. No `objc2-avf-audio`. iOS target only: FU §1 found no documented native-macOS trackpad path through Core Haptics, so macOS uses `NSHapticFeedbackManager` | hand-written `extern_class!` bindings: the generated crate is the same code, and the same generator produced the rest of the cohort |
| Foundation: `NSItemProvider`, `NSProgress` (drag items, promised data) | `objc2-foundation` | ws `0.3.2` / lock 0.3.2 | 0.3.2 (2025-10-04) | owner set as above; MIT | not named | add `NSItemProvider`, `NSProgress` where the drag items need them | — |
| Blocks (completion handlers, `resetHandler`, drag-item loaders) | `block2` | member `0.6.2` (iOS deps, macOS dev-deps) / lock 0.6.2 | 0.6.2 (2025-10-04) | same; MIT | not named | unchanged | — |
| `CGEvent` injection for `cargo xtask device macos-input` | `core-graphics` (already a ws dep, used by `tools/desktop-mcp`) | ws `0.25` / lock — | 0.25.0 (2025-05-27) | servo; MIT OR Apache-2.0 | not named | — | `objc2-core-graphics` 0.3.2 is the objc2-family equivalent. Switch only if `core-graphics` goes unmaintained; two CoreGraphics binding crates for one tool is waste |

## 3. Android

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Activity glue, `MotionEvent`, key events, IME glue | `android-activity` | member `0.6` in **two** manifests (`flui-app`, `flui-platform`), `native-activity` / lock 0.6.1 | 0.6.1 (2026-03-24) | owners `rib`, `MarijnS95`, `team:rust-mobile`; MIT OR Apache-2.0 | not named | `native-activity` today. Stylus handwriting and a real `InputConnection` need `game-activity` (GameTextInput) [U: whether the spec's Android IME task switches] | `winit` Android backend: wraps the same crate with fewer fields (no stylus axes) |
| Haptics (`VibrationEffect`, `performHapticFeedback`), DnD (`View.startDragAndDrop`, `OnReceiveContentListener`), `ViewConfiguration`, system-gesture exclusion | `jni` | ws `0.22.4` / lock 0.22.4 | 0.22.4 (2026-03-16) | owners `jrobsonchase`, `stanislav-tkach`, `dmitry-timofeev`, `rib`; MIT OR Apache-2.0 | not named | unchanged | `ndk`: has no haptics/DnD APIs, which are Java-only. [U] `OnReceiveContentListener` and drag listeners are Java interfaces. `jni` cannot implement them, so a small Java/Kotlin shim class (own code) is needed |
| NDK input types | **no direct dependency**; reach them through `android-activity` | — / lock `ndk` 0.9.0, `ndk-sys` 0.6.0 (transitive) | `ndk` 0.9.0 (2024-04-26) | owners `dvc94ch`, `MarijnS95`, `team:rust-mobile` | not named | — | a direct `ndk` edge duplicates what `android-activity` re-exports, and pins a crate whose last release is from 2024 |

## 4. Linux

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Windowing and input on Wayland/X11 | `winit` **0.30** now; 0.31 after a stable release (S7) | ws `0.30.12` / lock 0.30.13 | stable 0.30.13 (2026-03-02); pre-release 0.31.0-beta.3 (2026-09-04) | owners `tomaka`, `daxpedda`, `madsmtm`, `kchibisov`, `team:rust-windowing`; Apache-2.0 | named: wrapper for `unicode-segmentation`; RUSTSEC-2026-0192 (`ttf-parser` via decorations) ignored | unchanged | own Wayland/X11 backend: out of scope (requirements, "Вне охвата"). `smithay-client-toolkit` 0.21.1 (2026-07-23; lock has 0.19.2 through winit): only for an own Wayland backend |
| Key translation inside the winit backend | `ui-events-winit` | member `0.3` / lock 0.3.0 | 0.3.0 (2026-01-18) | owners `waywardmonkeys`, `xorgy`; Apache-2.0 OR MIT | not named | unchanged | — (see the blocker below) |

**What winit 0.31.0-beta.3 gives Linux** (checked against tag `v0.31.0-beta.3`):

- **Pen:** "Pen input support on Wayland, Windows, and Web" (changelog `v0.31.md`). The X11
  backend has no tablet-tool handling. The beta.3 fix notes that X11 devices "misclassified as
  pens" had their events discarded, so X11 pen input stays `Unavailable` (design §7 already
  says so).
- **Gestures:** `PinchGesture`, `PanGesture`, `RotationGesture` and `HoldGesture` exist on
  Wayland. X11 has none, matching design §7's `Unavailable` for X11 touchpad gestures.
- **DnD:** incoming drags work on X11 and Wayland: `DragEntered`/`DragMoved`/`DragDropped`/
  `DragLeft` plus a lazy `fetch_data_transfer` (`winit-core/src/data_transfer.rs` exists at the
  tag). Outgoing `start_drag` is implemented in `winit-win32`, `winit-appkit` and
  `winit-wayland`. The `winit-core` default returns `NotSupported`, and `winit-x11` does not
  override it. So "bidirectional DnD on winit 0.31" (design §7, Linux row) holds on Wayland only.
  X11 gets drop-target-only and a typed `Unavailable` for drag source.
- **Blocker:** `ui-events-winit` main still depends on `winit = "0.30.10"`
  (`endoli/ui-events` `ui-events-winit/Cargo.toml`, last push 2026-08-21). The 0.31 move
  therefore needs one of: an upstream release, a `[patch]` to a fork (needs `allow-git`), or
  replacing the ~one-function delegation with own translation over `keyboard-types`. The last
  is cheapest. The table is the one `ui-events` already ships for Android and can be read as a
  reference.

## 5. Web

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| DOM events, DnD, Vibration | `web-sys` | member `0.3` / lock 0.3.104 | 0.3.106 (2026-09-25) | owners `RReverser`, `daxpedda`, `guybedford`, `team:wasm-bindgen`; MIT OR Apache-2.0 | not named | add `DragEvent`, `DataTransfer`, `DataTransferItem`, `DataTransferItemList`, `TouchEvent`, `Touch`, `CompositionEvent`, `InputEvent`, `HtmlTextAreaElement`, `UserActivation` (stable; `Navigator::vibrate_with_pattern` is already under `Navigator`). `GestureEvent` (Safari pinch) exists but is `#[cfg(web_sys_unstable_apis)]` in `gen_GestureEvent.rs`; bind it by hand rather than set a crate-wide `--cfg` | `stdweb`/`gloo-events`: dead or an extra layer over the same bindings |
| JS glue | `wasm-bindgen`, `js-sys` | member `0.2` / lock 0.2.127; ws `js-sys = "0.3"` / lock 0.3.104 | 0.2.129 / 0.3.106 (2026-09-25) | same | not named | — | — |
| `EditContext` (Web IME, P14) | **own `extern "C"` block** (~15 items, FU §2), feature-detected, with a hidden `<textarea>` fallback | — | — | — | — | — | `web-sys`: no `EditContext`, `TextUpdateEvent` or `TextFormatUpdateEvent` in 0.3.106's 1,727 features, and no `EditContext` anywhere in `wasm-bindgen/wasm-bindgen` (code search, 0 hits). Delete the module once web-sys binds it |
| Pen tilt and prediction (`PointerEvent.altitudeAngle`, `azimuthAngle`, `getPredictedEvents`) | **own `extern` items** next to the EditContext ones | — | — | — | — | — | `web-sys` main `gen_PointerEvent.rs` has `twist` and `get_coalesced_events`, but no `altitude`/`azimuth`/`predicted` (0 matches) |

## 6. Accessibility

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Tree model, custom actions (A1, A7) | `accesskit` + `accesskit_consumer` | ws `0.25.1` / `0.39.1`; lock same | 0.25.1 / 0.39.1 (2026-09-25) | owners `DataTriny`, `mwcampbell`; MIT OR Apache-2.0 | not named | unchanged | — |
| Desktop adapters | `accesskit_windows`, `accesskit_macos`, `accesskit_unix` | member `0.35.1` / `0.27.1` / `0.24`; lock 0.35.1 / 0.27.1 / 0.24.0 | same versions, 2026-09-25 (current) | same | not named | unchanged (`a11y` feature) | `accesskit_winit` 0.34.1: FLUI owns the window, not winit, on Win32/macOS |
| Mobile adapters | **not adopted in this spec**; contribute upstream (A7) | — | `accesskit_ios` 0.2.1 (2026-09-25, first release 2026-05-11; owner `DataTriny`); `accesskit_android` 0.9.0 (2026-09-25) | MIT OR Apache-2.0 | not named | — | Design §17 lists them as "upstream AccessKit iOS/Android", but **both exist now**. `accesskit_ios` 0.2.1 depends on `objc2 ^0.5.1`, `objc2-ui-kit ^0.2.0`, which would be a second Apple binding generation next to the 0.6/0.3 cohort. `accesskit_android` depends on `jni ^0.21.1`, a second `jni` next to 0.22.4. Adopt each once it moves to the current cohort; record the custom-action gaps (FU recommendation 5) against these crates |

## 7. Keyboard and cursor

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| W3C `Key`/`Code` for gesture shortcuts (A2) | `keyboard-types` | ws `0.8` / lock 0.8.3 | 0.8.3 (2025-10-02) | owners `pyfisch`, `madsmtm`, `kchibisov`, `team:rust-windowing`; MIT OR Apache-2.0 | not named | `serde` only where recordings carry keys (through the `serde` feature chain) | own enum: duplicates the W3C tables |
| Cursor shapes (D4 drag cursor, resize) | `cursor-icon` | ws `1.2` / lock 1.2.0 | 1.2.0 (2025-05-25) | owners `kchibisov`, `notgull`, `team:rust-windowing`; MIT OR Apache-2.0 OR Zlib | not named | unchanged | — |
| Pointer vocabulary | **own** (`flui-platform-api`, ADR-0143) | ws `ui-events = "0.3"` / lock 0.3.0 | 0.3.0 (2026-01-18) | owners `waywardmonkeys`, `xorgy` | not named | reference only; keep the existing backend-internal uses (Android keycode tables in `platforms/android/input.rs`, winit key delegation) | `ui-events` types in signatures: `PointerGesture` has only `Pinch(f32)`/`Rotate(f32)` without phases (BP §3), and has no `InputSequenceId`, `Revise` or `Virtual` kind |

## 8. Recording serialization (R2, R3, R7)

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| DTO derives | `serde` | ws `1.0` (`derive`, `rc`) / lock 1.0.229 | 1.0.229 (2026-07-18) | `dtolnay`, `team:serde-rs`; MIT OR Apache-2.0 | not named | `flui-platform-api/serde` already exists; DTOs in `flui-testing` with pinned `#[serde(rename)]` | — |
| JSON Lines text format | `serde_json` + **own framing** (`to_writer` + `\n`; `Deserializer::into_iter` to read) | ws `1.0` / lock 1.0.151 | 1.0.154 (2026-10-11) | `dtolnay`; MIT OR Apache-2.0 | not named | default (`std`). Not `preserve_order` or `arbitrary_precision`: no float-as-string surprises in recordings; `float_roundtrip` if the round-trip gate (R7) shows drift [U] | `jsonlines`/`serde_jsonlines` crates: ~20 lines of framing do not justify a dependency |
| Compact binary for long captures (FU recommendation 7) | **none in this spec.** design §10 and R2 specify JSON Lines only. If profiling shows a need: `rmp-serde` | — | `rmp-serde` 1.3.1 (2025-12-23) | owners `kornelski`, `3Hren`; MIT | not named | `to_vec_named` (map-encoded structs) | see below |

Binary candidates:

- **`bincode`: rejected.** RUSTSEC-2025-0141 (2025-12-16, informational "unmaintained"):
  development ceased "due to a doxxing and harassment incident". The crates.io 3.0.0
  (2025-12-16, owner `ghost_3672`) is the end-of-life release. With `unmaintained = "all"`,
  `cargo xtask deps` fails on any edge to it.
- **`postcard` 1.1.3 (2025-07-24, `jamesmunns`, MIT OR Apache-2.0): acceptable but second
  choice.** Its wire format is specified and stable at 1.x. But it is not self-describing, so a
  reader cannot skip unknown fields. That breaks the "unknown keys ignored" versioning rule FU
  takes from `libinput record`, because DTO structs grow fields (pen axes, `Revise`). The
  default feature `heapless-cas` pulls `heapless` 0.7, whose target-specific
  `atomic-polyfill` (RUSTSEC-2023-0089, unmaintained) counts under deny's all-targets graph.
  If chosen, use `default-features = false, features = ["use-std"]`.
- **`bitcode` 0.6.9: rejected.** Its format is not stable across versions, which is wrong for
  checked-in recordings.
- **`ciborium` 0.2.2: rejected.** CBOR would be the natural self-describing choice, but the
  last release was 2024-01-24.
- **`rmp-serde`: preferred if needed.** It is self-describing with named fields, so it keeps the
  JSON evolution rules with only a size win. It has 2 normal dependencies (`rmp`, `serde`).

## 9. Small collections, IDs and errors

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Inline small lists (hit-test paths, per-event contacts, coalesced samples, arena members) | `smallvec` 1 | ws `1.13` (`union`, `const_generics`) / lock 1.16.1 | stable 1.16.3 (2026-10-10); 2.0.0-beta.2 (2026-10-03) | `SimonSapin`, `Ms2ger`, `team:servo`; MIT OR Apache-2.0; old advisories all patched in 1.x | not named | unchanged; do not move to 2.0 betas | `arrayvec` 0.7.8 (lock has it transitively): a fixed cap with no spill would turn a 9th finger or a deep path into an error. `tinyvec` 1.13.3: needs `Default` elements and is no gain over smallvec here |
| Nested-scroll chain snapshot (C9 "no allocations") | **own: a `Vec` reused by the dispatcher** (cleared, not dropped) | — | — | — | — | — | `SmallVec<[_; N]>`: allocates past N on deep nesting. A reused `Vec` allocates once per high-water mark and never again |
| Session/sequence tables (DnD sessions, scroll and pan-zoom sequences) | `slab` behind FLUI's generational key pattern (AGENTS "ID offset") | ws `0.4.12` / lock 0.4.12 | 0.4.12 (2026-01-31) | `carllerche`, `team:tokio-rs`; MIT | not named | — | `slotmap`: a second arena idiom next to the workspace's slab + generation pattern. `HashMap`: no slot reuse |
| `InputSequenceId`, `DragSessionId` | **own** `NonZeroU64` counter, permanent refusal on exhaustion (B3) | — | — | — | — | — | `uuid` (already transitive via accesskit): 128-bit and random, while B3 wants monotonic and refusal at exhaustion |
| Small keyed maps (latch by `(InputSurface, DeviceId)`) | `rustc-hash` `FxHashMap`, or a linear `SmallVec` for ≤ 4 entries | ws `2.1` | — | — | not named | — | std `HashMap` with SipHash: slower for small integer keys and seeds randomly per process, which nondeterministic iteration does not need |
| Errors (`RecordingError`, composition errors) | `thiserror` 2 | ws `2.0` / lock 2.0.20 (+1.0.69 transitively) | 2.0.21 (2026-09-23) | `dtolnay`; MIT OR Apache-2.0 | not named | — | `anyhow` in libraries: forbidden by `docs/PANIC-POLICY.md` / AGENTS |

## 10. Flag sets: `PointerKindSet`, `TransferActions`

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| `allowed_devices: PointerKindSet` (G3); `TransferActions`/`AllowedActions` (D3) | `bitflags` 2 (new normal edge of `flui-platform-api`) | ws `2.10` / lock 2.13.2 (+1.3.2 transitively) | 2.13.2 (2026-09-10) | `KodrAus`, `rust-lang-owner`; MIT OR Apache-2.0; no RustSec entries | not named | `serde` through `flui-platform-api/serde` (text form `"COPY \| MOVE"` reads well in JSONL) | `enumset` 1.1.14 (2026-07-20; `tiffany352`, `Lymia`): `#[derive(EnumSetType)]` needs a fieldless enum, but `PointerKind` has `Pen { tool }` and is `#[non_exhaustive]`. It would force a parallel fieldless kind enum anyway and adds a proc-macro (`enumset_derive`). Hand-rolled `u8`: what `TransferActions` is today (`data_transfer.rs:122`, "no external bitflags dependency"). It is a second copy of set algebra, `Debug` and iteration |

`PointerKind -> PointerKindSet` mapping (`Pen { .. }` → `PEN`, etc.) is own code, a
`const fn` next to the enum. Cost: `bitflags!` types implement the public `bitflags::Flags`
trait, which makes `bitflags` 2 a public dependency of a Stable contract crate. Its major
version has been stable since 2023, so this is acceptable.

## 11. Testing and benchmarks

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Arena, axis-decision and round-trip properties (G2 44°/45°/46°, R7) | `proptest` | ws `1` / lock 1.11.0 | 1.12.0 (2026-10-10) | `Centril`, `AltSysrq`, `team:proptest-rs`; MIT OR Apache-2.0 | not named | dev-only | `quickcheck`: weaker shrinking and no strategy combinators; the workspace already standardized on proptest |
| Compile-fail: typed `GestureDetector` slots (W1), `unstable-input` gating | `trybuild` | ws `1.0` / lock 1.0.121 | 1.0.122 (2026-10-07) | `dtolnay`; MIT OR Apache-2.0 | not named | dev-only, its own nextest group | — |
| Benches (pointer assembly, channel, observer with no subscribers, Q3) | `criterion` | ws `0.8` / lock 0.8.2 | 0.8.2 (2026-02-04) | `berkus`, `bheisler`, `lemmih`, `team:rust-bus`; Apache-2.0 OR MIT | not named | unchanged | `divan` 0.1.21: last release 2025-04-10; a second harness. Allocation budgets are better pinned by a counting allocator in a test than by a bench |
| Arena decision-trace snapshots | `insta` (optional) | ws `1` / lock 1.48.0 | 1.49.0 (2026-10-03) | `mitsuhiko`, `max-sixty`; Apache-2.0 | not named | no extra features (`assert_snapshot!` on the text trace) | golden recordings (R8) are **inputs**, read as fixture files, not snapshots |

## 12. Time

| Need | Choice | ws / lock | Latest (date) | Maintenance, license | deny.toml | Target features | Alternatives rejected |
|---|---|---|---|---|---|---|---|
| Wasm-safe `Instant` at the platform edge | `web-time` | ws `1.1` / lock 1.1.0 | 1.1.0 (2024-03-01) | `daxpedda` (also a wasm-bindgen and winit owner); MIT OR Apache-2.0 | not named | — | none. The spec removes reads from the arena (design §1: deadlines on `EventTime`), and `flui-interaction` has 26 `web_time` mentions to retire. The crate stays for backend timestamps and `FrameClock` |
| QPC per device/sequence (P4) | **own**, over `windows` `Win32_System_Performance` | — | — | — | — | — | `quanta`: TSC calibration; QPC is what `POINTER_INFO.PerformanceCount` is expressed in |

## 13. Spatial index for `InputRegions` (P7)

**Own code: a per-window `Vec` of rectangles, rebuilt at compositing, scanned linearly.** The
consumer is the OS (Android `setSystemGestureExclusionRects` caps the exclusion at 200 dp per
edge, iOS takes edges, not rects). The count is single digits, and the query rate is one
hit-test per Down. `rstar` 0.13.0 (2026-05-24, `team:georust`, MIT OR Apache-2.0) is an R*-tree
for thousands of entries. It would add a dependency for nothing measurable.

## 14. Existing hand-rolled code: keep or replace

| Code | Verdict | Reason |
|---|---|---|
| `TransferActions(u8)` (`crates/flui-platform-api/src/data_transfer.rs:125`) | **Replace with `bitflags`**, in the same change as `PointerKindSet` | one set idiom for named flags; serde and `Debug` for free |
| `Modifiers(u16)` (`keyboard/modifiers.rs`) | Keep, or migrate in the same mechanical commit | W3C-shaped with a pinned `Debug` (`{CONTROL, SHIFT}`, a doctest). Migration is optional |
| `PointerButtons(u32)` (`pointer/buttons.rs`) | Keep | a set indexed by `PointerButton` values, not a closed list of named flags |
| Velocity least-squares solver (`processing/lsq_solver.rs`, 323 lines) | Keep | a 2–3 degree weighted fit; `nalgebra` would be a large dependency for one small solve |
| One Euro filter (`processing/one_euro.rs`, 307 lines with tests) | Keep | `one-euro` 0.9.0 (2026-09-10, ~11k downloads) and `one-euro-rs` 0.2.0 (2019) are small and young or stale; the filter is a few lines of math wired to FLUI's time type |
| Resampler (`processing/resampler.rs`) | Keep | frame-synchronous, FLUI's own vsync contract |
| `PointerScript` replay (`crates/flui-testing/src/replay.rs`) | Keep and extend (R1) | no crate models virtual-time pointer scripts |
| winit key delegation through `ui-events-winit` | Keep on 0.30; replace with own `keyboard-types` translation if `ui-events-winit` has not moved when winit 0.31 goes stable | §4 blocker |

## 15. Manifest hygiene found along the way

These are outside this spec's scope. Each conflicts with AGENTS "shared dependencies go through
`[workspace.dependencies]`" and is worth a separate small change:

- `android-activity = { version = "0.6", ... }` is declared literally in both `flui-app` and
  `flui-platform`.
- `slab = "0.4"` appears in `flui-rendering` and `flui-semantics`, and `smallvec = "1.15"` in
  `flui-semantics`, although the workspace has both entries.
- `wasm-bindgen-test = "=0.3.77"` is repeated literally in `flui-engine`, `flui-foundation` and
  `flui-runtime`, next to the workspace entry with the same pin.
- `objc2`/`objc2-foundation` are repeated by version in `flui-platform`'s iOS section, and
  `web-sys`/`wasm-bindgen` are member-only. Fine while one crate uses them, but the new
  `objc2-core-haptics` should get a workspace entry next to the cohort.

## Open items

- [U] Whether `windows-bindgen` 0.65 generates `InputHapticsManager` from the SDK 10.0.28000
  winmd against `windows-core` 0.62.
- [U] Whether the Android IME/stylus tasks need `android-activity`'s `game-activity` feature,
  and the Java shim for listener interfaces.
- When winit 0.31 goes stable: re-check X11 drag source and pen, and `ui-events-winit`'s winit
  requirement.
- When `windows` 0.100 publishes: a dedicated migration together with `accesskit_windows`.
