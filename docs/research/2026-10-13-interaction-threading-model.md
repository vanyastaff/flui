# Interaction threading model: one owner thread per UI runtime, or a threaded input stack?

- **Date:** 2026-10-13, base `main` @ `10d39b228`
- **Question (owner):** does the input stack (`HostInput` → pointer routing → gesture arena → nested
  scroll channel → DnD, haptics, recording) stay single-threaded per UI runtime with owner-local
  `Rc`/`!Send` state, or go multi-threaded?
- **Inputs:** [ADR-0027](../adr/ADR-0027-owner-affine-ui-realms.md),
  [ADR-0091](../adr/ADR-0091-one-owner-thread-isolated-realms-raster-thread.md),
  [ADR-0097](../adr/ADR-0097-no-process-global-state-gate.md),
  [ADR-0136](../adr/ADR-0136-owner-local-ui-surfaces.md),
  [ADR-0159](../adr/ADR-0159-gesture-arena-owner-delivery.md),
  [ADR-0161](../adr/ADR-0161-immutable-owner-local-gesture-recognizers.md),
  [ADR-0175](../adr/ADR-0175-owner-local-scheduling-and-animation.md),
  [ADR-0182](../adr/ADR-0182-complete-owner-execution.md),
  [ADR-0190](../adr/ADR-0190-owner-local-render-invalidation.md),
  [send-flip design](../plans/specs/send-flip/design.md),
  [interaction completeness design](../plans/specs/flui-interaction/completeness/design.md),
  `design/architecture.md` §8.2–§8.5.
- Paths are from the repository root. External code is pinned to the commit read:
  GPUI `zed-industries/zed@26363aab`, Xilem `linebender/xilem@271a27a6`, Bevy
  `bevyengine/bevy@12a94e0c`.

## Recommendation

