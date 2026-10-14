# Mobile and web platform layer: what to support (iOS, Android, Web)

A deduplicated list of the features a production UI framework's platform layer needs on iOS (UIKit),
Android (Activity/JNI) and the web (wasm + canvas). Other frameworks were used as the checklist:
Flutter (core channels and first-party plugins), Tauri v2 (core and official plugins), Jetpack /
Compose, Compose Multiplatform (CMP) for iOS and web, and AccessKit. Researched 2026-10-14.

## Legend

**Tier**
- **Core**: the framework's platform layer has to own this, because it feeds layout, input, text,
  accessibility, lifecycle or rendering. A package cannot add it later without hooks in the embedder.
- **Svc**: a small system service that is usually shipped in core (plain-text clipboard, haptics,
  orientation lock). It is cheap to add and is expected out of the box.
- **Plug**: capability level. It belongs in a package on top of a stable embedder API (pickers,
  notifications, biometrics, sensors).

**Reference abbreviations**
- **FC**: Flutter core (a framework or engine channel or class; named).
- **FP**: Flutter first-party plugin (`flutter/packages`).
- **F3**: only third-party in Flutter. This shows the feature is *not* in Flutter's core.
- **TC / TP**: Tauri v2 core / official plugin in `plugins-workspace` with an Android and iOS implementation.
- **CJ**: Jetpack / Compose on Android.
- **CMP**: Compose Multiplatform for the `uikit` and `web` targets.
- **AK**: AccessKit.

Tauri renders through a system WebView, so the WebView covers IME, accessibility and insets for it.
Its plugin list shows the *capability* surface, not the UI-embedder surface.

**API levels** are given where a behavior depends on them. Web support figures come from MDN
browser-compat-data (BCD) as of today. In BCD, "chrome" means desktop Chrome unless noted.

---

## 1. Window, surface and insets

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Render surface lifecycle | Create, resize and destroy the GPU surface with the native view. Drop it on background and recreate it on return. | `CAMetalLayer`-backed `UIView` in a `UIWindowScene`. The system rejects GPU command submission from the background. | `SurfaceView`/`ANativeWindow`. The surface is destroyed around `onStop` (android-activity `InitWindow`/`TermWindow`). | `<canvas>` with WebGPU/WebGL2. Handle `webglcontextlost` and `GPUDevice.lost`. | FC engine; CMP `MetalView`/`MetalRedrawer` | Core |
| Device pixel ratio | Logical-to-physical scale, with change notification. | `UITraitCollection.displayScale` / `UIScreen.scale` | `DisplayMetrics.density`, `Configuration.densityDpi` | `devicePixelRatio`, plus a `matchMedia('(resolution: Ndppx)')` change listener. `ResizeObserver` `devicePixelContentBoxSize` gives exact canvas pixels (Chrome 84, Firefox 108, **no Safari**). | FC `FlutterView.devicePixelRatio` | Core |
| Frame pacing and refresh rate | Vsync-driven frames and high refresh rate. | `CADisplayLink` + `preferredFrameRateRange` (iOS 15). Above 60 Hz on iPhone needs Info.plist `CADisableMinimumFrameDurationOnPhone=true`. | `Choreographer`/`AChoreographer`, `Surface.setFrameRate` (API 30) | `requestAnimationFrame` (throttled in hidden tabs) | FC (the template sets the plist key); CMP `PlistSanityCheck` errors out without it | Core |
| Safe area / view padding | Insets for the notch, status bar and home indicator. | `safeAreaInsets` / `safeAreaLayoutGuide` | `WindowInsets` `systemBars`, `displayCutout`, `waterfall` | CSS `env(safe-area-inset-*)` (needs `viewport-fit=cover`) | FC `FlutterView.padding`/`viewPadding`; CJ `WindowInsets.safeDrawing`/`safeContent`; CMP `WindowInsetsManager` | Core |
| Keyboard (IME) inset and animation | Bottom inset taken by the soft keyboard, animated in step with it. | `UIKeyboardWillChangeFrameNotification`; `UIKeyboardLayoutGuide` (iOS 15) | `WindowInsets.Type.ime()` + `WindowInsetsAnimation` (API 30), frame by frame | `visualViewport` resize; `<meta name=viewport content="interactive-widget=resizes-content">` (Chrome Android 108, Firefox Android 133, not Safari); VirtualKeyboard API `overlaysContent`/`geometrychange` (Chromium 94 only) | FC `viewInsets`; CJ `WindowInsets.ime`, `imeAnimationSource`/`Target`; CMP `KeyboardInsetsManager` | Core |
| System-gesture insets and exclusion | Edge areas where OS gestures win, and the ability to reclaim some of them. | Defer only: `preferredScreenEdgesDeferringSystemGestures`. No inset values. | `WindowInsets` `systemGestures`, `mandatorySystemGestures`, `tappableElement`; `View.setSystemGestureExclusionRects` (API 29, capped per edge) | n/a | FC `systemGestureInsets`; CJ `Modifier.systemGestureExclusion` | Core |
| Edge-to-edge | The app draws under the system bars and pads by insets. | Always the case | Enforced for targetSdk 35 on Android 15. Since targetSdk 36 the opt-out (`windowOptOutEdgeToEdgeEnforcement`) is disabled. `enableEdgeToEdge()`. | `viewport-fit=cover` | FC `SystemUiMode.edgeToEdge`; CJ insets guide | Core |
| Display cutout, folds, corner radii | Geometry of non-drawable or non-interactive regions. | Safe area only. No public corner-radius API. | `DisplayCutout` (API 28); Jetpack WindowManager `WindowInfoTracker` → `FoldingFeature` (fold/hinge, FLAT/HALF_OPENED); `WindowInsets.getRoundedCorner` (API 31) | `horizontal/vertical-viewport-segments` media queries (Chrome 138); Device Posture API (Chrome 132) | FC `displayFeatures`, `displayCornerRadii` (Android only; corner radii drive the predictive-back page transition) | Core for cutout; optional for folds |
| Launch screen | What shows before the first frame. | Launch storyboard / `UILaunchScreen` (mandatory) | Android 12+ SplashScreen API (core-splashscreen) | HTML shown until the wasm boots | FC templates | Core (packaging) |

