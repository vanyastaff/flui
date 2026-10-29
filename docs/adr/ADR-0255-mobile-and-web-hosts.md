# ADR-0255: How Android, iOS and the Web host FLUI

- **Status:** Accepted (2026-10-20). Not implemented: the code changes land as separate PRs after
  this ADR, and a platform's row in `docs/platform-parity.md` moves only after a run on that
  platform (Confirmation).
- **Date:** 2026-10-20
- **Supersedes:** [ADR-0247](ADR-0247-android-java-view.md) in three places: item 1, where it embeds
  the dex in the native library with `include_bytes!` and has every packager ship that copy (the
  sources, the checked-in dex, the digest gate and "no JDK, no build script" stand); item 2, except
  its first sentence (the installation is posted to the Java main thread and never waited on): the
  in-memory and file class loaders and the `hasCode="false"` manifest; item 9 in full, whose
  escalation this ADR takes.
- **Related:** [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) (the platform crates and
  where host code lives), [ADR-0152](ADR-0152-capability-seam-revised.md) (the capability seam and
  `Completion`), [ADR-0154](ADR-0154-capability-crates.md) (capability crates and their native
  code), [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (the file that gains `targetSdk`, and
  the gate style the newer APIs below follow),
  [ADR-0249](ADR-0249-the-android-loop-blocks-until-woken.md) (every producer of work wakes the
  looper), [ADR-0254](ADR-0254-window-contract.md) (the window the hosts present),
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) and
  [ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md) (the text
  store host the iOS and Web editors serve), [ADR-0097](ADR-0097-no-process-global-state-gate.md)
  (no process-wide state), [ADR-0240](ADR-0240-semantics-identifier.md) (the identifier the
  accessibility trees carry)
- **Sources:** other projects are read at pinned commits and cited as `owner/repo@sha path:line`:
  AOSP (`aosp-mirror/platform_frameworks_base@1cdfff555f`, a mirror last pushed 2025-11-12),
  AccessKit (`AccessKit/accesskit@a39329d28`), android-activity v0.6.1
  (`rust-mobile/android-activity@b4ddf059b`), Flutter (`flutter/flutter@6597889f57`), Slint
  (`slint-ui/slint@277e7676ce`), Compose Multiplatform
  (`JetBrains/compose-multiplatform-core@13b7599795`), winit
  (`rust-windowing/winit@1893565716`) and MDN's browser-compat-data
  (`mdn/browser-compat-data@5da921db9`), with Apple's TN3187 and the Android developer guides named
  where used. How Java reaches the process (an in-memory dex against a dex in the APK against
  `GameActivity`) is decided in decision 1 from the sources cited there.
  Lines of FLUI's own tree are at `main` `083b0f75d`.

## Context and problem statement

A mobile or browser host is the object the system calls: the Activity, the scene delegate, the page.
What an app can do on each platform is bounded by what that object receives and what it can declare
in the manifest, plist or page.

