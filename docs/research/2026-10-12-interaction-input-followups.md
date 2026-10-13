# Interaction input: follow-up research

- **Date:** 2026-10-12. Companion to
  [`2026-10-12-interaction-input-best-practices.md`](2026-10-12-interaction-input-best-practices.md)
  (the "prior note"). Workspace baseline unchanged: `windows = "0.62"` (0.62.2 resolved),
  `accesskit = "0.25.1"`, `web-sys` 0.3.106, `winit` 0.30.12 (0.31.0-beta.3 is the newest
  pre-release on crates.io).
- **Scope:** the owner's decisions are taken as given (FLUI-owned input vocabulary per
  ADR-0143; arena for contact ownership plus a synchronous nested-scroll delta chain;
  `Simultaneous` relation and nestable composition superseding ADR-0167; `AxisPolicy`;
  DirectManipulation on Windows; winit 0.31 on Linux; two-tier haptics superseding ADR-0031;
  unified DnD extending ADR-0038; a `trace` feature in `flui-platform-api`; per-platform gesture
  presets; semantics actions on every gesture widget; no new crates). This note makes them
  concrete.
- **Method:** primary sources only. Source code is cited at a commit SHA with line numbers;
  Apple reference pages were read through their documentation JSON (the abstract and
  availability fields), Microsoft and W3C pages directly.
- **Legend:** **[U]** = could not be confirmed from a primary source in this session.
  **[†]** = canonical page cited but not re-read in this session.

---

## Recommendations summary

1. **Windows touchpad haptics now has an app API.** `InputHapticsManager` (UniversalApiContract
   19, Windows 11 SDK 10.0.28000.1721, March 2026) routes a waveform to "the device that most
   recently sent input" with a fallback waveform. The pinned `windows` 0.62.2 does **not**
   contain it, although `windows-rs` main does. The Windows haptics backend therefore needs a
   `windows` crate bump or a narrow hand-written binding; plan the bump.
2. **Trackpad gestures are certified by trace replay plus hardware, never by OS injection.**
   Windows injection accepts only `PT_TOUCH`/`PT_PEN`. macOS magnify/rotate can be
   synthesized only in-process through private CoreGraphics fields, as WebKit's test runner
   does. Neither belongs in FLUI. The `PlatformInput` trace format is the conformance boundary.
3. **Web IME: EditContext where it exists, a hidden `<textarea>` elsewhere, behind one
   adapter that drives FLUI's `TextStore`.** EditContext ships only in Chromium (121+); Firefox
   has a positive position but no implementation; WebKit has no position. `web-sys` (0.3.106 and
   wasm-bindgen main) has no binding, so FLUI writes ~15 `extern` items. VS Code uses exactly
   this split (EditContext by default when supported, textarea fallback).
4. **Haptic primitive patterns = Core Haptics' event model.** Android 16's
   `BasicEnvelopeBuilder` adopted the same normalized `intensity`/`sharpness` control points,
   so `Transient { intensity, sharpness }` and `Continuous { envelope }` is the common subset.
   Every pattern call carries a mandatory `HapticIntent` fallback, mirroring Windows'
   `TrySendHapticWaveform(waveform, fallback)` and Android's `VibrationEffect.Builder` fallback.
5. **Gesture accessibility is blocked upstream, not in FLUI's model.** AccessKit models
   `custom_actions` on `Node`, but no platform adapter on AccessKit main exposes them. The
   AT-SPI adapter exposes only `"click"`, and none maps `ShowContextMenu`. FLUI also never
   publishes its `CustomSemanticsAction` list to AccessKit (only the `CustomAction` bit). Fix
   FLUI's side now; contribute adapter support upstream (macOS `NSAccessibilityCustomAction`,
   iOS `UIAccessibilityCustomAction`, Android `AccessibilityAction(id, label)`). UIA has no
   custom-action concept [U].
6. **Nested scroll chain: Compose's four-callback contract, synchronous.** Pre-scroll runs
   **outermost-first**, post-scroll innermost-first (Compose `NestedScrollNode.kt` L81–85). FLUI
   keeps fling admission synchronous and lets each owner start its own ballistic run, which is
   what ADR-0169 already does. The dispatcher snapshots the chain, validates every returned
   consumption and holds no borrow across a callback.
7. **Trace format: versioned, device-described, relative-timestamped, two encodings.**
   Model it on `libinput record` (a version number bumped only on incompatible change, a
   device section, events relative to the recording start, unknown keys ignored). Serialize
   `PlatformInput` values with serde. Use JSON Lines for checked-in fixtures and a compact
   binary encoding for long captures. Record both the OS timestamp and arrival time.

---

## 1. Resolving the prior note's unverified items

