# Android Java side: how FLUI gets the Java code that registers IME, drag-and-drop and handwriting callbacks

- **Date:** 2026-10-17. FLUI base is the `interaction-integration` worktree at `30bfcee6a`.
- **Question (owner):** FLUI's Android apps are `NativeActivity` with `android:hasCode="false"` and ship
  no Java. Drag and drop (`View.OnDragListener` / `onDragEvent`, `OnReceiveContentListener`,
  `startDragAndDrop`), IME (`View.onCreateInputConnection` → `InputConnection`,
  `InputMethodManager`, `EditorInfo`) and, later, stylus handwriting need Java code. How should
  FLUI ship it? (A) a d8-compiled shim embedded in the native library and loaded with
  `InMemoryDexClassLoader`; (B) moving to `GameActivity`; (C) a real `classes.dex` in the APK; or
  something better.
- **Inputs:** [ADR-0090](../adr/ADR-0090-ime-pull-text-store-contract.md) (pull text store; §"Live
  evidence" leaves "Android IME may need GameActivity" as an open hypothesis),
  [ADR-0211](../adr/ADR-0211-unified-drag-and-drop.md) (DnD sessions, escalation, cached feedback),
  [threading model](2026-10-13-interaction-threading-model.md) (Android already splits the Java main
  thread from `android_main`), `crates/flui-platform/src/platforms/android/`,
  `crates/flui-app/src/app/runner/android.rs`, `crates/flui-cli/src/build/android_package.rs`,
  `crates/flui-cli/templates/platforms/android/`, `tools/xtask/src/device/android_input.rs`,
  `docs/platform-parity.md`, `docs/gaps.md`, and spec task T42b/T42d
  (`docs/plans/specs/flui-interaction/completeness/tasks.md:124`).
- External code is pinned to the commit read: `android-activity` v0.6.1 (`b4ddf059`, the version
  in FLUI's `Cargo.lock`); Slint `86b42115`; winit `v0.30.12` and `master@758a50e3`; Bevy
  `85503941`; egui `86099aca`; androidx `9a589fa6`; Flutter `dd45f0c2`; wry `66420742`; Dioxus
  `7a817390`; Tauri `136a28ea`; xbuild `e67b501c`; cargo-apk `887423a9`; jni-rs `v0.22.4` (FLUI's
  locked version); AOSP `platform/frameworks/base` `main@1cdfff55`, `platform/frameworks/opt/gamesdk`
  `main@044fd03c`, `platform/libcore` `main@de876a01`. API levels come from the SDK's
  `platforms/android-35/data/api-versions.xml`, which is the database behind the "Added in API
  level" lines of the reference pages linked below.

## Recommendation

**Option (A): one Java source tree owned by `flui-platform`, compiled to a single `classes.dex` at
build time, embedded in the `.so`, and loaded at startup with `InMemoryDexClassLoader`.** Keep
`android.app.NativeActivity` and `android:hasCode="false"`. The shim adds one `FluiView extends
View` over the native content (`Activity.addContentView`). That view is the IME target, the drop
target, the drag source and the handwriting editor. Do not move to `GameActivity`. Keep (C), the
same Java sources shipped as an APK `classes.dex`, as the planned escalation for callbacks that
only a manifest-declared `Activity` subclass can receive (permission and activity results). Record
it now so the shim is written to work under both deliveries.

Reasons:

1. **No route without Java exists.** Every callback needed is either a `View` method override
   (`onCreateInputConnection`, `onCheckIsTextEditor`, `onDragEvent`) or a Java interface
   (`OnDragListener`, `OnReceiveContentListener`, `OnBackInvokedCallback`). JNI cannot define a
   class on Android: "`DefineClass` is not implemented" ([JNI tips][jni]). `java.lang.reflect.Proxy`
   covers only interfaces, and its `InvocationHandler` is itself an interface that a dex class must
   implement. So the real choice is how dex bytes reach the process, not whether they do.
2. **The `.so` and the Java it calls ship as one unit.** The native side registers native methods
   (`RegisterNatives`) and calls Java methods by signature. With (A) the two cannot be skewed:
   the bytes are `include_bytes!` in the same library. With (C), every packaging path must carry a
   matching dex: `flui build`'s SDK-tools path, the Gradle template, the `xtask device` probe, and a
   user's own Gradle project. R8 must also keep the JNI-only classes.
3. **It keeps FLUI's whole packaging story intact.** `flui build` packages with `aapt2`, `zipalign`
   and `apksigner` and no Gradle (`crates/flui-cli/src/build/android_package.rs:60-154`). The probe
   manifest is `hasCode="false"` (`tools/xtask/src/device/android_input.rs:45-60`). Any third-party
   packager also works unchanged: cargo-apk has a `hasCode` switch but no dex step
   (`ndk-build/src/manifest.rs:72-74`). (A) adds a build-time `javac` + `d8` step to
   `flui-platform`'s Android build only. The CLI already requires both a JDK (for `apksigner`)
   and the build-tools that contain `d8.bat`/`d8`.
4. **It is the shape a shipping Rust toolkit already runs.** Slint's Android backend does exactly
   this. Its build.rs compiles `SlintAndroidJavaHelper.java` with `javac` and `d8`. It embeds
   `classes.dex` with `include_bytes!` and loads it with `InMemoryDexClassLoader` on API ≥ 26, or
   with `DexClassLoader` from `codeCacheDir` below that. It adds a `SlintInputView extends View`
   with `onCreateInputConnection` to the `NativeActivity` window
   ([build.rs][slint-build], [javahelper.rs:313-366][slint-loader],
   [SlintAndroidJavaHelper.java:134-260, 412-425][slint-java]).
5. **The cost is small and measured.** Slint's 667-line helper compiles to a 23,524-byte
   `classes.dex` (`d8 --release --min-api 26`, build-tools 35.0.0, on this host). GameActivity
   instead requires AndroidX AppCompat (see (B)).
6. **GameActivity does not solve the problem.** Its Java side is a fixed `InputEnabledSurfaceView`
   whose `InputConnection` mirrors one `Editable` ([GameActivity.java][ga-java],
   [InputConnection.java][gti-ic]). It has no drag and drop, no handwriting and no
   `CursorAnchorInfo` path. Adding any of those means subclassing it in Java, so FLUI would still
   write and ship Java (Bevy's example does, in Kotlin under Gradle: [MainActivity.kt][bevy-main]).
   It also brings Gradle, AppCompat, a `SurfaceView` instead of the window surface, and a
   different input path ("separate from the `InputQueue` that `NativeActivity` uses",
   [GameActivity][agdk-ga]). That path would replace FLUI's tested `NativeActivity` input
   translation and its Galaxy S21 goldens.

What the shim must do differs by feature. The details are in §3–§5:

- **IME:** `FluiView` overrides `onCheckIsTextEditor`/`onCreateInputConnection`. Its
  `InputConnection` answers the IME on the Java main thread from an owner-published snapshot and
  forwards edits to the owner in order. It never blocks on the owner (§4).
- **Drag and drop:** drag events reach a `View` through `ViewRootImpl.handleDragEvent` →
  `dispatchDragEvent`, not through the `InputQueue` that `NativeActivity` takes. A full-window
  `FluiView` therefore receives them (§5). It answers `ACTION_DRAG_LOCATION` from ADR-0211's
  cached feedback and starts OS drags with `startDragAndDrop` (API 24).
- **Handwriting:** the same view sets the API 33+ hooks. Auto handwriting needs the stylus stream
  to reach `ViewPostImeInputStage`, so the native side must report those events unhandled or
  call `startStylusHandwriting` itself (§6).

**minSdk stays 23.** `InMemoryDexClassLoader` is API 26. On API 23–25 the shim takes the
`DexClassLoader` fallback that Slint ships. Android 14's read-only rule for dynamically loaded
files applies to apps targeting API 34 or higher running on Android 14, and the fallback runs
only on devices below API 26, so it never meets that rule. Drag and drop needs API 24
(`startDragAndDrop`); below that FLUI reports `DragError::Unavailable`, as ADR-0211 already
allows.

## 1. What FLUI has today

| Fact | Evidence |
|---|---|
| Activity is the framework's `android.app.NativeActivity`, `hasCode="false"`, `lib_name` metadata | `crates/flui-cli/templates/platforms/android/AndroidManifest.xml:7-18`; probe: `tools/xtask/src/device/android_input.rs:45-60` |
| `android-activity` 0.6.1 with `native-activity`, `jni` 0.22.4 | `Cargo.toml:270-271`, `Cargo.lock` |
| minSdk 23 (from `View.getRootWindowInsets`), targetSdk = newest installed platform | `crates/flui-cli/src/build/android_package.rs:139-152`, `crates/flui-cli/ARCHITECTURE.md:33-37`, Gradle template `minSdk = 23`, `targetSdk = 35` |
| `flui build` packages with SDK build-tools, no Gradle; Gradle only if the project adds `gradlew` | `crates/flui-cli/README.md:213-223`, `android_package.rs:60-131` |
| JNI already used for haptics, preferences, text sizing and gesture exclusion; work that touches views is posted with `AndroidApp::run_on_java_main_thread` | `crates/flui-platform/src/platforms/android/window.rs:152,335`, `haptics.rs:367`, `text_sizing.rs:380`; `preferences.rs:35-61` borrows the Activity global ref from `AndroidApp` |
| Posted Java-main work already carries a completion obligation that survives a failed attach and wakes the owner through `OwnerSignal` | `text_sizing.rs:184-238` (`Receipt`) |
| Motion and key events are reported `Handled` only when FLUI `default_prevented` them | `crates/flui-platform/src/platforms/android/mod.rs:303-331` |
| IME, DnD and handwriting are blocked on this decision | `docs/gaps.md:53`, `docs/platform-parity.md:134`, tasks T42b/T42d |

`android-activity`'s `native-activity` backend has no `InputConnection`. `text_input_state` returns
an empty state, `set_text_input_state` and `set_ime_editor_info` are no-ops, and
`show_soft_input` targets the decor view ([native_activity/mod.rs:365-457][aa-na]). Its README
says the same: "For advanced use cases, it would be possible to provide custom `InputConnection`
support in conjunction with `NativeActivity` but this isn't something that `android-activity`
provides" ([README][aa-readme]). It does give the two hooks the shim needs. One is
`android_on_create`, which runs on the Java main thread inside `Activity.onCreate` before
`android_main`. The other is `run_on_java_main_thread`, an eventfd on the Java main looper
([lib.rs:59-130][aa-lib], [main_callbacks.rs][aa-mc], [CHANGELOG 0.6.1][aa-changelog]).

## 2. Per-option analysis

### (A) Shim dex embedded in the `.so`, loaded with `InMemoryDexClassLoader`

- **Mechanism.** At startup, on the Java main thread, the native side does the following. It takes
  the Activity's class loader (`getClassLoader()`). It wraps the embedded bytes in a direct
  `ByteBuffer` and constructs `new InMemoryDexClassLoader(buffer, activityLoader)`. It then
  `loadClass`es `dev.flui.FluiView` through that loader. `FindClass` would not work here: on a
  natively attached thread it searches the "system" class loader ([JNI tips][jni], "Why didn't
  `FindClass` find my class?"). The native side calls `RegisterNatives` on the loaded class and
  constructs the view. The class doc says the loader "can be used to execute code that has not
  been written to the local file system" ([InMemoryDexClassLoader.java][imdcl-src]). It was added
  in API 26 ([reference][imdcl]). Slint's loader is a complete worked example, including the
  API 23–25 `DexClassLoader` path ([javahelper.rs:313-366][slint-loader]).
- **Lifecycle and threading.**
  - Load the dex and attach the view in `android_on_create` or with `run_on_java_main_thread`.
    The first is better: the view exists before the first frame. Each `NativeActivity`
    instance gets its own view. On Activity recreation `android-activity` runs a new
    `android_main` with a new `AndroidApp` (`crates/flui-app/src/app/runner/android.rs:405-418`).
  - Java→native callbacks arrive on the Java main thread, which is already attached. They must
    only enqueue `Send` data and wake the owner through the existing `OwnerSignal`. Slint does
    the same with `invoke_from_event_loop` ([javahelper.rs:630-660][slint-update]).
  - Owner→Java calls stay fire-and-forget posts. The owner never waits on the Java main thread.
    This is the deadlock-freedom invariant §4 depends on.
  - The owner holds the `FluiView` as a `jni::refs::Global`. Dropping a `Global` on an unattached
    thread attaches temporarily, and the jni docs warn about the cost: "Drop On an Attached Thread
    If Possible" ([global.rs:77-90][jni-global]). The view holds a native handle (a `jlong` to an
    `Arc`ed bridge). Teardown is ordered: the owner posts a detach that clears the handle on the
    Java main thread. Callbacks that arrive after `MainEvent::Destroy` then see a detached bridge,
    never a freed one.
  - Panics in a native method must be contained before they reach the JNI boundary, using the
    crate's `panic_boundary`.
- **minSdk.** 26 for the in-memory path. On 23–25 use the `DexClassLoader` fallback, which writes
  the bytes to `codeCacheDir` and loads them from there. The Android 14 rule ("all
  dynamically-loaded files must be marked as read-only", for apps targeting API 34+
  [behavior changes 14][a14-dcl]) cannot apply on those devices. The fallback should still write
  the file read-only and verify a digest, so it stays correct if the gate is ever lowered.
- **Play policy.** The Device and Network Abuse policy forbids downloading "executable code (such
  as dex, JAR, .so files) from a source other than Google Play" ([policy][play-dna]). Embedded
  bytes are part of the signed APK and are not downloaded, so the rule does not apply. Android's
  guidance still says to "avoid dynamically loading code whenever possible"
  ([behavior changes 14][a14-dcl]). Whether Play's or Play Protect's static review flags an
  `InMemoryDexClassLoader` of embedded bytes is **unverified**: no primary source states it either
  way. Slint ships this path publicly. The risk and its fallback are in §7.
- **APK size.** Tens of KB of dex inside the `.so`; measured above at 23.5 KB for Slint's helper.
  No AndroidX.
- **Build tooling.** build.rs on `target_os = "android"` runs `javac --release 8 -cp android.jar`
  and `d8 --release --min-api 26`. Slint uses the `android-build` crate for this
  ([build.rs][slint-build]). The alternative is a dex checked in and regenerated by an `xtask`
  command, with a gate that rebuilds it and compares bytes. That keeps `cargo build` free of a JDK
  dependency at the cost of a generated binary in the tree; choose this when writing the ADR.
  Either way it works with `flui build`, cargo-apk, xbuild and Gradle alike, because none of them
  need to know about it.
- **What it cannot do.** A class from an in-memory loader cannot be named in the manifest. It can
  never be the `Activity`, and it cannot receive callbacks only an Activity subclass gets:
  `onRequestPermissionsResult`, `onActivityResult`, `onNewIntent`. `NativeActivity` forwards none
  of these (its Java surface is [NativeActivity.java][na-java]). This is the trigger for (C).

### (B) `GameActivity` (androidx.games `games-activity` + `game-text-input`)

- **What it is.** "A direct descendant of `NativeActivity`" and "a derived class of
  `AppCompatActivity`". It renders into a `SurfaceView`, has its own input buffer instead of
  `InputQueue`, and does text input through GameTextInput ([GameActivity][agdk-ga]). Its Java
  `InputConnection extends BaseInputConnection` keeps an `Editable` copy and pushes the whole
  state to native on each change ([InputConnection.java:48-256][gti-ic]).
- **Lifecycle and threading.** Same split as today: `android_main` on its own thread. The text
  state crosses threads through GameActivity's C glue. `android-activity`'s author notes
  that `GameActivity_getTextInputState` likely "isn't thread safe" (issuetracker 294112477) and
  that it exposes modified UTF-8 ([game_activity/mod.rs:239-300][aa-ga]).
- **minSdk.** The AAR declares `minSdkVersion 21`, below FLUI's 23 (AAR manifest, and
  [game-activity/build.gradle][ga-gradle]).
- **Packaging.** `hasCode="true"`, and a Gradle build is effectively required. The AAR is consumed
  from Google Maven, and `android-activity` says to depend on
  `androidx.games:games-activity:4.4.0` and not to enable prefab ([README][aa-readme]). The 4.4.0
  POM lists no dependencies, but the class extends `AppCompatActivity`, so the app must add AppCompat
  itself, as Bevy does (appcompat, core, material in [libs.versions.toml][bevy-libs]). That
  breaks `flui build`'s no-Gradle path and the `hasCode="false"` probe.
- **APK size.** The library's own classes are small (`classes.jar` is 44,536 bytes uncompressed;
  the 1.8 MB AAR is mostly prefab static libraries). AppCompat and Core come on top (their 1.7.0 and
  1.13.1 AARs are 1.1 MB and 1.3 MB). The post-R8 increase was not measured here.