## 2. Orientation, configuration and multi-window

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Preferred orientations | Lock or restrict the rotation. | `supportedInterfaceOrientations` + `setNeedsUpdateOfSupportedInterfaceOrientations` / `UIWindowScene.requestGeometryUpdate` (iOS 16); Info.plist `UISupportedInterfaceOrientations(~ipad)` | `setRequestedOrientation` / manifest `screenOrientation`. **Ignored on displays with sw ≥ 600dp** for targetSdk 36 (opt-out property) and for 37 (no opt-out). | `screen.orientation.lock()` (Chrome Android, typically fullscreen only; no Safari); manifest `orientation` | FC `SystemChrome.setPreferredOrientations` | Svc |
| Configuration change without restart | Rotation, resize, dark mode, font scale, locale and density all arrive as events, not as a process or Activity restart. | Trait changes (`registerForTraitChanges`, iOS 17; `traitCollectionDidChange`); `viewWillTransition(to:with:)` | By default the Activity is **recreated**. Declare `android:configChanges`. Flutter's template lists orientation, keyboardHidden, keyboard, screenSize, smallestScreenSize, locale, layoutDirection, fontScale, screenLayout, density and uiMode, then handles `onConfigurationChanged`. | `resize` and `matchMedia` change events | FC Android manifest template; GameActivity exposes `getOrientation`, `getUIMode`, `getFontWeightAdjustment` and similar getters | Core |
| Arbitrary window size / multitasking | The window resizes to any size at runtime (split view, Stage Manager, freeform, desktop windowing). | `UIScene`. Split View, Slide Over, Stage Manager. iPadOS 26 adds freely resizable windows; `UIRequiresFullScreen` is deprecated (TN3192). | Multi-window, split screen, freeform. `resizableActivity` is ignored on large screens (see above). | Browser window resize | FC (no fixed-size assumption); CJ window size classes | Core |
| Multiple windows per app | Several scenes or windows in one process. | `UIApplicationSupportsMultipleScenes`; one `UIWindowScene` per window | Multi-instance Activities (`FLAG_ACTIVITY_MULTIPLE_TASK`) and desktop windowing | Separate tabs = separate app instances; `window.open`; Window Management `getScreenDetails` (Chromium) | FC multi-view (web embedding, desktop) | Optional |
| Picture-in-picture | A small floating window. | `AVPictureInPictureController` (video-centric) | `enterPictureInPictureMode` | Document PiP (Chrome 116) | (flag only) | Plug |

## 3. Input

### Touch

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Multi-touch pointer stream | Down, move, up and cancel per pointer, with IDs and timestamps. | `touchesBegan/Moved/Ended/Cancelled` (`UITouch`) | `MotionEvent` (`ACTION_POINTER_DOWN`, historical samples) | Pointer Events (`pointerType`), `setPointerCapture` | FC `PointerDataPacket` | Core |
| System cancellation | The OS steals the gesture (system swipe, incoming call). | `touchesCancelled` | `ACTION_CANCEL` | `pointercancel` | FC | Core |
| Coalesced and predicted samples | Full-rate samples and prediction, for ink and drags. | `coalescedTouches(for:)`, `predictedTouches(for:)`, `touchesEstimatedPropertiesUpdated` | `getHistorical*`; `View.requestUnbufferedDispatch`; androidx.input `MotionEventPredictor` | `getCoalescedEvents` / `getPredictedEvents` (Safari 18.2+, Chrome, Firefox) | — | Optional (core for drawing apps) |
| Browser gesture suppression | Stop the browser from zooming, pulling to refresh, swiping history or showing the long-press callout over the canvas. | — | — | CSS `touch-action`, `overscroll-behavior`, `-webkit-touch-callout`, `user-select`; viewport meta | FC web engine | Core (web) |

### Pen / stylus

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Stylus properties | Pressure, tilt, azimuth, roll, eraser and barrel buttons. | `UITouch.type == .pencil`, `force`, `altitudeAngle`, `azimuthAngle(in:)`, `rollAngle` (iOS 17.5) | `TOOL_TYPE_STYLUS`/`ERASER`, `AXIS_PRESSURE`/`TILT`/`ORIENTATION`, `BUTTON_STYLUS_PRIMARY` | `PointerEvent` `pressure`, `tiltX/Y`, `twist`, `altitudeAngle`/`azimuthAngle`, eraser button | FC `PointerDeviceKind.stylus`/`invertedStylus` | Core |
| Stylus hover | Pointer position before contact. | `UIHoverGestureRecognizer` with `zOffset` (iPadOS 16.1) | `ACTION_HOVER_*` with stylus tool type | `pointermove` with `pointerType=pen`, `buttons=0` | FC | Core |
| Pencil gestures | Double-tap or squeeze to switch tool. | `UIPencilInteraction` (squeeze in iOS 17.5) | Stylus buttons only | n/a | — | Optional |

