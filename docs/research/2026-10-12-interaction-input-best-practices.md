# Interaction input: best practices for a complete cross-platform stack

- **Date:** 2026-10-12. Workspace baseline: `winit = "0.30.12"`, `ui-events = "0.3"`,
  `windows = "0.62"`, `objc2 = "0.6.4"`, toolchain `1.99.0` (root `Cargo.toml`,
  `rust-toolchain.toml`).
- **Scope:** `flui-interaction`, the input half of `flui-platform-api`, and the
  `flui-platform` backends. The current inventory lives in
  [`docs/plans/specs/flui-interaction/matrix.md`](https://github.com/vanyastaff/flui/blob/deb9faea20fe6beb23c9ea0724f5a75e0f42595f/docs/plans/specs/flui-interaction/matrix.md) and is
  only summarized here.
- **Method:** every claim cites a primary source: a platform vendor's reference page, a W3C or
  WHATWG specification, or framework source at a file and line. Source code was read on the
  default branch on the date above. Line numbers drift, so the symbol name is given too.
- **Legend:** **[U]** marks a claim I could not confirm from a primary source in this session.
  **[†]** marks a vendor reference cited from its canonical URL whose page was not re-read in
  this session; these are long-stable APIs.

---

## Summary: ranked decisions

1. **Split gesture coordination into two typed channels.** Keep the arena for *discrete claims*:
   who owns a contact sequence (tap vs long press vs drag). Add a separate, synchronous
   *delta-consumption chain* for *continuous quantities*: scroll offset, zoom and fling
   velocity. Each scrollable ancestor sees the delta before and after the child, and can consume
   part of it. This is Compose's `NestedScrollConnection` model
   ([§1.2](#12-what-each-model-solves)), and it is the only surveyed model in which "parent yields
   to child until the child hits its edge" is a typed contract rather than an imperative
   override. Without it, nested scroll stays an arena hack, exactly as in Flutter.
2. **Turn ADR-0167's composition into a small algebra.** Today `GestureCompetition` has
   `Exclusive` and `RequireFirstFailure`, and nesting is refused
   (`CompositionError::AlreadyComposed`, `crates/flui-interaction/src/arena/composition.rs`).
   Add `Simultaneous`, meaning both branches may win the same pointers (pinch + pan + rotate on
   one view), and allow relations to nest as a tree. SwiftUI's `simultaneously` /
   `exclusively(before:)` / `sequenced(before:)` proves the algebra is sufficient. UIKit's
   pairwise delegate predicates show the non-local failure mode to avoid ([§1.1](#11-models-compared)).
3. **Make axis policy a typed property of drag recognizers**:
   `AxisPolicy::{Free, Locked(Axis), LockAfterSlop, Rails { .. }}`. Rails is what
   DirectManipulation calls `DIRECTMANIPULATION_CONFIGURATION_RAILS_X/Y`, which Chromium enables
   for precision touchpads ([§2.1](#21-windows)). Axis lock then needs no arena special case.
4. **Ancestors intercept declaratively, never imperatively.** Android's
   `requestDisallowInterceptTouchEvent` and Flutter's "eager winner" both let one node
   override another at runtime. Instead, give an ancestor an *observe-before* hook on the
   delta chain (Compose's `Initial` pass) and a typed `YieldPolicy` on the arena relation.
5. **On Windows, drive precision touchpads and touch pan/zoom through DirectManipulation**,
   mapping its viewport updates onto the existing `PanZoomEvent`, as both Chromium and
   Flutter's Windows engine do. The DirectManipulation COM calls spin nested message loops,
   which is a reentrancy hazard to design for up front ([§2.1](#21-windows)).
6. **Use the per-platform "complete backend" table in [§2](#2-platform-input-apis) as each
   backend's definition of done.** The biggest gaps relative to what platforms deliver are OS
   drag and drop on every backend, haptics on every backend, macOS pen (tablet events), Web IME
   (EditContext), Android stylus handwriting, and iOS Pencil squeeze, roll and hover.
7. **Keep the FLUI-owned vocabulary (ADR-0143). Use winit only where it is the best native
   path (Linux), and plan the move to winit 0.31.** 0.31 (beta.3, 2026-09-04) overhauls
   pointer events (pen, tablet, `FingerId`, `primary`) and replaces file drops with a real
   bidirectional DnD API. Even so, its gesture events don't exist on Windows or X11. Use
   linebender's `ui-events-*` adapters as translation references, not as types in signatures
   ([§3](#3-rust-ecosystem-adopt-vs-own)).
8. **Synthetic-first testing in three tiers**:
   - declarative scripts on the virtual clock, which already exist as
     `flui_testing::replay::PointerScript`;
   - recorded native traces captured at the `PlatformInput` boundary with both source and
     arrival times;
   - OS-level injection used only to certify each backend.

   Add **platform personality fixtures** (slops, timeouts, fling limits, wheel granularity) and
   run every script against every personality. Chromium's `SyntheticGestureTarget` is the
   precedent ([§4](#4-synthetic-first-testing)).
9. **Haptics in two tiers.** A semantic `HapticIntent` vocabulary, modeled on SwiftUI's
   `SensoryFeedback` superset and mapped per platform with explicit fallbacks. Above it, an
   optional capability-gated `HapticPattern` of primitives (Android `VibrationEffect.Composition`,
   Core Haptics). Haptics can target an input device: Windows pen haptics, the macOS trackpad.
   The pattern tier conflicts with ADR-0031's "no availability discovery" rule, so it needs a
   superseding ADR ([§5.1](#51-haptics)).
10. **One drag-and-drop API for in-app and inter-app drags.** Items carry lazily resolved typed
    representations. An in-process drop hands over the Rust value without serialization;
    promised files are supported where the OS has them. Two constraints shape it: the Web needs
    data synchronously at `dragstart`, and X11 reports the chosen action only at the end of the
    drag ([§5.2](#52-drag-and-drop)).
11. **Rust 1.99-era features that pay off here** ([§6](#6-rust-199-era-features-that-improve-the-api)):
    - `if let` guards (1.95) and let-chains (1.88) for recognizer state machines;
    - `assert_matches!` (1.96) for event-log assertions;
    - `cfg_select!` (1.95) for backend selection;
    - `get_disjoint_mut` (1.86) for multi-pointer slots;
    - `#[diagnostic::on_unimplemented]` and `#[diagnostic::do_not_recommend]` (1.78/1.85) on
      recognizer traits;
    - async closures (1.85) only at DnD and IO edges.

---

## 1. Gesture coordination models

### 1.1 Models compared

| System | Coordination primitive | Simultaneous | Failure dependency | Nested scroll | Source |
|---|---|---|---|---|---|
| Flutter | Per-pointer arena. `close` after down, `sweep` after up; first member wins by default; `hold`/`release` delay the sweep; an "eager winner" can win before close | No. One winner per pointer arena. `GestureArenaTeam` makes several recognizers compete *as one member*; the captain or first accepting member wins | Implicit via timing (double tap holds the arena) | No contract. Scrollables compete with axis-specific drag recognizers; nested and cross-axis cases are app-level workarounds (e.g. flutter/flutter#50946, open, "Make ListView scrollable within PageView") | [`arena.dart` L117–283](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/arena.dart) (`close` L134, `sweep` L157, `hold` L193, `release` L211, `eagerWinner` L69); [`team.dart` L45, L84, L102](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/team.dart); [#50946](https://github.com/flutter/flutter/issues/50946) |
| SwiftUI | Gesture values combined by algebra: `simultaneously(with:)`, `exclusively(before:)`, `sequenced(before:)`; attachment precedence via `gesture`, `highPriorityGesture`, `simultaneousGesture`, each with a `GestureMask` (`all`, `gesture`, `subviews`, `none`) | Yes, by composition | `exclusively(before:)` gives precedence to the first | Not exposed. iOS 18 adds `UIGestureRecognizerRepresentable` to bridge UIKit recognizers | [`GestureMask`](https://developer.apple.com/documentation/swiftui/gesturemask), [`simultaneousGesture`](https://developer.apple.com/documentation/swiftui/view/simultaneousgesture(_:including:)), [`highPriorityGesture`](https://developer.apple.com/documentation/swiftui/view/highprioritygesture(_:including:)), [`exclusively(before:)`](https://developer.apple.com/documentation/swiftui/gesture/exclusively(before:)), [`sequenced(before:)`](https://developer.apple.com/documentation/swiftui/gesture/sequenced(before:)), [`UIGestureRecognizerRepresentable`](https://developer.apple.com/documentation/swiftui/uigesturerecognizerrepresentable) (iOS 18) |
| UIKit | Recognizer state machines plus pairwise delegate predicates: `gestureRecognizerShouldBegin`, `shouldRecognizeSimultaneouslyWith`, `shouldRequireFailureOf`, `shouldBeRequiredToFailBy`, `shouldReceive` | Yes, opt-in per pair | `require(toFail:)` and the delegate pair | `UIScrollView` exposes its `panGestureRecognizer`; apps wire failure requirements by hand | [`UIGestureRecognizerDelegate`](https://developer.apple.com/documentation/uikit/uigesturerecognizerdelegate), [`require(toFail:)`](https://developer.apple.com/documentation/uikit/uigesturerecognizer/require(tofail:)) |
| Jetpack Compose | Three dispatch passes per event: `Initial` (ancestor→descendant), `Main` (descendant→ancestor), `Final` (ancestor→descendant). A change carries a consumption flag (`isConsumed`/`consume()`) | Yes. Several detectors on one node all see events; consumption arbitrates | Expressed by checking `isConsumed` | **`NestedScrollConnection`**: `onPreScroll`/`onPostScroll(available, source)` return the consumed `Offset`; `onPreFling`/`onPostFling` return consumed `Velocity`. A `NestedScrollDispatcher` lets custom nodes take part. `transformable(lockRotationOnZoomPan, canPan)`, `draggable2D` | [`PointerEvent.kt` L1093–1112, L620–636](https://github.com/androidx/androidx/blob/androidx-main/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer/PointerEvent.kt); [`NestedScrollModifier.kt` L41–109, L220–258](https://github.com/androidx/androidx/blob/androidx-main/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/nestedscroll/NestedScrollModifier.kt); [`Transformable.kt` L65–92](https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Transformable.kt); [`Draggable2D.kt` L140](https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Draggable2D.kt) |
| Android View | `onInterceptTouchEvent` steals from children; a child vetoes with `requestDisallowInterceptTouchEvent(true)` (API 1). `NestedScrollingParent3`/`Child3` added a pre/post protocol later | Ad hoc | Ad hoc | Imperative intercept, then the nested-scrolling protocol | [`ViewGroup.requestDisallowInterceptTouchEvent`](https://developer.android.com/reference/android/view/ViewGroup#requestDisallowInterceptTouchEvent(boolean)); [`NestedScrollingParent3`](https://developer.android.com/reference/androidx/core/view/NestedScrollingParent3) [†] |
| GPUI (Zed) | DOM-like `DispatchPhase::{Capture, Bubble}` listeners; propagation can be stopped | n/a, no recognizers | n/a | Per element | [`window.rs` L94–107](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/window.rs) |
| Masonry (Xilem) | Bubbling pointer events, `capture_pointer`/`release_pointer` on the event context. No arena found in `masonry_core` [U: search not exhaustive] | n/a | n/a | Portal widgets | [`contexts.rs` L456–481](https://github.com/linebender/xilem/blob/main/masonry_core/src/core/contexts.rs) |
| egui | Immediate mode: a widget declares `Sense` (`HOVER`, `CLICK`, `DRAG`, `FOCUSABLE`) and the context arbitrates by id | n/a | n/a | `ScrollArea` | [`sense.rs` L9–21](https://github.com/emilk/egui/blob/main/crates/egui/src/sense.rs) |
| Iced | `event::Status::{Ignored, Captured}`, merged so that `Captured` wins | n/a | n/a | `Scrollable` captures | [`core/src/event.rs` L41–65](https://github.com/iced-rs/iced/blob/master/core/src/event.rs) |
| Slint | Element handlers: `TouchArea`, `SwipeGestureHandler` (1.9), `ScaleRotateGestureHandler` (1.16, multi-touch pinch); nested swipe thresholds tuned in 1.13.1 | Per element | n/a | Thresholds | [`CHANGELOG.md`](https://github.com/slint-ui/slint/blob/master/CHANGELOG.md) (1.9.0, 1.13.1, 1.16.0 entries) |

### 1.2 What each model solves

- **Discrete ownership (tap vs long press vs drag vs double tap).** Flutter's arena is the best
  shape: it is per-pointer, resolves deterministically, and has explicit hold/sweep for
  multi-tap. FLUI already has it, plus owner-local composition (ADR-0159, ADR-0161, ADR-0167).
  The Rust UI crates (GPUI, Masonry, egui, Iced) have no equivalent. They rely on capture and
  propagation flags, which cannot express "drag wins only after tap fails". This is the gap
  every Flutter-like system has to fill itself.
- **Simultaneity (pinch + pan + rotate, pan while long-pressing).** UIKit and SwiftUI support it
  and Flutter does not. UIKit's answer is a pairwise predicate, `shouldRecognizeSimultaneouslyWith`.
  It is non-local: two recognizers written by different packages must each know about the
  other, and the relation is not visible from the view tree. SwiftUI's answer is
  compositional: `a.simultaneously(with: b)` is itself a gesture, and that is the shape to copy.
- **Nested scroll and "parent yields to child".** Only Compose types this. A continuous delta
  is offered to ancestors before the child (`onPreScroll`), then the leftover after it
  (`onPostScroll`), and each return value states how much was consumed. Fling velocity follows
  the same path (`onPreFling`/`onPostFling`). The arena is the wrong tool for this. Ownership
  of the pointer and ownership of the *delta* are different questions: during a nested scroll
  the child owns the pointer while the parent consumes part of the delta. Flutter conflates
  the two, which is why `NestedScrollView` and the many "nested scroll" community packages
  exist.
- **Axis lock.** Flutter gets axis lock by competing a vertical and a horizontal recognizer.
  Compose gets it by `Orientation` on `draggable`/`scrollable`. DirectManipulation makes it a
  configuration flag (`RAILS_X`/`RAILS_Y`, see §2.1). A configuration value is the most
  testable of the three.
- **Ancestor pre-emption.** Compose's `Initial` pass lets an ancestor *observe and consume
  before* descendants. It is the declarative form of Android's `onInterceptTouchEvent`.
  `requestDisallowInterceptTouchEvent` is the escape hatch FLUI should not need.

### 1.3 Recommended Rust shape

Two channels, both synchronous on the frame path (AGENTS.md "Frame path is synchronous").
Compose's fling hooks are `suspend`; in FLUI they return the leftover velocity immediately,
and the settle motion then runs on frame ticks (ADR-0188).

```rust
// Channel A: ownership. Extends ADR-0167; relations nest as a tree.
#[non_exhaustive]
pub enum GestureRelation { Exclusive, RequireFailureOf, Simultaneous }

let (pan, pinch) = arena.compose(GestureRelation::Simultaneous)?.into_branches();
let (tap, double_tap) = pan.compose(GestureRelation::RequireFailureOf)?.into_branches(); // nesting allowed

// Axis policy is recognizer configuration, not an arena special case.
#[non_exhaustive]
pub enum AxisPolicy { Free, Locked(Axis), LockAfterSlop, Rails { lock_angle: Angle, break_out: f64 } }

// Channel B: delta consumption, walked by a scroll-chain owner.
pub trait ScrollChainNode {
    /// Offered before the child; returns the part this node consumed.
    fn pre_scroll(&mut self, available: Offset, source: ScrollSource) -> Offset { Offset::ZERO }
    /// Offered after the child with what the child did not consume.
    fn post_scroll(&mut self, consumed: Offset, available: Offset, source: ScrollSource) -> Offset { Offset::ZERO }
    fn pre_fling(&mut self, available: Velocity) -> Velocity { Velocity::ZERO }
    fn post_fling(&mut self, consumed: Velocity, available: Velocity) -> Velocity { Velocity::ZERO }
}
#[non_exhaustive]
pub enum ScrollSource { Drag, Fling, Wheel { precise: bool }, PanZoom, Keyboard, Programmatic }
```

Properties to pin with tests:

- **Consumption is checked arithmetic.** A node cannot return more than was available (clamp
  and report via `debug_assert`). A `NaN` delta is refused at construction, consistent with
  ADR-0098 checked geometry.
- **The chain is computed once per gesture from the hit-test path**, so it is stable across a
  layout change mid-gesture.
- **`Simultaneous` branches each get acceptance callbacks.** Rejection of one branch does not
  cancel the other.
- **Multi-pointer transforms (`ScaleRecognizer`) feed the delta chain with
  `ScrollSource::PanZoom`**, so a zoomable image inside a pager hands horizontal overflow to
  the pager (Compose's `transformable(canPan = ...)`).

---

## 2. Platform input APIs

For each platform: the canonical model, what a complete backend consumes, and how to inject
synthetic input for certification. FLUI's current producer status is in `matrix.md`.

### 2.1 Windows

- **Canonical model.** `WM_POINTER*` messages with `POINTER_INFO`, `POINTER_PEN_INFO` and
  `POINTER_TOUCH_INFO`; `EnableMouseInPointer` unifies the mouse [†]
  ([WM_POINTERDOWN](https://learn.microsoft.com/en-us/windows/win32/inputmsg/wm-pointerdown)).
  FLUI already decodes these (`matrix.md`, "Сверка native producers").
- **DirectManipulation.** This is how precision-touchpad and touch pan/zoom/inertia reach a
  Win32 app. The app calls `IDirectManipulationViewport::SetContact` on `WM_POINTERDOWN`.
  Processing runs off the UI thread; `WM_POINTERCAPTURECHANGED` reports that DM consumed the
  contact
  ([DirectManipulation overview](https://learn.microsoft.com/en-us/windows/win32/directmanipulation/direct-manipulation-portal)).
  - Chromium uses it for precision touchpads: "only `DM_POINTERHITTEST` can be the first message
    of input sequence of touchpad input". It configures `TRANSLATION_X/Y`, `INERTIA`,
    `RAILS_X/Y` and `SCALING` with `VIEWPORT_OPTIONS_MANUALUPDATE` "for touchpad"
    ([`direct_manipulation_helper_win.cc` L82–99, L256–257](https://chromium.googlesource.com/chromium/src/+/main/content/browser/renderer_host/direct_manipulation_helper_win.cc)).
  - Its header warns that "almost every function in this class can spin a nested message loop"
    which can process `WM_DESTROY`
    ([`direct_manipulation_helper_win.h` L35–50](https://chromium.googlesource.com/chromium/src/+/main/content/browser/renderer_host/direct_manipulation_helper_win.h)).
    A FLUI backend must treat every DM call as reentrant (AGENTS.md "Treat user code as
    reentrant").
  - Flutter's Windows engine maps DM viewport callbacks to `OnPointerPanZoomStart/Update/End`
    ([`direct_manipulation.cc` L89–153, L239](https://github.com/flutter/flutter/blob/master/engine/src/flutter/shell/platform/windows/direct_manipulation.cc)).
    That maps directly onto FLUI's `PanZoomEvent`.
- **InteractionContext and Windows Ink** [†]. InteractionContext
  ([docs](https://learn.microsoft.com/en-us/windows/win32/api/interactioncontext/)) is a
  system recognizer FLUI should *not* adopt, because its own recognizers are the product. Low-latency
  ink via `InkPresenterDesktop`
  ([docs](https://learn.microsoft.com/en-us/windows/win32/api/inkpresenterdesktop/)) is an
  optional later capability.
- **Drag and drop (OLE)** [†]. `RegisterDragDrop` + `IDropTarget` (`DragEnter`, `DragOver`,
  `DragLeave`, `Drop`), `DoDragDrop` + `IDropSource`, and `IDragSourceHelper`/`IDropTargetHelper`
  for shell drag images
  ([RegisterDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-registerdragdrop),
  [IDragSourceHelper](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-idragsourcehelper)).
  `DoDragDrop` runs a modal loop, which is the same reentrancy hazard.
- **Haptics.** `Windows.Devices.Haptics.SimpleHapticsController` (Windows 10 1703+) exposes
  `SupportedFeedback`, `IsIntensitySupported` and `SendHapticFeedback*`. For a pen, the path is
  `PointerPoint.PointerId` → `PenDevice` → `SimpleHapticsController` ("requires both a
  haptic-capable pen and a device that supports the pen")
  ([SimpleHapticsController](https://learn.microsoft.com/en-us/uwp/api/windows.devices.haptics.simplehapticscontroller)).
  There is no general "UI haptic" API for haptic touchpads [U].
- **Personality** [†]. `GetDoubleClickTime`, `GetSystemMetrics(SM_CXDOUBLECLK/SM_CXDRAG)` and
  `SPI_GETWHEELSCROLLLINES`.
- **Injection.** `InjectSyntheticPointerInput` (Windows 10 1809+) injects `PT_TOUCH` (up to
  `MAX_TOUCH_COUNT` contacts) or `PT_PEN` (count must be 1) through a device from
  `CreateSyntheticPointerDevice`
  ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-injectsyntheticpointerinput)).
  Precision-touchpad gestures cannot be injected this way [U].

### 2.2 macOS

- **Canonical model.** `NSEvent`:
  - mouse and scroll, with `phase` and `momentumPhase` (10.7)
    ([phase](https://developer.apple.com/documentation/appkit/nsevent/phase-swift.property),
    [momentumPhase](https://developer.apple.com/documentation/appkit/nsevent/momentumphase));
  - trackpad gestures `.magnify`, `.rotate` and `.swipe` (10.5), and `.smartMagnify` (10.8);
  - `.pressure` (10.10.3, Force Touch);
  - tablet `.tabletPoint` and `.tabletProximity`, and `.changeMode` (10.15)
    ([EventType](https://developer.apple.com/documentation/appkit/nsevent/eventtype/magnify)).

  Pen on macOS means reading tablet subtypes on mouse events plus proximity events. FLUI has
  no producer for this yet.
- **Drag and drop.**
  - `NSDraggingDestination`, `NSDraggingSource`, `NSDraggingSession` (10.7).
  - `NSFilePromiseProvider` (10.12) for promised files
    ([NSFilePromiseProvider](https://developer.apple.com/documentation/appkit/nsfilepromiseprovider)).
- **Haptics.** `NSHapticFeedbackManager` (10.11) with patterns `generic`, `alignment` and
  `levelChange`, only on Force Touch trackpads
  ([docs](https://developer.apple.com/documentation/appkit/nshapticfeedbackmanager/feedbackpattern)).
  `CHHapticEngine` is listed for macOS 10.15+
  ([docs](https://developer.apple.com/documentation/corehaptics/chhapticengine)); which Mac
  hardware it actually drives is [U].
- **IME.** `NSTextInputClient`
  ([docs](https://developer.apple.com/documentation/appkit/nstextinputclient)).
- **Injection.** `CGEvent.post(tap:)` (10.4)
  ([docs](https://developer.apple.com/documentation/coregraphics/cgevent/post(tap:))). It
  requires the Accessibility (TCC) permission [U]. There is no public API to synthesize
  magnify or rotate gesture events [U], so trackpad gestures are certified with a physical
  device or tested at the `PlatformInput` boundary.

### 2.3 iOS / iPadOS

- **Canonical model.**
  - Raw `UITouch` via `touchesBegan/Moved/Ended/Cancelled`.
  - `UIEvent.coalescedTouches(for:)` and `predictedTouches(for:)` (iOS 9)
    ([coalesced](https://developer.apple.com/documentation/uikit/uievent/coalescedtouches(for:)),
    [predicted](https://developer.apple.com/documentation/uikit/uievent/predictedtouches(for:))).
  - Pencil: `altitudeAngle` (9.1) and `rollAngle` (17.5)
    ([rollAngle](https://developer.apple.com/documentation/uikit/uitouch/rollangle)).
  - Hover through `UIHoverGestureRecognizer` (13.0), with Pencil `zOffset` (16.1)
    ([zOffset](https://developer.apple.com/documentation/uikit/uihovergesturerecognizer/zoffset)).
  - `UIPencilInteraction` double tap (12.1) and `Squeeze` (17.5)
    ([Squeeze](https://developer.apple.com/documentation/uikit/uipencilinteraction/squeeze)).
  - Trackpad and mouse via `UIPointerInteraction` (13.4).

  Recommendation: consume raw touches for FLUI's own recognizers. Read trackpad pinch and
  rotate from system recognizers, as winit does, where it notes they are "not recognized by
  default" on iOS
  ([winit `event.rs` L365–438](https://github.com/rust-windowing/winit/blob/master/winit-core/src/event.rs)).
- **Drag and drop.** `UIDragInteraction` and `UIDropInteraction` (11.0)
  ([drag](https://developer.apple.com/documentation/uikit/uidraginteraction),
  [drop](https://developer.apple.com/documentation/uikit/uidropinteraction)).
- **Haptics.**
  - `UIFeedbackGenerator` (10.0) and `UIImpactFeedbackGenerator`.
  - `UICanvasFeedbackGenerator` (17.5) for drawing-canvas events
    ([docs](https://developer.apple.com/documentation/uikit/uicanvasfeedbackgenerator)).
  - Core Haptics `CHHapticEngine` (13.0) for patterns.
- **IME.** `UITextInput`, `UITextInteraction` (13.0) and `UIScribbleInteraction` (14.0, Pencil
  handwriting into text fields)
  ([docs](https://developer.apple.com/documentation/uikit/uiscribbleinteraction)).
- **Injection.** XCUITest `XCUIElement` provides `tap()`, `doubleTap()`,
  `tap(withNumberOfTaps:numberOfTouches:)`, `press(forDuration:thenDragTo:withVelocity:thenHoldForDuration:)`,
  `pinch(withScale:velocity:)`, `rotate(_:withVelocity:)` and the `swipe*` methods
  ([XCUIElement](https://developer.apple.com/documentation/xcuiautomation/xcuielement)).
  There is no pressure, tilt or Pencil injection [U].

### 2.4 Android

- **Canonical model.** `MotionEvent` with batched history (`getHistoricalX`, API 1/5), tool
  types, axes, and `ACTION_CANCEL` ([MotionEvent](https://developer.android.com/reference/android/view/MotionEvent)).
- **Stylus.** `InputMethodManager.startStylusHandwriting` and `View.setAutoHandwritingEnabled`
  are both API 33
  ([IMM](https://developer.android.com/reference/android/view/inputmethod/InputMethodManager#startStylusHandwriting(android.view.View))).
- **Content and drag and drop.** `View.setOnReceiveContentListener` (API 31) is the unified
  paste/drop/IME-content entry point
  ([View](https://developer.android.com/reference/android/view/View#setOnReceiveContentListener(java.lang.String[],%20android.view.OnReceiveContentListener))).
  `View.startDragAndDrop` is API 24.
- **Haptics.**
  - `View.performHapticFeedback` with `HapticFeedbackConstants`: `CONFIRM`, `REJECT`,
    `GESTURE_START` and `GESTURE_END` (API 30); `DRAG_START`, `GESTURE_THRESHOLD_ACTIVATE` and
    `_DEACTIVATE`, `SEGMENT_TICK`, `SEGMENT_FREQUENT_TICK`, `TOGGLE_ON` and `TOGGLE_OFF`,
    `NO_HAPTICS` (API 34)
    ([HapticFeedbackConstants](https://developer.android.com/reference/android/view/HapticFeedbackConstants)).
  - `VibrationEffect.Composition` (API 30): primitives `CLICK`, `TICK`, `QUICK_RISE`,
    `SLOW_RISE` and `QUICK_FALL` (30); `THUD`, `SPIN` and `LOW_TICK` (31)
    ([Composition](https://developer.android.com/reference/android/os/VibrationEffect.Composition)).
- **IME.** `InputConnection` [†].
- **Personality (AOSP source).**
  - `TOUCH_SLOP = 8` dp, `DOUBLE_TAP_TIMEOUT = 300` ms, `DEFAULT_LONG_PRESS_TIMEOUT = 400` ms,
    min and max fling velocity 50 and 8000
    ([`ViewConfiguration.java` L82–245](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/view/ViewConfiguration.java)).
  - Resource defaults: `config_viewConfigurationTouchSlop` 8dp, hover slop 4dp
    ([`config.xml` L2985–3005](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/res/res/values/config.xml)).
  - Compare Flutter: `kTouchSlop = 18.0`, `kLongPressTimeout = 500` ms, `kDoubleTapTimeout = 300` ms
    ([`constants.dart` L29–95](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/constants.dart)).

  So Flutter's "Android" touch slop is more than twice the platform's. This is the concrete
  argument for personality fixtures (§4).
- **Injection** [†]. `UiAutomation.injectInputEvent` and `Instrumentation.sendPointerSync`
  ([UiAutomation](https://developer.android.com/reference/android/app/UiAutomation)).

### 2.5 Linux (Wayland, X11)

- **Wayland gestures.** `zwp_pointer_gesture_swipe_v1`, `pinch_v1` and `hold_v1` (hold added in
  version 3). Pinch `update` carries dx/dy, `scale` and `rotation`; `end` carries `cancelled`
  ([pointer-gestures-unstable-v1](https://wayland.app/protocols/pointer-gestures-unstable-v1)).
- **Wayland tablets.** `tablet_v2` is **stable**, with tool, pad, ring, strip and the newer
  `zwp_tablet_pad_dial_v2`
  ([`stable/tablet/tablet-v2.xml`](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/raw/main/stable/tablet/tablet-v2.xml)).
- **Wayland text input.** `zwp_text_input_v3`
  ([xml](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/raw/main/unstable/text-input/text-input-unstable-v3.xml)).
- **Wayland drag and drop.** Core `wl_data_device` [†]. Staging has `xdg-toplevel-drag`
  (tab tear-off). No haptics protocol appears in staging
  ([staging tree](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/tree/main/staging)).
- **X11.** XInput 2.4 (September 2021) adds touchpad gestures `GesturePinch*` and `GestureSwipe*`
  ([XI2proto.txt §"XI 2.4"](https://gitlab.freedesktop.org/xorg/proto/xorgproto/-/raw/master/specs/XI2proto.txt)).
- **Injection.**
  - uinput ([kernel doc](https://www.kernel.org/doc/html/latest/input/uinput.html)) [†].
  - XTest [†].
  - On Wayland, libei via the RemoteDesktop portal [†]. The Rust crate `reis` 0.7.1 implements
    the libei protocol (crates.io).

### 2.6 Web

- **Pointer Events Level 3 is a W3C Recommendation (2026-06-30).** It defines
  `getCoalescedEvents()` (§10.1), `getPredictedEvents()` (§10.2), `pointerrawupdate` (§4.2.5),
  `altitudeAngle`/`azimuthAngle` (§4.1), and `touch-action` for direct manipulation (§8)
  ([PE3](https://www.w3.org/TR/pointerevents3/)). `persistentDeviceId` is only in the editor's
  draft, not in L3 ([ED](https://w3c.github.io/pointerevents/)).
- **Trackpad pinch.** Safari exposes `GestureEvent`
  ([WebKit JS](https://developer.apple.com/documentation/webkitjs/gestureevent)). Chromium and
  Firefox deliver trackpad pinch as `wheel` with `ctrlKey` set; no specification defines that
  convention [U].
- **Drag and drop.** HTML DnD (WHATWG HTML §6.11: `DataTransfer`, `DataTransferItemList`,
  `effectAllowed`/`dropEffect`) is supported in all engines
  ([HTML §6.11](https://html.spec.whatwg.org/multipage/dnd.html)). The drag data store is only
  writable during `dragstart`, so lazy data is impossible on the Web [†, same section,
  "drag data store mode"].
- **Vibration.** The Vibration API is a Candidate Recommendation Draft (2026-05-21). It is
  "implemented in Chromium-based browsers", WebKit opposes it, and Firefox removed it in 129
  ([Vibration](https://www.w3.org/TR/vibration/)). Treat Web haptics as Chromium-only.
- **IME.** EditContext is an editor's draft defining `textupdate`, `textformatupdate`,
  `characterboundsupdate`, `updateText`, `updateSelection`, `updateControlBounds` and
  `updateCharacterBounds` ([EditContext](https://w3c.github.io/edit-context/)). `web-sys`
  0.3.106 has **no `EditContext` feature** (checked against the crates.io feature list), so FLUI
  needs its own `wasm-bindgen` externs.
- **Injection.** WebDriver BiDi `input.performActions` with
  `input.PointerType = "mouse" / "pen" / "touch"` and `PointerCommonProperties` `width`,
  `height`, `pressure`, `tangentialPressure`, `twist`, `altitudeAngle` and `azimuthAngle`
  (§7.9.3.1); `input.setFiles` covers file inputs
  ([WebDriver BiDi](https://w3c.github.io/webdriver-bidi/)). This is the richest *standard*
  injection vocabulary surveyed, and FLUI's script format should be able to express at least
  this.

### 2.7 Summary: what a complete backend consumes

| Capability | Win32 | macOS | iOS | Android | Wayland / X11 | Web |
|---|---|---|---|---|---|---|
| Pen pressure, tilt and roll | `POINTER_PEN_INFO` | tablet subtypes | `UITouch` incl. `rollAngle` | `MotionEvent` axes | tablet-v2 / XI2 valuators | PE3 |
| Coalesced and predicted samples | pointer history | — [U] | `UIEvent` | historical | — | PE3 §10 |
| Trackpad pan/zoom/rotate with phases | DirectManipulation | `magnify`, `rotate`, `phase`, `momentumPhase` | system recognizers | — | pointer-gestures v3 / XI 2.4 | `GestureEvent` / ctrl+wheel |
| Pencil and pen buttons | barrel, eraser | — | squeeze, double tap | button state | tablet tool buttons | `buttons` |
| OS drag and drop | OLE | `NSDragging*` | `UIDrag/DropInteraction` | `startDragAndDrop`, `OnReceiveContent` | `wl_data_device` / XDND | HTML DnD |
| Haptics | `SimpleHapticsController` | `NSHapticFeedbackManager` | feedback generators, Core Haptics | `HapticFeedbackConstants`, `Composition` | none | `vibrate` (Chromium) |
| IME | TSF / IMM [†] | `NSTextInputClient` | `UITextInput` | `InputConnection` | text-input-v3 | EditContext |
| Synthetic injection | `InjectSyntheticPointerInput` | `CGEvent` | XCUITest | `UiAutomation` | uinput, XTest, libei | WebDriver BiDi |

---

## 3. Rust ecosystem: adopt vs own

| Crate (latest on crates.io, 2026-10-12) | What it gives | Decision |
|---|---|---|
| `winit` 0.31.0-beta.3 (stable 0.30.13; workspace pins 0.30.12) | See the 0.31 details below this table | **Adopt for Linux; plan the 0.31 upgrade.** It can't replace the native Win32, AppKit, UIKit, Android or Web backends: no Windows or X11 gestures, and no DirectManipulation. |
| `ui-events` 0.3.0 plus adapters `-winit`, `-web`, `-appkit`, `-uikit`, `-wayland`, `-x11`, `-xkb`, and `ui-input-state` | W3C-modeled data types (`PointerUpdate { coalesced, predicted }`). `PointerGesture` has only `Pinch(f32)` and `Rotate(f32)`, deltas with no phase ([`pointer/mod.rs` L254–335](https://github.com/endoli/ui-events/blob/main/ui-events/src/pointer/mod.rs)) | **Own** the vocabulary (ADR-0143 already does; FLUI's `PanZoomEvent` with phases is richer). **Read** the adapters as translation references for X11 and Wayland key and pointer mapping. |
| `keyboard-types` 0.8.3 | W3C `Key`/`Code` | Adopt (already). |
| `cursor-icon` 1.2.0 | CSS cursor names | Adopt (already). |
| `accesskit` 0.25.1 | Accessibility trees and adapters | Adopt (already). Out of scope here. |
| `smithay-client-toolkit` 0.21.1 | Wayland client helpers | Only if FLUI writes a native Wayland backend. Until then, winit owns it. |
| `windows` 0.62.2 | Win32 and WinRT, incl. `Windows.Devices.Haptics` and DirectManipulation COM [U: feature names not checked] | Adopt. |
| `objc2` 0.6.5, `objc2-app-kit`/`objc2-ui-kit` 0.3.2 | AppKit and UIKit bindings | Adopt. Core Haptics needs `objc2-core-haptics` [U: crate existence not checked]. |
| `android-activity` 0.6.1 | `MotionEvent` and `InputConnection` glue | Adopt. Haptics and DnD go through JNI. |
| `web-sys` 0.3.106 | DOM bindings; `PointerEvent`, `DataTransfer` and `Navigator` exist, `EditContext` does not | Adopt, plus hand-written externs for EditContext. |
| `reis` 0.7.1, `evdev` 0.13.2, `enigo` 0.6.1 | libei, uinput, cross-OS input synthesis | Use only in `xtask` and device-certification tooling, never in shipped crates. |

What winit 0.31 changes:

- **Pointer events.** A single `PointerMoved`/`PointerButton`/`PointerEntered`/`PointerLeft`
  family with `PointerKind`, `PointerSource`, `ButtonSource`, `FingerId` and `primary`. Pen
  input arrives on Wayland, Windows and Web; iOS gains Pencil force, altitude and azimuth
  ([0.31 changelog](https://github.com/rust-windowing/winit/blob/master/winit/src/changelog/v0.31.md),
  [beta.3 release](https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.3)).
- **Gestures.** `PinchGesture`, `PanGesture`, `RotationGesture`, `DoubleTapGesture` and
  `HoldGesture` exist only on macOS, iOS and Wayland. Windows and X11 get none
  ([`event.rs` L365–438](https://github.com/rust-windowing/winit/blob/master/winit-core/src/event.rs)).
- **Drag and drop.** File drops are replaced by `DragEntered`, `DragPosition`, `DragDropped`
  and `DragLeft`, with a `DataTransferId`, lazy `fetch_data_transfer` →
  `DataTransferReceived`, and outgoing `start_drag` → `OutgoingDragDropped`/`OutgoingDragCanceled`
  (`event.rs` L80–175). On X11 the proposed action may only be known at completion (same lines).
  This shape is close to FLUI's ADR-0038 staged offer, so mapping it is direct.
- **Activation clicks.** `PointerButton::is_macos_activation_click` replaces
  `with_accepts_first_mouse`.

---

## 4. Synthetic-first testing

### 4.1 Prior art

- **Flutter.** `TestGesture` (`down`, `moveBy`, `moveTo`, `up`, `cancel`, `panZoomStart`, …) and
  `TestPointer`, which builds `PointerPanZoomStart/Update/End` events
  ([`test_pointer.dart` L22, L358–446, L474–606](https://github.com/flutter/flutter/blob/master/packages/flutter_test/lib/src/test_pointer.dart)).
  Timing is explicit per call (`timeStamp`), and the test's fake clock drives deadlines.
- **Compose.** Modality scopes `TouchInjectionScope`, `MouseInjectionScope`,
  `TrackpadInjectionScope`, `KeyInjectionScope`, `RotaryInjectionScope` and
  `MultiModalInjectionScope`
  ([`ui-test/.../test/`](https://github.com/androidx/androidx/tree/androidx-main/compose/ui/ui-test/src/commonMain/kotlin/androidx/compose/ui/test)).
  - Touch has `down`, `moveTo`, `moveWithHistory`, `moveWithHistoryMultiPointer`, `up` and
    `cancel`, plus shapes `click`, `longClick`, `doubleClick`, `swipe` and `multiTouchSwipe`
    ([`TouchInjectionScope.kt` L102–555](https://github.com/androidx/androidx/blob/androidx-main/compose/ui/ui-test/src/commonMain/kotlin/androidx/compose/ui/test/TouchInjectionScope.kt)).
  - Trackpad has `panStart`, `panMoveBy`, `panEnd`, `scaleStart`, `scaleChangeBy` and
    `scaleEnd`
    ([`TrackpadInjectionScope.kt` L215–290](https://github.com/androidx/androidx/blob/androidx-main/compose/ui/ui-test/src/commonMain/kotlin/androidx/compose/ui/test/TrackpadInjectionScope.kt)).
  - `moveWithHistory` is the key idea: a test can feed coalesced samples.
- **Chromium.** `SyntheticGestureController` "repeatedly invokes the gesture object's
  ForwardInputEvent method … until the gesture has finished"
  ([`synthetic_gesture_controller.h` L27–30](https://chromium.googlesource.com/chromium/src/+/main/content/common/input/synthetic_gesture_controller.h)).
  Its `SyntheticGestureTarget` asks the *platform* for `GetTouchSlopInDips`,
  `GetSpanSlopInDips`, `GetMinScalingSpanInDips`, `GetMouseWheelMinimumGranularity`,
  `PointerAssumedStoppedTime` and the VSync parameters
  ([`synthetic_gesture_target.h` L27–63](https://chromium.googlesource.com/chromium/src/+/main/content/common/input/synthetic_gesture_target.h)).
  This is a ready-made definition of a *platform personality*. Gesture parameter types include
  pinch and smooth drag
  ([`synthetic_pinch_gesture_params.h`](https://chromium.googlesource.com/chromium/src/+/main/content/common/input/synthetic_pinch_gesture_params.h)).
- **Masonry.** `TestHarness` has `process_pointer_event`, `process_text_event`,
  `process_access_event`, `mouse_move`, `mouse_button_press`/`release`, `mouse_wheel`,
  `mouse_click_on(id)`, `animate_ms` and `render`
  ([`harness.rs` L439–844](https://github.com/linebender/xilem/blob/main/masonry_testing/src/harness.rs)).
  Mouse only; no touch or pen helpers at those lines.
- **FLUI today.** `flui_testing::replay::PointerScript` plus `HeadlessBinding::replay` advance a
  virtual clock and pump real frames through the production `GestureBinding`
  (`crates/flui-testing/src/replay.rs`). Trace provenance rules are in
  `crates/flui-interaction/docs/PERFORMANCE.md` ("Trace provenance and replay").

### 4.2 Recommended design

1. **Tier 1: declarative scripts.** Extend `PointerScript` constructors to cover the union of
   the Compose shapes, XCUITest shapes and BiDi properties:
   - `pinch { center, from_span, to_span, duration }`, `rotate`, two-finger pan;
   - `swipe_with_velocity`, `fling`;
   - pen with pressure, tilt and twist ramps, eraser flip, barrel button, hover-in and
     hover-out;
   - trackpad `PanZoom` phases plus OS momentum plus `ScrollInertiaCancel`;
   - notched wheel vs precise wheel;
   - `moveWithHistory`-style coalesced bursts;
   - cancel with each `CancelReason`.

   Each shape is a parametric curve sampled at an explicit `SampleRate`, so 60 Hz and 240 Hz
   variants come from one definition.
2. **Personality fixtures.** One plain struct, filled from documented sources:

   ```rust
   #[non_exhaustive]
   pub struct InputPersonality {
       pub touch_slop: f64, pub pan_slop: f64, pub scale_span_slop: f64,
       pub double_tap_timeout: Duration, pub double_tap_slop: f64,
       pub long_press_timeout: Duration,
       pub fling_velocity: RangeInclusive<f64>,
       pub wheel_line_px: f64, pub velocity: VelocityEstimatorKind,
   }
   ```

   Initial presets:
   - Android 8 dp / 300 ms / 400 ms / 50–8000 (§2.4);
   - Windows read from `GetDoubleClickTime`, `SM_CXDRAG` and `SPI_GETWHEELSCROLLLINES` on
     device and recorded;
   - Apple values [U: not published as constants; measure on device].

   Runtime reads come from ADR-0172 host preferences. A table-driven test runs every script
   × every personality and asserts recognizer *outcomes*: which recognizer won, the callback
   sequence and the terminal reason. It never asserts internal state.
3. **Tier 2: recorded native traces.** A recorder in `flui-platform` writes the
   `PlatformInput` stream *after* backend translation and *before* the binding. Per event it
   records the source timestamp, the arrival timestamp, the frame index and coalesced samples
   (PERFORMANCE.md already lists these fields). The format is versioned and serde-backed.
   Replay goes through the same `HeadlessBinding::replay` seam. Two modes: source-time (tests
   recognizers) and arrival-time (tests resampling and stall behavior). Golden traces from
   real devices live outside the public repo, per PERFORMANCE.md. A small anonymized set per
   platform can be checked in once the format stabilizes.
4. **Tier 3: OS injection, for backend certification only.** Use `InjectSyntheticPointerInput`,
   `CGEvent`, XCUITest, `UiAutomation`, uinput/libei and WebDriver BiDi (§2). Each certifies
   *translation*: native message in, the expected `PlatformInput` out. It is compared against
   the Tier 2 trace of the same physical gesture. It is not a gesture-semantics test. These
   run from `xtask`, per the "Rust device tooling" convention.

---

## 5. Haptics and drag-and-drop API shape

### 5.1 Haptics

Evidence:

- Every platform offers *semantic* feedback: SwiftUI `SensoryFeedback`, the iOS generators,
  Android constants and macOS patterns.
- Two platforms add *composable primitives*: Android `Composition` and Core Haptics.
- Windows offers *device-scoped waveforms*: `SimpleHapticsController` per pen or device.
- SwiftUI's `SensoryFeedback` (iOS 17 / macOS 14) is the broadest semantic set: `success`,
  `warning`, `error`, `selection`, `increase`, `decrease`, `start`, `stop`, `alignment`,
  `levelChange`, `pathComplete`, and `impact(weight:intensity:)` /
  `impact(flexibility:intensity:)`
  ([SensoryFeedback](https://developer.apple.com/documentation/swiftui/sensoryfeedback)).
- Android 34 adds gesture-threshold and toggle semantics (§2.4).

Shape:

```rust
#[non_exhaustive]
pub enum HapticIntent {               // semantic tier; always callable, may no-op
    Selection, ThresholdActivate, ThresholdDeactivate, SegmentTick,
    ToggleOn, ToggleOff, DragStart, GestureStart, GestureEnd,
    Alignment, LevelChange, Confirm, Reject,
    Impact { weight: ImpactWeight, intensity: UnitInterval },
    Notification(NotificationKind),   // success / warning / error
}
pub enum HapticTarget { System, Device(DeviceId) }   // pen (Windows), trackpad (macOS)
pub struct HapticPattern { /* primitives with intensity + delay; validated at construction */ }
```

- Intents map through a per-platform table, for example:
  - `ThresholdActivate` → Android `GESTURE_THRESHOLD_ACTIVATE` (API 34) or `LONG_PRESS` below
    34;
  - → iOS `UIImpactFeedbackGenerator(.medium)` [U: Apple gives no mapping];
  - → macOS `.generic`;
  - → Windows pen `Click` waveform if the device supports it;
  - → Web `vibrate(10)` on Chromium only.
- Android routes through `View.performHapticFeedback` so user settings apply.
- Recognizers *declare* their haptic points as data: long-press accept → `ThresholdActivate`;
  drag-to-reorder start → `DragStart`; snap → `SegmentTick`. Widgets then get platform-correct
  feedback without code, which removes the "haptic patterns" package category.
- `HapticPattern` needs `supports(&HapticPattern) -> bool`. ADR-0031 and the
  `flui-platform-api/src/haptic_feedback.rs` module docs currently forbid availability
  discovery, so this tier needs a superseding ADR. The intent tier keeps fire-and-forget.

### 5.2 Drag and drop

Platform constraints that shape the API:

- **Laziness.** Lazy data is supported natively by macOS (`NSPasteboardItem` providers,
  `NSFilePromiseProvider`), iOS (`NSItemProvider` via `UIDragInteraction`), Windows (OLE delayed
  rendering via `IDataObject` [†]) and winit 0.31 (`fetch_data_transfer`). It is **impossible
  on the Web**: data must be in the store at `dragstart` (§2.6).
- **Late action.** X11 may report the action only at the drop (winit `DragPosition` docs, §3).
- **Promised files.** macOS and Windows support them (`FILEDESCRIPTOR`/`FILECONTENTS` [†]);
  iOS through `NSItemProvider` file representations [†]; the Web does not.
- **Modal loops.** OLE `DoDragDrop` is a modal loop, and macOS drag sessions are callback-driven.
- **Existing FLUI design.** ADR-0038's staged offer (`DragDropEvent::{Entered, Moved, Dropped,
  Exited}` with the payload fetched lazily after `Dropped`, `crates/flui-platform-api/src/input.rs`)
  already fits the *target* side.

Recommendation:

- **The source side mirrors the target.** `DragSource::start(DragRequest { items, allowed:
  TransferActions, preview: DragPreview, origin: PointerId })`. Each `DragItem` holds an ordered
  list of `(UniformType, Representation)`, where `Representation` is `Eager(Bytes) |
  Lazy(Box<dyn FnOnce() -> Result<Bytes, TransferError>>) | PromisedFile { name, writer } |
  InProcess(Box<dyn Any>)`.
  - The backend materializes eagerly where the platform demands it (Web) and states so in its
    capability.
  - `InProcess` never leaves the process. A drop inside the same app hands over the typed value,
    so reorderable lists, kanban boards and slidable rows need no serialization.
  - Leaving the window converts to an OS session with only the serializable representations.
- **One target API for in-app and OS drags.** Hit-tested drop-target widgets get
  `Enter/Over/Leave/Drop` with a `DropProposal { allowed, modifiers, position }` and answer
  with `DropFeedback`. Spring-loaded targets and auto-scroll near edges are policies on the
  target, not packages.
- **Async closures (§6) belong only at the `Lazy` and `PromisedFile` edges** (file IO). The
  frame path receives completion as a next-frame event.

---

## 6. Rust 1.99-era features that improve the API

All checked against [RELEASES.md](https://github.com/rust-lang/rust/blob/main/RELEASES.md)
(the version headers and Language/Stabilized-APIs sections):

| Feature (version) | Concrete use here |
|---|---|
| let-chains, edition 2024 (1.88) | Recognizer transitions: `if let Some(slot) = self.slot(id) && slot.exceeds(slop) && self.axis.admits(delta) { … }` without nesting. |
| `if let` guards on match arms (1.95) | `match ev { PointerEvent::Move(m) if let Some(s) = self.slots.get_mut(m.id) => … }`: the event dispatch table stays flat. |
| `assert_matches!` (1.96) | Event-log and outcome assertions in table tests: `assert_matches!(log[..], [Accepted(Pan), Rejected(Tap, Reason::Superseded)])`. |
| `cfg_select!` (1.95) | Backend selection in `flui-platform` without nested `cfg_if`. |
| `<[T]>::get_disjoint_mut` / `HashMap::get_disjoint_mut` (1.86) | Two-pointer recognizers (pinch, rotate) mutate both contact slots safely. |
| Trait upcasting (1.86) | `&dyn GestureRecognizer` → `&dyn Diagnosticable` or `&dyn Any` for inspection, with no hand-written `as_any`. |
| RPITIT (1.75) and precise capturing `use<..>` (1.82) | `fn samples(&self) -> impl Iterator<Item = &PointerSample> + use<'_, Self>` on the event trait, so coalesced history is exposed without allocating. |
| `#[diagnostic::on_unimplemented]` (1.78), `#[diagnostic::do_not_recommend]` (1.85) | On `GestureRecognizer`/`ScrollChainNode`: "`{Self}` is not a gesture recognizer; build one from `GestureArena`". `do_not_recommend` hides blanket impls from error suggestions. |
| async closures (1.85) | Only at DnD lazy representations and promised-file writers (IO edge), never in recognizers. |
| `bool::ok_or` (1.98) | Fallible constructor validation (`(scale.is_finite()).ok_or(PanZoomError::NonFinite)?`). |

Established patterns, not new features:

- **Sealed traits** for closed sets: `GestureRelation` participants and `ScrollChainNode`
  implementors outside the crate, if FLUI wants to close them.
- **Typestate builders** where a missing step is a real mistake. For example, a recognizer that
  must be bound to an arena before callbacks are attached
  (`RecognizerBuilder<Unbound>` → `.in_arena(&arena)` → `RecognizerBuilder<Bound>`).
- **`#[non_exhaustive]`** on every input enum, as winit 0.31 now does for `PointerKind`,
  `PointerSource`, `ButtonSource` and `DndAction` (beta.3 notes).

---

## Unverified items

- Precision-touchpad gestures cannot be injected through `InjectSyntheticPointerInput`.
- Windows has no app-facing haptic-touchpad API.
- `CGEvent` posting needs the Accessibility permission, and magnify/rotate cannot be
  synthesized publicly.
- XCUITest has no pressure or Pencil injection.
- Chromium and Firefox map trackpad pinch to ctrl+wheel.
- The `windows` crate feature names for DirectManipulation and Haptics.
- Whether an `objc2-core-haptics` crate exists.
- Apple gesture constants (slop, timeouts).
- Which Mac hardware drives Core Haptics.
- Masonry has no arena (search not exhaustive).

Pages marked [†] were cited from canonical URLs without being re-read in this session.