**Stay single-threaded per UI runtime.** Every piece of input state that runs user code or reads
the render tree — `GestureBinding`, the arena, recognizers, the nested scroll channel, the DnD
session table, the haptics request queue, `InputTap` — is owner-local (`Rc`, `Cell`, `RefCell`,
`!Send`). Concurrency comes from *more owner threads* (one per UI runtime, ADR-0027's policy,
ADR-0091's H2 spike) and from the raster/worker lanes, never from splitting one runtime's input
path across threads.

1. **Arena and recognition stay on the owner thread.** Yes. Arena transitions commit before
   callbacks, and those callbacks re-enter the same arena (ADR-0159, ADR-0161); the callbacks
   are `!Send` closures pinned by trybuild; resolution needs a synchronous hit test against an
   `Rc<RefCell<PipelineOwner>>`. Every toolkit surveyed does gesture arbitration on its UI
   thread. Off-thread recognition exists only where it is *declarative* (Chromium's compositor
   and DirectManipulation take a pre-declared "scrollable/pannable" region, not user callbacks).
2. **Never split the platform thread from the owner thread for a given window.** Scale by giving
   each UI runtime its own thread that *is* the platform thread of its windows (Win32: create the
   HWND on the runtime's thread). A platform→owner hop would make every synchronous answer to the
   OS asynchronous — `default_prevented`, TSF pulls, DirectManipulation `SetContact`, accessibility
   queries, Windows input haptics — which is exactly what Flutter just undid on every platform
   (3.29 mobile, 3.35 macOS/Windows, 3.41 Linux). Two consequences to record: on macOS/iOS all
   runtimes share the main thread forever; on Linux, winit allows **one event loop per process**,
   so per-runtime owner threads there *would* be a split and should be dropped from ADR-0091's H2
   spike (Win32 only). Android already has a split we did not choose (`android-activity` runs
   `android_main` on its own thread) and shows the cost.
3. **Scroll, fling and DM inertia stay on the owner thread for 0.2.x.** Impl-side (compositor)
   scrolling is incompatible with `NestedScrollConnection` as specified: its synchronous
   `pre_scroll`/`post_scroll`/`post_fling` return values *are* the scroll-linked effect browsers
   cannot run off-thread. Chromium and Firefox handle such chains by falling back to the main
   thread or answering late; Windows' InteractionTracker replaces them with declarative chaining.
   Reserve a seam instead: scroll steps, snap and overscroll policy, axis claims and built-in fling
   curves stay plain `Send` data with no callbacks; offset changes go through the single offset
   primitive the spec already mandates; and a future `SceneSnapshot` scroll node plus a
   "main-thread scrolling reason" can admit *declarative-only* chains to a raster-side scroller
   later, additively.
4. **Multi-window/multi-runtime.** DnD between runtimes moves *data* (ADR-0038 formats, `Send`
   bytes), never the Rust value — answer Q35 that way regardless of thread topology. Pointer
   capture stays per window and works across Win32 threads only while a button is down, which
   matches the spec's "capture stays with A". Haptics are issued on the owner thread of the window
   that received the input (Windows routes `InputHapticsManager` per *thread*).
5. **Determinism** is promised per UI runtime, not across runtimes: each runtime's `HostInput`
   stream (with `EventTime`, arrival time, frames, AT requests and cross-runtime messages as
   entries) replays to identical arena decisions. A cross-runtime total order is not reproducible
   (Win32 orders input per thread queue) and must not be promised.
6. **Enforce with the compiler.** `assert_not_impl_any!` / trybuild `compile_fail` for the owner
   types (gap today: `GestureBinding` has no pin), `assert_impl_all!` for the data that must cross
   (`HostInput`, `InputSequenceId`, `ScrollStep`, `HapticRequest`, recording rows), and
   `compile_pass` fixtures proving that `Rc`-capturing implementors of `NestedScrollConnection`,
   `GestureRecognizer` and DnD callbacks are accepted.

## 1. What runs where today

### 1.1 Threads

| Thread | What runs there | Evidence |
|---|---|---|
| **Owner thread** (main thread on Win32, macOS, iOS, winit; `android_main` thread on Android; the JS thread on web) | Every `UiRuntime` (serially, ADR-0091 §1), `GestureBinding` and arena, focus, text input owner, frame pump, **and the raster lane inline** | One `thread_local!` `APP_RUNTIME` hosts all runtimes: `crates/flui-app/src/app/runner/host.rs:213`; inline lane: `crates/flui-app/src/app/raster_lane.rs:1-13`, `Arc<Mutex<RasterLane>>` at `crates/flui-app/src/app/runner/desktop.rs:222-233` |
| Raster thread | **Does not exist in production.** `RasterOwner::run_until_shutdown` (`crates/flui-engine/src/raster_owner.rs:1539`) has no caller outside its own `#[cfg(test)]` module (`:1714`) | `rg "run_until_shutdown\("` outside that file is empty |
| Android Java main thread | Activity, insets queries; input marshalled to the native thread | `crates/flui-platform/Cargo.toml:189` (`android-activity` 0.6, native-activity); `crates/flui-platform/ARCHITECTURE.md:55` ("safe-area geometry on the Java view thread"); [android-activity docs](https://docs.rs/android-activity/0.6.0/android_activity/): it will "spawn a dedicated thread to run your `android_main` function" |
| Win32 helper threads | Clipboard message-only owner window pump; COM apartment helper | `crates/flui-platform/src/platforms/windows/clipboard.rs:305`, `com_apartment.rs:136` |
| Font feed | `flui-host-fonts`, wakes the owner when done | `crates/flui-app/src/app/runtime.rs:1189-1191` |
| Workers / IO | `ExecutionServices` pools, hot-reload watcher; results return as owner commands | ADR-0027 §2–§4, ADR-0047 |
| DirectManipulation | Not implemented (`rg DirectManipulation crates/` is empty); the completeness spec plans it (§7) | — |

So the input stack has exactly one thread today. Concurrency between UI runtimes is a policy
ADR-0027 permits and ADR-0091 postpones to an H2 spike behind an `OwnerExecutor` trait.

### 1.2 Send and !Send

| `!Send` (pinned) | `Send` (pinned or by construction) | `Send` *by necessity of today's platform API* |
|---|---|---|
| `UiRuntime` (`crates/flui-runtime/src/ui_runtime/tests/mod.rs:9`), `PresentationState` (`crates/flui-runtime/src/presentation.rs:2088`), `OwnerHost` (`crates/flui-runtime/tests/contracts/owner_publication.rs:10`) | `PresentationAddress` (`crates/flui-app/src/app/window_registry.rs:151`) | `PlatformWindow: Send + Sync` (`crates/flui-platform-api/src/platform_window.rs:156`) |
| `GestureArena`, `GestureArenaEntry` (`crates/flui-interaction/src/arena/mod.rs:2179-2180`) | `InteractionDispatchHandle`, `PointerTarget`, `ResolvedRouteToken` (`crates/flui-interaction/tests/interaction_lane.rs:4240-4243`) | `on_input(Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>)` (`platform_window.rs:474`) — fires only on the owner, but the type does not say so |
| Recognizers, `RecognizerSet`, builders (trybuild: `recognizer_stays_on_its_thread.rs`, `recognizer_set_stays_on_its_thread.rs`, `builder_stays_on_its_thread.rs` in `crates/flui-interaction/tests/compile_fail/`) | `SceneSnapshot` (moves to raster, ADR-0027 §5), `Renderer: Send` (`crates/flui-engine/src/renderer.rs:490`) | `PlatformHaptics: Send + Sync` (`crates/flui-platform-api/src/haptics.rs:71`) |
| `InteractionLane` and its cells (`interaction_lane.rs:2703-2708`, tests `:4239`), `TextInputHandle`, `TextInputOwner` (`crates/flui-interaction/src/text_input.rs:1362-1363`) | `FrameWaker`, task `Waker` (ADR-0136 §2, ADR-0175) | The raster lane's `Arc<Mutex<…>>` exists only because callbacks must be `Send` (`desktop.rs:222-229` says so) |
| `OwnerPlatform`, `OwnerThreadToken` (`crates/flui-platform/src/traits/owner.rs:71`, `:212`; `host_window.rs:46`) | `SharedPlatform` (`owner.rs:404`) | |
| **Not pinned:** `GestureBinding` — its doc says it is owner-local (`crates/flui-interaction/src/binding.rs:466-469`) and it holds `Rc`/`RefCell` fields (`:472-519`), but no assertion fixes it | | |

The UI side already follows ADR-0027 §2. The remaining `Send` bounds are on the platform
callback surface, kept until every backend registers callbacks through owner-proof types
(ADR-0082 §4; `design/architecture.md` §8.2). Win32 already refuses off-owner registration
(`crates/flui-platform/ARCHITECTURE.md:504-530`).

### 1.3 How input flows

```text
OS (WndProc / NSView / android_main poll / DOM listener)          owner thread
  └─ PlatformWindow::on_input callback (Send-typed, owner-run)
       └─ owner_dispatch::dispatch_platform_input                 crates/flui-app/src/app/runner/owner_dispatch.rs:518
            └─ host.logical().presentation_dispatcher(addr).input(..)
                 └─ UiRuntime → GestureBinding → hit test (PipelineCell, Rc) → arena → recognizer callbacks
            ← InputOutcome::Unhandled? → DispatchEventResult { default_prevented }   :535
```

The answer to the OS is computed synchronously, inside the callback: an unhandled key keeps its
native default (Alt+F4, Alt+Space), a consumed one prevents it (`owner_dispatch.rs:507-535`). A
reentrant callback (nested Win32/AppKit pump) goes into the runtime-local FIFO (ADR-0027 §3).
Raster is inline on the same thread. Nothing in input crosses a thread.

## 2. Prior art

### Chromium (Blink + cc)

- Two threads per renderer: main (Blink) and compositor (impl). Multi-threaded cc exists so that
  "input and animations [are] responsively handled on one thread while the other thread is busy";
  the compositor can "handle pinch and scroll gestures responsively without involving Blink", and
  scroll deltas applied on the compositor are sent back to Blink with the next `BeginMainFrame`
  ([How cc works](https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md)).
  Commit moves main-thread state "atomically" to the pending tree; activation moves it to the
  active tree.
- Input arrives at `InputHandlerProxy` on the compositor; "some input can't be handled by the
  compositor thread" and goes to Blink (same doc). Events for the main thread are queued in
  `MainThreadEventQueue`, "a queue for events that need to be queued between the compositor and
  main threads", and continuous events are rAF-aligned
  ([`main_thread_event_queue.h`](https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/widget/input/main_thread_event_queue.h)).
  Chrome holds continuous input until "right before the next `requestAnimationFrame`" and exposes
  the merged points through `getCoalescedEvents`
  ([Inside look at modern web browser, part 4](https://developer.chrome.com/blog/inside-browser-part4)).
- **When the main thread is forced:** the compositor marks a "non-fast scrollable region" where
  handlers are attached; input there goes to the main thread first (part 4). The current reasons
  are enumerated in
  [`cc/input/main_thread_scrolling_reason.h`](https://chromium.googlesource.com/chromium/src/+/main/cc/input/main_thread_scrolling_reason.h):
  repaint reasons (`kHasBackgroundAttachmentFixedObjects`, `kNotOpaqueForTextAndLCDText`,
  `kBackgroundNeedsRepaintOnScroll`), hit-test reasons (`kScrollbarScrolling`,
  `kMainThreadScrollHitTestRegion`, `kFailedHitTest`), and `kWheelEventHandlerRegion` /
  `kTouchEventHandlerRegion` — scrolling "can be handled on the compositor thread but it might be
  blocked on the main thread" waiting to see whether a non-passive handler calls
  `preventDefault`. Passive listeners exist to remove that block (part 4).
- Fling is generated outside the renderer's main thread: `FlingController` is "used to progress an
  active fling on every begin frame" and emits `GestureScrollUpdate`/wheel events
  ([`components/input/fling_controller.h`](https://chromium.googlesource.com/chromium/src/+/main/components/input/fling_controller.h)).
- **Lesson:** threaded scrolling works because web content cannot run synchronous code that
  decides a scroll delta. Where it can (a non-passive listener), the browser either blocks on the
  main thread or ignores it. Scroll chaining across nested scrollers is declarative CSS
  (`overscroll-behavior`), not a callback.

### Firefox APZ (added as the clearest statement of the cost)

- APZ handles input before content: events "arrive from the hardware/widget code into the APZ";
  outside a dispatch-to-content region they are processed immediately
  ([AsyncPanZoom](https://firefox-source-docs.mozilla.org/gfx/AsyncPanZoom.html)).
- Inside that region APZ waits for content's `preventDefault` decision with a deadline of
  "400ms … on desktop, and 600ms on mobile"; if content is late, "APZ assumes preventDefault()
  will not be called" (same doc).
- Effects computed from scroll position on the main thread run late, so they are "laggy, janky,
  or jittery"; the remedy is to express them declaratively (`position: sticky`, scroll snap) so
  they "run on the compositor thread"
  ([Scroll-linked effects](https://firefox-source-docs.mozilla.org/performance/scroll-linked_effects.html)).
  A `NestedScrollConnection` is a scroll-linked effect with a return value.

### Flutter

- Four task runners: platform, UI ("where the engine executes all Dart code for the root
  isolate"), raster, IO. "Interacting with the Flutter engine in any way must happen on the
  platform thread", and on Android/iOS the platform thread pipes input, so "a blocked platform
  thread can also cause gestures to be dropped"
  ([Engine architecture](https://github.com/flutter/flutter/blob/main/docs/about/The-Engine-architecture.md)).
- Gestures (`GestureBinding`, arena, recognizers, `Scrollable`, ballistic simulations) are Dart,
  so they run on the UI thread; there is no raster-side scrolling.
- **Flutter merged the platform and UI threads.** 3.29: "Flutter on Android and iOS execute Dart
  code on the application's main thread … there is no longer a separate UI thread"; the split had
  made "most platform interop require serialized and asynchronous message passing"
  ([What's new in 3.29](https://flutter.dev/blog/whats-new-in-flutter-3-29)). 3.35 did the same on
  macOS and Windows because "the split-thread design prevented Flutter apps and plugins from using
  Dart FFI to interoperate with native APIs that must be called on the platform thread"
  ([breaking change](https://docs.flutter.dev/release/breaking-changes/macos-windows-merged-threads));
  Linux followed in 3.41 ([index](https://docs.flutter.dev/release/breaking-changes)). The raster
  thread stays separate.

### Android (View system)

- The main thread "is in charge of dispatching events to the appropriate user interface widgets,
  including drawing events"; "the Android UI toolkit is *not* thread-safe", with two rules: don't
  block the UI thread, don't touch the toolkit from another thread
  ([Processes and threads](https://developer.android.com/guide/components/processes-and-threads)).
- `InputReader` decodes device events and "sends input events to the InputDispatcher which forwards
  them to the appropriate window" ([Input](https://source.android.com/docs/core/interaction/input));
  an app that has not handled an input event within 5 seconds gets an ANR
  ([ANRs](https://developer.android.com/topic/performance/vitals/anr)).
- Rendering is two phases: "Record View#draw on the UI thread" and "DrawFrame on the
  `RenderThread`"; "the UI thread is blocked while the `syncFrameState` of the `RenderThread` is
  running" ([Slow rendering](https://developer.android.com/topic/performance/vitals/render)).
  Scrolling and flinging (`OverScroller`, `RecyclerView`) are driven by the UI thread; RenderThread
  replays recorded display lists.

### iOS / UIKit and Core Animation

- Apple's split is *process*-level: the app runs an event phase ("touches, networking callbacks,
  keyboard presses and timers") and a commit phase on its side, then "the next stage happens in a
  separate process called the render server"
  ([Explore UI animation hitches and the render loop](https://developer.apple.com/videos/play/tech-talks/10855/)).
  Committed Core Animation animations are performed by the render tree, "private to Core
  Animation", in hardware
  ([Core Animation basics](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CoreAnimation_guide/CoreAnimationBasics/CoreAnimationBasics.html)).
- Event handling is main-thread: for AppKit, "the main thread of the application is responsible
  for handling events … if two different threads are responding to key events, the keys could be
  received out of order", and `NSView` must be used "only from the main thread"
  ([Thread safety summary](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html)).
  UIKit's responder and gesture-recognizer types are main-actor-isolated (Apple's documentation
  pages are script-rendered and could not be quoted here; the rule is the one AppKit states).
- Gesture recognizers, `UIScrollView` tracking and deceleration run in the app on the main thread;
  only already-committed animations survive a blocked main thread. (Deceleration being main-run-loop
  driven is observed behaviour, not a quoted contract.)

### Jetpack Compose

- "Compose operates on the main thread, but it has been designed from the start with
  multithreading in mind"; composables "cannot currently be run in parallel", and side effects
  belong in "callbacks such as `onClick` that always execute on the UI thread"
  ([Thinking in Compose](https://developer.android.com/develop/ui/compose/mental-model)).
- Pointer input, `NestedScrollConnection`, `Scrollable` and fling all run on the main thread;
  Compose has no RenderThread-side scrolling. FLUI's channel is modelled on Compose's, so it
  inherits that placement.

### GPUI (Zed)

- `ForegroundExecutor` carries `not_send: PhantomData<Rc<()>>` and its `spawn` takes
  `impl Future + 'static` with no `Send`; `BackgroundExecutor::spawn` requires `Send + 'static`
  (`crates/gpui/src/executor.rs:27-32`, `:103-105`, `:354-360` @ 26363aab).
- `App` is `Rc<AppCell>` (`crates/gpui/src/app.rs:84`, `:148`); the platform window's
  `on_input` takes `Box<dyn FnMut(PlatformInput) -> DispatchEventResult>` **without `Send`**
  (`crates/gpui/src/platform.rs:1258`), and the window wires it to `window.dispatch_event`
  through `handle.update(..).log_err()` (`crates/gpui/src/window.rs:2041-2048`), so an event that
  arrives while the window is already being updated is logged and dropped rather than queued.
- **Lesson:** the shape FLUI's ADR-0027 chose, minus the `Send` on the platform callback, plus a
  reentrancy queue GPUI lacks.

### Xilem / Masonry

- `RenderRoot::handle_pointer_event(&mut self, event: PointerEvent) -> Handled` and
  `redraw(&mut self)` (`masonry_core/src/app/render_root.rs:548`, `:613` @ 271a27a6): one owner,
  `&mut`, synchronous.
- Async work talks back through `MessageProxy<M: AnyDebug + Send>` over a `RawProxy: Send + Sync`
  (`xilem_core/src/message_proxy.rs:29`, `:60`): `Send` messages in, owner-local tree out — the
  `SignalSender`/`FrameWaker` shape.

### Bevy

- Input systems run in `PreUpdate` inside the `InputSystems` set
  (`crates/bevy_input/src/lib.rs:106-165` @ 12a94e0c); picking produces pointer input in `First`
  and processes hover, hits and events in ordered `PreUpdate` sets
  (`crates/bevy_picking/src/lib.rs:260-276`, `:381-396`). Systems run in parallel only where the
  schedule proves no conflict; UI input is an ordered chain of sets, so the parallelism does not
  reach arbitration.
- Input is batched per app update, so latency is quantized to the update; rendering is pipelined
  onto a separate render thread (`crates/bevy_render/src/pipelined_rendering.rs`, "render thread"
  span at `:185`).
- Thread-affine state is a `NonSend` resource in a `Send` world, which Bevy's own tracker wants to
  eliminate ([#17517](https://github.com/bevyengine/bevy/issues/17517), "Eliminate `NonSend`
  resources by making `World` optionally `!Send`"). ADR-0027 already rejected this pattern.

### Windows: DirectManipulation and InteractionTracker

- "Direct Manipulation processing occurs on a separate, independent thread from the UI thread. As
  a result, output transforms can run in parallel to activity on the UI thread"; the app calls
  `SetContact` when it receives `WM_POINTERDOWN`, and DM reports consumption with
  `WM_POINTERCAPTURECHANGED`
  ([Direct Manipulation](https://learn.microsoft.com/en-us/windows/win32/directmanipulation/direct-manipulation-portal)).
  Independent motion requires DM to drive DirectComposition visuals through
  `DCompDirectManipulationCompositor`; FLUI renders through wgpu, so it would use DM's
  manual-update mode and apply transforms on its own frame, as Chromium and Flutter do (BP §2.1).
- InteractionTracker exists because manipulation performance "is dependent on the UI thread"; it
  runs on an animation engine "that operates on an independent thread", and nested behaviour is
  declared up front as "Rails & Chaining" and modifiers
  ([InteractionTracker](https://learn.microsoft.com/en-us/windows/apps/develop/composition/interaction-tracker-manipulations)).

### Synthesis

| System | Recognition / arbitration | Scroll/fling | Off-thread scroll? | Nested scroll |
|---|---|---|---|---|
| Chromium | main (JS), compositor for plain scroll | compositor + browser fling | yes, with main-thread reasons | declarative (`overscroll-behavior`) |
| Firefox | main | APZ (controller thread) | yes, 400/600 ms content deadline | APZ handoff |
| Flutter | UI = platform thread (since 3.29/3.35/3.41) | UI | no | Dart callbacks |
| Android Views | UI | UI | no | `NestedScrollingParent3` callbacks on UI |
| UIKit | main | main | no (render server only runs committed animations) | delegate callbacks on main |
| Compose | main | main | no | `NestedScrollConnection` on main |
| GPUI, Masonry | owner | owner | no | — |
| Bevy | ordered systems | — | — | — |
| WinUI InteractionTracker / DM | UI hit test, then system thread | system thread | yes | declarative chaining |

Every system that keeps nested scroll as user callbacks runs scrolling on the UI thread; every
system that scrolls off-thread makes chaining declarative or answers late.

## 3. Answers

### Q1. Gesture recognition and the arena stay on the owner thread

Yes, and the reasons are structural:

- **Reentrancy is the contract.** "Every state transition finishes before member callbacks or user
  destructors run, so either can reenter the same arena without an outstanding storage borrow"
  (ADR-0159). Recognizer callbacks, `cancel()`, last-owner drops and deferred resolutions all
  re-enter on the same stack (ADR-0161). Across threads, "re-enter" would become "race", and the
  first-failure custody of ADR-0127/0178/0182 assumes one stack.
- **Callbacks are `!Send` by design.** Recognizers, builders and `RecognizerSet` are `!Send + !Sync`
  (ADR-0161, trybuild fixtures above); widgets capture `Rc` state and a `WriterSource` (send-flip
  §c). Making them `Send` reverses ADR-0136/0175 and pushes `Arc<Mutex<_>>` into every callback.
- **Arbitration reads the render tree.** Hit testing walks `PipelineCell = Rc<RefCell<PipelineOwner>>`
  (ADR-0091 §2), and `PanClaim`, `allowed_devices` and `InputRegions` come from layout
  (completeness §4, §6). A second thread would need a snapshot of hit-test geometry, which is
  Chromium's non-fast-scrollable-region machinery.
- **The OS wants an answer now.** `default_prevented` is returned from inside the callback
  (`owner_dispatch.rs:535`); so is the DM `SetContact` decision and the web `preventDefault`.
- **Cost of being wrong is bounded.** A recognizer callback that blocks stalls its runtime only,
  which is the same failure mode as Compose, Flutter, UIKit and Android (ANR at 5 s).

### Q2. Separating the platform event thread from the UI runtime thread

**Recommendation: no split per window; multiple owner threads where the OS allows, each one the
platform thread of its windows.**

| Platform | Can the window's event thread differ from the main thread? | What FLUI should do |
|---|---|---|
| macOS | AppKit: events on the main thread; `NSView` main-thread only (Apple thread-safety summary) | All runtimes on the main thread, forever |
| iOS | UIKit main-thread | Same |
| Win32 | Yes: "a thread's message queue receives all mouse and keyboard messages for the windows created by the thread"; apps "with multiple threads can include a message loop in each thread that creates a window" ([Messages and message queues](https://learn.microsoft.com/en-us/windows/win32/winmsg/about-messages-and-message-queues)) | Per-runtime owner thread that **creates its own HWNDs** — platform thread = owner thread per window. This is ADR-0091's H2 spike |
| Linux (winit) | winit: "the EventLoop must be created on the main thread, and only once per application", "constructing multiple event loops is not supported" ([winit `EventLoopBuilder`](https://docs.rs/winit/0.30.12/winit/event_loop/struct.EventLoopBuilder.html)); `with_any_thread` moves the one loop, it does not add loops | One owner thread. A second runtime thread would be a platform→owner split; drop Linux from the H2 spike |
| Android | Already split: Java main thread vs `android_main` (`android-activity`) | Keep the owner on `android_main`; treat every Java-side query as async (as safe-area does today) |
| Web | One JS thread | One owner |

Implications of a split, if anyone proposes it:

- **`HostInput` becomes a cross-thread message.** It is data and can be `Send` (§Q6), but every
  synchronous result the platform needs — `DispatchEventResult` (`owner_dispatch.rs:518-536`), the
  Win32 keyboard default rule (`crates/flui-platform/ARCHITECTURE.md:1637`), TSF pulls that must
  hold the store on the owner (ADR-0135), AccessKit action and tree queries, DM `SetContact` on
  `WM_POINTERDOWN`, web `preventDefault` — would need a block-with-deadline (APZ's 400 ms) or a
  declarative pre-answer (Chromium's region map). That is the interop tax Flutter removed.
- **Input timeline.** `EventTime` comes from the event (QPC on Win32, completeness §6), so a hop
  does not corrupt timestamps. It adds a queueing stage where coalescing and ordering must be
  re-proved (ADR-0027 §4: input is "causal order, never coalesced or reordered"), and makes
  arrival time a second clock to record.
- **Latency.** A hop costs a cross-thread wake per event batch. The benefit — the OS sees a
  responsive window while the runtime is blocked (no "ghost window" after "several seconds",
  same Microsoft page) — is real only for move/resize/close, and those already need the owner
  for layout and the close protocol (ADR-0027 §7).
- **Haptics** routed per thread (Q4) would break if input arrives on one thread and the request
  is made on another.

Per-runtime owner threads on Win32 have their own price, listed in Q4: cross-thread capture,
per-thread focus and key state, and cross-thread OLE drag.

### Q3. Scroll, fling and DirectManipulation inertia off the owner thread?

**Recommendation: keep them on the owner thread through 0.2.x; reserve the seam below.**

What impl-side scrolling would need in FLUI, and what it would cost:

1. **A raster/compositor thread that owns scroll offsets.** None runs in production (§1.1), and
   `flui-layer` has no scroll node: offsets are baked into transforms/paint by the owner each frame.
   Chromium's model needs a property tree with scroll nodes, a commit that reconciles owner-side
   and impl-side offsets, and deltas returned with the next main frame.
2. **A hit-test snapshot on that thread** (non-fast-scrollable regions) and a list of
   "main-thread scrolling reasons" deciding which chains qualify.
3. **The nested scroll channel becomes two APIs.** `pre_scroll`, `post_scroll` and `post_fling`
   return consumed deltas synchronously (completeness §5). A user `Rc` implementor cannot run on
   the compositor thread (`!Send`, and it reads/writes widget state). The options are those of the
   browsers: (a) block the impl thread on the owner every step — no gain; (b) apply the scroll
   first and tell the connection later — the "laggy, janky, or jittery" scroll-linked effect, and
   it breaks `FlingHandoff`, `ScrollEndReason`, `Preempted` semantics; (c) only declarative chains
   (`OverscrollBehavior`, `SnapPolicy`, `AxisLock`, `PanClaim`) run impl-side and any user
   connection forces owner scrolling. Option (c) is the only coherent one, and it is additive.
4. **DM inertia.** DM computes inertia on its own thread regardless; FLUI consumes the viewport
   transform on its frame in manual-update mode (Chromium and Flutter do the same; BP §2.1). A
   blocked owner freezes the visible scroll even though DM's state advances; when the owner
   resumes, the next frame jumps to DM's current transform. Independent DM requires DComp visuals,
   which FLUI does not use.

**Seam to reserve now (each item costs nothing today and keeps option (c) open):**

- `ScrollStep`, `FlingStep`, `ScrollCause`, `ScrollDevice`, `ScrollEndReason`, `SnapPolicy`,
  `OverscrollBehavior`, `PanClaim`, `AxisLock`, `InputSequenceId`: plain `Send + 'static` data, no
  callbacks, no `Rc`. Pin with `assert_impl_all!`.
- Built-in fling and snap curves as concrete simulation types that stay `Send` even though the
  `Simulation` trait has no `Send` supertrait (`crates/flui-animation/src/simulation.rs:274`).
- All offset changes go through the one dispatcher the spec mandates ("один примитив смещения",
  completeness §5). That dispatcher is the place a future "is this chain impl-eligible" check and
  a `SceneSnapshot` scroll node would hook in.
- Do not expose scroll offsets as something layout or paint read from a shared cell; they stay
  the dispatcher's output for this frame.
- Record the decision in the ADR that supersedes ADR-0169: scrolling is owner-driven; an
  impl-side scroller, if ever built, admits only chains with no user connection, and reports a
  typed main-thread reason otherwise.

### Q4. Multi-window and multi-runtime

ADR-0027 makes one runtime per window the desktop default; ADR-0091 hosts them on one thread
through H2. Design for the threaded case now, so behaviour does not change when the topology does.

- **DnD between runtimes** (spec Q35). The Rust value lives in its source runtime's session table
  (`!Send`, `Rc`); another runtime, even on the same thread, receives **only the ADR-0038 data
  transfer** (typed formats, `Send` bytes) plus the session identity (nonce, `UiRuntimeId`,
  `DragSessionId`, completeness §9) for "same process" detection. Win32 makes this mandatory when
  threaded: `DoDragDrop` runs a loop that calls `IDropTarget` methods on the target window
  ([DoDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-dodragdrop)), and
  the registering thread "must be pumping messages … otherwise any application that drags an
  object over the window … will hang"
  ([RegisterDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-registerdragdrop)).
  A target runtime busy in a frame stalls the source's drag loop, so target callbacks must answer
  from owner-local state without waiting on the source runtime. Promising cross-runtime value
  transfer would require a `Send` payload bound on `DragItem` that a single-runtime drag does not
  need.
- **Pointer capture.** `SetCapture` sets capture "to the specified window belonging to the current
  thread"; over "a window created by another thread, the system will direct mouse input to the
  specified window only if a mouse button is down"
  ([SetCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture)).
  A drag from window A over window B therefore stays with A while the button is down, on one or
  many threads — the spec's Q2 row. Hover capture without a button does not cross threads, so
  `MouseTracker` and wheel latch must stay keyed by `InputSurface` (completeness §3), never by a
  process-wide "current pointer".
- **Focus and key state** are per thread on Win32: `AttachThreadInput` exists to "share … input
  states (such as keyboard states and the current focus window)", and events of attached threads
  are processed "in the order they were received" only while attached
  ([AttachThreadInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-attachthreadinput)).
  FLUI's focus is per runtime already (ADR-0027 §8). Do not use `AttachThreadInput` to fake a
  shared state; it also resets key state.
- **Haptics.** Windows input haptics route "to the device that most recently sent input to the
  thread, so it must run on the UI thread" (FU §1, `InputHapticsManager.GetForCurrentThread`).
  Under per-runtime threads that is correct by construction if the request runs on the owner of
  the window that received the input; it breaks under a platform→owner split. UIKit/AppKit
  generators are main-thread, which is the owner there. `HapticTiming::FramePresented` depends on
  a raster ack; once raster is threaded that ack crosses threads on the lossy telemetry lane
  (ADR-0027 §4), so the haptic queue's deadline → `Suppressed` (completeness §8) is the delivery
  guarantee, not the ack. `PlatformHaptics: Send + Sync` (`haptics.rs:71`) can stay a service
  type, but its Win32 backend must be owner-affine.
- **`thread_local!` lane registries** (`LOCAL_LANES`, `crates/flui-interaction/src/routing/interaction_lane.rs:997`)
  are thread-scoped, not runtime-scoped. They stay correct with one runtime per thread and with
  several runtimes per thread only if keyed by lane identity; keep them on ADR-0097's removal list.

### Q5. Recording and replay determinism

- **Unit of determinism: one UI runtime.** Its `HostInput` stream (platform input, window events,
  frames, AT requests) with `EventTime`, arrival time and dispatch result is self-contained
  (completeness §1, §10). Two replays give the same arena decisions once the arena stops reading
  wall time (`web_time::Instant` in `crates/flui-interaction/src/arena/mod.rs:73`,
  `binding.rs:1376`; completeness §1 already schedules the move to `EventTime`). ADR-0159 already
  fixes deadline polling order independent of storage layout.
- **Everything that enters a runtime from another thread is a recorded entry, at its commit
  point:** worker results and `SignalSender` writes commit only at Idle anchors (ADR-0027 §3);
  record them there. Raster acks that drive pacing or `FramePresented` haptics enter as `Frame`
  entries. Cross-runtime effects (DnD enter/drop, close requests, activation changes) are
  `HostInput` entries at the receiver. With these, threads do not affect replay of one runtime.
- **No cross-runtime total order.** Win32 posts each input to "the message queue of the thread
  that created the destination window", and only attached threads see input "in the order they
  were received" (pages above). Two runtimes on two threads have no reproducible interleaving; a
  recording tool that merges streams uses arrival timestamps for display only.
- **Recording must not add a lock to the frame path.** `InputTap` writes one stream per runtime on
  its owner thread; serialization and file IO go to a worker as owned bytes. No shared
  `Arc<Mutex<Writer>>` across runtimes.
- **Headless:** one virtual clock per runtime (ADR-0176's presentation clocks), already the
  harness shape (`design/architecture.md` §8.3).

### Q6. What must be `Send`, what must not, and how the compiler enforces it

| Must be `!Send + !Sync` | Must be `Send` (data or wake capability) |
|---|---|
| `GestureBinding` (**add the pin**), `GestureArena`, entries, every recognizer and builder, `RecognizerSet` | `HostInput` and its variants, `PlatformInput`/`PointerEvent` data, `InputSequenceId`, `InputSurface`, `DeviceId` |
| `NestedScrollDispatcher`, the per-sequence chain snapshot, scroll endpoints (`Rc`/`Cell`, completeness §5) | `ScrollStep`, `FlingStep`, `ScrollCause`, `ScrollDevice`, `ScrollEndReason`, `FlingHandoff`, `SnapPolicy`, `OverscrollBehavior`, `PanClaim`, `AxisLock` |
| DnD session table and `DragSource`/`DropTarget` callbacks | `DragSessionId`, the session identity triple, ADR-0038 transfer payloads |
| Haptics request queue and observer delivery | `HapticRequest`, `HapticPattern` and parts, `HapticOutcome`, `HapticCapabilities` |
| `InputTap`, `GestureObserver` delivery, `MouseTracker`, `PointerSequences` services | `InputRecording` rows/DTOs, `PointerScript` output |
| `TextInputOwner`/`Handle` (pinned), `InteractionLane` (pinned) | `FrameWaker`, `InteractionDispatchHandle` (pinned), `PresentationAddress` (pinned) |

Traits whose implementors capture `Rc` must carry **no** `Send` supertrait or bound:
`NestedScrollConnection`, `GestureRecognizer`/`GestureArenaMember` (ADR-0161), DnD source/target
callbacks, `GestureObserver`.

Mechanisms, in the order AGENTS.md prefers them:

1. **Types first.** An owner-local type is `!Send` because it holds `Rc`/`Cell`; when it is
   otherwise plain data (a token, an id-only handle that must not leave the owner), add
   `PhantomData<Rc<()>>` (GPUI's `not_send`) or `PhantomData<*const ()>` (FLUI's
   `OwnerThreadToken`, `crates/flui-platform/src/traits/host_window.rs:46`). Never
   `unsafe impl Send` for UI types (send-flip R5; `deny.toml` bans `send_wrapper`-style crates in
   that plan).
2. **Static assertions in `tests/`** (public API, per "Writing tests"):
   `static_assertions::assert_not_impl_any!(GestureBinding: Send, Sync);` next to the existing
   arena pin, and `assert_impl_all!(HostInput: Send, 'static)`-style rows for the data column. One
   table file per crate, joined to `tests/main.rs`.
3. **trybuild `compile_fail`** for the user-visible owner types, in the existing
   `crates/flui-interaction/tests/compile_fail/` group (already in the nextest trybuild filter,
   `.config/nextest.toml:59`): `gesture_binding_stays_on_its_thread`,
   `nested_scroll_dispatcher_stays_on_its_thread` (in `flui-widgets`' routable/compile-fail
   group), `drag_session_stays_on_its_thread`. The `.stderr` names the `Rc` field, which is the
   point.
4. **`compile_pass` consumer rows** proving the negative bound is *absent*: an
   `Rc<Cell<f64>>`-capturing `NestedScrollConnection`, recognizer and drop target compile through
   `flui` and `flui-sdk` (send-flip R2's `owner_local_values_fit_every_flip_position`). This is the
   test that fails if someone re-adds `+ Send` to a trait.
5. **Platform callbacks.** When ADR-0082 §4 step 2 lands owner-proof registration, drop `+ Send`
   from `on_input` (`platform_window.rs:474`) as GPUI does; the raster lane's
   `Arc<Mutex<RasterLane>>` then becomes owner-local on inline platforms (ADR-0091 consequences).
6. **Ledger, if adopted.** The `cargo xtask thread-boundary` rustdoc-JSON gate proposed in
   send-flip (d1) and ADR-0136 (unadopted) is the only check that sees *every* `Send` position;
   until it exists, items 2–4 are the enforcement. Clippy's `arc_with_non_send_sync` catches the
   reverse mistake (an `Arc` around owner state).

## 4. Do not do (keeps the later options open)

- Do not give `HostInput`, scroll steps or haptic requests a field that holds `Rc`, a callback or a
  borrowed render object; they are the data that may cross a thread later.
- Do not add a `Send` bound to `NestedScrollConnection`, `GestureRecognizer`, DnD or observer
  traits "for the future"; that is a breaking reversal of ADR-0175 for a scroller that would not
  call them anyway (Q3 (c)).
- Do not key pointer, latch, capture or hover state by anything process-wide; key by
  `InputSurface` and `DeviceId`.
- Do not let the arena or recognizers read wall time; read `EventTime`.
- Do not design cross-runtime DnD around moving the Rust value.

## 5. Open, unverified on this host

- Per-runtime Win32 owner threads are unmeasured (ADR-0091's H2 spike: "with one window's UI
  runtime blocked, another window stays within its frame budget"). The cross-thread OLE drag and
  capture rows above are documentation-derived, not run.
- UIKit's main-actor annotations and `UIScrollView` deceleration placement could not be quoted
  from Apple's script-rendered reference pages; the AppKit archive statement is the quoted source.
- Whether DM's manual-update transform should be pulled at frame start or pushed by its
  callback is a backend detail for the DM task (completeness §7); it does not change the thread.
- Win32 `InputHapticsManager` per-thread routing is quoted from FU §1; no device run.

## Sources

Primary documents and code read for this note:

- Chromium: [How cc works](https://chromium.googlesource.com/chromium/src/+/main/docs/how_cc_works.md);
  [`main_thread_scrolling_reason.h`](https://chromium.googlesource.com/chromium/src/+/main/cc/input/main_thread_scrolling_reason.h);
  [`main_thread_event_queue.h`](https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/widget/input/main_thread_event_queue.h);
  [`fling_controller.h`](https://chromium.googlesource.com/chromium/src/+/main/components/input/fling_controller.h);
  [Inside look at modern web browser, part 4](https://developer.chrome.com/blog/inside-browser-part4).
- Firefox: [AsyncPanZoom](https://firefox-source-docs.mozilla.org/gfx/AsyncPanZoom.html);
  [Scroll-linked effects](https://firefox-source-docs.mozilla.org/performance/scroll-linked_effects.html).
- Flutter: [Engine architecture](https://github.com/flutter/flutter/blob/main/docs/about/The-Engine-architecture.md);
  [What's new in 3.29](https://flutter.dev/blog/whats-new-in-flutter-3-29);
  [Merged threads on macOS and Windows](https://docs.flutter.dev/release/breaking-changes/macos-windows-merged-threads);
  [Breaking changes index](https://docs.flutter.dev/release/breaking-changes).
- Android: [Processes and threads](https://developer.android.com/guide/components/processes-and-threads);
  [Input](https://source.android.com/docs/core/interaction/input);
  [ANRs](https://developer.android.com/topic/performance/vitals/anr);
  [Slow rendering](https://developer.android.com/topic/performance/vitals/render);
  [android-activity](https://docs.rs/android-activity/0.6.0/android_activity/).
- Apple: [Thread safety summary](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html);
  [Core Animation basics](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CoreAnimation_guide/CoreAnimationBasics/CoreAnimationBasics.html);
  [Explore UI animation hitches and the render loop](https://developer.apple.com/videos/play/tech-talks/10855/).
- Compose: [Thinking in Compose](https://developer.android.com/develop/ui/compose/mental-model).
- GPUI (`zed-industries/zed@26363aab`): `crates/gpui/src/executor.rs`, `app.rs`, `platform.rs`, `window.rs`.
- Xilem (`linebender/xilem@271a27a6`): `masonry_core/src/app/render_root.rs`, `xilem_core/src/message_proxy.rs`.
- Bevy (`bevyengine/bevy@12a94e0c`): `crates/bevy_input/src/lib.rs`, `crates/bevy_picking/src/lib.rs`,
  `crates/bevy_render/src/pipelined_rendering.rs`; [#17517](https://github.com/bevyengine/bevy/issues/17517).
- Windows: [Direct Manipulation](https://learn.microsoft.com/en-us/windows/win32/directmanipulation/direct-manipulation-portal);
  [InteractionTracker](https://learn.microsoft.com/en-us/windows/apps/develop/composition/interaction-tracker-manipulations);
  [Messages and message queues](https://learn.microsoft.com/en-us/windows/win32/winmsg/about-messages-and-message-queues);
  [SetCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture);
  [AttachThreadInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-attachthreadinput);
  [DoDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-dodragdrop);
  [RegisterDragDrop](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-registerdragdrop).
- winit: [`EventLoopBuilder`](https://docs.rs/winit/0.30.12/winit/event_loop/struct.EventLoopBuilder.html).