- **Coverage.** IME only, as a single-field `Editable` mirror. No DnD, no handwriting, no
  `CursorAnchorInfo`, no rich content. A subclass in Java would be needed for all of those. The
  library version is coupled: Bevy's catalog notes `gamesActivity` "must be compatible with
  `android-activity` crate", and `android-activity` cannot "guarantee that the next 4.x stable
  release will be compatible" ([README][aa-readme]).
- **Verdict.** It solves the smallest part (basic IME) at the highest packaging cost, and FLUI would
  still write Java. Rejected.

### (C) A real `classes.dex` in the APK (`hasCode="true"`)

- **Mechanism.** The same Java sources as (A), compiled and added to the APK as `classes.dex`.
  Two shapes are possible:
  - (C1) keep `android.app.NativeActivity` in the manifest and load `FluiView` through the
    Activity's own class loader (`getClassLoader().loadClass`, since `FindClass` on
    `android_main` still sees only the system loader, [JNI tips][jni]);
  - (C2) declare `dev.flui.FluiActivity extends NativeActivity` in the manifest.
    `NativeActivity` reads `android.app.lib_name` from the started activity's metadata
    ([NativeActivity.java:126-150][na-java]), so a subclass keeps working with
    `android-activity`'s glue.
- **Lifecycle and threading.** Identical to (A). (C2) also gets Activity-only callbacks (permission
  and activity results, `onNewIntent`) as plain overrides.