**Android.** FLUI's apps are `android.app.NativeActivity` with `android:hasCode="false"`, and
[ADR-0247](ADR-0247-android-java-view.md) added one Java host view, `FluiView`, delivered as an
in-memory dex: input methods, drag and drop, pasted content and handwriting work through it. What
is missing is accessibility, back, live insets (the keyboard's included), activity results,
permissions and warm deep links. Everything on that list except results, permissions and warm
deep links can be done from the injected view. Those three cannot: `NativeActivity` forwards none
of `onNewIntent`, `onActivityResult`, `onRequestPermissionsResult`, `onTrimMemory`,
`onMultiWindowModeChanged`, `onTopResumedActivityChanged` and `onUserLeaveHint`
([aosp-mirror/platform_frameworks_base@1cdfff555f core/java/android/app/NativeActivity.java:126-301](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f/core/java/android/app/NativeActivity.java#L126-L301);
android-activity's README says the same of the class of problem, [rust-mobile/android-activity@b4ddf059b
README.md:266-283](https://github.com/rust-mobile/android-activity/blob/b4ddf059b/README.md#L266-L283)), and a class that exists only in an in-memory dex cannot be named by the
manifest, so no share target, receiver or `FileProvider` either. ADR-0247 item 9 named this switch
as the escalation, to take "when FLUI needs a callback only a manifest-declared class receives".
That need is now the owner's list.
The manifest and the packager are behind the framework as well: the template declares three of
the eleven `configChanges` values Flutter's declares
(`crates/flui-cli/templates/platforms/android/AndroidManifest.xml:5-22`;
[flutter/flutter@6597889f57
packages/flutter_tools/templates/app/android.tmpl/app/src/main/AndroidManifest.xml.tmpl:12](https://github.com/flutter/flutter/blob/6597889f57/packages/flutter_tools/templates/app/android.tmpl/app/src/main/AndroidManifest.xml.tmpl#L12)), and
an Activity the system recreates is a new `android_main` and a whole new Rust runtime
(`crates/flui-app/src/app/runner/android.rs:441-447`); the SDK-tools packager sets `targetSdk` to
the newest installed platform (`crates/flui-cli/src/build/android_package.rs:70-97,147-148`), so
installing `platforms;android-36` silently turns on targetSdk 36 behaviour: no `KEYCODE_BACK`,
predictive-back animations by default, edge-to-edge without an opt-out
([Android 16 behavior changes](https://developer.android.com/about/versions/16/behavior-changes-16)).

**iOS.** The host is already Rust over `UIApplicationMain` and UIScene: `FluiAppDelegate`,
`FluiSceneDelegate`, `FluiView` and `FluiViewController` are `objc2::define_class!` classes, and
the generated `Info.plist` carries the scene manifest that Apple's
[TN3187](https://developer.apple.com/documentation/technotes/tn3187-migrating-to-the-uikit-scene-based-life-cycle)
will require (`crates/flui-cli/src/build/ios_package.rs:552-577`). Missing
are `UITextInput`, accessibility, the scene callbacks for URLs, user activities and shortcuts,
keyboard insets, a back gesture and several plist keys. Every one is reachable through objc2.

**Web.** Text goes through `EditContext` where it exists and a hidden `<textarea>` elsewhere; the
page is a static scaffold copied once; there is no accessibility tree, no routing, no semantics
host, and the dev server answers 404 to any path that is not a file
(`crates/flui-cli/src/serve.rs:262-301`). The textarea sets no `font-size` (iOS Safari zooms the
page on focus below 16 px, which Flutter documents and guards against:
[flutter/flutter@6597889f57
engine/src/flutter/lib/web_ui/lib/src/engine/semantics/text_field.dart:12-46](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/lib/web_ui/lib/src/engine/semantics/text_field.dart#L12-L46)) and no
`enterkeyhint` (`crates/flui-native/src/text/web.rs:634-665`).

The question this record answers: how do Android, iOS and the Web host FLUI, so that accessibility,
back, insets, activity results, permissions, deep links and text input reach the framework from the
object the system calls, and how are the non-Rust parts of those hosts built and delivered?

## Decision drivers

- What an app can do on each platform is bounded by what the host object receives and what it can
  declare in the manifest, plist or page.
- `NativeActivity` forwards none of `onNewIntent`, `onActivityResult`,
  `onRequestPermissionsResult`, `onTrimMemory`, `onMultiWindowModeChanged`,
  `onTopResumedActivityChanged` and `onUserLeaveHint`, and a class that exists only in an in-memory
  dex cannot be named by the manifest.
- Building FLUI for Android needs no JDK and no build script: the sources, the checked-in dex and
  the digest gate stay (ADR-0247).
- The Java side queues and wakes and never waits for the owner, and the owner never waits for the
  Java main thread; every producer of work wakes the looper, and a paused app costs no CPU between
  real events (ADR-0247 item 5, ADR-0249).
- No process-global state: the `JavaVM` and Activity pointers live in the provider context, never in
  a `static` (ADR-0097).
- The framework's iOS host stays Swift-free and Xcode-free; Swift appears only inside capability
  crates that need a Swift-only framework.
- CJK composition needs marked text, which `UITextInput` carries and `UIKeyInput` cannot.
- A browser exposes no signal that assistive technology is present, and `EditContext` is
  Chromium-only, so the `<textarea>` path must be correct everywhere.
- A default-priority back callback turns the system's predictive-back animation off, so back is
  registered only while the framework can pop.
- Generated project files are written whole and never hand-edited, so the framework can change
  them; platform behaviour must not depend on the newest SDK installed on the build machine.
- Native source stays in the crate whose Rust it must match, and the CLI takes it from the
  dependency version the app's lockfile resolves.
- Support is claimed only by an executed run; CI executes Linux only, so mobile and browser
  behaviour needs device and browser runs.

## Considered options

1. The hosts decided below: `FluiActivity` over `NativeActivity` with a buffering queue and a
   checked-in dex delivered in the APK, an iOS host in pure Rust on objc2, and a Web host where
   Rust owns the DOM.
2. Android: stay on `hasCode="false"` and use the deprecated framework `Fragment` for results.
3. Android: `GameActivity`.
4. Android: an own Activity with own native glue (Flutter's model).
5. Android: `accesskit_android::InjectingAdapter`.
6. Android: the dex built by a `build.rs`.
7. iOS: a Swift or Objective-C shim (Flutter's, Compose's and Tauri's shape).
8. iOS: winit's UIKit implementation.
9. iOS: making `FluiView` the first responder for text.
10. iOS: registering objc2 protocol objects from capability crates.
11. Web: `EditContext` alone.
12. Web: waiting for an AccessKit web adapter.
13. Web: the Navigation API alone.
14. Web: a static scaffold the user edits.

## Decision

Chosen option: 1, because it takes the escalation ADR-0247 item 9 named, now that the owner's list
needs callbacks only a manifest-declared class receives, while keeping the injected view, the
checked-in dex and the Xcode-free iOS build, and gives each host the accessibility, back, inset and
text-input paths its platform requires.

### Android

1. **`FluiActivity` is the Activity.** `dev.flui.platform.FluiActivity extends
   android.app.NativeActivity` ships in the APK as part of the checked-in dex and is named by the
   generated manifest, which declares `android:hasCode="true"`. `android-activity`'s
   `native-activity` backend, the taken surface and the native input queue stay as they are.

2. **A buffering queue carries what `NativeActivity` drops.** `FluiActivity` forwards `onNewIntent`,
   `onActivityResult`, `onRequestPermissionsResult`, `onTrimMemory`, `onMultiWindowModeChanged`,
   `onTopResumedActivityChanged` and `onUserLeaveHint` into a Java queue in `dev.flui.platform`,
   which the owner drains on its own thread. The queue buffers, because `onNewIntent` can arrive
   before `android_main` runs. Every new producer wakes the looper (ADR-0249): an entry that waited
   for the next unrelated event would deliver a deep link or a permission result late. ADR-0247
   item 5 holds: the Java side queues and wakes, never waits for the owner, and the owner never
   waits for the Java main thread.

3. **`FluiView` stays the one host view.** ADR-0247 items 3 to 8 and 10 are unchanged: one full-window
   view, owner-to-view calls posted, answers from state the owner published earlier. It gains the
   following, none of which needs the Activity subclass:
   - **Accessibility.** An `AccessibilityDelegate` on the view, driven by
     `accesskit_android::Adapter`, the low-level core, behind a
     `flui_semantics::PlatformAccessibility` implementation in the host (the contract already has
     publish, is-active and activation and action listeners:
     `crates/flui-semantics/src/platform.rs:66-95`). FLUI writes the delegate and its
     provider in `FluiView`, about 70 lines. TalkBack's touch exploration arrives as hover events
     ([aosp-mirror/platform_frameworks_base@1cdfff555f
     services/accessibility/java/com/android/server/accessibility/gestures/TouchExplorer.java](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f/services/accessibility/java/com/android/server/accessibility/gestures/TouchExplorer.java)),
     and under `NativeActivity` they reach the native queue before the view
     ([core/java/android/view/ViewRootImpl.java:7836-7860](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f/core/java/android/view/ViewRootImpl.java#L7836-L7860)), so the owner feeds them
     to `Adapter::on_hover_event` and reports the event handled when it returns events. The first
     request for the tree must be answered no later than the next display refresh
     ([AccessKit/accesskit@a39329d28 accesskit/src/lib.rs:3335-3348](https://github.com/AccessKit/accesskit/blob/a39329d28/accesskit/src/lib.rs#L3335-L3348)), so a request on
     the Java main thread is a producer that wakes the owner (ADR-0249). The adapter's update,
     action and hover calls may come from any thread with the caller providing exclusion
     ([adapters/android/src/adapter.rs:284-286,801-803,864-866](https://github.com/AccessKit/accesskit/blob/a39329d28/adapters/android/src/adapter.rs#L284-L286)); raising
     queued events happens on the Java main thread holding none of the locks the view's callbacks
     take ([adapters/android/src/event.rs:338-347](https://github.com/AccessKit/accesskit/blob/a39329d28/adapters/android/src/event.rs#L338-L347)). That is ADR-0247 item 5 again.
   - **Back.** An `OnBackInvokedCallback` (API 33; `OnBackAnimationCallback` from API 34), registered
     through the Activity's `getOnBackInvokedDispatcher()` from a per-API nested holder, as the class
     already does for other APIs, and registered **only while the framework can pop**. A
     default-priority callback turns the system's predictive-back animation off, so an app with
     nothing to pop must not hold one ([Android's predictive back
     guide](https://developer.android.com/guide/navigation/custom-back/predictive-back-gesture));
     Flutter registers only while the framework can pop
     ([flutter/flutter@6597889f57
     engine/src/flutter/shell/platform/android/io/flutter/embedding/android/FlutterActivity.java:728-737](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/shell/platform/android/io/flutter/embedding/android/FlutterActivity.java#L728-L737)).
     "Can the app pop" is a bit the owner publishes like the other
     state the view answers from, and the same contract serves iOS (decision 14) and the Web
     (decision 19). Each API above the floor is a gate under ADR-0244 decision 4.
   - **Insets.** `setOnApplyWindowInsetsListener` and a `WindowInsetsAnimation.Callback` (API 30) on
     the view, as Slint wires them from its injected view
     ([slint-ui/slint@277e7676ce
     internal/backends/android-activity/java/SlintAndroidJavaHelper.java:465-482](https://github.com/slint-ui/slint/blob/277e7676ce/internal/backends/android-activity/java/SlintAndroidJavaHelper.java#L465-L482)), replace the
     polled `safe_area::sample` (one `getRootWindowInsets` read after a resize,
     system bars and cutout only: `safe_area` in `crates/flui-native/src/window/android.rs`)
     and add the IME inset, which exists nowhere in the pipeline today.
     Each change is posted to the owner.

4. **Delivery: the dex is in the APK and the Activity's class loader loads it.** `install_view` loses
   `InMemoryDexClassLoader`, the `DexClassLoader` over a read-only code-cache copy and the copy
   itself, so the API 23 to 25 branch and the question of how Play or Play Protect treats an
   in-memory dex disappear. The library no longer embeds the dex. The view is still found with
   `loadClass` through that loader, never `FindClass`, which searches the system loader on a native
   thread, and its natives are still bound with `RegisterNatives`; the installation is still posted
   to the Java main thread. The sources, the checked-in `classes.dex`, `classes.dex.toml` and the
   `cargo xtask android-dex` digest gate are as ADR-0247 item 1 states them, so building FLUI for
   Android needs no JDK. Delivery is never mixed (ADR-0247 item 9): the in-memory path is gone, not
   kept as a fallback.

5. **A protocol constant is checked before binding.** The Java side declares `FluiHost.PROTOCOL`; Rust
   compares it with its own constant before it binds any native. A dex and a library from different
   versions would otherwise fail as `UnsatisfiedLinkError` or `NoSuchMethodError` far from the cause.
   The CLI takes the dex from the dependency version the app's lockfile resolves (decision 21), and
   the constant catches a packager that bypasses it. A mismatch is an error naming both values,
   raised before any native is bound.

6. **The manifest is generated.** The CLI writes it whole, with a generated-file header, only when it
   changed, and never expects it to be edited by hand: a scaffolded copy cannot receive `hasCode`,
   the Activity name or `configChanges`. It declares at least:
   - `android:name="dev.flui.platform.FluiActivity"` and `android:hasCode="true"`;
   - `android:enableOnBackInvokedCallback="true"`, without which registration below targetSdk 36 is
     refused (decision 3; [aosp-mirror/platform_frameworks_base@1cdfff555f
     core/java/android/window/WindowOnBackInvokedDispatcher.java:696-705](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f/core/java/android/window/WindowOnBackInvokedDispatcher.java#L696-L705));
   - `configChanges` of at least Flutter's list, `orientation|keyboardHidden|keyboard|screenSize|
     smallestScreenSize|locale|layoutDirection|fontScale|screenLayout|density|uiMode`, so a change of
     night mode, font scale, locale or window size reaches `ConfigChanged` instead of recreating the
     Activity and replacing the runtime;
   - `android:launchMode="singleTop"`, so a new intent reaches the running instance through
     `onNewIntent` instead of starting a second Activity and a second Rust runtime;
   - `<queries>` and intent filters, contributed by capability crates through their
     `[package.metadata.flui.capability]` declarations (ADR-0154), and a launch-screen theme.

   The CLI owns a FLUI base plus marker-delimited blocks that capability crates contribute; the app
   owns its label, icon and additional filters. `aapt2 link`, the SDK-tools packager's linker, does
   not merge manifests, which is why the blocks are the CLI's job there; Gradle's merger handles
   library manifests on its path. `windowSoftInputMode` is not decided here (Confirmation).

7. **`targetSdk` is pinned, next to the floor.** `platform-support.toml` gains an Android `targetSdk`
   beside `api`, reaching the packagers the way `minSdk` does under ADR-0244 decisions 2 and 10, and
   `android_package.rs` uses it instead of the newest installed platform. It starts at 35, the
   value the Gradle template already declares
   (`crates/flui-cli/templates/platforms/android/build.gradle.kts:13`), so the SDK-tools path
   and the Gradle path agree from the first day. Raising it is a reviewed change that carries the
   targetSdk 36 behaviours of decisions 3 and 6. ADR-0244's floors and gates are unchanged;
   `targetSdk` is a packaging value, not a floor.

8. **The host bridge.** A capability crate reaches the host through the provider context
   (ADR-0152), which gives it `owner()` and the window first. On Android the bridge is
   `FluiActivity` and its queue, and it offers:
   - `launch_for_result`, which starts an intent and returns a `Completion<_>` completed by the
     result entry the owner drains;
   - `request_permissions`, which returns a `Completion<_>` completed by the permission-result
     entry; it is added with the first capability that needs a permission (ADR-0152), while
     `launch_for_result` arrives with `flui-system`'s file picker (the Storage Access Framework
     answers through an activity result).

   The raw `JavaVM` and Activity pointers are held in the provider context, never in a `static`
   (ADR-0097). On Apple the bridge is events in FLUI's own types (URL, reopen, scene) delivered from
   the scene and application delegates; on the Web it is the page's lifecycle. The bridge does not ask
   a capability crate to register objc2 protocol objects or Java classes of its own. Win32 and Linux
   need nothing from this ADR.

9. **Triggers to leave `NativeActivity`.** The taken surface means Views never paint in the window
   (`ViewRootImpl` returns before drawing when the app owns the surface:
   [aosp-mirror/platform_frameworks_base@1cdfff555f core/java/android/view/ViewRootImpl.java:5652-5659](https://github.com/aosp-mirror/platform_frameworks_base/blob/1cdfff555f/core/java/android/view/ViewRootImpl.java#L5652-L5659)), and `NativeActivity` takes
   touch from the native queue. Two requirements end that:
   - native views (a web view, a map, video) inside the UI, which cannot be inline children;
   - `InputLatency::Low` on Android, which needs a Java touch path.

   If either arrives, the next step is a decision of its own: `GameActivity` when `flui build`'s
   Gradle-free path is no longer a hard requirement, and an own Activity with own glue when it is.
   `FluiView`, `FluiInputConnection` and the accessibility, back and inset code are View-level and
   move unchanged; only the Activity class and the native glue change.

### iOS

10. **Pure Rust on objc2; no Swift or Objective-C in the framework.** `UIApplicationMain`,
    `FluiAppDelegate`, `FluiSceneDelegate` and the plist scene manifest stay. They pass both of
    TN3187's adoption tests (a scene manifest, and
    `application:configurationForConnectingSceneSession:options:`), and a delegate installed at
    runtime would be too late for UIKit's decision.

11. **Text input is a separate invisible responder view.** A Rust-defined view conforming to
    `UITextInput`, with `UITextInteraction` for the system selection UI, backed by the shared
    `TextStore` host (ADR-0090, ADR-0135). It is separate from `FluiView`, which does not become the
    first responder; Flutter and Compose make the same split ([flutter/flutter@6597889f57
    engine/src/flutter/shell/platform/darwin/ios/framework/Source/FlutterTextInputPlugin.mm:140-141](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/shell/platform/darwin/ios/framework/Source/FlutterTextInputPlugin.mm#L140-L141);
    [JetBrains/compose-multiplatform-core@13b7599795
    compose/ui/ui-uikit/src/iosMain/objc/CMPUIKitUtils/CMPUIKitObjcUtils/CMPTextInputView.h:20](https://github.com/JetBrains/compose-multiplatform-core/blob/13b7599795/compose/ui/ui-uikit/src/iosMain/objc/CMPUIKitUtils/CMPUIKitObjcUtils/CMPTextInputView.h#L20)).
    `UITextInput` carries marked text,
    which CJK composition requires; a `UIKeyInput` implementation cannot, which is all winit
    implements ([rust-windowing/winit@1893565716 winit-uikit/src/view.rs:333-353](https://github.com/rust-windowing/winit/blob/1893565716/winit-uikit/src/view.rs#L333-L353)). Enabling
    `objc2-ui-kit`'s `UITextInput`, `UITextInputTraits` and `UITextInteraction` features is part of
    the change. Whether a full `UITextInput` conformance (about thirty methods, a tokenizer, an input
    delegate) composes through `define_class!` was unproven when this was decided; it composed
    Japanese and Chinese on the iOS Simulator (Confirmation).

12. **Accessibility is `accesskit_ios::Adapter`**, the low-level one, forwarded from `FluiView`'s
    `isAccessibilityElement`, `accessibilityElements` and `accessibilityHitTest:`. All its methods run
    on the main thread, which is the owner thread on iOS, so the Android cross-thread problem does not
    arise ([AccessKit/accesskit@a39329d28 adapters/ios/src/adapter.rs:318](https://github.com/AccessKit/accesskit/blob/a39329d28/adapters/ios/src/adapter.rs#L318), "all adapter methods must
    be called on the main thread"). The adapter is on objc2 0.5 beside FLUI's 0.6
    ([adapters/ios/Cargo.toml:21-31](https://github.com/AccessKit/accesskit/blob/a39329d28/adapters/ios/Cargo.toml#L21-L31)); its entry points take and return raw
    pointers, which makes the skew a build-size cost, not a blocker (Confirmation).

13. **Scene callbacks carry links and activities.** `FluiSceneDelegate` gains
    `scene:willConnectToSession:options:` (cold-start URLs and user activities),
    `scene:openURLContexts:`, `scene:continueUserActivity:` and
    `windowScene:performActionForShortcutItem:`. They reach the app as events in FLUI's types
    (decision 8). Flutter's scene delegate forwards the same four
    ([flutter/flutter@6597889f57
    engine/src/flutter/shell/platform/darwin/ios/framework/Source/FlutterSceneDelegate.mm:32-101](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/shell/platform/darwin/ios/framework/Source/FlutterSceneDelegate.mm#L32-L101));
    FLUI's has none of them (`crates/flui-native/src/owner/apple/ios.rs`).

14. **Keyboard, back and the plist.**
    - The keyboard's frame arrives through the keyboard notifications, which work at the iOS 13
      floor. `UIKeyboardLayoutGuide` (iOS 15) is a gated refinement under ADR-0244 decision 4.
    - Back is app-owned on iOS: a `UIScreenEdgePanGestureRecognizer` feeding the same "can the app
      pop" contract as Android (decision 3).
    - The generated `Info.plist` gains `UIApplicationSupportsIndirectInputEvents`, iPad orientations,
      URL types and usage-description strings. The last two come from capability metadata, and the
      app can override a usage string.

15. **Swift appears only inside capability crates.** A capability that needs a Swift-only framework
    (App Intents, ActivityKit, SwiftUI-only API) ships a Swift package under the crate's `ios/`
    directory, and `flui-cli` builds it only for an app that depends on that crate. The framework's
    own iOS host stays Swift-free.

### Web

16. **Rust owns the DOM; the page is a mount point and a loader.** The host creates one root
    element and puts into it, in this order, the canvas, a text-input host and a semantics host. The
    semantics host is last, so the root's hit-test order leaves any platform view reachable.
    `flui-cli` generates the page from a template: `<base href>` for hosting under a sub-path,
    `viewport-fit=cover`, the manifest and the module import, and the CLI regenerates it instead of
    leaving a scaffold for the user to edit. There are no separate JavaScript files; `web-sys` and
    the existing `inline_js` snippets suffice. A page that supplies its own canvas keeps working as
    it does today, and owns its layout.

17. **Text input: the `<textarea>` is the contract; `EditContext` is a Chrome refinement.**
    `EditContext` ships in Chrome 121 and its derivatives only ([mdn/browser-compat-data@5da921db9
    api/EditContext.json](https://github.com/mdn/browser-compat-data/blob/5da921db9/api/EditContext.json)), so Firefox and every iOS browser need
    the textarea, and it is the path that must be correct everywhere; neither Flutter's web engine
    nor Compose Multiplatform's web target relies on `EditContext`. It gains: a font-size floor of
    16 px on iOS Safari; `enterkeyhint` from the field's purpose; and an autofill policy in place of
    the hard-coded `autocomplete="off"`. When accessibility is on, the semantic text field's own
    element doubles as the editing element, so a screen reader and the IME address one element. The
    text of a canvas field is reachable to assistive technology whichever path edits it.

18. **Accessibility is a parallel ARIA DOM written in Rust.** AccessKit lists its web adapter as
    planned ([AccessKit/accesskit@a39329d28 README.md:45-48](https://github.com/AccessKit/accesskit/blob/a39329d28/README.md#L45-L48)); Flutter builds a parallel DOM with a
    lazy placeholder and its semantics host last ([flutter/flutter@6597889f57
    engine/src/flutter/lib/web_ui/lib/src/engine/view_embedder/dom_manager.dart:16-46](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/lib/web_ui/lib/src/engine/view_embedder/dom_manager.dart#L16-L46),
    [.../semantics/semantics_helper.dart:238](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/lib/web_ui/lib/src/engine/semantics/semantics_helper.dart#L238)). A `PlatformAccessibility` implementation for the Web
    consumes `TreeUpdate` through
    `accesskit_consumer` and maintains one element per node with its role and ARIA attributes, in the
    semantics host. It is enabled lazily, from a hidden focusable placeholder control that
    assistive technology activates, because a browser exposes no signal that it is present.

19. **Routing is the History API, with a single entry.** `pushState`, `replaceState` and `popstate` are
    the baseline. A single-entry strategy keeps a fake entry so that Back always produces a `popstate`
    and never leaves the app unasked (Flutter's `SingleEntryBrowserHistory`: [flutter/flutter@6597889f57
    engine/src/flutter/lib/web_ui/lib/src/engine/navigation/history.dart:17-25,124](https://github.com/flutter/flutter/blob/6597889f57/engine/src/flutter/lib/web_ui/lib/src/engine/navigation/history.dart#L17-L25)); this feeds the
    same "can the app pop" contract (decision 3).
    The Navigation API (Chrome 102, Firefox 147, Safari 26.2; [mdn/browser-compat-data@5da921db9
    api/Navigation.json](https://github.com/mdn/browser-compat-data/blob/5da921db9/api/Navigation.json)) is a refinement. `flui run`'s server
    answers any path that is not a file with the page, before path routes are enabled.

20. **Safe area and the keyboard.** `env(safe-area-inset-*)` gives the safe area, which needs
    `viewport-fit=cover` from the generated page, and `visualViewport` gives the keyboard's size.
    `interactive-widget` is not used: Safari does not offer it.

### Where non-Rust code lives and how the CLI builds it

21. **Native source stays in the crate whose Rust it must match, and travels with it.** The CLI finds
    it from the dependency version in the app's lockfile with `cargo_metadata` (as `web.rs` already
    does for `wasm-bindgen`), not from a copy in the app.

    | Concern | Rule |
    |---|---|
    | Android host sources | `crates/flui-native/android/` (the `android` directory of the core host): `java/`, `classes.dex`, `classes.dex.toml`, the `cargo xtask android-dex` gate, `FluiHost.PROTOCOL` |
    | Why a checked-in dex, not `build.rs` | As in ADR-0247: no JDK or build-tools version in a user's build. The cost, one binary under review, is already accepted |
    | Android build, SDK-tools path | `aapt2 link` the generated manifest, add the native libraries and `classes.dex` from the resolved crate directory, `zipalign`, `apksigner`. No `javac` or `d8` |
    | Android build, Gradle path | The CLI writes the host sources into a generated, git-ignored source directory with a generated-file header on every build, or points a `sourceSets` entry at the crate; R8 keep rules for `dev.flui.**`, whose classes only native code names |
    | Play distribution | Requires an AAB, which only the Gradle path produces. Bundletool route not examined |
    | Manifest | Generated from a FLUI-owned base plus capability blocks between markers the CLI owns (decision 6) |
    | iOS | Nothing in the framework. A capability crate may ship a Swift package under `ios/`, built only for an app that depends on it (decision 15) |
    | Web | Nothing beyond Rust; the generated page is the only non-Rust file |
    | Version coupling | The CLI takes dex, sources and Swift packages from the lockfile's dependency version; the protocol constant (decision 5) catches what bypasses the CLI |

    Generated project files are written whole, with a header, only when changed, and are never
    hand-edited. No build script runs `javac`, `d8` or `swiftc`.

### Consequences

- Neutral, because the tree changes as the table below lists.

  | Piece | Change |
  |---|---|
  | `crates/flui-cli/templates/platforms/android/AndroidManifest.xml` | Becomes the generated manifest of decision 6 |
  | `crates/flui-cli/src/build/android_package.rs` | Adds `classes.dex` as a zip entry; `targetSdk` from `platform-support.toml`; stops describing the path as `hasCode="false"` |
  | `crates/flui-cli/templates/platforms/android/build.gradle.kts` | Host sources through a generated directory or `sourceSets`; keep rules; the `proguard-rules.pro` it names but the template set lacks |
  | `crates/flui-native/src/owner/android/java_view.rs` | Class loading through the Activity's loader; the in-memory and file loaders deleted; the protocol check |
  | `crates/flui-native/android/java/` | `FluiActivity`, the host queue, the accessibility delegate, back registration, the inset and IME-animation listeners |
  | `crates/flui-native/src/window/android.rs` (`safe_area`) | Event-driven, and includes the IME inset |
  | `crates/flui-native/src/accessibility/` | An Android `PlatformAccessibility` module: the low-level `Adapter`, hover from the native queue, a wake of the owner on activation |
  | `platform-support.toml` and the generated floors | `targetSdk` |
  | `crates/flui-native/src/owner/apple/ios.rs` and the `ios` files of `text/`, `accessibility/`, `window/` and `input/` | The `UITextInput` view, the accessibility adapter, the scene callbacks, keyboard observation, the edge pan |
  | `crates/flui-cli/src/build/ios_package.rs` | The plist keys of decision 14 |
  | `crates/flui-native/src/text/web.rs` | Font-size floor, `enterkeyhint`, the autofill policy, reuse of the semantic field |
  | `crates/flui-native/src/accessibility/`, `crates/flui-native/src/window/web.rs` | The semantics host and ARIA DOM; the history strategy, ownership of the root element |
  | `crates/flui-cli/templates/platforms/web/index.html`, `crates/flui-cli/src/serve.rs` | A generated page with `<base>` and `viewport-fit`; a history fallback in the dev server |

- Bad, because a packager other than `flui-cli` (cargo-apk, a hand-written Gradle project) must
  add the dex and the manifest itself; ADR-0247 item 1's "every packager ships the dex" no longer
  holds.
- Good, because the protocol constant turns a mismatch between the dex and the library into a
  named error.
- Bad, because apps scaffolded earlier hold a manifest the CLI no longer writes from a template.
- Good, because the SDK-tools path gains one zip entry and still needs no JDK and no Gradle.
- Neutral, because ADR-0249 gets its new producers: the queue's entries, and the accessibility
  request from the Java main thread.
- Good, because a paused app still costs no CPU between real events.
- Good, because the declared `configChanges` of decision 6 end the Activity recreation, and the
  replaced runtime, that a system dark-mode, font-scale or locale change likely causes under the
  current template, for the changes it lists; not run (see Confirmation).
- Neutral, because ADR-0247 stands for items 1 (sources, checked-in dex, digest gate), 2 (first
  sentence), 3 to 8 and 10; its "Alternatives rejected" entry on a dex in the APK, and the
  consequence "no packaging path changes", are reversed by decisions 4 and 21.
- Neutral, because `docs/gaps.md` and `docs/platform-parity.md` keep the text input,
  accessibility, system back, deep link and mobile system UI rows; a row moves only after a run on
  that platform, and each consumer-visible change adds a `changelog.d` entry.
- Neutral, because capability crates that need a result from the OS (the first is the location
  pilot, which needs Activity results and permissions on Android) are written against the bridge
  of decision 8, and that pilot is the first consumer that runs it.

### Confirmation

Little of this ADR has run: the `UITextInput` claim below ran on the iOS Simulator. The repository's other evidence for it is the Android device run of
ADR-0247 (a Galaxy S21, Android 15, a manual run), which covered the
in-memory loader this ADR retires, so the same run repeats with the dex in the APK. The claims below
are open, with the run that settles each:

| Claim | Run |
|---|---|
| The view installs and the S21 input run passes with the dex delivered by the APK | a manual input run on the S21, and on an API 23 to 25 image, which ADR-0247 never ran |
| A system dark-mode, font-scale or locale change reaches `ConfigChanged` and no longer replaces the runtime | A device: `adb shell cmd uimode night yes`, `settings put system font_scale 1.3`; watch for a second `android_main` |
| A warm deep link, an activity result and a permission result reach the owner, including one that arrives before `android_main` | A device, with the location pilot as the first consumer |
| TalkBack's hover events reach the native queue and are consumed through `Adapter::on_hover_event`; `accesskit_android`'s minimum API; the `viewIdResourceName` mapping for ADR-0240 identifiers | A device with TalkBack, on Android 6 and 15 |
| `windowSoftInputMode`: `adjustResize` against `adjustNothing` with a self-rendered surface and the IME animation | A device: count `WindowResized` events during a keyboard animation |
| `UITextInput` through `define_class!` composes CJK text | Ran 2026-10-28 on the iOS 27.1 Simulator (an M1 host, the on-screen keyboard tapped by XCUITest and by hand): the Japanese Romaji keyboard composed `にほんご` and committed `日本語`, and the nine-key Pinyin keyboard composed `ni hao` and committed `你好`, each into an `EditableText`. Open: a device |
| `accesskit_ios::Adapter` builds and runs beside objc2 0.6 | An iOS Simulator run, and a VoiceOver pass |
| iOS Safari zooms on the textarea today; the 16 px floor stops it | iPhone Safari |
| The ARIA DOM, the lazy placeholder, the single-entry history and the textarea's `enterkeyhint` and autofill behaviour | Chrome, Firefox and Safari, desktop and mobile, with a screen reader |

Not examined: signing, entitlements (Associated Domains for universal links), asset catalogs and
IPA export for an iOS app staged without Xcode; an AAB without Gradle (bundletool); platform views
under a View-hosted surface, which waits for the requirement of decision 9.

Linux CI runs the parts that need no device: the generated manifest carries the declared
attributes, the packager adds the dex and uses the pinned `targetSdk`, `xtask platform-floor`
agrees with `platform-support.toml` for the new key, and the protocol check rejects a mismatch
before any native is bound. The support claim for a platform is the run.

- `cargo xtask android-dex`, extended to every `build.rs` (ADR-0154), fails a build script that
  names `javac`, `d8` or `swiftc` (decision 21); a generated project file that differs from the
  output of its generator fails the golden check of that generator (decisions 6 and 21).

## Pros and cons of the options

### 1. The hosts decided above

- Good, because `FluiActivity` forwards what `NativeActivity` drops through a buffering queue, so
  warm deep links, activity results, permissions and manifest-declared components become possible
  (decisions 1, 2, 6 and 8).
- Good, because `FluiView` stays the one host view and gains accessibility, back and live insets
  without needing the Activity subclass (decision 3).
- Good, because the low-level `accesskit_android::Adapter` plus about 70 lines of FLUI's own Java
  gives the accessibility behaviour under FLUI's threading and panic rules (decision 3).
- Good, because the dex is in the APK and the Activity's class loader loads it: the API 23 to 25
  branch and the question of how Play or Play Protect treats an in-memory dex disappear, and
  building FLUI for Android still needs no JDK (decision 4).
- Good, because a protocol constant checked before any native is bound turns a dex and a library
  from different versions into a named error (decision 5).
- Good, because the generated manifest declares `configChanges`, `launchMode` and the back
  callback, and `targetSdk` is pinned, so the build no longer depends on the newest installed
  platform (decisions 6 and 7).
- Good, because iOS stays pure Rust on objc2 with the scene manifest TN3187 will require, a
  separate invisible `UITextInput` responder view, `accesskit_ios::Adapter` and scene callbacks
  (decisions 10 to 14).
- Good, because the Web host has Rust owning the DOM, a `<textarea>` that is correct everywhere
  with `EditContext` as a refinement, a parallel ARIA DOM that follows Flutter's more complete
  design (a lazy placeholder, the semantics host last, the text field doubling as the editor) and
  not Compose's always-on tree, History routing, and safe-area and keyboard handling (decisions 16
  to 20).
- Good, because native source travels with the crate whose Rust it must match, and the CLI finds
  it from the lockfile (decision 21).
- Bad, because `NativeActivity` keeps its limits: Views never paint in the window under the taken
  surface, and native views or `InputLatency::Low` on Android would need a different Activity
  (decision 9).
- Bad, because a packager other than `flui-cli` must add the dex and the manifest itself, and apps
  scaffolded earlier hold a manifest the CLI no longer writes from a template.
- Bad, because little of it has run: a full `UITextInput` conformance through `define_class!` has
  composed CJK text on the Simulator only, and `accesskit_ios` is on objc2 0.5 beside FLUI's 0.6, a build-size cost (see
  Confirmation).

### 2. Android: stay on `hasCode="false"` and use the deprecated framework `Fragment` for results

- Bad, because three things cannot be delivered: warm deep links (nothing forwards `onNewIntent`;
  `getIntent()` keeps returning the first intent), results and permissions (only through a
  `Fragment` that is deprecated since API 28, its result methods since 29, and whose delivery from
  an in-memory dex nobody has run), and any manifest-declared component. ADR-0247 item 9 already
  named this switch.
- Bad, because the accessibility, back and inset code would be written against a delivery path
  that then retires; the `Fragment` route would be a stopgap for results and permissions only,
  never a design.

### 3. Android: `GameActivity`

- Bad, because it costs Gradle, AppCompat and a games-activity AAR whose version the crate must
  pin (android-activity's README pins 4.4.0 and warns that the next 4.x may not be compatible:
  [rust-mobile/android-activity@b4ddf059b README.md:228-241](https://github.com/rust-mobile/android-activity/blob/b4ddf059b/README.md#L228-L241)), and removes `flui build`'s
  Gradle-free path.
- Bad, because its IME is weaker than `FluiView`'s: one `Editable`, `requestCursorUpdates` that
  only calls `super`, no `commitContent` or handwriting override, and no room for the generation
  tags the edit mirror needs.
- Bad, because FLUI would still write the Java for accessibility, back, drag and drop and
  handwriting.
- Bad, because whether `android-activity`'s `InputEvent` abstraction hides its different input
  path from FLUI's translation code is unverified.

### 4. Android: an own Activity with own native glue (Flutter's model)

- Good, because it is the only route that removes every limit of `NativeActivity`.
- Bad, because it replaces `android-activity`'s looper, thread, lifecycle, saved state and input
  handling, and nothing on the owner's list needs it; decision 9 names when it would.

### 5. Android: `accesskit_android::InjectingAdapter`

- Bad, because it loads a second dex through its own `InMemoryDexClassLoader`
  ([AccessKit/accesskit@a39329d28 adapters/android/src/inject.rs:225-241](https://github.com/AccessKit/accesskit/blob/a39329d28/adapters/android/src/inject.rs#L225-L241)), keeps process-global
  handle maps and `unwrap`s on JNI paths (`:88-105`, `:118-134`), and panics if the host view
  already has an accessibility delegate (`:359-363`); its reasons are also in decision 3.

### 6. Android: the dex built by a `build.rs`

- Bad, because every Android build would depend on a JDK and a build-tools version, as ADR-0247
  already found (Slint's build script warns of a JDK 21 failure of exactly this:
  [slint-ui/slint@277e7676ce internal/backends/android-activity/build.rs:67-79](https://github.com/slint-ui/slint/blob/277e7676ce/internal/backends/android-activity/build.rs#L67-L79)).

### 7. iOS: a Swift or Objective-C shim (Flutter's, Compose's and Tauri's shape)

- Bad, because it adds a second toolchain to a build that is Xcode-free, for classes Rust can
  already define.
- Bad, because the reason Flutter and Compose write theirs in Objective-C, that Dart and
  Kotlin/Native cannot subclass `UIView`, does not apply to Rust.

### 8. iOS: winit's UIKit implementation

- Bad, because it implements `UIKeyInput` only, so it cannot compose marked text.
- Bad, because it has no UIScene support, which TN3187 will require.

### 9. iOS: making `FluiView` the first responder for text

- Bad, because it puts the text-input protocol surface on the content view; the separate invisible
  responder view keeps it off. This is a judgement, not a measured result.

### 10. iOS: registering objc2 protocol objects from capability crates

- Bad, because it leaks objc2 types and versions into every capability crate; the bridge delivers
  events in FLUI's own types instead.

### 11. Web: `EditContext` alone

- Bad, because it excludes Firefox and every iOS browser.

### 12. Web: waiting for an AccessKit web adapter

- Bad, because it is planned, with no date, while Flutter and Compose both ship a parallel DOM
  today.

### 13. Web: the Navigation API alone

- Bad, because it needs Safari 26.2 and gains little over History for FLUI's needs.

### 14. Web: a static scaffold the user edits

- Bad, because it cannot receive framework changes.

## More information

- **Not implemented.** Nothing in this ADR has run; the code changes land as separate changes
  after this ADR, and a platform's row in `docs/platform-parity.md` moves only after a run on that
  platform (see Confirmation).
- **Revisit when.** Native views (a web view, a map, video) inside the UI, or `InputLatency::Low`
  on Android, arrive (decision 9): the next step is then a decision of its own, `GameActivity`
  when `flui build`'s Gradle-free path is no longer a hard requirement and an own Activity with
  own glue when it is.
- **Open questions with their runs.** `windowSoftInputMode`, a full `UITextInput` conformance
  through `define_class!`, the `accesskit_ios` objc2 skew and the Web behaviours are listed with
  the run that settles each under Confirmation; signing, entitlements, asset catalogs, IPA export
  and an AAB without Gradle are not examined.
