# ADR-0247: Android's Java side is a `flui-platform` view loaded from the native library

- **Status:** Accepted (2026-10-17). Implemented: the shim, its loader, the input method host,
  the drop target, the drag source, the content receiver and the handwriting request; run on a
  Galaxy S21 (Android 15) through [`cargo xtask device android-input`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs).
- **Date:** 2026-10-17
- **Superseded-by:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) for item 1's embedding of the dex
  in the library with `include_bytes!` and its "every packager ships that copy" (the sources, the
  checked-in dex, the digest gate and "no JDK" stand), item 2 except its first sentence (the
  in-memory and file loaders and the `hasCode="false"` manifest), and item 9 (taken there). Items 3
  to 8 and 10 stand, and item 3's "that loader" is the Activity's class loader. The "Alternatives
  rejected" entry on a dex in the APK and the consequence "no packaging path changes" follow items 1
  and 2.
- **Amends:** [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the Android row of its
  platform mapping, and its GameActivity hypothesis), [ADR-0211](ADR-0211-unified-drag-and-drop.md)
  (the Android target and source)
- **Related:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (every Android and JNI type
  stays in `flui-platform`), [ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md)
  (the store's commit gate), [ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md) (the `TextStoreHost` a
  window offers), research note
  [`2026-10-17-android-java-side.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-17-android-java-side.md)

## Context

FLUI's Android apps are `android.app.NativeActivity` with `android:hasCode="false"`. Input
methods, drag and drop, pasted and keyboard content, and stylus handwriting all reach an app only
through Java: a `View` override (`onCreateInputConnection`, `onCheckIsTextEditor`,
`onDragEvent`) or a Java interface (`OnReceiveContentListener`). JNI cannot define a class on
Android, and `java.lang.reflect.Proxy` covers only interfaces, so the question is how dex bytes
reach the process, not whether they do. `android-activity`'s `NativeActivity` backend has no
`InputConnection`; GameActivity has a fixed single-`Editable` one with no drag and drop or
handwriting, and brings Gradle and AppCompat with it. The research note compares the options.

## Decision

1. **FLUI ships Java code on Android.** Its sources live in `flui-platform`, under
   `crates/flui-platform/android/java`, in one package, `dev.flui.platform`. They are compiled to
   one dex, `crates/flui-platform/android/classes.dex`, which is checked in and embedded in the
   native library with `include_bytes!`. Building FLUI for Android needs no JDK and no build
   script, and every packager (`flui build`, Gradle, cargo-apk) ships the dex the library was
   built with. `classes.dex.toml` records the digests of the sources and the dex and the tools
   that compiled them; `cargo xtask android-dex` checks both digests in `cargo xtask checks`
   (no JDK needed), `--rebuild` recompiles with `javac` and `d8` and requires the same bytes, and
   `--write` regenerates them.
2. **Delivery.** At platform start the backend posts the installation to the Java main thread
   (`AndroidApp::run_on_java_main_thread`), not to `android_on_create`, whose symbol belongs to
   the application. From API 26 the bytes load through `InMemoryDexClassLoader`; on API 23–25
   they are written read-only to the code cache, compared byte for byte with the library's, and
   loaded through `DexClassLoader`. Either loader is parented to the Activity's. The manifest
   stays `NativeActivity` with `hasCode="false"`.
3. **Binding.** The view class is found through that loader (`loadClass`), never `FindClass`,
   which searches the system loader on a native thread. Its native methods are bound with
   `RegisterNatives`. The view holds a handle to the native bridge, which the platform clears on
   the Java main thread before it frees the bridge.
4. **One `FluiView` per Activity**, laid over the whole content with `addContentView`, focusable
   and drawing nothing. It takes no touch FLUI handles (the native input queue sees input
   first), and it is the only Java input method target, drop target, drag source, receiver of
   pasted and keyboard content, and handwriting editor.
5. **Threads.** The owner never waits for the Java main thread: owner-to-view calls are posted.
   The view's native methods only queue data for the owner and wake it. What the system needs
   answered at once (the input method's reads, a drag's acceptance, a drop's result) comes from
   state the owner published earlier: the mirror of the focused field and the drop feedback the
   target cached.
6. **Input method.** The view's `InputConnection` reads and edits a mirror of the focused field
   on the Java main thread. The owner publishes a generation-tagged snapshot of the field's
   `TextStore` when a field takes focus and whenever the store changes outside the input method's
   edits (a text change restarts the input method, a selection change only tells it). After each
   edit, or batch of edits, the view reports its state; it becomes one ordered edit against the
   snapshot it was made on, which the owner replays into the store under an asynchronous lock.
   An edit made on a snapshot older than the latest one published is dropped, because that
   snapshot supersedes it, and the owner restarts the input method on the store's text (a newer
   snapshot that only moved the selection would leave the dropped edit's text in it). A step the
   store refuses restarts it the same way. A
   protected field is mirrored as one bullet per code unit. Android hands every key to the input
   method before native code, so a key that reaches FLUI while the input method composes is one
   it passed on, and arrives marked `ImeComposition::Active`. The editor action arrives as Enter
   (Next and Previous as Tab and Shift+Tab). The store contract is unchanged, and offsets cross
   JNI in UTF-16 code units. This amends ADR-0090's Android row and rejects its GameActivity
   hypothesis.
7. **Drag and drop.** `onDragEvent` is the target: an offer is minted from the
   `ClipDescription` when a drag enters, the drop is answered from the target's cached feedback,
   and the drop copies the clip's text, HTML and URIs out, holding the drop's URI permissions,
   per offer, until the target concludes it (a refused drop releases them at once); a URI's bytes are read through the content resolver when
   requested. A drag from another app permits a copy; one this process started permits what its
   source advertised. `startDragAndDrop` (API 24) is the source when a drag leaves its window
   (`OsDragStart::OnLeavingWindow`); Android takes the whole clip at the start, `file:` URIs and
   bytes stay out of it, and the drag is global. Below API 24, or without the view,
   `outgoing_drag()` is `Unavailable` and a drag stays in its window. `OnReceiveContentListener`
   (API 31) receives only pasted and keyboard text, which reaches the focused field as one input
   method edit; content that is not text is handed back. This amends ADR-0211's Android target
   and source.
8. **Handwriting.** Android's automatic initiation sees only the stylus events native code
   leaves unhandled, so the view's automatic handwriting is off, and a pen tip that touches down
   inside the focused field's bounds asks `startStylusHandwriting` (API 33).
9. **Escalation.** When FLUI needs a callback only a manifest-declared class receives
   (permission and activity results, `onNewIntent`), the same sources ship as an APK
   `classes.dex` and the manifest names a `NativeActivity` subclass of FLUI's; the in-memory path
   then retires, so delivery is never mixed. The same switch is the answer if Play or Play
   Protect ever flags the in-memory dex.
10. **No AndroidX and no GameActivity dependency.**

## Alternatives rejected

- **GameActivity:** Gradle and AppCompat for every app, IME-only coverage through one
  `Editable`, and FLUI would still write Java for drag and drop and handwriting.
- **`classes.dex` in the APK by default:** every packager would need a version-matched dex step,
  and R8 rules for classes only native code names, for no capability gain until an Activity
  subclass is needed (item 9).
- **No Java, or `Proxy`:** cannot override `View` methods; `DefineClass` is not implemented on
  Android.
- **A build script running `javac` and `d8`:** every Android build would depend on a JDK and a
  build-tools version (Slint documents a JDK 21 failure of exactly this), for bytes that change
  only with the sources.
- **Blocking the Java main thread on the owner for each input method read:** a long frame turns
  into `null` answers mid-composition, and the input method gives up after 2 s.

## Consequences and verification

- The library grows by the dex (about 18 KB). No packaging path changes.
- Linux CI runs the pure halves: `shared::android_ime` in `tests/android_ime.rs`
  (`android_ime_adapter`: a keyboard's composition reaching the store in order across a frame
  transaction, the store composing what the view composes, an edit on a superseded snapshot
  dropped, a refused step, surrogate pairs, a protected field, the key rule, `EditorInfo`, the
  editor actions, handwriting and the cursor anchor) and `shared::android_drag` in
  `tests/android_drag.rs` (`android_drag_mapping`). `assert_backend_conforms` holds the Android
  profiles: `a_key_during_composition_is_the_input_methods` with Android's key rule, and
  `outgoing_drag_is_delivered_as_claimed` with the view, below API 24 and without the view.
  `input_capabilities_contracts` states the outgoing drag per API level.
  `flui_testing::text_store_kit::assert_conforms` certifies stores; the view is a host, not a
  store, and replays into stores that pass it.
- [`cargo xtask device android-input`](https://github.com/vanyastaff/flui/blob/e8a42673ad23a2e88614940a266cd01224dcb25f/tools/xtask/src/device.rs) on a Galaxy S21 (Android 15, API 35): the in-memory loader
  installed the view; a system drag the probe started from a held touch dropped its text on the
  probe's own window and the source heard `Dropped(COPY)`, so `startDragAndDrop` works under
  `NativeActivity`'s `takeSurface` (the research note's open risk 3); Samsung Keyboard opened on
  the focused field with FLUI's `EditorInfo`, composed three tapped letters into the store,
  passed a hardware Escape on during the composition, which arrived marked composing, committed
  on space and sent Done as Enter. A manual run carried a drag into Chrome in a pop-up window,
  which declined it; the source heard `Cancelled`.
- Not run: the API 23–25 file loader (no such device or image ran), Gboard, `CursorAnchorInfo`
  placement, paste through the content receiver, stylus handwriting (no pen on the S21), and a
  drag from or into another app that completes. Play Protect: the probe installed over `adb` and
  ran with no warning, but the device does not verify `adb` installs
  (`verifier_verify_adb_installs=0`), so no Play or Play Protect review of the in-memory dex has
  been observed (risk 1 stays open; item 9 is its fallback).