- **minSdk.** No new floor.
- **Play policy.** None; this is ordinary app code. It is also visible to Play's analysis and to
  ART's normal compilation.
- **APK size.** Same dex, as a separate entry.
- **Build tooling.** `flui build`'s SDK-tools path would add `javac` + `d8` and insert
  `classes.dex` into the zip before `zipalign`. Both tools are already located
  (`android_package.rs:60-100`; `d8.bat` ships in build-tools 35.0.0 on this host). The Gradle
  template gets `src/main/java` plus a `proguard-rules.pro` that keeps the JNI-only classes, since
  R8 strips classes referenced only from native code. Wry/Tauri/Dioxus show the cost of this
  shape: Kotlin templates that build.rs copies into the app's Gradle project
  ([wry build.rs:31-80][wry-build]). cargo-apk can set `hasCode` but has no dex step; xbuild
  generates a Gradle project with Kotlin support ([xbuild gradle/mod.rs][xbuild-gradle]).
  `cargo-ndk` + Gradle is the route `android-activity` recommends in general ("a more-standard,
  Gradle-based build system", [README][aa-readme]).
- **Verdict.** The correct escalation once FLUI needs an Activity subclass. As the default today it
  makes every packager responsible for a version-matched dex, for no gain over (A).

### Other routes considered

- **No Java at all.** Impossible for these callbacks (Recommendation, reason 1).
- **`java.lang.reflect.Proxy` for the interfaces.** Covers `OnDragListener` and
  `OnReceiveContentListener`. It cannot override `onCreateInputConnection`, and its
  `InvocationHandler` must still be a dex class. No gain.
- **Embedding the dex as an APK asset or a raw resource and loading it the same way.** This moves
  the bytes out of the `.so` and back into packaging, which is the skew (A) avoids.

## 3. How the shim plugs into `NativeActivity`

`NativeActivity.onCreate` calls `getWindow().takeSurface(this)` and `takeInputQueue(this)`. It then
sets a plain `NativeContentView` as content and focuses it ([NativeActivity.java:126-144][na-java]).
Taking the input queue inserts two stages into `ViewRootImpl`'s pipeline
([ViewRootImpl.java:1756-1764][vri-stages]):

- `NativePreImeInputStage` sends **key** events to native before the IME
  ([7552-7590][vri-preime]);
- `NativePostImeInputStage` sends every remaining event to native after the IME. Only events
  native reports unhandled continue to `ViewPostImeInputStage` and the views
  ([7836-7860][vri-postime]).

`EarlyPostImeInputStage`, which runs before native, still records the last touch point that
`startDragAndDrop` uses ([7728-7830][vri-early], [View.java:29293-29350][view-sdd]).

So a `FluiView` added with `addContentView(view, MATCH_PARENT)`:

- receives no touches FLUI handled (correct: FLUI's arena owns them);
- can take focus and be served by `InputMethodManager`. Show the keyboard with
  `imm.showSoftInput(fluiView, 0)` after `requestFocus()`, not with `android-activity`'s
  decor-view helper. Slint found that helper does not work with native-activity
  ([javahelper.rs:420-428][slint-show], [SlintAndroidJavaHelper.java:512-516][slint-java]);
- receives drag events, which do not use the input queue (§5).

Its surface is not used: FLUI keeps rendering to the window surface it already gets from
`takeSurface`. The view draws nothing.

## 4. IME: what the `InputConnection` must be

The platform contract:

- "An editor should start by implementing `View#onCreateInputConnection(EditorInfo)` to return its
  own input connection". A custom editor should "provide your own subclass of
  `BaseInputConnection`" and "mimic the default TextView implementation"
  ([InputConnection.java:74-116][ic-src]).
- `onCheckIsTextEditor()` must return `true` ([View.java][view-src]; stylus guide below).
- IME calls arrive on the view's handler thread, which is the Java main thread.
  `getHandler()` (API 24) can move them to another `Looper`, but "the system only uses `Looper`"
  and some callbacks stay on the main thread before API 33
  ([InputConnection.java:975-990, 1299-1314][ic-handler]).
- The IME waits at most 2,000 ms for each read (`MAX_WAIT_TIME_MILLIS`) and gets `null` after that
  ([RemoteInputConnection.java:67-188][ric]). The input dispatcher's ANR limit is 5 seconds
  ([ANRs][anr]).

The constraint for FLUI: the document lives in ADR-0090's `TextStore` on the owner thread
(`android_main`), and the IME asks synchronously on the Java main thread. There are two shapes:

1. **Snapshot plus ordered edits (recommended).** The owner publishes an immutable snapshot after
   every commit: text in UTF-16, selection, composition, `EditorInfo` fields, and a generation. It
   swaps an `Arc` the Java side reads through a native call, or pushes the snapshot into Java
   directly.
   - Reads (`getTextBeforeCursor`, `getTextAfterCursor`, `getSelectedText`,
     `getSurroundingText` API 31, `getExtractedText`) answer from the snapshot without waiting.
   - Edits (`commitText`, `setComposingText`, `setComposingRegion`, `deleteSurroundingText`,
     `finishComposingText`, `setSelection`, `performEditorAction`, `commitContent` API 25)
     apply to the local copy, so the next read is consistent. They are queued to the owner in
     order, tagged with the generation, and the owner replays them through the store's verbs
     inside a read-write grant.
   - `beginBatchEdit`/`endBatchEdit` delimit one owner transaction.
   - When the owner's state diverges (an app edit), it pushes back with
     `imm.updateSelection(...)` or `imm.restartInput(view)`.
   - `CursorAnchorInfo` is a push on Android anyway: `requestCursorUpdates` (API 21) arrives as an
     edit-like request, and the owner answers with `updateCursorAnchorInfo` after layout.

   Every shipping native-renderer stack surveyed works this way. GameTextInput keeps an
   `Editable` and calls the listener's `stateChanged` ([InputConnection.java:48-256, 537-547][gti-ic]).
   Slint's `SlintEditable` overrides `replace`, coalesces inside batch edits, and calls
   `updateText` → `invoke_from_event_loop`. Its `setText` pushes owner state back with
   `restartInput`/`updateSelection`
   ([SlintAndroidJavaHelper.java:150-243][slint-java], [javahelper.rs:630-660][slint-update]).
   The difference from both: FLUI's owner stays the authority through ADR-0090's store and
   ADR-0142's commit gate, instead of trusting the Java copy.

2. **Synchronous pull across threads.** Each IME read blocks the Java main thread on an owner lock
   grant, which maps one-to-one onto ADR-0090's `LockTiming::Sync`/`SyncLockUnavailable`. It is
   deadlock-free only while the owner never waits on the Java main thread. But every read then
   waits for the owner's current frame, and a long frame (pipeline compile, surface recreation)
   turns into `null` answers mid-composition. Rejected as the default. It can still serve the
   rare read that needs layout, such as `getTextBoundsInfo` (API 34), with an explicit short
   deadline that degrades to "unsupported".

ADR-0090 §4's row "Android: `InputConnection` maps onto the same verbs" should be made precise
in the ADR this note feeds. Android uses an asynchronous adapter: snapshot reads on the Java side,
and edits replayed through the verbs on the owner. The store contract is unchanged. Offsets cross
JNI as UTF-16 (`text_store::utf16`).

## 5. Drag and drop: how events reach a View

- **Path.** The window manager delivers `DragEvent`s through `IWindow.dispatchDragEvent` →
  `ViewRootImpl.handleDragEvent` → `mView.dispatchDragEvent(event)`. The input stages are not
  involved ([ViewRootImpl.java:9103-9165][vri-drag]). Inside the tree,
  `View.dispatchDragEvent` → `callDragEventHandler` calls the `OnDragListener` first and falls
  back to `onDragEvent` ([View.java callDragEventHandler][view-src]). A view must return `true`
  for `ACTION_DRAG_STARTED` to keep receiving events until `ACTION_DRAG_ENDED`
  ([drag and drop guide][dnd-guide]). Because `FluiView` is a subclass, it overrides `onDragEvent`
  and needs no listener object.
- **Mapping to ADR-0211.**
  - `ACTION_DRAG_STARTED`/`ENTERED`/`LOCATION`/`EXITED` become `PlatformInput::DragDrop` hover
    events. The view-side answer to a `LOCATION` comes from ADR-0211 item 5's cached
    `DropFeedback`: the Java main thread cannot wait for the owner's hit test.
  - `ACTION_DROP` delivers the `ClipData`. Take URI permissions with
    `Activity.requestDragAndDropPermissions(event)` (API 24) while handling the drop, then hand
    the data to the owner as a `DataTransferOffer`.
  - The return value for `ACTION_DROP` must be decided on the Java main thread, so it is the
    cached acceptance, the same rule ADR-0211 applies to the Win32 OLE loop.
- **`OnReceiveContentListener` (API 31) is not the drag target.** The default `View.onDragEvent`
  accepts `STARTED` and, on `DROP`, builds a `ContentInfo` and calls `performReceiveContent`. It
  never surfaces locations ([View.java onDragEvent][view-src]). That cannot drive FLUI's hover,
  spring-loading and autoscroll. The listener is the right receiver for **paste and keyboard
  rich content**: "a unified API that lets your app accept content from any source: clipboard,
  keyboard, or dragging" ([receive rich content][rich]). Set it on `FluiView` on API 31+.
  Custom views must not bypass it: "use the Jetpack API `InputConnectionCompat.createWrapper`"
  when overriding `onCreateInputConnection` ([receive rich content][rich]). Without AndroidX the
  shim does the platform equivalent. Below API 31 it handles `commitContent` (API 25) and
  `EditorInfo.contentMimeTypes` (API 25) directly. Taking `androidx.core` for `ContentInfoCompat`
  is not worth a Gradle dependency for one class.
- **Source side.** `FluiView.startDragAndDrop(clip, shadow, localState, flags)` (API 24) starts
  the OS session when ADR-0211 escalates. `DRAG_FLAG_GLOBAL` (API 24) allows cross-app drops. The
  call is refused on a detached view or an invalid surface
  ([View.java:29293-29310][view-sdd]). It uses the last touch point and pointer id that
  `EarlyPostImeInputStage` recorded, so it works although FLUI consumed the touch. The call must
  run on the Java main thread, so the owner posts it at ADR-0211 item 7's quiescent point. The
  shadow is a `DragShadowBuilder` that draws a bitmap the owner rasterized. Compose uses the
  same shape: one host view calls `setOnDragListener(dragAndDropManager)` and `startDragAndDrop`
  with its own `DragShadowBuilder` ([AndroidComposeView.android.kt:1288, 1830-1850][compose-dnd]).
- **Unverified on a device.** Whether `ViewRootImpl.mSurface.isValid()` is true under
  `NativeActivity`'s `takeSurface` was not checked. `startDragAndDrop` returns `false` when it is
  not ([View.java:29302-29305][view-sdd]). This is the first thing the device spike must check.

## 6. Stylus handwriting

The platform requires the following ([custom text editors][hw-custom], [stylus in text
fields][hw-fields]):

- `onCheckIsTextEditor` and `onCreateInputConnection`;
- automatic initiation (`setAutoHandwritingEnabled`, API 33) or explicit
  `InputMethodManager.startStylusHandwriting(view)` (API 33) after
  `isStylusHandwritingAvailable()` (API 34);
- `EditorInfo.setStylusHandwritingEnabled` (API 35) and `setSupportedHandwritingGestures`
  (API 34);
- `performHandwritingGesture`/`previewHandwritingGesture` (API 34);
- `CursorAnchorInfo` with editor bounds and visible line bounds, through `updateCursorAnchorInfo`;
- the `TYPE_HANDWRITING` pointer icon on hover.

The `NativeActivity`-specific catch: `ViewRootImpl` feeds `HandwritingInitiator` from
`ViewPostImeInputStage` ([ViewRootImpl.java:8107][vri-hw]), which runs after native. Automatic
initiation therefore sees only stylus events FLUI reports `Unhandled`
(`crates/flui-platform/src/platforms/android/mod.rs:303-331`). The simpler rule for FLUI, which
already hit-tests on the owner, is to call `startStylusHandwriting(fluiView)` on a stylus down
inside a focused editor's handwriting bounds. Flutter's `ScribePlugin` makes the same explicit
call ([ScribePlugin.java:74-101][flutter-scribe]). Compose selects an API 24 or API 34 manager
implementation for it ([ComposeInputMethodManager.android.kt:66-153][compose-hw]). Handwriting
reuses the IME's `InputConnection`, so it adds no delivery question of its own.

## 7. What other frameworks do

| Framework | Java side | IME | DnD | Evidence |
|---|---|---|---|---|
| `android-activity` | none (NativeActivity) or GameActivity's AAR | NativeActivity: soft keyboard show/hide only; GameActivity: GameTextInput state | none | [native_activity/mod.rs:365-457][aa-na], [game_activity/mod.rs:239-340][aa-ga], [README][aa-readme] |
| winit (0.30.12, master) | none of its own | `set_ime_allowed` → `show_soft_input`/`hide_soft_input`; cursor area and purpose are no-ops; GameActivity text events fall to "Unknown android_activity input event" | none | [android/mod.rs:909-919, 469-471][winit-030], [winit-android event_loop.rs:995-1027][winit-master] |
| egui / eframe | none; picks a winit backend feature | via winit, so soft keyboard only | none | [eframe Cargo.toml:44-52][eframe] |
| Bevy | Kotlin `MainActivity : GameActivity()` under Gradle, AppCompat + Core + Material, `minSdk = 26` | via winit (no `TextEvent`) | none | [build.gradle.kts][bevy-gradle], [MainActivity.kt][bevy-main], [Cargo.toml:584-587][bevy-cargo] |
| Slint | **(A)**: one Java file → `javac` + `d8` in build.rs → `include_bytes!` → `InMemoryDexClassLoader` (API 26+) / `DexClassLoader` (below); works on NativeActivity or GameActivity | `SlintInputView extends View` + `BaseInputConnection` over a `SpannableStringBuilder` mirror, pushed to the event loop | none | [build.rs][slint-build], [javahelper.rs][slint-loader], [SlintAndroidJavaHelper.java][slint-java] |
| Tauri / Dioxus mobile (wry) | **(C)**: Kotlin `WryActivity : AppCompatActivity` templates written into the app's Gradle project by build.rs; `TauriActivity`/`MainActivity` extend it | the system `WebView` | the `WebView`'s | [WryActivity.kt:35][wry-act], [wry build.rs][wry-build], [TauriActivity.kt:14][tauri-act], [MainActivity.kt.hbs][dioxus-act] |
| Jetpack Compose | the host view is Kotlin (`AndroidComposeView`) | `onCheckIsTextEditor`/`onCreateInputConnection` on the single host view, delegating to the active text-input session | `setOnDragListener(dragAndDropManager)` and `startDragAndDrop` on the same host view | [AndroidComposeView.android.kt:1288, 1845, 3438-3456][compose-dnd] |
| Flutter (checklist) | Java embedding (`FlutterView`) | `FlutterView.onCreateInputConnection`; `ScribePlugin` for handwriting | no engine-level `onDragEvent` | [FlutterView.java:908][flutter-view], [ScribePlugin.java][flutter-scribe] |

Synthesis:

- Every stack with real IME has a `View` subclass that owns `onCreateInputConnection`.
- Every stack with drag and drop puts it on that same host view.
- The only one that does this without Gradle is Slint, with (A).

## 8. Risks

1. **Play or Play Protect treats in-memory dex as suspicious.** No primary source says it does;
   the written policy is about *downloaded* code ([policy][play-dna]). Mitigation: the Java
   sources are delivery-agnostic, so moving them to (C1) is a packaging change. The ADR should
   name that switch.
2. **API 23–25 fallback.** It writes to `codeCacheDir`. Write the file read-only and verify its
   digest before loading. This is the only on-disk dynamic code.
3. **`startDragAndDrop` under `takeSurface`.** Unverified (§5). If it fails, the fallback is a
   transparent, non-focusable child window for the source. Measure before designing it.
4. **Two threads own one editing session.** Generation-tagged edits, owner authority, and
   `restartInput` on divergence must be tested against Gboard and Samsung Keyboard: batch edits,
   `deleteSurroundingText` across a composition, emoji surrogate pairs, CJK. Slint's and
   GameTextInput's shipped bugs are the checklist: modified UTF-8, missing locking
   ([game_activity/mod.rs:262-282][aa-ga]).
5. **Teardown races.** Java callbacks can arrive after `MainEvent::Destroy` and after the owner
   dropped the bridge. The bridge must be `Arc`/`Weak` with a detached state, and Java-held
   handles cleared on the Java main thread. Follow AGENTS.md's "accepted work must remain
   deliverable" for queued IME edits.
6. **Process globals.** Slint caches its class loader in a `static OnceLock`
   ([javahelper.rs:316][slint-loader]). FLUI's `globals` gate requires an ADR-0097 grant or
   per-platform ownership. Per-Activity ownership is natural here, because each `android_main`
   gets a new `AndroidApp`.
7. **Build reproducibility.** A build.rs that shells out to `javac`/`d8` makes the Android build
   depend on a JDK and build-tools version. The JDK 21 + old build-tools failure Slint documents
   is an example ([build.rs:67-76][slint-build]). A checked-in, gate-verified dex avoids it.
8. **Activity-only callbacks.** Runtime permissions (`POST_NOTIFICATIONS`, camera) and activity
   results will force (C2). That is a planned step, not a surprise.

## 9. Testing on a real device

The existing harness is `cargo xtask device android-input`: a `hasCode="false"` probe on a Galaxy
S21 driven by `adb shell input` and `uinput`, recorded and replayed
(`tools/xtask/src/device/android_input.rs`, `docs/platform-parity.md:112-115`). It extends to
this work without changing packaging under (A):

- **Loader.** On launch, log the loaded class loader and API path (in-memory or file). Run once
  on an API 26+ device (the S21) and once on an API 23–25 emulator image for the fallback.
- **IME.** Select keyboards with `adb shell ime list -s` / `ime set <id>`. These are
  `InputMethodManagerService` shell commands ([InputMethodManagerService.java:6326-6340][imms]).
  Gboard and Samsung Keyboard are the two that matter. `adb shell input text` injects key events,
  which reach native before the IME and never touch the `InputConnection`, so it is not an IME
  test. Compose test text with the keyboard (`uinput` taps on its keys), or with a test IME
  installed as a second probe APK. Assert through the recorder: the owner's store state after
  each step, plus `dumpsys input_method` for the served view and `EditorInfo`.
- **DnD.** `adb shell input draganddrop x1 y1 x2 y2 [duration]` performs a long-press drag
  ([InputShellCommand.java:297, 346][isc]). Check three cases: within the window (ADR-0211's
  in-window path, no OS session); escalation to a second app in split screen, with `Files` or a
  second FLUI probe as the target; and a drop from another app into FLUI, checking URI permission
  grants.
- **Handwriting.** No hardware pen on the S21 ([platform-parity.md:113][parity]). Android 15+
  simulates a stylus with `adb shell setprop debug.input.simulate_stylus_with_touch true`
  (`persist.debug.input.simulate_stylus_with_touch` on 14) ([stylus in text fields][hw-fields]).
  The `uinput` pen the probe already creates is the second source.
- **Parity rows.** IME, DnD and handwriting rows in `docs/platform-parity.md` move to ✅ only after
  these runs, per AGENTS.md.

## 10. The decision an ADR should record

- **Title:** "Android's Java side is a `flui-platform` shim loaded from the native library".
- **Decision.**
  1. FLUI ships Java code on Android. Its sources live in `flui-platform` (one package,
     `dev.flui.platform`). They are compiled to one dex at build time (build.rs or a gate-checked
     generated file; pick one) and embedded in the native library.
  2. Default delivery: `InMemoryDexClassLoader` (API 26+), parented to the Activity's class
     loader, loaded on the Java main thread in `android_on_create`. API 23–25 load the same bytes
     with `DexClassLoader` from a read-only, digest-checked file in `codeCacheDir`. The manifest
     stays `NativeActivity`, `hasCode="false"`.
  3. Classes are found through that loader and native methods are bound with `RegisterNatives`;
     `FindClass` is never used for shim classes.
  4. One `FluiView` per Activity, full-window, focusable, drawing nothing. It is the only Java
     IME target, drop target, drag source, rich-content receiver and handwriting editor.
  5. Threads. The owner never blocks on the Java main thread; owner→Java work is posted. Java→owner
     callbacks enqueue `Send` data and wake the owner. Answers the OS needs synchronously (IME
     reads, drag-location acceptance, drop result) come from owner-published snapshots and cached
     feedback.
  6. IME adapter: snapshot reads plus generation-tagged, ordered edits replayed into ADR-0090's
     store on the owner. This amends ADR-0090 §4's Android row and closes its GameActivity
     hypothesis as rejected.
  7. Drag and drop: `onDragEvent` on `FluiView` for targets, `startDragAndDrop` for escalated
     sources (API 24; `Unavailable` below). `OnReceiveContentListener` (API 31) only for paste and
     keyboard content. This amends ADR-0211's Android rows.
  8. Escalation rule: when FLUI needs a callback only a manifest-declared class can receive, the
     same sources ship as an APK `classes.dex` and the manifest names `FluiActivity extends
     NativeActivity`. The in-memory path then retires; there is never a mixed delivery.
  9. No AndroidX and no GameActivity dependency.
- **Alternatives rejected:** GameActivity (packaging cost, IME-only coverage, still needs Java);
  `classes.dex` by default (version skew across packagers, a Gradle-or-d8 step everywhere, for no
  capability gain until an Activity subclass is needed); `Proxy`/no Java (cannot override `View`
  methods; `DefineClass` unsupported).
- **Verification the ADR names:** a loader test that runs on both API paths. The device rows from
  §9. A `compile_fail` or `assert_not_impl_any!` pin that the owner-side bridge is `!Send`, while
  the Java-thread callbacks compile against `Send` payloads only. A conformance row in
  `assert_backend_conforms` for `DragError::Unavailable` below API 24.

## 11. Open, unverified on this host

- `startDragAndDrop` with `NativeActivity`'s `takeSurface` (§5, risk 3). No device run.
- Play or Play Protect behavior for in-memory dex of embedded bytes (risk 1). Policy text only.
- Post-R8 size of AppCompat + Core for (B). Only AAR sizes were measured.
- Whether `jni` 0.22's `native_method`/`bind_java_type!` contain panics at the boundary was not
  read; assume they do not until checked.
- Chromium's `ThreadedInputConnection` (a third data point for §4) was not read.

## Sources

Primary documents and code read for this note:

- Android platform docs: [JNI tips][jni];
  [`InMemoryDexClassLoader`][imdcl] and its [source][imdcl-src];
  [Android 14 behavior changes, safer dynamic code loading][a14-dcl];
  [NDK concepts, native activities][ndk-concepts] ("set `android:hasCode` to `false`");
  [GameActivity][agdk-ga]; [drag and drop in views][dnd-guide];
  [receive rich content][rich]; [stylus input in text fields][hw-fields];
  [custom text editors and handwriting][hw-custom]; [ANRs][anr].
- Google Play: [Device and Network Abuse policy][play-dna].
- AOSP `platform/frameworks/base@1cdfff55`: [NativeActivity.java][na-java];
  [ViewRootImpl.java][vri-stages]; [View.java][view-src]; [InputConnection.java][ic-src];
  [RemoteInputConnection.java][ric]; [InputShellCommand.java][isc];
  [InputMethodManagerService.java][imms].
- AOSP `platform/frameworks/opt/gamesdk@044fd03c`: [GameActivity.java][ga-java];
  [gametextinput/InputConnection.java][gti-ic]; [game-activity/build.gradle][ga-gradle].
  Maven: `androidx.games:games-activity:4.4.0` AAR and POM from `dl.google.com/android/maven2`.
- Android SDK `platforms/android-35/data/api-versions.xml` (API levels); build-tools 35.0.0 `d8`
  (dex size measurement).
- android-activity v0.6.1: [README][aa-readme]; [lib.rs][aa-lib];
  [native_activity/mod.rs][aa-na]; [game_activity/mod.rs][aa-ga]; [main_callbacks.rs][aa-mc];
  [CHANGELOG][aa-changelog].
- Slint `86b42115`: [build.rs][slint-build]; [javahelper.rs][slint-loader];
  [SlintAndroidJavaHelper.java][slint-java].
- winit: [v0.30.12 android/mod.rs][winit-030]; [master winit-android event_loop.rs][winit-master].
- egui: [eframe Cargo.toml][eframe].
- Bevy `85503941`: [Cargo.toml][bevy-cargo]; [mobile build.gradle.kts][bevy-gradle];
  [libs.versions.toml][bevy-libs]; [MainActivity.kt][bevy-main].
- wry `66420742`: [WryActivity.kt][wry-act]; [build.rs][wry-build]. Tauri `136a28ea`:
  [TauriActivity.kt][tauri-act]. Dioxus `7a817390`: [MainActivity.kt.hbs][dioxus-act].
- androidx `9a589fa6`: [AndroidComposeView.android.kt][compose-dnd];
  [ComposeInputMethodManager.android.kt][compose-hw].
- Flutter `dd45f0c2`: [FlutterView.java][flutter-view]; [ScribePlugin.java][flutter-scribe].
- Tooling: [cargo-apk ndk-build manifest.rs][cargo-apk]; [xbuild gradle/mod.rs][xbuild-gradle].
- jni-rs v0.22.4: [refs/global.rs][jni-global].

[jni]: https://developer.android.com/training/articles/perf-jni
[imdcl]: https://developer.android.com/reference/dalvik/system/InMemoryDexClassLoader
[imdcl-src]: https://android.googlesource.com/platform/libcore/+/de876a01b29230b877c9f408348c38a90ee724a5/dalvik/src/main/java/dalvik/system/InMemoryDexClassLoader.java
[a14-dcl]: https://developer.android.com/about/versions/14/behavior-changes-14#safer-dynamic-code-loading
[ndk-concepts]: https://developer.android.com/ndk/guides/concepts#naa
[agdk-ga]: https://developer.android.com/games/agdk/game-activity
[dnd-guide]: https://developer.android.com/develop/ui/views/touch-and-input/drag-drop/view
[rich]: https://developer.android.com/develop/ui/views/receive-rich-content
[hw-fields]: https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/stylus-input-in-text-fields
[hw-custom]: https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/custom-text-editors
[anr]: https://developer.android.com/topic/performance/vitals/anr
[play-dna]: https://support.google.com/googleplay/android-developer/answer/9888379
[na-java]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/app/NativeActivity.java#126
[vri-stages]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#1756
[vri-preime]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#7552
[vri-early]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#7728
[vri-postime]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#7836
[vri-hw]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#8107
[vri-drag]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/ViewRootImpl.java#9103
[view-src]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/View.java
[view-sdd]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/View.java#29293
[ic-src]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/inputmethod/InputConnection.java#74
[ic-handler]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/view/inputmethod/InputConnection.java#1299
[ric]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/core/java/android/inputmethodservice/RemoteInputConnection.java#67
[isc]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/services/core/java/com/android/server/input/InputShellCommand.java#297
[imms]: https://android.googlesource.com/platform/frameworks/base/+/1cdfff555f4a21f71ccc978290e2e212e2f8b168/services/core/java/com/android/server/inputmethod/InputMethodManagerService.java#6326
[ga-java]: https://android.googlesource.com/platform/frameworks/opt/gamesdk/+/044fd03c4a7d3b75aeb6ca2bd7fb6155d2cdb787/game-activity/src/main/java/com/google/androidgamesdk/GameActivity.java
[gti-ic]: https://android.googlesource.com/platform/frameworks/opt/gamesdk/+/044fd03c4a7d3b75aeb6ca2bd7fb6155d2cdb787/game-text-input/src/main/java/com/google/androidgamesdk/gametextinput/InputConnection.java
[ga-gradle]: https://android.googlesource.com/platform/frameworks/opt/gamesdk/+/044fd03c4a7d3b75aeb6ca2bd7fb6155d2cdb787/game-activity/build.gradle
[aa-readme]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/README.md
[aa-lib]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/android-activity/src/lib.rs#L59-L130
[aa-na]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/android-activity/src/native_activity/mod.rs#L365-L457
[aa-ga]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/android-activity/src/game_activity/mod.rs#L239-L340
[aa-mc]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/android-activity/src/main_callbacks.rs
[aa-changelog]: https://github.com/rust-mobile/android-activity/blob/v0.6.1/android-activity/CHANGELOG.md
[slint-build]: https://github.com/slint-ui/slint/blob/86b42115c52ac01b54c72902eb4b493aaceb1476/internal/backends/android-activity/build.rs
[slint-loader]: https://github.com/slint-ui/slint/blob/86b42115c52ac01b54c72902eb4b493aaceb1476/internal/backends/android-activity/javahelper.rs#L313-L366
[slint-update]: https://github.com/slint-ui/slint/blob/86b42115c52ac01b54c72902eb4b493aaceb1476/internal/backends/android-activity/javahelper.rs#L630-L660
[slint-show]: https://github.com/slint-ui/slint/blob/86b42115c52ac01b54c72902eb4b493aaceb1476/internal/backends/android-activity/javahelper.rs#L420-L428
[slint-java]: https://github.com/slint-ui/slint/blob/86b42115c52ac01b54c72902eb4b493aaceb1476/internal/backends/android-activity/java/SlintAndroidJavaHelper.java#L134-L260
[winit-030]: https://github.com/rust-windowing/winit/blob/v0.30.12/src/platform_impl/android/mod.rs#L909-L919
[winit-master]: https://github.com/rust-windowing/winit/blob/758a50e35a928dbf72bf05ae91f12fee7a540a83/winit-android/src/event_loop.rs#L995-L1027
[eframe]: https://github.com/emilk/egui/blob/86099aca1276fe786921992c8d5a68914ee79bec/crates/eframe/Cargo.toml#L44-L52
[bevy-cargo]: https://github.com/bevyengine/bevy/blob/85503941e488152711a4695bbe248e9ed9f453fc/Cargo.toml#L584-L587
[bevy-gradle]: https://github.com/bevyengine/bevy/blob/85503941e488152711a4695bbe248e9ed9f453fc/examples/mobile/android/app/build.gradle.kts
[bevy-libs]: https://github.com/bevyengine/bevy/blob/85503941e488152711a4695bbe248e9ed9f453fc/examples/mobile/android/gradle/libs.versions.toml
[bevy-main]: https://github.com/bevyengine/bevy/blob/85503941e488152711a4695bbe248e9ed9f453fc/examples/mobile/android/app/src/main/kotlin/org/bevyengine/example/MainActivity.kt
[wry-act]: https://github.com/tauri-apps/wry/blob/6642074224a2db9eb197f69d21b300b02b5dcd49/src/android/kotlin/WryActivity.kt#L35
[wry-build]: https://github.com/tauri-apps/wry/blob/6642074224a2db9eb197f69d21b300b02b5dcd49/build.rs#L31-L80
[tauri-act]: https://github.com/tauri-apps/tauri/blob/136a28eaa879e8aa706f9b2e05a050c46dcc1f4f/crates/tauri/mobile/android-codegen/TauriActivity.kt#L14
[dioxus-act]: https://github.com/DioxusLabs/dioxus/blob/7a817390a69294f614b3055aae2dacdf25c050a2/packages/cli/assets/android/MainActivity.kt.hbs
[compose-dnd]: https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/ui/ui/src/androidMain/kotlin/androidx/compose/ui/platform/AndroidComposeView.android.kt#L1288
[compose-hw]: https://github.com/androidx/androidx/blob/9a589fa61f8661e0cbef9123dcc98d3c1f655fc7/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/text/input/internal/ComposeInputMethodManager.android.kt#L66-L153
[flutter-view]: https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/engine/src/flutter/shell/platform/android/io/flutter/embedding/android/FlutterView.java#L908
[flutter-scribe]: https://github.com/flutter/flutter/blob/dd45f0c2f98adca9d327649a61076d030c1eb426/engine/src/flutter/shell/platform/android/io/flutter/plugin/editing/ScribePlugin.java#L74-L101
[cargo-apk]: https://github.com/rust-mobile/cargo-apk/blob/887423a9150df52f6699332ba39e58ae4c24c877/ndk-build/src/manifest.rs#L72-L74
[xbuild-gradle]: https://github.com/rust-mobile/xbuild/blob/e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1/xbuild/src/gradle/mod.rs
[jni-global]: https://github.com/jni-rs/jni-rs/blob/v0.22.4/crates/jni/src/refs/global.rs#L77-L90
[parity]: ../platform-parity.md