| Item | Finding | Status |
|---|---|---|
| Windows app-facing touchpad haptics | **Exists.** `InputHapticsManager.GetForCurrentThread()` → `TrySendHapticWaveform(waveform, fallback)`, `TrySendHapticWaveformForPlayCount(…, intensity, playCount, replayPauseInterval)`, `TryStopFeedback`, `IsSupported()`. Min SDK 10.0.28000.1721 (March 2026); `Align`/`Collide`/`Step`/`Grow` waveforms need 10.0.28000.1839 (April 2026). It routes to the device that most recently sent input to the thread, so it must run on the UI thread. Supported devices "such as mice, touchpads, and pens" ([haptics how-to](https://learn.microsoft.com/en-us/windows/apps/develop/input/haptics)). The listed hardware is Surface Laptop 8 touchpads, MX Master 4 and three pens. | Resolved |
| `windows` crate exposure | `Devices_Haptics`, `Win32_Graphics_DirectManipulation`, `Win32_UI_Input_Pointer` are features of `windows` 0.62.2 (crates.io feature list). The 0.62.2 source has `SimpleHapticsController` but **no** `InputHapticsManager` (0 matches in `src/Windows/Devices/Haptics/mod.rs`). `windows-rs` main has it ([`mod.rs` @ a6b6ef5](https://github.com/microsoft/windows-rs/blob/a6b6ef58f8efb2f2bdef86a2f6cd5f768eb7c545/crates/libs/windows/src/Windows/Devices/Haptics/mod.rs)). | Resolved |
| Precision-touchpad injection on Windows | `CreateSyntheticPointerDevice`: pointerType "Must be either PT_TOUCH or PT_PEN" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice)). There is no touchpad path; DirectManipulation's touchpad input must be certified on hardware. | Resolved |
| macOS `CGEvent.post` permission | `CGPreflightPostEventAccess()` and `CGRequestPostEventAccess()` exist (macOS 10.15) as the documented gate for posting events. The reference pages carry no prose naming the TCC pane. | Partly: the gate is confirmed; that it is the Accessibility pane is still [U] |
| Synthesizing magnify/rotate on macOS | There is no public API. WebKit's test runner builds an `NSEvent` in-process from `CGEventCreate(nullptr)` with private type `kCGSEventGesture = 29` and private fields `kCGEventGestureHIDType` (110), `kCGEventGestureZoomValue` (113), `kCGEventGestureRotationValue` (114), then calls the private `-[NSEvent _initWithCGEvent:eventRef:]` ([`SyntheticNSEvent.mm` L116–137 @ 58c2111](https://github.com/WebKit/WebKit/blob/58c2111ae9464868fee99051f4aa25e58b610f5c/Tools/TestRunnerShared/mac/SyntheticNSEvent.mm#L116-L137), [`CoreGraphicsTestSPI.h` L41–44, L75](https://github.com/WebKit/WebKit/blob/main/Tools/TestRunnerShared/spi/CoreGraphicsTestSPI.h)). Because it is never posted, it needs no TCC permission, but it is private SPI and in-process only. **Not suitable for FLUI**; test at the `PlatformInput` boundary. | Resolved |
| macOS coalesced samples (table cell "— [U]") | `NSEvent.isMouseCoalescingEnabled` (10.5, default enabled) covers "mouse-moved, mouse-dragged, and tablet events" ([docs](https://developer.apple.com/documentation/appkit/nsevent/ismousecoalescingenabled)). There is no public API that returns the coalesced samples, so the backend's only option is turning coalescing off. | Resolved; the cell should read "coalescing toggle only" |
| Core Haptics on Mac hardware | `CHHapticEngine` lists macOS 10.15, but the Core Haptics framework page lists only iOS, iPadOS, Mac Catalyst, tvOS and visionOS, not native macOS ([framework](https://developer.apple.com/documentation/corehaptics)). The documented Mac path is game controllers: `GCDeviceHaptics.createEngine(withLocality:)` (macOS 11) returns a `CHHapticEngine` ([docs](https://developer.apple.com/documentation/gamecontroller/gcdevicehaptics/createengine(withlocality:))). The built-in Force Touch trackpad is driven by `NSHapticFeedbackManager` only. | Partly: whether a native Mac `CHHapticEngine()` drives the trackpad is [U]; treat it as unsupported |
| XCUITest pressure / Pencil | The public `XCUIElement` API has no pressure. WebDriverAgent reaches pressure through the private `XCPointerEventPath -pressDownWithPressure:atOffset:` ([header L42–44](https://github.com/appium/WebDriverAgent/blob/master/PrivateHeaders/XCTest/XCPointerEventPath.h)), gated on `XCUIDevice.supportsPressureInteraction` ([`FBW3CActionsSynthesizer.m` L273–279](https://github.com/appium/WebDriverAgent/blob/master/WebDriverAgentLib/Utilities/FBW3CActionsSynthesizer.m)). There is no altitude, azimuth or roll anywhere, so Pencil tilt has no injection path. | Resolved (no public path) |
| ctrl+wheel pinch convention | Defined by implementations, not by any spec. Chromium: `CreateSyntheticWheelFromTouchpadPinchEvent` adds `kControlKey`, with `delta_y = 100 * ln(scale)`; "to convert a wheel deltaY back to a scale use Math.exp(-deltaY/100)" ([`touchpad_pinch_event_queue.cc` L19–60](https://github.com/chromium/chromium/blob/main/components/input/touchpad_pinch_event_queue.cc)). Firefox: `PinchGestureInput` sets `MODIFIER_CONTROL` and matches Chrome "so that pages can implement custom pinch gesture handling" ([`widget/InputData.cpp` L678, L697–716](https://github.com/mozilla-firefox/firefox/blob/main/widget/InputData.cpp)); macOS shipped in Firefox 55 ([bug 1052253](https://bugzilla.mozilla.org/show_bug.cgi?id=1052253)). | Resolved (de facto). The web backend decodes `ctrlKey && deltaMode==PIXEL` wheel as `PanZoom` with `scale = exp(-deltaY/100)`. This cannot be told apart from a real Ctrl+wheel [U], so only `PanZoom` from Safari `GestureEvent` is unambiguous. |
| `objc2-core-haptics` | Exists, version 0.3.2 on crates.io. | Resolved |
| Masonry has no arena | Masonry routes pointer events to a single `pointer_capture_target`, else hit-test, then bubbles with `set_handled` ([`masonry_core/src/passes/event.rs` L22–26, L256–259 @ 271a27a](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry_core/src/passes/event.rs#L22-L26)). Its only "arena" types are storage (`widget_arena.rs`, `property_arena.rs`). | Resolved |
| Apple gesture constants (slop, timeouts) | No public UIKit/AppKit constant was found. `NSEvent.doubleClickInterval` [†] is the only documented timing. | Still [U]: presets for Apple platforms must be measured, not cited |

---

## 2. Web IME

### 2.1 EditContext status

- **Spec:** W3C Working Draft, 10 September 2026, from the Web Editing WG
  ([TR](https://www.w3.org/TR/edit-context/)). Surface: `new EditContext({text, selectionStart,
  selectionEnd})`; `updateText`, `updateSelection`, `updateControlBounds`,
  `updateSelectionBounds`, `updateCharacterBounds`, `attachedElements`, `characterBounds`;
  events `textupdate`, `textformatupdate`, `characterboundsupdate`, `compositionstart`,
  `compositionend`. It attaches through `element.editContext = ctx`, to one element at a time
  ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/EditContext_API)).
- **Support (MDN browser-compat-data, `api/EditContext.json`):** Chrome 121, Edge mirrors
  Chrome; Firefox `false` ([bug 1904161](https://bugzil.la/1904161)); Safari and iOS Safari
  `false` ([webkit.org/b/269922](https://webkit.org/b/269922)).
- **Positions:** Mozilla **positive**, with compatibility and accessibility concerns
  ([mozilla/standards-positions#199](https://github.com/mozilla/standards-positions/issues/199),
  closed). WebKit has **no position** ([WebKit/standards-positions#243](https://github.com/WebKit/standards-positions/issues/243),
  open, `position: null` in `summary.json`).
- **Rust bindings:** `web-sys` 0.3.106 has no `EditContext`/`TextUpdateEvent` feature (none in
  `src/features`). wasm-bindgen main has no EditContext WebIDL either.

### 2.2 How others do it

- **VS Code** ships both. `editor.editContext` defaults to `true` and takes effect only when
  `env.editContextSupported` ([`editorOptions.ts` L2088, L6330 @ main](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/config/editorOptions.ts)).
  The native path listens to `textformatupdate`, `characterboundsupdate` and `textupdate`, and
  pushes `updateSelectionBounds`/`updateControlBounds`/`updateCharacterBounds`
  ([`nativeEditContext.ts` L196–199, L575–611 @ 5f0e803](https://github.com/microsoft/vscode/blob/5f0e8037292058da10b7dd99c942685ca8d99275/src/vs/editor/browser/controller/editContext/native/nativeEditContext.ts#L196-L199)).
  Screen-reader content is a separate DOM mirror (`screenReaderSupport.ts`); the textarea
  implementation stays under `editContext/textArea/`.
- **Flutter web** uses a hidden, transparent `<input>`/`<textarea>` (`color`, `background` and
  `caret-color` set to `transparent`) that mirrors the full editing state. Per-engine strategies
  handle the quirks: `SafariDesktopTextEditingStrategy`, `IOSTextEditingStrategy`,
  `AndroidTextEditingStrategy`, `FirefoxTextEditingStrategy`
  ([`text_editing.dart` L87–91, L1344, L1436, L2058, L2240, L2309 @ 2ea6d1c](https://github.com/flutter/flutter/blob/2ea6d1cbec36bb58af5523ad95a921f946492beb/engine/src/flutter/lib/web_ui/lib/src/engine/text_editing/text_editing.dart#L87-L91)).
  No EditContext.
- **egui** uses a single 1×1 px, `opacity: 0` `<input>` and handles
  `compositionstart`/`input`/`compositionend`
  ([`text_agent.rs` L27–50, L76–96 @ 65e7db3](https://github.com/emilk/egui/blob/65e7db3c06d779c60ac56647bdd3011ed8ba1cbd/crates/eframe/src/web/text_agent.rs#L27-L50)).
  The input carries no surrounding text, so IME reconversion and autocorrect context are lost.
- **Xilem** on the web is `xilem_web`, which renders real DOM elements, so it uses native
  inputs. Masonry has no web text path. Neither is a pattern for a canvas renderer.

### 2.3 Recommended design (all three engines)

One `WebTextInput` adapter inside the web backend implements the platform half of
`TextInputHandle::attach` and talks to FLUI's `TextStore` (ADR-0090, ADR-0142) only through
the store's lock and observer protocol. It has two strategies, chosen once per attach by
feature detection (`"EditContext" in window`):

| Concern | EditContext strategy (Chromium) | Mirror-textarea strategy (Gecko, WebKit, fallback) |
|---|---|---|
| Host element | the canvas, or a focusable `<div>` overlaying it, with `editContext` set | an off-screen-clipped, transparent `<textarea>` (not `display:none`, which loses focus and IME) positioned at the caret |
| Text → OS | `updateText`/`updateSelection` from `TextStoreObserver::text_changed`/`selection_changed` | write a **window** of the store text (the paragraph around the selection, capped to a few KiB) plus `setSelectionRange`, recording its offset |
| OS → text | `textupdate` → `request_lock(ReadWrite)` → replace `[updateRangeStart, updateRangeEnd)` | `beforeinput` (`inputType`, `getTargetRanges`) → store edit; `input`/`composition*` as a resync check |
| Composition style | `textformatupdate` → store composition attributes | `compositionupdate` data only; no underline styles exist |
| Geometry | `characterboundsupdate` → `layout_changed` query → `updateCharacterBounds`; `updateControlBounds`/`updateSelectionBounds` each frame the caret moves | move the textarea to the caret rect, so the IME candidate window anchors there |
| Accessibility | separate DOM mirror from the semantics tree (as VS Code does) | the textarea's own value is announced; keep it in sync with the window |

Rules that keep this sound:

- **UTF-16 at the edge.** EditContext and DOM offsets are UTF-16 code units. The store already
  has a `utf16` module; convert there, never in the strategy.
- **Echo suppression is generational.** Writing to the textarea fires `input`, so tag writes
  with the store `generation` and ignore events whose post-state equals the last write.
  Mismatches resync from the store, which is authoritative.
- **Late-arriving IME events are next-frame work.** DOM events land outside the frame. They
  take a store lock (the `CommitGate` already defers grants) and never call into build.
- **Bindings:** a private module with
  `#[wasm_bindgen] extern "C" { type EditContext; #[wasm_bindgen(constructor)] fn new(init: &EditContextInit) -> EditContext; #[wasm_bindgen(method, js_name = updateText)] fn update_text(this: &EditContext, start: u32, end: u32, text: &str); … }`
  plus `type TextUpdateEvent` with `update_range_start`, `update_range_end`, `text`,
  `selection_start`, `selection_end` getters, and a `#[wasm_bindgen(method, setter = editContext)]`
  on `HtmlElement`. Detection uses `js_sys::Reflect::has(&window, &"EditContext".into())`.
  When `web-sys` gains a binding, delete the module.
- **Tests:** the strategy logic is pure (`event → store edits`) and is table-tested
  headlessly. Browser runs (`wasm-bindgen-test` in Chrome and Firefox) cover only that
  feature detection picks each strategy. IME composition itself cannot be driven by WebDriver
  [U], so it is a manual certification row per engine.

---

## 3. Haptic primitive patterns

### 3.1 What each platform can express

| Platform | Discrete primitives | Parametric | Capability query |
|---|---|---|---|
| Core Haptics (iOS 13+, Catalyst; Mac via `GCDeviceHaptics`) | `hapticTransient` | `hapticContinuous` with `hapticIntensity`, `hapticSharpness` (0–1), parameter curves; AHAP files [†] | `CHHapticEngine.capabilitiesForHardware().supportsHaptics` ([docs](https://developer.apple.com/documentation/corehaptics/chhapticdevicecapability/supportshaptics)) |
| UIKit generators (iOS 10+) | impact light/medium/heavy/soft/rigid, selection, notification success/warning/error; `UICanvasFeedbackGenerator` (17.5) | `impactOccurred(intensity:)` [†] | none (best effort) |
| Android `VibrationEffect.Composition` (API 30) | `CLICK`, `THUD`, `SPIN`, `QUICK_RISE`, `SLOW_RISE`, `QUICK_FALL`, `TICK`, `LOW_TICK`; `addPrimitive(id, scale, delay, delayType)` with `DELAY_TYPE_PAUSE` / `DELAY_TYPE_RELATIVE_START_OFFSET` ([`VibrationEffect.java` L1516–1602](https://github.com/aosp-mirror/platform_frameworks_base/blob/d01d77c9547b1a288f4d1135059af8f8a5149ecc/core/java/android/os/VibrationEffect.java#L1516-L1602)) | Android 16 (API 36): `BasicEnvelopeBuilder.setInitialSharpness`, `addControlPoint(intensity, sharpness, durationMillis)` with intensity/sharpness in [0, 1], ending at zero intensity, about 16 points recommended ([L2010–2106](https://github.com/aosp-mirror/platform_frameworks_base/blob/d01d77c9547b1a288f4d1135059af8f8a5149ecc/core/java/android/os/VibrationEffect.java#L2010-L2106)) | `areAllPrimitivesSupported`, `arePrimitivesSupported`, `areEnvelopeEffectsSupported`. "If a composition contains even one unsupported primitive, the entire vibration fails to play"; `VibrationEffect.Builder` (Android 16, 26Q4) adds framework fallback ([guide](https://developer.android.com/develop/ui/views/haptics/custom-haptic-effects)) |
| Windows (`InputHapticsManager`, `SimpleHapticsController`) | `KnownSimpleHapticsControllerWaveforms`: `Click`, `Press`, `Release`, `Hover`, `Align`, `Collide`, `Step`, `Grow`, `Error`, `Success`, `BuzzContinuous`, `RumbleContinuous`, plus pen ink waveforms ([docs](https://learn.microsoft.com/en-us/uwp/api/windows.devices.haptics.knownsimplehapticscontrollerwaveforms)) | intensity 0–1, play count and pause (`…ForPlayCount`) | `IsSupported()`, `SupportedFeedback`, `IsIntensitySupported` |
| macOS `NSHapticFeedbackManager` (10.11) | `generic`, `alignment`, `levelChange` | none | none (Force Touch trackpad only) |
| Web Vibration (Chromium only, per prior note) | — | on/off durations only | `"vibrate" in navigator` |
| Gamepad (`GamepadHapticActuator`) | — | `dual-rumble`/`trigger-rumble` with `duration`, `startDelay`, `strongMagnitude`, `weakMagnitude` (0–1) ([Gamepad](https://w3c.github.io/gamepad/)) | `effects` list |

The intersection: a **transient** with intensity (and sharpness where the device has it), and
a **continuous** segment with an intensity/sharpness envelope. Android's normalized envelope
was designed with the same two axes as Core Haptics, so a single model fits both rich
backends. Everything else is an approximation of it.

### 3.2 Typed model

```rust
// flui-platform-api (no new crate)
#[non_exhaustive]
pub enum HapticIntent {            // tier 1: semantic, always playable or silently dropped
    Selection, Impact(ImpactWeight), Notification(Outcome),
    Alignment, Step, Boundary, DragLift, DragDrop, Confirm,
}

pub struct Unit(f32);              // validated finite 0.0..=1.0 (TryFrom<f32>)

pub enum HapticEvent {
    Transient { at: Duration, intensity: Unit, sharpness: Unit },
    Continuous { at: Duration, envelope: Envelope },
}
pub struct Envelope { start_sharpness: Unit, points: SmallVec<[ControlPoint; 8]> } // ends at 0 intensity
pub struct ControlPoint { intensity: Unit, sharpness: Unit, ramp: Duration }        // ramp > 0

pub struct HapticPattern { events: SmallVec<[HapticEvent; 8]>, total: Duration }
impl HapticPattern { pub fn builder() -> PatternBuilder { … } } // validates order, overlap, caps

pub struct HapticCapabilities {     // queried once per device change, never per call
    pub transient: Fidelity, pub continuous: Fidelity,
    pub sharpness: bool, pub intensity_steps: Option<NonZeroU8>,
    pub max_control_points: Option<NonZeroU16>, pub min_ramp: Duration,
}
pub enum Fidelity { Native, Approximated, None }

pub trait PlatformHaptics {
    fn perform(&self, intent: HapticIntent);                         // tier 1
    fn capabilities(&self) -> HapticCapabilities;
    fn play(&self, pattern: &HapticPattern, fallback: HapticIntent) -> PlayOutcome; // tier 2
}
#[must_use] pub enum PlayOutcome { Exact, Approximated, FellBack, Dropped }
```

Validation lives in the builder: `Unit` is a finite value in [0, 1], ramps are positive,
events are ordered, an envelope ends at zero, and the total duration is capped (5 s
suggested). A bad pattern is an `Err` at construction, never at play time. `PlayOutcome` is for
tests and diagnostics; widget code may ignore it, so haptics stay fire-and-forget as ADR-0031
required.

### 3.3 Degradation rules (one table, pinned by a table test per backend)

1. **Exact** when every event maps natively: Core Haptics; Android API 36+ with
   `areEnvelopeEffectsSupported`.
2. **Transient mapping on Android API 30–35:** choose a primitive by sharpness. Above 0.66 use
   `CLICK`. From 0.33 to 0.66 use `TICK`, or `CLICK` when intensity exceeds 0.7. Below 0.33 use
   `LOW_TICK`, or `THUD` when intensity exceeds 0.7. `scale` is the intensity. Check the chosen
   primitives in one `arePrimitivesSupported` call cached per vibrator; a single missing one
   means the whole pattern **falls back** (Android plays nothing otherwise).
3. **Continuous without envelopes:** use a `createWaveform` amplitude series when
   `hasAmplitudeControl` [†], dropping sharpness. With no amplitude control, play
   `RumbleContinuous`/`BuzzContinuous` for the duration (Windows) or **fall back**.
4. **Windows:** a transient becomes the nearest waveform by intent tag when the pattern has
   one (`Align`, `Step`, `Collide`), otherwise `Click`. Intensity applies through
   `…ForPlayCount` only if `IsIntensitySupported`. Sharpness is dropped (Approximated).
5. **macOS, Web, gamepad:** `NSHapticFeedbackManager` cannot play patterns, so it always
   **falls back** to the intent's mapped pattern (`Alignment` → `.alignment`, `Step` →
   `.levelChange`, others → `.generic`). Web vibrates `max(20 ms, transient)` pulses only on
   Chromium with a user activation. A gamepad maps `strong = intensity·(1 − sharpness)`,
   `weak = intensity·sharpness`.
6. **Never** stretch duration, raise intensity, or play while the window is hidden. The Gamepad
   API rejects with `InvalidStateError` when the document is hidden, and FLUI applies that
   rule uniformly. The system "haptics off" setting wins over both tiers.

---

## 4. Accessibility mapping of gesture actions

### 4.1 What exists

- **AccessKit (main @ fb7efa8):** `Action::CustomAction` "Requires `ActionRequest::data` to be
  set to `ActionData::CustomAction`". `pub struct CustomAction { id: i32, description: String }`
  sits on `Node` via `custom_actions`/`set_custom_actions`/`push_custom_action`. `Increment`,
  `Decrement`, `ShowContextMenu`, `Expand`, `Collapse` and `Scroll*` also exist
  ([`accesskit/src/lib.rs` L285–350, L761–765, L2681](https://github.com/AccessKit/accesskit/blob/fb7efa8cdbe6ebb48106f30c1c66163ebc781b49/accesskit/src/lib.rs#L761-L765)).
  There is **no `Dismiss`** action.
- **AccessKit adapters at the same commit:**
  - No adapter references `custom_actions` or `ShowContextMenu`.
  - The AT-SPI node reports `n_actions` = 1 and `"click"` only for clickable nodes
    ([`atspi-common/src/node.rs` L525–538](https://github.com/AccessKit/accesskit/blob/fb7efa8cdbe6ebb48106f30c1c66163ebc781b49/adapters/atspi-common/src/node.rs#L525-L538)).
  - iOS counts `CustomAction` only in `has_non_scroll_action` ([`ios/src/node.rs` L102–113](https://github.com/AccessKit/accesskit/blob/fb7efa8cdbe6ebb48106f30c1c66163ebc781b49/adapters/ios/src/node.rs#L102-L113)).
  - macOS dispatches `Increment`/`Decrement`/`Click`/`Focus`/`SetValue`/`ScrollIntoView`.
  - Android maps scroll to `ACTION_SCROLL_FORWARD/BACKWARD` and `SetValue` to
    `ACTION_SET_PROGRESS`.
- **Platforms:**
  - macOS `NSAccessibilityCustomAction` (10.13), `accessibilityPerformShowMenu()` and
    `accessibilityPerformCancel()` (10.10).
  - iOS `UIAccessibilityCustomAction` (8.0), `accessibilityIncrement()` (4.0),
    `accessibilityPerformEscape()` (5.0), `accessibilityScroll(_:)` (4.2).
  - Android `AccessibilityAction(int actionId, CharSequence label)`, plus the standard
    `ACTION_DISMISS`, `ACTION_CONTEXT_CLICK`, `ACTION_PRESS_AND_HOLD`,
    `ACTION_DRAG_START/DROP/CANCEL`, `ACTION_SCROLL_IN_DIRECTION`
    ([`AccessibilityNodeInfo.java` L6128–6445](https://github.com/aosp-mirror/platform_frameworks_base/blob/d01d77c9547b1a288f4d1135059af8f8a5149ecc/core/java/android/view/accessibility/AccessibilityNodeInfo.java#L6128-L6445)).
  - AT-SPI `Action` is a list of named actions.
  - UIA has fixed control patterns (`Invoke`, `ExpandCollapse`, `RangeValue`, `Scroll`) and
    custom patterns registered by agreement between provider and client
    ([UIA custom patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-custompropertieseventscontrolpatterns)).
    No Narrator surface for custom actions was found [U].
- **FLUI today:**
  - `flui-protocol` defines `SemanticsAction::{LongPress, Increase, Decrease, CustomAction,
    Dismiss, …}`. `flui-semantics` stores `CustomSemanticsAction { id: i32, label, hint }`
    (`properties.rs` L118), merges custom actions correctly, and routes inbound
    `ActionData::CustomAction(id)`.
  - Outbound, `apply_actions` adds only the `Action::CustomAction` bit
    (`accesskit_translation.rs` L394–395). **Nothing calls `set_custom_actions`**, so even an
    adapter that supported custom actions would see an empty list.
  - `Dismiss` is documented as having no equivalent (L319).
  - `ShowContextMenu` → `LongPress` is inbound only.

### 4.2 Recommended mapping

| Gesture widget | FLUI semantics | AccessKit | Platform result once adapters support it |
|---|---|---|---|
| Swipe actions (row reveals Archive/Delete) | one `CustomSemanticsAction` per revealed action, label = the action's label; `Tap` stays the row's primary action | `custom_actions` + `CustomAction` | macOS/iOS custom actions rotor; Android `AccessibilityAction(id, label)`; AT-SPI named actions; UIA: none, so also expose the actions in the context menu |
| Reorder | custom actions "Move up", "Move down", "Move to start", "Move to end", localized; after each move, announce the new position through the live region support from ADR-0202 | `custom_actions` | the same paths; Android may additionally map `ACTION_DRAG_*` later |
| Dismiss (sheet, snackbar, swipe-to-dismiss card) | `SemanticsAction::Dismiss` | **no AccessKit action**: add `CustomAction` "Dismiss" until upstream has one | iOS `accessibilityPerformEscape`, macOS `accessibilityPerformCancel`, Android `ACTION_DISMISS` once upstream maps them |
| Zoom (pinch on image/map) | `Increase`/`Decrease` with a numeric value (zoom percent) | `Increment`/`Decrement` + `numeric_value` | works on macOS today (adapter dispatches both); iOS `accessibilityIncrement`; Android via `ACTION_SET_PROGRESS` for range nodes |
| Long-press menu | `LongPress` | `ShowContextMenu` (FLUI should emit it outbound, mirroring the inbound map) | macOS `accessibilityPerformShowMenu`, Android `ACTION_LONG_CLICK`/`ACTION_CONTEXT_CLICK`, UIA none (keyboard Shift+F10/Menu key must also open it) |

Work items, in order:

1. FLUI: publish `custom_actions` (stable `id`s per node, `description` = label) in
   `apply_actions`. Emit `ShowContextMenu` for `LongPress`. Add a test that inspects the
   produced `accesskit::Node`; it fails today.
2. Every gesture widget registers these in its semantics, with a test per widget that fails
   without it (owner decision). A test asserting that the gesture recognizer exists is not
   enough.
3. Upstream AccessKit: custom actions in the macOS, iOS, Android and AT-SPI adapters, plus a
   `Dismiss` action or a documented mapping. Until it lands, record each mapping above as a
   known gap in the semantics matrix. Do not work around it with platform types (ADR-0082).

---

## 5. Nested scroll and arena composition

### 5.1 Prior art

- **Compose:**
  - `NestedScrollConnection` has `onPreScroll(available, source)`,
    `onPostScroll(consumed, available, source)`, `suspend onPreFling(available)` and
    `suspend onPostFling(consumed, available)`, each returning what it consumed.
    `NestedScrollDispatcher` drives them, and `NestedScrollSource` is `UserInput`,
    `SideEffect`, `Drag`, `Fling`, `Relocate` or `Wheel`
    ([`NestedScrollModifier.kt` L41–93, L109–289 @ 080d2b3](https://github.com/androidx/androidx/blob/080d2b3e5326ba80392d93442c4a51a02dc22650/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt#L41-L93)).
  - Each node's `onPreScroll` asks its **parent first**, then itself with the remainder
    ([`NestedScrollNode.kt` L81–85](https://github.com/androidx/androidx/blob/080d2b3e5326ba80392d93442c4a51a02dc22650/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollNode.kt#L81-L85)).
    Pre-scroll is therefore root-to-leaf, and post-scroll leaf-to-root.
  - Fling handlers are `suspend`, because a parent animates its own fling inside them.
- **Android Views:** `NestedScrollingParent3.onNestedScroll(…, int[] consumed)` adds to the
  consumed array and passes it on to its own parent
  ([`NestedScrollingParent3.java`](https://github.com/androidx/androidx/blob/main/core/core/src/main/java/androidx/core/view/NestedScrollingParent3.java)).
  This is the same contract with an out-parameter.
- **SwiftUI:** `simultaneousGesture` gives "the same priority as the view's existing gestures",
  `highPriorityGesture` takes precedence, and `Gesture.simultaneously(with:)`,
  `exclusively(before:)` and `sequenced(before:)` compose values; `GestureMask` controls
  subview gestures ([docs](https://developer.apple.com/documentation/swiftui/view/simultaneousgesture(_:including:))).
  This is the algebra the owner chose for the arena; scrolling is outside it.
- **Masonry:** a scrolled `Portal` calls `set_handled()` on the whole wheel event. Its docs
  tell nested scrollers to do the same "to prevent accidental double-scrolling due to event
  bubbling" ([`portal.rs` L42–44, L427–451](https://github.com/linebender/xilem/blob/271a27a6d4a930f7878d404f9014e3c50a3a9b88/masonry/src/widgets/portal.rs#L42-L44)).
  A partial delta is not handed over.
- **GPUI:** every scrollable hitbox handles `ScrollWheelEvent` in the bubble phase. The
  `ongoing_scroll` filter locks precise (trackpad) scrolls to an axis when
  `restrict_scroll_to_axis` is set, and `allow_concurrent_scroll = false` zeroes the minor axis
  ([`div.rs` L3324–3385 @ 506beb3](https://github.com/zed-industries/zed/blob/506beb34de3f433707b7ebe8d8ad2d80f856af6c/crates/gpui/src/elements/div.rs#L3339-L3385)).
  That is effectively `AxisPolicy::LockAfterSlop` for wheel input, with no delta chain.
- **FLUI today:**
  - ADR-0169's `FlingEndpoint` is a mounted owner with a weak parent and an `alive` bit, and
    hands residual ballistic velocity to the nearest live same-axis ancestor
    (`flui-widgets/src/scroll/scrollable.rs` L96–105).
  - `GestureArena::compose` supports `Exclusive` and `RequireFirstFailure` and refuses nesting
    with `CompositionError::AlreadyComposed` (`flui-interaction/src/arena/composition.rs`
    L8–13, L89–95).

### 5.2 Shape A (recommended): a synchronous connection chain

```rust
// flui-interaction::scroll_chain (module, not crate)
#[non_exhaustive] pub enum ScrollSource { Drag, Wheel, Fling, Keyboard, Programmatic }

pub struct ScrollDelta2(Vector<f64>);      // logical px, finite, validated
pub struct Velocity2(Vector<f64>);         // logical px/s, finite

#[diagnostic::on_unimplemented(message = "`{Self}` is not a scroll-chain participant")]
pub trait ScrollConnection {
    fn pre_scroll(&self, available: ScrollDelta2, source: ScrollSource) -> ScrollDelta2 { ZERO }
    fn post_scroll(&self, consumed: ScrollDelta2, available: ScrollDelta2,
                   source: ScrollSource) -> ScrollDelta2 { ZERO }
    fn pre_fling(&self, available: Velocity2) -> Velocity2 { ZERO }
    /// Admission only: an owner that accepts starts its own ballistic run on its own
    /// ticker (ADR-0169) and returns what it took. No async, no awaiting the run.
    fn post_fling(&self, consumed: Velocity2, available: Velocity2) -> Velocity2 { ZERO }
}

pub struct ScrollChain { links: SmallVec<[Weak<ChainLink>; 4]> } // root .. leaf, owner-local (Rc)
struct ChainLink { connection: Rc<dyn ScrollConnection>, alive: Cell<bool>, generation: u64 }

impl ScrollChain {
    /// Root-first pre pass, leaf scroll, leaf-first post pass. Returns what nobody took.
    pub fn dispatch(&self, delta: ScrollDelta2, source: ScrollSource,
                    leaf: impl FnOnce(ScrollDelta2) -> ScrollDelta2) -> ScrollDelta2;
    pub fn fling(&self, velocity: Velocity2, leaf: impl FnOnce(Velocity2) -> Velocity2) -> Velocity2;
}
```

Reentrancy and failure rules (AGENTS.md "Recurring defects"):

- **Snapshot before calling.** `dispatch` upgrades the `Weak` links into a local `SmallVec`
  first, so a callback that unmounts its own scrollable or a parent's cannot invalidate the
  iteration. A link whose `alive` bit was cleared mid-pass is skipped from then on. Identity is
  the link's `Rc` plus its generation, never a label.
- **No borrow across a callback.** Links hold no `RefCell` borrowed across `pre_scroll`. A
  connection mutates its own state behind its own `Cell`/`RefCell`, released before it
  returns.
- **Validate every return.** The result must be finite, the same sign per axis as
  `available`, and no larger in magnitude. Otherwise the dispatcher clamps it to the
  admissible range, emits a diagnostic, and the remainder continues. A connection cannot
  publish non-finite geometry.
- **Panics.** The dispatch runs inside the existing frame containment boundary, and the leaf
  is not double-applied. A link that panicked is marked closed. This needs a table test with
  three rows: one link panicking, two links panicking, and the next dispatch after
  containment.
- **Axis projection** happens in each link, as in ADR-0169, so orthogonal ancestors see a zero
  component and cost nothing.

### 5.3 Shape B (alternative): declarative consumption policies

Each link supplies data instead of callbacks: a `ScrollOrder` (`SelfFirst`, `ParentFirst`,
`EdgeOnly`) plus its own `fn consume(&mut self, d) -> d` scroll primitive. The dispatcher folds
over the slice. This leaves less user code in the loop, but pull-to-refresh and collapsing
headers need state-dependent ordering (retract first, then scroll), which ends up as a
callback anyway. Keep `ScrollOrder` as **ready-made `ScrollConnection` impls** on top of shape
A rather than as the mechanism.

### 5.4 Arena side

Replace `GestureCompetition` with a relation on a composition tree:

- `#[non_exhaustive] enum GestureRelation { Exclusive, RequireFailureOf, Simultaneous }`.
- `arena.compose(relation) -> GestureBranches`. A branch can call `compose` again, which
  removes `AlreadyComposed`.
- Resolution walks the tree:
  - `Simultaneous` lets both sides hold the contact; each side still resolves internally.
  - `RequireFailureOf` blocks the second branch until the first has wholly failed.
  - `Exclusive` keeps the single winner.
- Every recognizer that tracks a direction takes an `AxisPolicy`. `LockAfterSlop` resolves the
  axis on the first slop crossing, which is how the arena picks between orthogonal drags.

### 5.5 The three compositions

1. **Vertical list in a horizontal pager.**
   - Arena: the pager's horizontal drag and the list's vertical drag, both `LockAfterSlop`, in
     an `Exclusive` relation. The first axis past slop wins.
   - Chain: the list dispatches its vertical deltas; the pager projects them to zero. At the
     list edge, the remainder goes to the next same-axis ancestor (for example a collapsing
     app bar). The fling remainder takes the same path (ADR-0169).
2. **Photo pinch/pan inside a pager.**
   - Arena: the photo's scale-pan recognizer and the pager drag are `Exclusive`. A second
     pointer before the pager wins lets scale claim.
   - The photo is a **chain link** below the pager. While zoomed, the photo's pan consumes
     horizontal deltas until its content edge, and `post_scroll` hands the remainder to the
     pager's connection, which turns the page. This is the case the arena alone cannot express:
     one recognizer owns the contact, and two owners share its delta.
3. **Pull-to-refresh above a list.**
   - The refresh indicator is an ancestor link.
   - `pre_scroll`: while the indicator is extended and the delta retracts it, consume (retract
     before the list scrolls).
   - `post_scroll`: with `source == Drag` and the list at its top, consume the overscroll to
     extend.
   - `pre_fling`: if the indicator is past its threshold, consume the velocity and trigger the
     refresh; otherwise, return zero.
   - `Wheel` and `Fling` sources never extend it, a rule written as a table row.

---

## 6. Input trace format

### 6.1 Prior art

- **`libinput record`:**
  - YAML with top-level `version` ("only increased for backwards-incompatible changes. A
    parser must ignore unknown keys"), `ndevices`, `libinput`, `system` (`os`, `kernel`,
    `dmi`) and `devices`.
  - Each device carries `evdev` (`name`, `id`, `codes`, `absinfo [min,max,fuzz,flat,resolution]`,
    `properties`), `udev` properties, `quirks`, and `events` as frames of
    `[sec, usec, type, code, value]` ending in `SYN_REPORT`. The example frames start at
    `[0, 0]`, so timestamps are relative to the recording start.
  - hidraw timestamps come from `clock_gettime` and "may be greater than a subsequent evdev
    event's timestamp". Key codes are obscured by default.
  - Sources: [libinput-record(1)](https://manpages.debian.org/testing/libinput-tools/libinput-record.1.en.html),
    [tools](https://wayland.freedesktop.org/libinput/doc/latest/tools.html).
- **evemu:** a line format with a `# EVEMU 1.3` version header (major = redesign; 1.2 added
  abs resolution, 1.3 LED/switch state), device lines `N:`/`I:`/`A:`, and events as
  `E: <sec>.<usec> <type> <code> <value>`. Source: the
  [README](https://github.com/bentiss/evemu/blob/master/README.md), from a GitHub mirror last
  pushed in 2017; the canonical freedesktop GitLab blocked automated reads.
- **Chromium synthetic gestures:** `SyntheticPointerActionParams` with
  `PointerActionType {PRESS, MOVE, RELEASE, CANCEL, LEAVE, IDLE}` and per-action `width`,
  `rotation_angle`, `force`, `tangential_pressure`, tilt and twist
  ([`synthetic_pointer_action_params.h` L28–36](https://github.com/chromium/chromium/blob/main/content/common/input/synthetic_pointer_action_params.h)).
  This is an action script, not a recording.
- **Flutter:** `PointerEventRecord(timeDelay, events)`, where `timeDelay` is "relative to the
  start of `handlePointerEventRecord`". The replay returns the lag per record
  ([`widget_tester.dart` L603, L668–684 @ 2d7f046](https://github.com/flutter/flutter/blob/2d7f0463b493f3eded9a24f18a6c7a08142e5284/packages/flutter_test/lib/src/widget_tester.dart#L668-L684)).
  It exists in memory only and has no serialization or version.
- **WebDriver BiDi `input.performActions`:** per-source action lists with pointer types and
  `pressure`, `tangentialPressure`, `twist`, `altitudeAngle`, `azimuthAngle`, `width`, `height`
  (prior note, §2.6). This is the vocabulary a trace must be able to *express*, and it is
  another script format.

### 6.2 Recommended FLUI trace

A trace records **`PlatformInput` values as backends emit them**: the boundary the headless
backend replays and the conformance kit asserts. It does not record raw OS events; that is
`libinput record`'s job, and the backend translators get their own fixtures.

```text
// JSON Lines, one object per line, feature `trace` (serde, already optional in flui-platform-api)
{"flui_trace":1,"created_by":"flui-platform/<version> win32","clock":"event_time_relative",
 "window":{"logical_size":[800,600],"scale":1.5},
 "devices":[{"id":1,"kind":"touch","max_contacts":10},{"id":2,"kind":"pen","tools":["tip","eraser"],
             "axes":["pressure","altitude","azimuth","twist"]}],
 "personality":{"double_click_ms":500,"drag_slop_lp":4.0,"wheel_lines":3}}
{"t":0,"arr":120,"ev":{"Pointer":{"Down":{...}}}}
{"t":8333,"arr":8401,"ev":{"Pointer":{"Move":{..., "coalesced":[...]}}}}
```

Decisions:

- **Versioning:** an integer `flui_trace` major version in the header. Readers ignore unknown
  keys (`libinput`'s rule), and the major version changes only on an incompatible change. Every
  serialized enum is `#[non_exhaustive]` in Rust and **tagged** externally in serde. An unknown
  variant in a newer trace becomes a typed `TraceError::UnknownEvent { line }`, not a skip, so
  replay never silently drops input.
- **Timestamps:** two fields per record. `t` is the `EventTime` the backend stamped, in
  microseconds relative to the first event; it drives recognizers and velocity. `arr` is the
  arrival time at the dispatcher, for latency and jitter analysis. `EventTime` is already a
  process-relative nanosecond stamp (`event_time.rs`), so rebasing loses nothing; microseconds
  match `libinput`'s precision and halve the digits.
- **Devices:** a header list keyed by `DeviceId`, plus `DeviceAdded`/`DeviceRemoved` events
  in-stream for hot-plug. The descriptor holds what recognizers branch on (kind, tools,
  supported axes, contact count), not vendor IDs. Contrast `libinput`, whose replay must
  recreate the kernel device.
- **Environment:** window logical size and scale factor (positions are logical, per
  `PlatformInput`'s docs), and the gesture-personality values in effect. Without them a replay
  on another host resolves taps and slops differently.
- **Privacy:** keyboard text and IME commit strings are redacted to `"a"` by default
  (`libinput`'s rule). `--keep-text` is opt-in for fixtures that need it.
- **Encodings:** JSON Lines for checked-in fixtures (diffable, line-addressable errors,
  hand-editable for minimal repros). For long device captures, a `postcard` or `bincode`
  stream of the same serde types behind the same header, chosen by file magic. Both go through
  one `TraceReader`. Measure size before committing to the binary path: at 1 kHz pen input
  with coalesced samples, JSON is roughly 150–250 bytes per event, 10–15 MB per minute [U, an
  estimate, not a measurement].
- **Replay semantics:** the headless backend feeds events in `t` order on a virtual clock,
  advancing frames by the trace's own timing. Fixtures stay deterministic, unlike Flutter's
  wall-clock replay. Conformance kit rows per backend: "native recording X translates to trace
  Y" (golden) and "trace Y drives recognizer outcome Z" (behavior).
- **Scripts versus traces:** a small `TraceBuilder` turns BiDi-like action scripts (press,
  move with interpolation, release, pause) into the same trace type, so synthetic gestures and
  recorded ones share one replay path.

---

## Remaining unverified items

- The TCC pane governing `CGEventPost` (the gate functions are confirmed, the pane name is
  not).
- Whether a native macOS `CHHapticEngine()` drives the built-in trackpad.
- Apple gesture slop and timeout constants. No public constants exist; measure them.
- Narrator or any UIA client consuming custom actions.
- Distinguishing a real Ctrl+wheel from a trackpad pinch on Chromium and Firefox.
- WebDriver-driven IME composition.
- Trace size figures (estimate only).

Pages marked [†] were cited from canonical URLs without being re-read in this session.