### Mouse / trackpad

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Hover and buttons | Indirect pointer, secondary click. | Info.plist `UIApplicationSupportsIndirectInputEvents=YES` (in Flutter's template); `UIHoverGestureRecognizer`; `UIEvent.buttonMask` | `SOURCE_MOUSE`, `ACTION_HOVER_MOVE`, `ACTION_BUTTON_PRESS`, `getButtonState` | Pointer Events (`mouse`), `contextmenu` | FC (iOS uses `UIHoverGestureRecognizer`) | Core |
| Scroll, pan and pinch from trackpad or wheel | Precise scrolling and zoom gestures. | `UIPanGestureRecognizer.allowedScrollTypesMask`, `UIPinchGestureRecognizer`, `UIRotationGestureRecognizer` (all used by Flutter iOS) | `ACTION_SCROLL` (`AXIS_VSCROLL/HSCROLL`); touchpad `CLASSIFICATION_TWO_FINGER_SWIPE`/`PINCH` (API 34) | `wheel` (`ctrlKey` set for pinch in Chromium and Firefox); Safari `gesturestart/change` | FC `PointerPanZoom*` events | Core |
| Cursor shape | Arrow, text, hand, resize and other shapes. | `UIPointerInteraction`/`UIPointerStyle` (iPadOS 13.4; shapes and effects, not named cursors) | `View.setPointerIcon` / `PointerIcon` (API 24) | CSS `cursor` | FC `flutter/mousecursor` (Android, web, desktop; **not iOS**) | Core (desktop-class) |
| Pointer lock / capture | Relative motion with a hidden cursor. | `prefersPointerLocked` (iPadOS 14) | `View.requestPointerCapture` (API 26) | `requestPointerLock` (no iOS Safari) | — | Optional |

### Hardware keyboard

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Physical and logical key events | Down, up and repeat with a physical code, logical key, characters and modifiers. | `pressesBegan/Ended` with `UIPress.key` (`UIKey`, iOS 13.4) | `KeyEvent` (keycode, scancode, meta state, `KeyCharacterMap`, repeat) | `KeyboardEvent` `key`/`code`/`location`/`repeat`; `isComposing` / keyCode 229 during IME | FC `flutter/keyevent` | Core |
| Keyboard state sync | Modifiers and keys already held when focus arrives. | `UIEvent.modifierFlags` | `KeyEvent.getMetaState` | `getModifierState` | FC `flutter/keyboard` `getKeyboardState` | Core |
| Shortcut discoverability and menu bar | List the available commands. | `UIKeyCommand` + `UIMenuBuilder`; iPadOS 26 menu bar (`UIMainMenuSystem`) | `Activity.onProvideKeyboardShortcuts` (Keyboard Shortcuts Helper) | n/a; Keyboard Lock (Chromium, fullscreen) to claim reserved keys | FC `flutter/menu` (macOS only) | Optional (core for iPad) |
| Keyboard focus navigation | Tab and arrow traversal; system full keyboard access. | Full Keyboard Access, `UIFocusSystem` | D-pad and Tab focus navigation | DOM order / `tabindex` in the accessibility DOM | FC focus system | Core |

## 4. Text input, IME, spell check and autofill

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| IME connection | Two-way editing state (text, selection, composing/marked range) with the OS input method. | `UITextInput` (+`UIKeyInput`) on a responder view | `View.onCreateInputConnection` → `InputConnection` + `EditorInfo`. **NativeActivity has no IME support.** GameActivity's GameTextInput exposes text, selection and composing state, `setImeEditorInfo` and show/hide. | Hidden `<input>`/`<textarea>` driven by `beforeinput` and `composition*` (Flutter web, CMP `BackingDomInput`), or `EditContext` (**Chromium 121 only**) | FC `flutter/textinput` (`setClient`, `setEditingState`, `updateEditingState[WithDeltas]`) | Core |
| Show / hide soft keyboard | — | `becomeFirstResponder` / `resignFirstResponder` | `InputMethodManager.showSoftInput`, `WindowInsetsControllerCompat.show(ime())` | `focus()` on the hidden element. iOS Safari generally needs this inside a user gesture. | FC `TextInput.show/hide` | Core |
| Keyboard configuration | Input type, action key, capitalization, autocorrect, suggestions, smart quotes/dashes, keyboard appearance, incognito, hint locales, secure entry. | `UITextInputTraits` (`keyboardType`, `returnKeyType`, `autocapitalizationType`, `autocorrectionType`, `smartQuotesType`, `keyboardAppearance`, `isSecureTextEntry`, `textContentType`) | `EditorInfo.inputType`/`imeOptions` (`IME_FLAG_NO_PERSONALIZED_LEARNING`), `hintLocales` | `type`, `inputmode`, `enterkeyhint`, `autocapitalize`, `autocorrect`, `spellcheck` | FC `TextInputConfiguration` (`inputType`, `inputAction`, `textCapitalization`, `keyboardAppearance`, `enableIMEPersonalizedLearning`, `hintLocales`, `allowedMimeTypes`, …) | Core |
| Caret and character geometry for the IME | Lets the candidate window, floating keyboard, handwriting and loupe sit at the text. | `firstRect(for:)`, `caretRect(for:)`, `selectionRects(for:)` | `InputMethodManager.updateCursorAnchorInfo` (`CursorAnchorInfo`; `EditorBoundsInfo` API 33). Not exposed by the GameActivity C API. | Move the hidden input to the caret (Flutter `GloballyPositionedTextEditingStrategy`), or `EditContext` `updateCharacterBounds`/`updateControlBounds` | FC `TextInput.setEditableSizeAndTransform`/`setMarkedTextRect`/`setCaretRect`/`setSelectionRects` | Core |
| Native selection affordances | Handles, loupe, floating cursor. | `UITextInteraction` (iOS 13); `UITextSelectionDisplayInteraction` + `UITextLoupeSession` (iOS 17); floating cursor (space-bar trackpad) | The app draws them (`Magnifier`, API 28) | Browser-native only for real DOM text | FC (framework draws; `TextInputClient.updateFloatingCursor`) | Core |
| System edit / context menu | The OS-drawn Cut/Copy/Paste menu. | `UIEditMenuInteraction` (iOS 16). Its Paste avoids the iOS 16+ "Allow Paste" alert. | Floating `ActionMode` (`TYPE_FLOATING`) | Browser `contextmenu` (enable or suppress) | FC `ContextMenu.showSystemContextMenu` (iOS 16+); FC `flutter/contextmenu` (web enable/disable) | Core |
| Selection actions | Look Up, Search Web, Translate, Share and process text with other apps. | `UIReferenceLibraryViewController`, the Translate sheet, the share sheet | `Intent.ACTION_PROCESS_TEXT` activities (needs a manifest `<queries>`); share intent | — | FC `LookUp.invoke`, `SearchWeb.invoke`, `Translate.invoke`, `Share.invoke` (iOS); FC `flutter/processtext`, `Share.invoke` (Android) | Optional |
| Spell check | System suggestions for misspelled ranges. | `UITextChecker` | `TextServicesManager` → `SpellCheckerSession` | No JS API. Underlines appear only in real editable elements. | FC `flutter/spellcheck` (Android, iOS) | Optional |
| Undo integration | System undo gestures and buttons drive the editor's history. | `UndoManager` on the responder (shake, three-finger gestures, iPad shortcut bar) | No system-level hook | `beforeinput` `historyUndo`/`historyRedo` | FC `flutter/undomanager` (iOS) | Optional |
| Autofill (passwords, OTP, addresses) | Password managers and SMS codes fill the app's fields. | `textContentType` (`.password`, `.oneTimeCode`, …) + Associated Domains `webcredentials` | Autofill framework (`AutofillManager`, virtual structure, API 26). Not exposed by the GameActivity C API. | `autocomplete` on real `<input>` elements inside a `<form>` | FC `TextInput.requestAutofill`/`finishAutofillContext`, `AutofillGroup` (a hidden form on web) | Core |
| Stylus handwriting into fields | Write directly into text fields. | Scribble: `UIIndirectScribbleInteraction` for custom text (iOS 14) | `InputMethodManager.startStylusHandwriting`, `View.setAutoHandwritingEnabled` (API 33); `InputConnection.performHandwritingGesture` (API 34) | n/a | FC iOS (`scribbleInteractionBegan`, `requestElementsInRect`, `focusElement`); FC `flutter/scribe` (Android) | Optional (tablet-critical) |
| Rich content from the keyboard | GIFs, stickers and images inserted by the IME or by paste. | Paste configuration; Genmoji `NSAdaptiveImageGlyph` (iOS 18) | `InputConnection.commitContent`; `OnReceiveContentListener` (API 31) | `paste`/`drop` with files | FC `allowedMimeTypes` (Android) | Optional |
| Dictation, Live Text, Writing Tools | System text services that edit through the text API. | Dictation via `UITextInput`; Live Text `captureTextFromCamera` (iOS 15); `UIWritingToolsCoordinator` for custom text views (iOS 18.2) | Voice IMEs go through `InputConnection` | Through the hidden input | FC `LiveText.isLiveTextInputAvailable` | Optional |
| Password visibility policy | Briefly show the last typed character. | n/a | "Show passwords" setting; Android 17 splits it into physical-keyboard and touch settings | n/a | FC `brieflyShowPassword` | Optional |

## 5. Accessibility

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Semantics tree bridge | Expose a virtual node tree (role, label, value, state, frame, actions). | `UIAccessibilityElement` / container (`accessibilityElements`, `accessibilityFrame`, traits, `accessibilityActivate`, increment/decrement, `accessibilityScroll`) | `AccessibilityNodeProvider` + `AccessibilityNodeInfo` (virtual views), `AccessibilityEvent`. Not in the GameActivity API, so it needs JNI or Java. | **Parallel DOM** with ARIA roles (Flutter web `flt-semantics`; CMP web `ComposeWebSemanticsListener`) | FC engine bridges; AK `accesskit_ios`, `accesskit_android` (web adapter only "planned") | Core |
| Assistive-tech detection and lazy enable | Build semantics only when needed. | `UIAccessibilityIsVoiceOverRunning`, Switch Control, Speak Screen | `AccessibilityManager.isEnabled`/`isTouchExplorationEnabled` | No detection (privacy). Flutter web adds an off-screen "enable accessibility" placeholder (`flt-semantics-placeholder`). | FC | Core |
| Announcements / live regions | Speak dynamic changes. | `UIAccessibility.post(.announcement/.screenChanged/.layoutChanged/.pageScrolled)` | Live regions (`setAccessibilityLiveRegion`). `announceForAccessibility` is **deprecated in API 36**. Flutter Android reports `supportsAnnounce=false`. | `aria-live`; `ariaNotify` (Chrome 141, Firefox 150, Safari 27 per BCD) | FC `SemanticsService.announce`, `liveRegion` | Core |
| Custom actions and rotors | Extra actions on a node; navigation by element kind. | `UIAccessibilityCustomAction`, `UIAccessibilityCustomRotor` | `AccessibilityNodeInfo.addAction` (custom `AccessibilityAction`) | DOM buttons / ARIA | FC `customSemanticsActions` | Core |
| Editable text semantics | Value, selection, cursor movement and set-text for screen readers. | `UITextInput` + `accessibilityValue` | `ACTION_SET_TEXT`, `ACTION_SET_SELECTION`, movement granularities | A real `<input>` in the accessibility DOM | FC | Core |
| Route, pane and modal changes | Tell the screen reader that the context switched. | `.screenChanged`, `accessibilityViewIsModal` | `setAccessibilityPaneTitle` (API 28), `TYPE_WINDOW_STATE_CHANGED` | `role=dialog`, `aria-modal` | FC `scopesRoute`/`namesRoute` | Core |
| Voice Control labels | Alternate spoken names. | `accessibilityUserInputLabels` (iOS 13) | `contentDescription` (Voice Access) | `aria-label` | — | Core |

The user's accessibility *preferences* are listed under §10.

## 6. Clipboard and drag-and-drop

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Plain-text clipboard | Copy, paste, and "has text" without reading it. | `UIPasteboard.general`. Programmatic reads show the "Allow Paste" alert (iOS 16+), but `hasStrings` does not. `UIPasteControl` or the system edit menu avoids the alert. | `ClipboardManager`. Android 12+ shows a toast on read. Android 13 previews copies; `ClipDescription.EXTRA_IS_SENSITIVE` masks the preview. | `navigator.clipboard.readText/writeText` (Firefox 125+, Safari 13.1+; needs permission or user activation), or `paste`/`copy` events with `clipboardData` (no prompt, user-initiated) | FC `Clipboard.setData/getData/hasStrings` (text/plain only); TP clipboard-manager (`writeText`, `readText`, `writeImage`, `readImage`, `writeHtml`, `clear`); CMP `PlatformClipboard` | Svc (core) |
| Rich clipboard | Images, HTML, files and URIs. | Pasteboard items with UTTypes | `ClipData` with MIME types and content URIs | `ClipboardItem` (`write`: Chrome 76, Firefox 127, Safari 13.1) | F3 (super_clipboard); TP clipboard-manager | Optional |
| OS drag-and-drop | Drag between the app and other apps or files. | `UIDragInteraction`/`UIDropInteraction` (iOS 11; on iPhone since iOS 15) | `View.startDragAndDrop` (API 24), `DragAndDropPermissions`, `OnReceiveContentListener` | HTML DnD (`dragstart`/`dragover`/`drop`, `DataTransfer`, dropped files) | F3 in Flutter; CMP iOS `draganddrop` (core) | Optional |

## 7. Lifecycle and background

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| App / scene lifecycle | resumed → inactive → hidden → paused (→ detached). | `UISceneDelegate` (`sceneDidBecomeActive`, `sceneWillResignActive`, `sceneDidEnterBackground`, `sceneWillEnterForeground`). **UIScene becomes mandatory for apps built with the SDK after iOS 26** (TN3187). | Activity `onStart`/`onResume`/`onPause`/`onStop`/`onDestroy` (android-activity events) | `visibilitychange` + window `focus`/`blur` (what Flutter web listens to); `pagehide`/`pageshow` (bfcache); `freeze`/`resume` (Chromium only) | FC `flutter/lifecycle` (`AppLifecycleState`: detached, resumed, inactive, hidden, paused); CMP `SceneActiveStateListener`/`SceneForegroundStateListener` | Core |
| Window focus vs visibility | Visible but not focused (Control Center, split screen, a dialog). | `sceneWillResignActive` | `onWindowFocusChanged`; `onTopResumedActivityChanged` (multi-resume) | `focus`/`blur` | FC `onViewFocusChange` | Core |
| Memory pressure | Free caches when asked. | `didReceiveMemoryWarning` | `onTrimMemory` (`ComponentCallbacks2`) | none | FC `flutter/system` `memoryPressure` | Core |
| State restoration | Survive process death (common on Android). | Scene `stateRestorationActivity` (`NSUserActivity`) / UIKit restoration | `onSaveInstanceState` `Bundle` | `history.state`, `sessionStorage` | FC `flutter/restoration` (`RestorationManager`) | Core |
| Exit / close | Leave the app. | No supported quit API | `finish()` / `finishAndRemoveTask()` | `window.close()` (only for script-opened windows); `beforeunload` prompt (not on iOS Safari) | FC `SystemNavigator.pop`, `System.exitApplication`/`requestAppExit` (desktop) | Svc |
| Keep screen awake | Suppress the idle dimming. | `UIApplication.isIdleTimerDisabled` | `FLAG_KEEP_SCREEN_ON` / `View.setKeepScreenOn` | Screen Wake Lock (Chrome 84, Firefox 126, Safari 16.4; iOS 18.4) | CMP core (`ApplicationIdleTimer` iOS, `WebWakeLockManager` web); F3 in Flutter | Svc |
| App-switcher privacy and capture protection | Hide sensitive content in Recents, screenshots and screen share. | Overlay in `sceneWillResignActive`; detect recording via `sceneCaptureState` (iOS 17) / `UIScreen.isCaptured` | `FLAG_SECURE`; `Activity.setRecentsScreenshotEnabled` (API 33); `View.setContentSensitivity` (API 35). Android 17: `setContentCaptureEnabled(false)` no longer disables capture, so use `FLAG_SECURE`. | n/a | FC `flutter/sensitivecontent` (Android) | Optional |
| Background execution | Work after leaving the foreground. | `beginBackgroundTask`, `BGTaskScheduler` (iOS 13) | WorkManager, typed foreground services; Android 17 restricts background audio | Service Worker; Background Sync (Chromium) | F3 in Flutter | Plug |

## 8. Back navigation

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| System back event | The OS back button or gesture pops the framework's route stack. | **No system back.** The edge swipe is a `UINavigationController` gesture (`interactivePopGestureRecognizer`; iOS 26 adds `interactiveContentPopGestureRecognizer` for swipes anywhere), so a self-rendering framework implements it itself. CMP iOS maps a `UIScreenEdgePanGestureRecognizer` to its back-event API. | `OnBackPressedDispatcher` / `OnBackInvokedCallback` (API 33). For targetSdk 36, `onBackPressed` and `KEYCODE_BACK` **are no longer delivered**. | Browser back → `popstate` / Navigation API `navigate`; `CloseWatcher` (Chrome 126, Firefox 149; Safari preview) consumes back for dialogs | FC `flutter/navigation` `popRoute`; CMP `IosBackNavigationEventInput` | Core |
| "Can I pop?" declaration | Tell the OS whether the app will consume back. Otherwise the back-to-home animation can't play. | n/a | Enable or disable the callback | n/a | FC `SystemNavigator.setFrameworkHandlesBack` | Core |
| Predictive back progress | Stream gesture progress so the UI can preview the destination. | Framework-driven interactive pop | `OnBackAnimationCallback` + `BackEvent` (progress, touch, swipe edge; API 34); AndroidX `handleOnBackStarted/Progressed/Cancelled`. System animations are on for opted-in apps on Android 15 and on by default at targetSdk 36. | n/a | FC `flutter/backgesture` (`startBackGesture`, `updateBackGestureProgress`, `commitBackGesture`, `cancelBackGesture`); CJ `PredictiveBackHandler` | Core (Android) |
| URL and history routing (web) | Routes map to the address bar; back and forward work. | — | — | History API `pushState`/`replaceState`/`popstate`; Navigation API (Chrome 102, Firefox 147, Safari 26.2); hash vs path strategy | FC `flutter/navigation` (`selectSingleEntryHistory`/`selectMultiEntryHistory`, `routeInformationUpdated`) | Core (web) |

## 9. System UI chrome

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Status bar style and visibility | Light or dark icons; hide. | `preferredStatusBarStyle` (`.darkContent` iOS 13), `prefersStatusBarHidden` | `WindowInsetsControllerCompat.isAppearanceLightStatusBars`, `hide(statusBars())`. `setStatusBarColor` is deprecated (API 35); bars are transparent under edge-to-edge. | `<meta name="theme-color">`; `apple-mobile-web-app-status-bar-style` (home-screen apps) | FC `SystemChrome.setSystemUIOverlayStyle` (`statusBarBrightness` iOS, `statusBarIconBrightness` Android, `systemStatusBarContrastEnforced`) | Core |
| Navigation bar / home indicator | Icon contrast, auto-hide. | `prefersHomeIndicatorAutoHidden` | `isAppearanceLightNavigationBars`, `setNavigationBarContrastEnforced` (API 29) | n/a | FC `systemNavigationBarIconBrightness`/`ContrastEnforced` | Core |
| Immersive / fullscreen | Hide the system bars, transiently reveal them by swipe. | Hide the status bar + auto-hide the home indicator | `hide(systemBars())` + `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE` | Fullscreen API (Chrome 71, Firefox 64, Safari 16.4; partial on iOS) | FC `SystemUiMode` (leanBack, immersive, immersiveSticky, edgeToEdge, manual), `setSystemUIChangeCallback` | Svc |
| Status-bar tap (scroll to top) | The system "scroll to top" gesture. | iOS only (status bar tap) | — | — | FC `flutter/status_bar` `handleScrollToTop` | Core (iOS) |
| App-switcher label and color | Title and colour in Recents. | No counterpart (Flutter's iOS handler is a no-op) | `ActivityManager.TaskDescription` | `document.title` | FC `SystemChrome.setApplicationSwitcherDescription` | Svc |
| App icon badge | A number on the icon. | `UNUserNotificationCenter.setBadgeCount` (iOS 16) | Launcher-dependent (notification dots) | Badging API (Chrome desktop 81, Safari 17 macOS, iOS 16.4; not Chrome Android) | — | Plug |

## 10. System preferences (read and observe)

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Dark mode | Platform brightness. | `userInterfaceStyle` | `Configuration.uiMode` night mask | `prefers-color-scheme` | FC `platformBrightness` | Core |
| Text scale | User font size, non-linear where the OS does it. | Dynamic Type `UIContentSizeCategory` (+ change notification), `UIFontMetrics` | `Configuration.fontScale`; non-linear scaling on Android 14 (`TypedValue.applyDimension`) | Not exposed to canvas (browser zoom changes DPR) | FC `textScaleFactor` + `PlatformDispatcher.scaleFontSize` (non-linear) | Core |
| Bold text | — | `UIAccessibilityIsBoldTextEnabled` | `Configuration.fontWeightAdjustment` (API 31) | n/a | FC `AccessibilityFeatures.boldText` | Core |
| Reduce motion / animations off | — | `UIAccessibilityIsReduceMotionEnabled`, prefer cross-fade | `Settings.Global.TRANSITION_ANIMATION_SCALE` (Flutter observes it) / `ANIMATOR_DURATION_SCALE` | `prefers-reduced-motion` | FC `disableAnimations`, `reduceMotion`; CMP `MotionDurationScale` | Core |
| Contrast | Increase contrast / high contrast. | `accessibilityContrast` / Darker System Colors | `UiModeManager.getContrast()` (API 34, −1..1) | `prefers-contrast` (Chrome 96, Firefox 101, Safari 14.1); `forced-colors` (Chrome 89, Firefox 89, Safari 16) | FC `highContrast` (web reads `forced-colors`) | Core |
| Other accessibility flags | Invert colors, reduce transparency, on/off labels, differentiate without color, button shapes, autoplay. | `UIAccessibility*` status flags | `ACCESSIBILITY_DISPLAY_INVERSION_ENABLED` | `prefers-reduced-transparency` (Chrome 118 only) | FC `invertColors`, `onOffSwitchLabels`, `autoPlayAnimatedImages`, `autoPlayVideos`, `deterministicCursor` | Optional |
| Locales and RTL | Ordered preferred locales, per-app language. | `Locale.preferredLanguages` + change notification; per-app language in Settings | `Configuration.getLocales`; `LocaleManager` (API 33) | `navigator.languages` + `languagechange` | FC `locales`/`onLocaleChanged`; FC `flutter/localization` `getStringResource` (Android) | Core |
| 24-hour clock | — | `DateFormatter.dateFormat(fromTemplate:"j")` | `DateFormat.is24HourFormat` | `Intl.DateTimeFormat().resolvedOptions().hourCycle` | FC `alwaysUse24HourFormat` | Svc |
| System fonts and fallback | The system UI font and emoji/script fallback for self-rendered text. | CoreText (`CTFontCreateUIFontForLanguage`) | `/system/fonts`, `SystemFonts.getAvailableFonts` (API 29) | No enumeration except Local Font Access (Chrome desktop 103). Ship or download fonts; Flutter web downloads Noto fallbacks. | FC `systemFontFamily`; FC web `font_fallbacks` | Core |
| Text-spacing overrides | User line, letter and word spacing (WCAG 1.4.12). | — | — | Browser and extension stylesheets | FC `lineHeightScaleFactorOverride`, `letterSpacingOverride`, `wordSpacingOverride`, `paragraphSpacingOverride` (set by the web engine only) | Optional |

## 11. Haptics and sound

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Semantic haptics | Impact, selection, success, warning and error feedback tied to UI events. | `UIImpactFeedbackGenerator`, `UISelectionFeedbackGenerator`, `UINotificationFeedbackGenerator`; Core Haptics for patterns | `View.performHapticFeedback(HapticFeedbackConstants.*)` (honours user settings, no `VIBRATE` permission); `Vibrator`/`VibrationEffect` (needs `VIBRATE`) | `navigator.vibrate` (**not Safari**) | FC `HapticFeedback` (`vibrate`, `light/medium/heavyImpact`, `selectionClick`, `success/warning/errorNotification`); TP haptics (`vibrate`, `impactFeedback`, `notificationFeedback`, `selectionFeedback`); CJ `LocalHapticFeedback` | Svc (core) |
| System sounds | Click and tick sounds. | `AudioServicesPlaySystemSound` (Flutter uses key-click and "wheels of time" IDs) | `View.playSoundEffect(SoundEffectConstants.CLICK)` | n/a | FC `SystemSound.play` | Svc |

## 12. Share

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Outgoing share sheet | Share text, URLs or files. | `UIActivityViewController` (needs a popover anchor on iPad) | `Intent.ACTION_SEND` + `Intent.createChooser` / `ShareCompat` | `navigator.share` (Chrome Android 61, Chrome desktop 128, Safari 12.1; Firefox desktop flag only; needs user activation) | FC `Share.invoke` (text, from the selection menu; iOS and Android); F3 share_plus for full sharing; no Tauri official share plugin | Svc / Plug |
| Incoming share target | Receive shared content. | Share Extension (a separate app extension target) | `intent-filter` `ACTION_SEND`/`SEND_MULTIPLE` | Manifest `share_target` (Chromium) | — | Plug |

## 13. URL open, deep links and entry points

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Open URL externally | Hand a URL to the system or another app. | `UIApplication.open(_:options:)`, `canOpenURL` (+ `LSApplicationQueriesSchemes`) | `Intent.ACTION_VIEW` (package visibility `<queries>`, API 30) | `window.open` / navigation | FP url_launcher (`LaunchMode`: `platformDefault`, `inAppWebView`, `inAppBrowserView`, `externalApplication`, `externalNonBrowserApplication`); TP opener (`openUrl`, `openPath`, `revealItemInDir`); CMP `IosUriHandler`/`PlatformUriHandler` | Svc |
| In-app browser and OAuth session | Browser UI or an auth session without leaving the app. | `SFSafariViewController`; `ASWebAuthenticationSession` | Custom Tabs (androidx.browser) | Popup or redirect | FP url_launcher `inAppBrowserView` | Plug |
| Incoming deep links (custom scheme) | Launch or route the app from a URL. | `CFBundleURLTypes`; `scene(_:openURLContexts:)` | `intent-filter` VIEW + scheme; `onNewIntent` | The page URL itself | FC `flutter/navigation` `pushRouteInformation` (with `FlutterDeepLinkingEnabled` / `flutter_deeplinking_enabled`); TP deep-link | Core (routing entry) |
| Verified https links | Universal Links / App Links. | Associated Domains + `apple-app-site-association`; `scene(_:continue:)` (`NSUserActivity`) | `assetlinks.json`, `autoVerify` | n/a | FC (same path); TP deep-link | Core |
| Cold-start link delivery | A link delivered at launch, not only while running. | `connectionOptions.urlContexts`/`userActivities` in `scene(_:willConnectTo:options:)` | `getIntent()` in `onCreate` vs `onNewIntent` (`singleTop`) | Initial URL | FC | Core |
| App shortcuts / quick actions | Long-press icon actions. | `UIApplicationShortcutItem` (static plist + dynamic), `windowScene(_:performActionFor:)` | `ShortcutManager(Compat)` (static, dynamic, pinned) | Manifest `shortcuts` | FP quick_actions | Plug |

## 14. Files and pickers

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Activity-result plumbing | Route picker, permission and intent results back to the caller. | Presented view controllers + delegates | `registerForActivityResult` / `onActivityResult`. Needs the Java-side Activity. | Promises (with user activation) | FC Android embedding `ActivityPluginBinding.addActivityResultListener` / `addRequestPermissionsResultListener` (plugins depend on it) | Core (infra) |
| Open file(s) | A system document picker. | `UIDocumentPickerViewController(forOpeningContentTypes:)` (iOS 14), security-scoped URLs | SAF `ACTION_OPEN_DOCUMENT`/`GET_CONTENT`, `content://` via `ContentResolver` | `<input type=file>` (+ `showPicker()`); `showOpenFilePicker` (Chromium only) | FP file_selector; TP dialog + fs | Plug |
| Save / export | Pick a destination. | `UIDocumentPickerViewController(forExporting:)` | `ACTION_CREATE_DOCUMENT` | `showSaveFilePicker` (Chromium) or `<a download>` | FP file_selector (save location not supported on Android, iOS or web, per its README) | Plug |
| Pick a directory | — | Document picker with `UTType.folder` | `ACTION_OPEN_DOCUMENT_TREE` | `showDirectoryPicker` (Chromium) | FP file_selector (Android yes; iOS and web no) | Plug |
| Photo / video picker | Privacy-preserving media selection. | `PHPickerViewController` (iOS 14, no permission) | Photo Picker `PickVisualMedia` (Android 11+, backported through Play services); partial access `READ_MEDIA_VISUAL_USER_SELECTED` (API 34) | `<input type=file accept=image/* capture>` | FP image_picker | Plug |
| Open-with / file associations | The app handles a file type. | `CFBundleDocumentTypes`, `LSSupportsOpeningDocumentsInPlace` | `intent-filter` VIEW + mimeType | File Handling `launchQueue` (Chrome desktop only) | — | Plug |

## 15. Permissions

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Runtime permission model | Query, request and explain capability access. | Per-framework authorization calls (`AVCaptureDevice.requestAccess`, `CLLocationManager.requestWhenInUseAuthorization`, `PHPhotoLibrary.requestAuthorization`, `UNUserNotificationCenter.requestAuthorization`). The Info.plist `NS*UsageDescription` strings are mandatory. | Manifest `<uses-permission>` + runtime request (`ActivityResultContracts.RequestPermission`), `shouldShowRequestPermissionRationale`, one-time grants, `POST_NOTIFICATIONS` (API 33) | Permissions API `query` (Safari 16+); prompts come from feature calls and usually need a secure context and user activation | TC plugin `checkPermissions`/`requestPermissions` with `PermissionState {Granted, Denied, Prompt, PromptWithRationale (Android only)}`; F3 in Flutter (permission_handler) | Core (shared state type) + Plug (each capability) |

## 16. Storage and paths

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Bundled assets | Read the app's packaged resources. | `Bundle.main` | `AssetManager` (`AAssetManager` in the NDK) | `fetch` from the origin | FC asset loading | Core |
| App directories | Temp, cache, support, documents, downloads. | `FileManager`: `tmp`, `Caches`, `Library/Application Support`, `Documents` | `getCacheDir`, `getFilesDir`, `getNoBackupFilesDir`, `getExternalFilesDir` | No filesystem: OPFS `navigator.storage.getDirectory` (Chrome 86, Firefox 111, Safari 15.2), IndexedDB, Cache API | FP path_provider (temporary, support, library (iOS), documents, cache, external (Android), downloads); TC `tauri::path` resolver + TP fs | Plug (core needs cache/temp internally) |
| Key-value preferences | Small persistent settings. | `UserDefaults` | `SharedPreferences` / DataStore | `localStorage` | FP shared_preferences (DataStore by default on Android); TP store (this is a KV store, not an app store) | Plug |
| Persistence guarantees | Eviction and backup policy. | Caches can be purged; backup exclusion | Auto Backup rules | `StorageManager.persist()`/`estimate()`; Safari's ITP evicts script-writable storage of non-installed sites after 7 days without interaction | — | Optional |

## 17. Notifications

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Local notifications | Scheduled or immediate, with actions. | `UNUserNotificationCenter` (categories, foreground presentation delegate) | `NotificationCompat`, channels (API 26), `POST_NOTIFICATIONS` (API 33), exact-alarm permission | Notifications API / `ServiceWorkerRegistration.showNotification`. On iOS only Home Screen web apps (16.4+). | TP notification; F3 in Flutter | Plug |
| Push | Remote delivery. | APNs | FCM | Push API + VAPID | F3 (FlutterFire) | Plug |
| Tap → route | Open the app at the right screen. | `userNotificationCenter(_:didReceive:)` | `PendingIntent` → Activity intent | `notificationclick` in the service worker | Shares the deep-link routing path (§13) | Core (routing hook) |

## 18. Biometrics and secure storage

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Biometric prompt | Face/fingerprint user verification. | `LAContext.evaluatePolicy` (`NSFaceIDUsageDescription`) | androidx.biometric `BiometricPrompt` | WebAuthn platform authenticator only (no generic unlock prompt) | FP local_auth; TP biometric (iOS without biometric cryptography; Android `biometricCipher`) | Plug |
| Secure storage | Hardware-backed secrets. | Keychain (`SecItem`, access control with biometry) | Android Keystore (`setUserAuthenticationRequired`) | No equivalent (best effort: non-extractable `CryptoKey` in IndexedDB) | F3 in Flutter; TP stronghold is desktop only (no mobile implementation) | Plug |
| Passkeys / credentials | Sign-in credentials. | AuthenticationServices `ASAuthorizationController` | Credential Manager (androidx.credentials) | WebAuthn / `navigator.credentials` | F3 | Plug |

## 19. In-app review and purchase (flag only)

| Feature | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|
| In-app review prompt | StoreKit `requestReview` | Play In-App Review API | n/a | F3 in Flutter | Plug |
| In-app purchase | StoreKit 2 | Play Billing Library | Payment Request / Digital Goods API (TWA) | FP in_app_purchase; nothing official in Tauri | Plug |

## 20. Sensors, location, camera and other device capabilities (flag only)

| Capability | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|
| Location | CoreLocation | Fused Location / `LocationManager` | Geolocation | TP geolocation; F3 in Flutter | Plug |
| Camera / scanning | AVFoundation | CameraX / Camera2 | `getUserMedia` | FP camera; TP barcode-scanner | Plug |
| Motion sensors | CoreMotion | `SensorManager` | DeviceOrientation/Motion (iOS needs `DeviceOrientationEvent.requestPermission()`) | F3 | Plug |
| NFC | Core NFC | `NfcAdapter` | Web NFC (Chrome Android) | TP nfc | Plug |
| Torch | `AVCaptureDevice.torchMode` | `CameraManager.setTorchMode` | n/a | TP torch | Plug |

## 21. Also needed: native view interop

| Feature | Definition | iOS | Android | Web | References | Tier |
|---|---|---|---|---|---|---|
| Embedded native views | Host a platform view (WebView, map, video, ads) inside the framework's scene, with correct clipping, z-order, hit-testing and accessibility. | `UIView` in the hierarchy | `View` (hybrid composition / texture) | HTML element overlay | FC `flutter/platform_views` (+ `platform_views_2`); FP webview_flutter, google_maps_flutter, video_player; CMP `UIKitView`/`AndroidView` | Core (architecture: it shapes the compositor, hit-testing and the accessibility tree, so decide it early) |

---

## Platform deadlines that change the baseline

- **iOS (TN3187):** UIKit requires the scene-based lifecycle when you build with the SDK released after
  iOS 26. Otherwise the app does not launch.
- **iPadOS 26:** `UIRequiresFullScreen` is deprecated (TN3192) and windows resize freely. iPad also
  gains a menu bar (`UIMainMenuSystem`, `UIMenuBuilder`).
- **Android 15 (targetSdk 35):** edge-to-edge is enforced and `setStatusBarColor` /
  `setNavigationBarColor` are deprecated.
- **Android 16 (targetSdk 36):**
  - The edge-to-edge opt-out no longer works.
  - Predictive back is on by default: `onBackPressed` and `KEYCODE_BACK` are not delivered.
  - Orientation and resizability requests are ignored on displays with sw ≥ 600dp (an opt-out still exists).
  - `announceForAccessibility` is deprecated.
- **Android 17 (targetSdk 37):**
  - The opt-out for ignored orientation and resizability is gone.
  - Background audio is restricted.
  - Password masking has a separate setting for physical keyboards.
  - IMEs can flag the selected conversion candidate (`TextAttribute.isTextSuggestionSelected`).
- **Web:**
  - The Navigation API now ships in all three engines (Firefox 147, Safari 26.2).
  - `ariaNotify` ships in Chrome 141, Firefox 150 and Safari 27 per BCD.
  - `EditContext` is still Chromium-only, so production text input needs the hidden-element strategy
    as the baseline.

## Notes for Rust-native backends

These are verified against the crates' sources or headers and are relevant to an
android-activity/UIKit/wasm stack.

- **android-activity, NativeActivity:** has no IME support beyond physical-style key events. Its
  README calls it insufficient for production text input.
- **android-activity, GameActivity:** the C API (`GameActivity.h`, `gametextinput.h`) provides:
  - inset types (status/navigation/caption bars, IME, cutout, system and mandatory gestures,
    tappable element, waterfall);
  - configuration getters (orientation, uiMode, fontWeightAdjustment, locale);
  - IME text, selection and composing state, plus `setImeEditorInfo` and show/hide.

  The headers expose **nothing** for any of the following:
  - `CursorAnchorInfo`
  - handwriting gestures
  - `commitContent`
  - autofill
  - accessibility

  Those need a Java/Kotlin `InputConnection` and an `AccessibilityNodeProvider` reached over JNI.
  `accesskit_android` implements the accessibility side.
- **AccessKit** has released `accesskit_ios` (UIAccessibility) and `accesskit_android` adapters. Its
  **web adapter is only planned**, so web accessibility means owning a parallel ARIA DOM, as Flutter
  web and CMP web do.
- **Activity results and permissions** on Android require hooks on the Java-side Activity.
  `NativeActivity` provides none, so every picker, permission and share flow depends on that layer.
- **iOS Info.plist keys** a self-rendering framework must ship by default:
  - `UIApplicationSceneManifest`
  - `CADisableMinimumFrameDurationOnPhone`
  - `UIApplicationSupportsIndirectInputEvents`
  - the `UISupportedInterfaceOrientations` variants
  - the `NS*UsageDescription` strings for each capability used

  These are all present in Flutter's iOS template, except the usage strings.

---

## Sources

**Primary sources read for this list (GitHub raw files via `gh api`, docs via fetch):**
- Flutter `packages/flutter/lib/src/services/system_channels.dart`, `system_chrome.dart`, `text_input.dart`: https://github.com/flutter/flutter/tree/master/packages/flutter/lib/src/services
- Flutter `packages/flutter/lib/src/widgets/system_context_menu.dart`: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/system_context_menu.dart
- Flutter engine `lib/ui/platform_dispatcher.dart`, `lib/ui/window.dart` (`AccessibilityFeatures`, `ViewPadding`, `DisplayFeature`): https://github.com/flutter/flutter/tree/master/engine/src/flutter/lib/ui
- Flutter iOS embedder `FlutterPlatformPlugin.mm`, `FlutterViewController.mm`: https://github.com/flutter/flutter/tree/master/engine/src/flutter/shell/platform/darwin/ios/framework/Source
- Flutter Android embedder `PlatformChannel.java`, `FlutterView.java`, `AccessibilityBridge.java`: https://github.com/flutter/flutter/tree/master/engine/src/flutter/shell/platform/android/io/flutter
- Flutter web engine (`app_lifecycle_state.dart`, `media_query_manager.dart`, `text_editing/text_editing.dart`, `semantics/semantics_helper.dart`, `navigation/history.dart`): https://github.com/flutter/flutter/tree/master/engine/src/flutter/lib/web_ui/lib/src/engine
- Flutter app templates (Android manifest `configChanges`, iOS Info.plist keys): https://github.com/flutter/flutter/tree/master/packages/flutter_tools/templates/app
- flutter/packages README and type files for path_provider, file_selector, url_launcher, shared_preferences: https://github.com/flutter/packages/tree/main/packages
- Tauri plugins-workspace (plugin list, mobile folders, READMEs, `guest-js` of haptics, biometric, opener, clipboard-manager): https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins
- Tauri `crates/tauri/src/plugin.rs` (`PermissionState`): https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/plugin.rs
- Compose Multiplatform `compose/ui/ui/src/{iosMain,webMain}` (file listing, `IosBackNavigationEventInput`, `PlistSanityCheck`, `WebWakeLockManager`): https://github.com/JetBrains/compose-multiplatform-core/tree/jb-main/compose/ui/ui/src
- android-activity README and vendored GameActivity / GameTextInput headers: https://github.com/rust-mobile/android-activity
- AccessKit README (adapter list): https://github.com/AccessKit/accesskit
- MDN browser-compat-data JSON (all web version numbers above): https://github.com/mdn/browser-compat-data
- MDN EditContext API: https://developer.mozilla.org/en-US/docs/Web/API/EditContext_API
- Android 16 behavior changes (targetSdk 36): https://developer.android.com/about/versions/16/behavior-changes-16
- Android 17 behavior changes (targetSdk 37): https://developer.android.com/about/versions/17/behavior-changes-17
- Compose WindowInsets types: https://developer.android.com/develop/ui/compose/system/insets
- Android predictive back: https://developer.android.com/guide/navigation/custom-back/predictive-back-gesture
- Android API 36 diff for `View` (`announceForAccessibility` deprecated): https://developer.android.com/sdk/api_diff/36/changes/android.view.View
- Apple TN3187, UIScene lifecycle: https://developer.apple.com/documentation/technotes/tn3187-migrating-to-the-uikit-scene-based-life-cycle
- Apple TN3192, `UIRequiresFullScreen` deprecation: https://developer.apple.com/documentation/technotes/tn3192-migrating-your-app-from-the-deprecated-uirequiresfullscreen-key
- Apple `UIWritingToolsCoordinator`: https://developer.apple.com/documentation/uikit/uiwritingtoolscoordinator
- WWDC25 session 243 (iPad menu bar) and 282 (flexible UIKit apps): https://developer.apple.com/videos/play/wwdc2025/243/ and https://developer.apple.com/videos/play/wwdc2025/282/
- Apple forums thread on iOS 26 `interactiveContentPopGestureRecognizer`: https://developer.apple.com/forums/thread/803771

**Not fetched in this session:** the other UIKit, Android and Web API names in the tables are standard
reference symbols. They are cited from the reference pages at developer.apple.com/documentation/uikit,
developer.android.com/reference and developer.mozilla.org, and their API levels come from general
knowledge. Check those before quoting a level in an ADR. The least-checked claims are these:

- the iOS 17.5 Pencil roll and squeeze APIs;
- the iPadOS 14 `prefersPointerLocked`;
- the Android API 34 touchpad `MotionEvent` classifications;
- Safari's 7-day ITP storage cap.
