# flui-native Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

The contracts this crate's backends implement — `PlatformWindow`,
`PlatformTextInput`, `PlatformHaptics`, `Clipboard`, the display snapshot
vocabulary (`DisplaySnapshot`, `DisplayId`, `RefreshPeriod`), the
`data_transfer` transport and the window and input vocabulary — live in
`flui-platform` (ADR-0082), and this crate does not re-export them: each item
has one path (ADR-0151 §9), so a host and `flui-app` import them from
`flui_platform`. What stays here is what names an OS stack, AccessKit or tokio:
`Platform`, the owner-thread capability, `HostWindow` and every host.
`PlatformAccessibility`, the trait the hosts' AccessKit adapters implement, is
`flui-semantics`' (ADR-0082 §2, amended) and is imported from
`flui_semantics::platform`. Only `flui-app` may
depend on this crate (`allowed-dependents`); a crate that needs a capability
trait depends on `flui-platform` instead.

## Invariants

- **The crate is laid out by feature, and its module graph is checked
  (ADR-0151 §10).** `src/` holds, at its top, the crate root (`lib.rs`, which
  picks a host), the table (`host.rs`), `Platform` and its one implementation
  (`platform.rs`), `HostWindow` (`host_window.rs`), the error vocabulary
  (`error.rs`), the table-test runner (`table_test.rs`) and, until `Platform`
  loses its executor, `executor.rs` and `task.rs`. Its directories are the
  tables (`hosts/`), the owner thread and each platform's loop (`owner/`), the
  OS versions (`version/`), the storage position's vocabulary (`storage/`) and
  one directory per feature of the closed list: `window`, `input`, `text`,
  `display`, `clipboard`, `drag`, `accessibility`, `haptics`, `preferences`,
  `lifecycle`. There is no directory of code shared across features and no
  directory per platform: what two platforms of a feature share is that
  feature's model, and each platform is a file (or, where one OS object or the
  3 000-line limit needs it, a directory) in every feature it fills, named in
  std's words (`windows`, `apple` with `macos`/`ios`, `android`, `web`,
  `linux` with `wayland`/`x11`, `winit`, `headless`). A public name a host
  offers beyond `Platform` (a window, the headless test controls, the UIKit
  scene events) is named at the crate root, except one OS's window
  extensions, which are named in that OS's module of `os` (`os::windows`,
  `os::macos`), the modules the facade publishes as `flui::native::<os>`.
  `[package.metadata.flui.modules]` gives the roles `cargo xtask module-dag`
  checks: the ten features; `owner`, `version` and `storage` as `base`, which
  names no feature; `hosts`, which nothing names; and the crate-level files
  (`crate-level`: `host`, `platform`, `host_window`, `error`, `executor`,
  `task`, `os`). A top-level module with none of these roles is a finding, so a
  directory the layout does not name cannot come back unchecked
  (`table_test`, built for tests only, needs none). A platform file names its
  model, a base and other features' models; every other edge is a
  named, dated exception whose `reason` says what removes it, and one is
  left: the winit host's displays on Linux reading `display::linux` (until
  the native Linux host, ADR-0151 §11). No window file names another
  feature's: a window's text input, accessibility bridge, actuator and
  chrome reach it through the host table; a feature keeps its own state of a
  window, keyed by the window's identity and set up and dropped by its own
  window hooks; a window reads a display, preference or lifecycle fact
  through an owner hook; and the features that answer one AppKit or UIKit
  view join it through the owner's hooks.
- **A host is a table, and `Platform` has one implementation (ADR-0151 §10).**
  `host::Host` names one type for each position of the closed list (loop,
  window, displays, input, text, accessibility, clipboard, drag, haptics,
  preferences, lifecycle, chrome, storage), each bounded by its position's
  trait; `hosts/<os>.rs` holds the table and its constructor and nothing else,
  and a table that leaves a position out does not compile
  (`tests/ui/host_table_names_every_position.rs`). A position a platform does
  not have is `Absent<F>`, which answers in that position's words for "not
  here" (no drag offered, `Unsupported { NotOnThisPlatform }`, unknown
  preferences); no position or service method falls back to an inert default
  (the clipboard's `write` and `read` derive from its `clipboard`).
  `HostPlatform<H>` implements `Platform` by asking the position that holds
  each method; the application-level positions are static functions over the
  host's loop (`O`), which owns what the operating system hands out once per
  process, and the window-level positions (text, accessibility, haptics,
  chrome) are what each window hands out. Each makes one window's part as the
  window opens, `for_window(&Ref)`, from what the window position says its
  features reach a window by (`host::Window::Ref`): owner vocabulary any
  feature may name (`owner::windows::WindowRef`,
  `owner::apple::macos::ContentViewRef`, `owner::apple::ios::ViewRef`,
  `owner::android::ActivityRef`, `owner::web::CanvasRef`, the winit window,
  `()` headless). `HostPlatform` builds the table's `host::WindowPositions`, one
  function per position, and hands it to the window position's `Shared`, so a
  window cannot make another type than its table names. The positions make a
  window's four parts together (`WindowPositions::for_window`, one
  `host::WindowFacets` the window keeps as one value). The window keeps what
  they made, hands it out through `PlatformWindow::{text_input, haptics,
  system_chrome}` and `HostWindow::{accessibility, text_store_host,
  capture_text_sizing}`, and tells the text input and the bridge when it goes
  (`TextInput::detach`, `WindowAccessibility::detach`); a window whose table
  leaves a position out hands out that position's `Absent`. The public
  names (`WindowsPlatform`, `HeadlessPlatform`, …) are `HostPlatform` over each
  table, and what a host offers beyond `Platform` (headless test controls, a
  scene registration) is an inherent method on `HostPlatform<H>` for the `H`
  whose loop it is. The executor, the application path, URL and path opening,
  the file prompts and the compositor name are no position: the loop carries
  them (`host::Services`) until the system capability crate takes them.
  The Android host has no public platform name: it starts over the Activity
  the system hands only to `android_main`, through `android_platform(app)`.
- **No public path names an OS type (ADR-0151 §8).** The crate's public API,
  hidden items included, names from outside the workspace only the crates of
  the `allow` list in `[package.metadata.flui.api-closure]` (the cursor, the
  callback lock, the raw handles, the pointer assembly's buffers and the web's
  `Instant`), on every target it builds a host for: `cargo xtask api-closure`
  documents it for Linux, Windows and macOS with every feature and for iOS,
  Android and wasm32. A list of what may be named fails closed: an OS binding
  a dependency brings is a finding without anyone listing it. The windows a
  host builds are crate-private: the platform hands out `Arc<dyn HostWindow>`,
  a raw window or display handle leaves through `raw-window-handle`, and what
  one OS offers beyond the window contract is an extension implemented for
  `dyn HostWindow` (`os::windows::WindowsWindowExt::win32`,
  `os::macos::MacOSWindowExt::appkit`) over
  `HostWindow::as_any`, whose one body every window gets through the seal: it
  answers `None` for another host's window, and otherwise a view of the OS's
  window (`Win32Window`, `AppKitWindow`) whose commands cannot meet another
  host's. The facade publishes the `os` modules, through `flui_app::native`,
  as `flui::native::<os>` behind its `native` feature (ADR-0254 §8), and
  `flui_app::native::with_window` reaches the window a presentation is drawn
  in. A view's method does what it says or reports that it cannot. A
  `Win32Window` command or query first checks that its window still owns the
  handle and that the call is on the window's owner thread
  (`Unsupported` with `NoWindow`, `NotOnTheOwnerThread`), and a refusal of the
  desktop window manager is `Refused` with its `HRESULT`; each attribute is a
  constant that pairs its ordinal with its value's type, and each mapping is
  one table read both ways. The AppKit backdrop (`AppKitWindow::set_backdrop`)
  is an `NSVisualEffectView` material or Liquid Glass, an
  `NSGlassEffectView` looked up by name (macOS 26; `Unsupported` with `NotOnThisDevice` before),
  holding FLUI's view; it answers `MacOSBackdropError` for a closed window, a
  window off the AppKit main lane, a content view something else put there, a
  view AppKit did not create and a corner radius that is not finite. What a
  window is created with on one OS is not an extension but that OS's field of
  the contract's `WindowOptions::os_options`, which only that OS's host reads:
  the Win32 window opens with its corners and title-bar theme
  (`Win32WindowOptions`). Two things name a crate the list leaves out, each
  named in the
  manifest: the entry `android_platform(app)`, which takes the Activity
  (`entries`), and the implementations of `flui-semantics`'
  `PlatformAccessibility`, whose methods take AccessKit's tree as that trait
  does (`traits`). Tests in `tests/` read a window's `HWND` the way an
  embedder does, through `raw-window-handle` (`win32_handle` in
  `tests/main.rs`); `tests/os_extensions.rs`, its own harness so winit builds
  its loop on the main thread, pins that a headless and a winit window have
  no Win32 view and a Win32 window has one, that a Win32 window opens with
  the corners and theme its options ask for, that its view refuses another
  thread and a closed window, and, on a Mac, the AppKit backdrop's views.
- **The owner thread's machinery lives in `owner/`, which names no feature
  (ADR-0151 §10).** `owner` holds the owner-thread capability (`OwnerPlatform`,
  `SharedPlatform`, `PlatformProxy`, `PendingWindow`, their errors, and the
  crate-private `OwnerHooks` and `ProxyTransport` a host implements),
  `owner::signal` the coalesced owner turns and quit admission, `owner::slot`
  the owner-thread storage for a value that is not `Send`, and
  `owner::panic_boundary` the containment a panic meets before it reaches the
  operating system (`os_entry`, the one boundary of every OS entry, below),
  `owner::call_point` the one point where the
  platform calls a handler, `owner::handlers` the application's handler set
  and a window's callbacks, `owner::notices` the trait a loop pushes system
  notices through (`NoticeChannel`, which the lifecycle feature implements; a
  wake marks the display and window caches that registered `StaleOnWake`
  first) and the lifecycle hooks a loop keeps (`LifecycleHooks`),
  `owner::sources` what a loop keeps of a feature's source
  (`SystemPreferenceSource`), and `owner::watch` an observation a feature
  started for a loop. `owner` is a `base` of
  `[package.metadata.flui.modules]`, so `cargo xtask module-dag` refuses an
  import from it into a feature's directory. Its public items are named at the
  crate root only. Its tests name no feature or host either: they run over
  stand-ins of the owner's own (`owner::windows::testing::QuietHost<C>`, a
  Win32 host with the window hooks `C` gives, none by default;
  `owner::winit::testing::{QuietHost<D>, QuietTable, quiet_platform}`, a winit
  loop whose hooks report nothing but the displays `D` gives, under a table
  whose window position opens no window; `owner::notices::testing::NoNotices`),
  and a test that needs a host's real windows is that feature's. A feature's
  test that runs the winit loop by hand reaches it through
  `owner::winit::testing` (`install_control_lane`, `WinitApp::for_test` and
  accessors marked `_for_test`), and the loop keeps its state private.
- **Each platform's loop is a module of `owner/` (ADR-0151 §10).**
  `owner::windows` is the Win32 message loop and its wake deadlines, the
  owner's message-only window (`control`), the thread's COM apartment, FLUI's
  window class with its raw window procedure, and the hidden top-level window
  that hears the system's broadcasts (`broadcast`); `owner::apple` is
  `NSApplication`'s loop with the owner lane and the wake pump (`macos`) and
  `UIApplicationMain` with its delegates and scenes (`ios`); `owner::android`
  the looper over `android-activity`, the JNI `FluiView` and the NDK lookup;
  `owner::web` the `requestAnimationFrame` loop; `owner::winit` winit's
  `ApplicationHandler`; `owner::headless` the test double's owner turns. The
  hosts' tables name these loops. A multiplexer of the operating system that
  serves several features dispatches through a hook declared beside it, which
  each feature's file implements and the host's table names: on Windows the
  table implements `owner::windows::WindowsHost`, whose `window_class` and
  `broadcast_subscribers` list, in order, what the owner dispatches to. On
  macOS every window's content view is the owner's
  (`owner::apple::macos::content_view`, `FLUIContentView`): it hands its
  events to the input's `ViewInput` and its `NSTextInputClient` methods to the
  text input's `ViewText`, one of each per view, made by the table's
  `MacosHost::view_hooks`, routes a key press between them, and enters the
  display pass (`owner::apple::macos::display_pass`) for the frame its
  `drawRect:` asks the window for. On iOS the window's view hands its
  touches to the input's `ViewTouches` (`IosHost::view_touches`, one per
  view), and its display link's cadence and its screen to the loop's
  `HostDisplays` (`observe_link`, `move_window`, `forget_link`). A
  loop keeps a feature's state the same way: each `owner` module declares one
  host trait (`WindowsHost`, `MacosHost`, `IosHost`, `AndroidHost`,
  `WebHost`, `WinitHost`, `HeadlessHost`) whose methods the table implements
  to make what the loop holds, each through a hook trait declared beside the
  loop, and the loop is the table's position for that feature, so the table
  cannot name one type and build another (the displays, the preferences and
  the lifecycle below).
  - FLUI's window class has `window_proc::<H>` as its procedure. Win32 calls
    it with a window's handle and no pointer of FLUI's, so it finds the hooks
    of `H`'s class on its thread (`owner::windows::window_class`, the Win32
    trampoline ADR-0097 grants: `WindowsHost::window_class` makes the hooks
    once per owner thread, as instances that may keep state); it holds the
    panic boundary and offers each message to them in order — the first
    `Some` ends the dispatch, `None` passes it on, and a message no hook
    answers goes to `DefWindowProcW`. A created window is announced to the
    same hooks in order (`window_created`, through the owner's state,
    `OwnerShares::window_hooks`) as a `CreatedWindow` that carries its
    `WindowIdentity` and its `WindowScale`: the window's one DPI, which its
    procedure changes at `WM_DPICHANGED` and every hook reads, keeping no
    copy (seeded once as the window opens: the DPI the system reports, else
    that of the scale it was placed at). A hook keeps its windows in an
    `owner::windows::WindowTable`, put at announcement, read only for the
    window a reference names, taken at destruction or `WM_NCDESTROY` and
    ended with the table released; after each hook
    the window must still be the one created, or no later hook hears of it and
    its open fails (`WindowGone`). Windows holds the class once per process,
    so the class registration records its host and a loop for another host is
    refused with `PlatformError::Init`. Each hook lists the messages it
    answers (`WindowMessages::answers`, with the `wParam` telling apart the
    timers and system commands that share a message) and answers no other;
    the procedure asserts it in debug builds. A hook may hear a message it
    does not answer and pass it on. The hooks are, in order, the text services
    (`text::windows::Tsf`: a new window's TSF connection, its scale, and the
    window's moves, which TSF hears before the procedure answers them; it
    answers nothing), the window's own procedure
    (`window::windows::proc::WindowProc`: creation, close and destruction,
    size, position and their limits, DPI, focus, paint, the cursor, the
    title, the modal size-move and menu loops with their deadline timer, the
    setting changes, the frame and fullscreen requests posted to it and the
    keyboard's system-menu command), the input
    (`input::windows::InputWindows`: mouse, pointer, wheel, keyboard,
    characters and the keyboard layout, and the precision touchpad's hit
    test and pacing timer, with the DirectManipulation viewport it gives each
    new window), the session (`lifecycle::windows::SessionWindows`: each
    window's session notification, and the lock and unlock it answers as the
    window's step) and the drop target
    (`drag::windows::DropTargetRegistration`, which registers each window
    created with the loop's `HostDrag` the announcement carries, keeps the
    registration it hands back and revokes it as the window is destroyed;
    it answers nothing). The reference a hook is handed a window by,
    `owner::windows::WindowRef` (the handle and the identity of its context),
    is the owner's; only the window feature reaches the context through it
    (`WindowRef::with_context`), and the window's own procedure borrows it for
    its own messages only.
  - A hook keeps what it needs of a window in itself, keyed by the window's
    identity: made when the window is announced, dropped when it is destroyed
    (`WindowMessages::destroyed`, which the window's procedure announces at
    `WM_DESTROY`) or at `WM_NCDESTROY` for one whose destruction was not
    announced. No pointer of FLUI's goes into a property of the window, which
    another process may write, and a message, step or read for a window a
    hook does not hold finds nothing. The hooks hear the window's steps
    (`WindowMessages::step`, a `WindowStep`: its size, minimizing, focus
    loss, paint, modal size-and-move loop, DPI change and the rectangle it
    asks for applied, setting changes, its close callbacks having run, the
    session's lock and a key the application let through), which the hook
    that handles each announces in hook order where it is complete. The
    input's state of a window (its message clock, pointer registry, touchpad
    viewport, hover, modifiers, dead keys and a pending surrogate) is its
    own; what the window, drag and drop and haptics read of it the owner
    names (`WindowInputs`: the hover, the modifiers, the kind of a press, a
    pen in range), read-only, through the class on their thread
    (`owner::windows::window_inputs`). The text services' state of a window
    is theirs too; the window hands out its text-store host through the
    class's text (`WindowTexts`, `owner::windows::window_texts`). A keydown
    the application lets through is a step, which the window answers with its
    own keys, the fullscreen hotkey; the session's lock is a step the window
    answers by hiding. Tests:
    `a_window_message_reaches_the_hooks_in_order`,
    `a_window_a_hook_destroyed_is_announced_to_no_later_hook`,
    `a_second_host_is_refused_the_window_class` (two instances of the owner's
    quiet host, told apart by type),
    `two_windows_keep_their_own_input`,
    `a_windows_input_is_gone_once_it_is_destroyed`,
    `a_window_the_input_never_saw_has_no_input`, and
    `hosts::windows::tests::the_window_hooks_answer_disjoint_messages`, which
    fails if two of the table's hooks list one message, timer or system
    command.
  - The owner control owns the broadcast receiver and builds it from the
    subscribers the host names over its objects (`OwnerObjects`, with the
    sources `WindowsHost::{preferences, lifecycle}` built from `OwnerSeeds`):
    the displays' `DisplayBroadcasts` (`WM_DISPLAYCHANGE` and a work-area
    change, `WM_SETTINGCHANGE` with `SPI_SETWORKAREA`), the preference source's
    `PreferenceBroadcasts` (setting changes) and the lifecycle's `HostEvents`.
    The receiver tells them a message in order until one answers; a
    subscriber's panic is contained, and the message is answered what a
    subscriber gives for that case (`answer_after_panic`: the lifecycle grants
    a session-end query), 0 otherwise. Windows reads no result from a display
    or setting broadcast, so the displays and the preferences only observe
    (they answer nothing) and every subscriber hears those messages whatever
    the order; only the lifecycle answers, the session-end query and the power
    broadcast, which no other subscriber reads. Each subscriber admits wakes of
    its own: the preferences' closes as their source drops, the displays' when
    the receiver detaches it before destroying its window. winit on Windows
    builds a receiver of its own for its displays and, when they open, its
    preference source; a preference source that fails to open leaves the
    displays subscribed. Tests: `broadcast_subscribers_hear_a_message_in_order`,
    `the_displays_and_the_preferences_hear_every_change_in_either_order`,
    `a_display_change_reaches_the_displays_when_the_preferences_cannot_open`,
    `display_broadcasts_mark_the_monitors_stale_and_wake_until_detached`,
    `the_winit_loop_s_displays_hear_a_display_change`.
  - The owner's message-only window answers its own messages (the wake, the
    exit policy) and offers any other, in order, to the `OwnerMessages` hooks
    of the objects the loop holds: today the drag transport the table makes
    (`WindowsHost::drag`). It answers `START_DRAG`, from which a session's
    `DoDragDrop` runs on a fresh owner turn, and `MODAL_FRAME`, for which the
    owner delivers the frames due while the modal loop runs
    (`owner::windows::modal_loop::ModalFrames`, which the owner window holds
    and hands a hook weakly), and it cancels a session that has not started
    when the window is destroyed. Every hook hears that end, each in a
    boundary of its own, so one that panics leaves the next told. A window's
    `DefWindowProc` move and size loops deliver through a `ModalFrames` of
    their own, at the owner's modal pace (`owner::modal_pace`). Tests:
    `ole_drag_and_drop_on_real_windows`,
    `a_replaced_wake_hook_gets_its_modal_frame`,
    `every_owner_hook_hears_the_window_go_after_one_panics`.

  On Android the table implements `owner::android::AndroidHost`, which makes
  what the loop holds (the displays, the preference source, the notice
  channel, what the Activity's callbacks mean, the input, the clipboard and
  the drag-and-drop transport: `displays`, `preferences`, `notices`,
  `activity`, `input`, `clipboard`, `transfer`) and names the features'
  objects `FluiView` hands its reports: the text feature's `InputMethod`
  (`input_method`), which answers the input method's calls through the view,
  and the transport, to which the view hands its drag events.

  A loop keeps a feature's object in a field typed by a hook trait declared
  beside the loop, which says what the loop and the windows it opens ask of
  the object, and the owner's host trait makes it: `WindowsHost::drag`
  (`HostDrag`: the owner window's `OwnerMessages`, a window's drag source and
  drop target), `WebHost::clipboard`, `WebHost::drag` (`HostDrag`: the
  canvas's drags, registered into the canvas's registration,
  `CanvasRegistration`, which the window implements: listeners and timers
  that run only while it accepts, which a feature reads read-only
  (`Accepting`), what a feature ends as it retires (`on_retire`: the input
  releases the pointers it captured), pointer custody and the focus area;
  and `CanvasDrag`) and `WebHost::input` (`HostInput`: the canvas's
  pointers, keys, wheel and gestures, registered into the same
  registration, which answer the key route the window hands its text input,
  `CanvasKeys`),
  `WinitHost::clipboard`, `WinitHost::drag` (`FileDrops`) and
  `WinitHost::input` (`LoopInput`: the session's pointers and keys as the
  loop's events and turns bring them), `AndroidHost::clipboard`,
  `AndroidHost::transfer` (`ViewTransfer`, made for `FluiView`) and
  `AndroidHost::input` (`ActivityInput`: the native queue's motion and key
  events and the scroll factors), `HeadlessHost::devices`
  (`SimulatedDevices`: the devices a test attaches) and `HeadlessHost::offers`
  (`SimulatedOffers`: the drag model's offer table). The table names the
  loop as such a position (`type Drag = WindowsLoop`, `type Input =
  AndroidLoop`, as it names the loop for the displays, the preferences and
  the lifecycle), and the loop answers it from that field, so a table cannot
  name one type and build another. The headless loop holds the simulated system's drag state
  (`owner::headless::OsDrags`) as it holds the rest of that system;
  `drag::headless` is the data-transfer source, a window's drag source and the
  test's side over it. A position whose object is made per call (the Win32,
  AppKit and UIKit clipboards) stays the feature's type. The windows are held
  the same way, through the hooks of the window invariant below.
- **A feature is a directory of `flui-native`: its model beside one file per
  platform (ADR-0151 §10).** `text` was the first. The model is `ime` (what
  the Android and web hosts mirror of a focused field, and the one edit
  between two versions of it), `sizing` (the native text size a presentation
  captures, `capture_text_sizing`, which a platform provides as a
  `NativeTextSize`) and the `Text` position's trait, `TextInput`: the handle
  a window holds its text input by answers one `WindowTextInput`, either
  `Ime` (the field tells the input method whether it may compose and where
  its caret is: AppKit, winit, headless) or `Store` (the platform reads or
  mirrors the field's store: TSF, Android, web), and
  `Absent<position::Text>` answers none; each window's accessor goes through
  it, as do its text sizing (`capture_sizing`, `invalidate_sizing`,
  `fence_sizing`) and its going (`detach`). Where the text feature keeps a
  window's state itself the handle is a key to it: Android's input method
  keeps each window's field by `owner::android::ActivityWindowId`, and the
  Web's text input lives in the canvas window's owner-thread slot. Beside it,
  one file per platform with its pure half a module inside it:
  `windows/mod.rs` (the pure `geometry`, the `Tsf` class hook) with `tsf/`,
  `apple/{macos, ios}.rs`, `android.rs` (the pure `ime`, `input`, `sizing`),
  `web.rs` (the pure `ime`, `input`), `winit.rs` and `headless.rs`. `cargo
  xtask module-dag` lists each feature directory in `features`: a platform
  module names its model, `owner/`, `version/` and other features' models,
  never another platform's module; `owner/` and `version/` name no feature's
  item; each edge into another feature's platform file is a named exception
  that goes when the feature keeps its own per-window state. The
  pure halves are built for their target and for tests on every host; which
  key belongs to the input method is the input feature's
  (`input::android::keys`, `input::web::keys`), and a window's text input
  tells it whether the field composes. Tests of the model and the pure
  halves sit beside them (`text::ime::tests`, `text::android::ime::tests`,
  `text::web::ime::tests`, `text::windows::geometry::tests`).
- **Clipboard, drag and drop, accessibility and haptics are feature
  directories (ADR-0151 §10).** Each holds its model beside one file per
  platform, and `[package.metadata.flui.modules]` lists the four as
  `features`: `cargo xtask module-dag` holds a platform file to its own model,
  `owner/`, `version/` and other features' models. The models are
  `clipboard::lock` (the Win32 session lock the Win32 and `arboard` clipboards
  share), `drag::offers` (the offer table every data-transfer source mints its
  ids in, `TransferCompleter`, the producer half of a delivery, `TransferUri`,
  a URI list as the hosts that read paths or URIs write it, and what a media
  type offers and its bytes carry, shared
  by Android and the Web), `accessibility::bridge` and `haptics::degradation`.
  A platform is one file. One whose decisions need no OS keeps them in a
  module of the file under `forbid(unsafe_code)`, named for what it decides
  (`drag::windows::crossing`, what crosses OLE; `drag::android::clip`, what a
  clip offers, holds and answers; `drag::web::mapping`, how `DataTransfer`
  types and effects map); the file is compiled under `cfg(any(<os>, test))`
  and its OS side under `cfg(<os>)`, so every host tests that module. A child
  file is a module's tests, the mechanism of one OS object
  (`drag::windows::com`, OLE's COM objects), or a split past 3 000 lines. A
  position a platform does not have is `Absent`: drag on macOS and
  iOS, accessibility on Android, iOS and the Web and under winit off Linux,
  haptics on winit. Android's clipboard is an in-process stand-in
  until the system clipboard is reached through JNI.

  Accessibility has no Cargo feature (ADR-0151 §15, as amended).
  `accesskit_windows` and `accesskit_macos` are unconditional dependencies of
  their OS, and every Win32 and AppKit window builds its adapter. The Linux
  adapter comes with the Linux host: while that host is winit, `accesskit_unix`
  rides `winit-backend` beside the `zbus` the host needs for the Settings
  portal and logind (`flui-app` turns `winit-backend` on for every Linux
  build), and `accessibility::{bridge, linux}` compile under
  `all(target_os = "linux", feature = "winit-backend")`, written once, in
  `accessibility/mod.rs`. The native Wayland and X11 host makes
  `accesskit_unix` and `zbus` unconditional Linux dependencies and that gate
  `target_os = "linux"`.

  The table's positions make the accessibility adapter and the actuator a
  window hands out (`host::Accessibility::for_window`,
  `host::Haptics::for_window`); the Android window takes its drag source from
  the loop's `ViewTransfer` hook (`drag_source`), and the Win32 and Web
  windows reach theirs through the loop's `HostDrag`; the headless window
  takes its drag source from its loop, which the table's
  `HeadlessHost::drag_source` tells how to make it. The Win32 drop-target hook
  keeps each window's registration, which the transport makes as the window
  is created and the hook revokes as it is destroyed. The Win32 drag
  transport and actuator name a window by `owner::windows::WindowRef`, the
  owner's vocabulary, and never reach its context: the drop target keeps the
  window's callbacks and its `WindowScale` from the announcement, and what
  they read of the window's input (the kind of a
  press, the pen in range) the input answers read-only
  (`owner::windows::WindowInputs`). The Web drag answers
  its events and reads their position as the input does, through the
  owner's `owner::web::event`.
- **The displays are a feature directory, `display/` (ADR-0151 §10).** Its
  model is `display::source`, the id registry and last snapshot every native
  source keeps, and the one publisher of a measured refresh period. A period
  is made by the contract's constructors alone: every source hands the rate
  or interval its platform reports straight to the one for its unit
  (`RefreshPeriod::{from_hz, from_ratio, from_millihertz, from_secs_f64}`),
  whose one check refuses what describes no cadence, and the two that
  measure a cadence, the iOS display link and the browser's frame estimate,
  publish through `MeasuredPeriod::offer`, which replaces the published
  period only when a measurement leaves its band (`outside_band`, the band
  rule the estimate also judges its frames' agreement by).
  `display::source::mapping` reads one table of cadences through the
  Android sample, the Windows rule, the Wayland and RandR models, the
  display link and the estimator; the macOS source hands CoreGraphics' rate
  to `from_hz` from its FFI half. Beside it is one file per platform: `windows.rs` (the
  monitors, `source`; their pure `sample` rules; the `DisplayBroadcasts`
  subscriber, `broadcasts`), `apple/` (`macos.rs`, `ios.rs`, and in
  `mod.rs` the pure `screen` keys and bounds the two share and
  `link_rate`), `android.rs` (`source` and the pure `sample`), `web.rs`
  (`source` with the capabilities it detects, `events`, and the pure
  `capability`, `dppx_query`, `refresh_estimate`, `screen_identity`),
  `linux/` (Wayland and X11 outputs), `winit.rs` (the source over
  `display::linux` that the winit host reads on Linux) and `headless.rs`. The
  halves of a platform file are inline modules: a pure one is
  `cfg(any(<os>, test))` with `forbid(unsafe_code)`, so the Linux host runs
  its tests, and the one that calls the operating system is `cfg(<os>)`. A
  platform file has a child file only for a mechanism of one operating-system
  object, for a split the 3 000-line limit forces, or for a module's tests
  (`windows/message_tests.rs`); `linux/` is such a split, past the limit as
  one file even without its tests, one file per protocol connection and per
  pure model. A loop is
  its table's displays position (`type Displays = <Loop>`) and holds its
  displays through the hook its `owner` module declares
  (`owner::<os>::HostDisplays`, on headless the snapshot itself), which the
  table fills through the owner's host trait (`WindowsHost`, `MacosHost`,
  `IosHost`, `AndroidHost`, `WebHost`, `WinitHost`, `HeadlessHost`); the
  windows a loop opens reach the displays through the same hook, whose
  implementations hold the bodies. Every observer a display source starts for
  the loop (AppKit's and UIKit's screen notifications, Android's refresh-rate
  callback, the browser's listeners, the Linux connection thread under winit)
  comes back as an `owner::watch::Watch`, which is `#[must_use]` and which the
  loop keeps until it stops, so an unwind cannot skip a stop; the macOS loop
  keeps its watch in a `NativeOwner`, which releases it on the main thread.
  The winit host on Windows and macOS reads the Win32 and AppKit hosts' own
  sources, which implement `owner::winit::HostDisplays` beside their own
  loop's hook; on Linux `display::winit` reads `display::linux`, the one edge
  between platform files `module-dag` admits, by name, until winit goes
  (ADR-0151 §11). The Android loop's poll rule is the loop's
  (`owner::android::poll`). The iOS source reads the OS version as it is built
  (`version::gate::apple::AppleVersion::read`). Which windows the winit loop
  looks at again on Linux, because winit may have moved them to another output
  without an event, is the loop's (`WinitApp::sync_unannounced_displays`).
- **The preferences and the lifecycle are feature directories,
  `preferences/` and `lifecycle/` (ADR-0151 §10).**
  - `preferences`: the model is `read` (the bounded read admission the Android
    and AppKit sources take) and `locales` (preferred languages from the forms
    the systems spell them in); the platform files are `windows` (WinRT and
    Win32, told of changes by the broadcast receiver), `apple::{macos, ios}`,
    `android` (`configuration`, and the rules `contrast` and `text_weight`),
    `web` (`browser`, and the rule `media`), `linux` (the Settings portal,
    whose watch names no winit type, so the Wayland and X11 hosts start it as
    the winit host does) and `headless`.
  - `lifecycle`: the notice channel (`notice`) and the one position of the
    loops that hear the lifecycle themselves; the platform files are `windows`
    (`host_events`, and the rules `messages` and `memory_watch`),
    `apple::macos` (`notices`, and the rule `memory_pressure`), `android` (the
    Activity's callbacks, and the rule `rule`), `web`, `linux` (the inbox, and
    beside it `logind` and `pressure`, each with its rule, `signals` and
    `triggers`) and `headless`.
  - One file per platform, as for the displays: a child file holds only a
    mechanism tied to one OS object (`lifecycle::linux::{logind, pressure}`),
    or a module's tests (`lifecycle/windows/tests.rs`).
  - A loop holds what the two features made through the hooks its `owner`
    module declares, which the table fills through the same host trait as the
    displays (`WindowsHost::{preferences, notices, lifecycle}`,
    `MacosHost::{preferences, notices, notice_sources}`,
    `IosHost::{preferences, notices}`, `AndroidHost::{preferences, notices,
    activity}`, `WebHost::{preferences, page}`, `WinitHost::{notices,
    preferences, host_notices}`, `HeadlessHost::notices`): the preference
    source as `owner::sources::SystemPreferenceSource` or a loop's own trait
    over it (`OwnerPreferences`, `UIKitPreferences`, `ActivityPreferences`,
    `PortalPreferences`), the notice channel as
    `owner::notices::NoticeChannel` (`lifecycle::notice::SystemNotices`
    implements it), the Windows lifecycle as `OwnerLifecycle`, and what the
    Activity's callbacks mean as `ActivityCallbacks`. A source the loop starts
    only once it runs is an object the table makes with a method that starts
    it (`NoticeSources::listen` and `PageLifecycle::listen`, which return a
    `Watch`; `PreferenceStart` and `HostNoticeStart` under winit, whose signal
    exists only once it runs).
  - Each loop is its table's preference position, and its lifecycle position
    on every host but headless, whose position is its test handle
    (`HeadlessPreferences`, `HeadlessHostEvents`). The loops that hear the
    lifecycle in their own callbacks (AppKit's and UIKit's delegates, the
    Activity's, the winit loop's turns on Linux) implement
    `owner::notices::LifecycleHooks`, and one generic impl in
    `lifecycle/mod.rs` makes them the position; the Windows and web loops
    answer their own. winit on Windows and macOS hears no session end or
    notice, so its lifecycle is `Absent<Lifecycle>`, and each registration is
    dropped as it is made
    (`winit_lifecycle_registrations_drop_their_captures_without_a_source`).
  - What the Activity's callbacks mean is the lifecycle feature's
    (`lifecycle::android`, with the trim level it reads), behind the looper's
    `ActivityCallbacks` hook; the looper keeps their vocabulary
    (`owner::android::activity_callbacks`). What else a loop hears in its own
    callbacks stays in `owner/`: the macOS reply to
    `applicationShouldTerminate:` (`owner::apple::terminate_gate`) and UIKit's
    scene-destruction error block (`owner::apple::ios`). The JNI access to the
    Activity every feature uses is `owner::android::activity::with_activity`,
    and the observer on an AppKit notification centre the macOS sources
    register through is `owner::apple::macos::observation`; Android's scroll
    factors are input's (`input::android`).
- **The window feature is `window/`, and a loop holds its windows without
  naming them (ADR-0151 §10).** The model is what two or more platforms share:
  the published state (`window::state`), the one rule that decides a window's
  mode from what its window system says (`window::state::mode_of` over
  `ModeFlags`: minimized before fullscreen before maximized; winit's
  queries, AppKit's flags, the X11 window manager's properties and the Win32
  window's tracked mode each read their report into the flags and nothing
  else), the desktop window configuration
  (`window::config`, crate-private: its windows open with F11 as the
  fullscreen key) and the acknowledgement
  a host waits for before it publishes a window it attached
  (`window::installation`, public at the crate root as `WindowInstallation`,
  `PendingWindowInstallation`, `InstallationCancelled`).
  `window::state::mapping` reads one table through winit's flags, the X11
  window manager's properties, AppKit's raw flags (which keep full screen
  above a miniaturized flag, a row of their own) and the Win32 tracked mode.
  Whether a window is seen is two facts of its snapshot, `visible` and its
  mode, which `WindowState::is_seen` reads together; each window reads both
  from its own OS. Each platform's
  window is one file beside it (`android`, `web`, `winit`, `headless`, and
  `linux/x11`, the live X11 layer the winit host uses), or a directory where
  its halves or one OS object need one: `windows/` (the pure `limits` and
  `mode`; the window, the set and position, the
  procedure and the backdrop under `cfg(windows)`) and `apple/` (the pure
  `geometry` with its `frame`, `mode`, `surface` (the occlusion bit) and
  `screen_edges`, `macos` with its `delegate` and its public `ext`, and
  `ios`). The platform's system chrome (the table's `Chrome` position) is in
  the same file.
  - **The host seam.** The window position names what a platform's windows
    share, `host::Window::Shared`: the Win32 handle map and configuration
    (`WindowSet`), the macOS configuration, the UIKit window factory, the
    winit frame inset and window manager's view, the headless window model,
    and on every host the table's window-level positions
    (`host::WindowPositions`, all Android and the Web share). The host's
    constructor builds it with
    `HostPlatform::from_parts(|positions| shared, |shared| Loop::new(...))`
    (or `try_from_parts` for a loop that can fail): the platform keeps the
    one instance and the loop gets that instance as the hook it declares, so
    the two cannot see different windows; the platform passes it to every
    call of the position.
  - **The loops' hooks.** Each loop declares what it asks of its windows, and
    the window file implements it: `owner::handlers::{CallbackWindow,
    OpenWindows}` (a window's callbacks; whether any window is open), shared
    by the hooks that extend them; `owner::windows::ClassWindows` (the
    deadline redraws and modal paints); `owner::apple::macos::WindowRegistry`
    (the open AppKit windows, for the wake pump's frames and the stop);
    `owner::apple::ios::{SceneWindow, SceneWindows, SceneConsumer,
    SceneInstallation}` (a scene's window, made and driven by the scene
    delegate; the application's handler of `IOSSceneEvent`s, which carry the
    window's installation, adapted by the window feature, whose accepted
    connection returns what the loop watches of that installation, so a
    connection cannot leave the loop with nothing to publish or roll back);
    `owner::android::ActivityWindow` (the Activity's one window);
    `owner::winit::{LoopWindow, LoopWindows}` (the loop numbers, registers
    and finishes each window, `LoopWindows::build` only builds it from a
    `WindowSeed`, and `LoopWindows::take_unreported` names the X11 windows
    the loop reads again); and `owner::headless::TrackedWindows`.
  - The Win32 window-thread affinity rules (`owner::hwnd_affinity`) are
    owner machinery: the owner control and the loop apply them as the
    window does; they sit beside the Win32 loop, under `cfg(any(windows,
    test))`, so every host tests them. The X11 window manager's view
    (`window::WindowManagerView`, its error `window::WindowManagerError`) is a
    model trait: the winit window reads it, `window::linux::x11`'s probe
    answers it, and the winit host connects them, so neither platform file
    names the other.
  - No window file names another feature's, and no feature's code names the
    window's. A feature keeps its own state of a window: the Win32 text
    services, drop target and session by the window's identity, each set up
    by its window hook's `created` and dropped when the window goes, none
    reaching the window's context; the Android input method its field by
    `ActivityWindowId`; the iOS text input the text size it captured from
    the view's traits. A window reads the facts it needs through its loop:
    the headless windows the laid-out displays and the drag source
    (`HeadlessState::window_display`, `display_bounds`, `window_drag_source`),
    the UIKit window its link cadence and screen through `HostDisplays`, the
    AppKit window its frames through the owner's `bounds_of`. The features
    that answer one AppKit or UIKit view (the input, the text input, the
    window's drawing) join it through the owner's hooks.
- **The input is a feature directory, `input/` (ADR-0151 §10).** Its model
  is what the platforms share: `assembly` (the pointer registry every backend
  assembles its contacts through, the decoder of native readings, a
  touchpad viewport's readings, and the `native-capture` journal; it names
  no platform file and forbids `unsafe`), `availability` (each backend's
  input capabilities table), `keys` (the Win32 virtual keys and the mouse
  buttons' codes, `MouseButtonCodes`, the `adapter` from `ui-events` key
  results that Android and winit share, with keyboard-types' locations
  matched variant by variant, and the one table of key locations, whose
  named rows give the number the DOM reports for each location and the
  codes that imply it, which Win32 and AppKit read it from; a character no
  key typed is `CHARACTER_WITHOUT_A_KEY`), `regions` (a window's input
  regions projected to device pixels, rounded outward, and handed the OS only
  when the pixels, the scale or the display changed; every window that takes
  them, the headless one too, projects through it), `scroll` (the one wheel step: a
  platform's amounts in the unit it picks, each axis turned to FLUI's
  direction by whether it moves the offset or the content, an idle axis
  `+0.0` whichever zero the platform reported, a non-finite amount refused,
  which winit and AppKit deliver as a zero step and the Web and Android
  drop) and `event_timestamp_ns` (public at the crate root).
  `input::keys::mapping` reads one table of keys through the Win32 scancode
  table, the AppKit keycode table, the DOM's number and the keyboard-types
  adapter; `input::scroll::mapping` reads one table of turns through the
  Win32 registry, the Web's, Android's, AppKit's and winit's steps. Beside it, one file per platform, each pure
  half forbidding `unsafe`: `windows/` (`mod.rs`: `InputWindows`, the
  class's input hook, which keeps each window's `InputWindow` keyed by its
  identity, and `raw`, the messages decoded; `direct_manipulation.rs`, the
  touchpad's DirectManipulation viewport, one OS object, with its pure
  `manipulation` decoder), `apple.rs` (the pure `keys` and `wheel`; `macos`,
  `ViewInputs`, the input of one content view, and its events; `ios`,
  `TouchInputState`, the touches of one view), `android.rs`
  (the pure `keys`, `motion` and `scroll`; `AndroidInput`), `web.rs` (the
  pure `keys`, `wheel` and `pinch`; `events`, `WebInput`), `winit.rs` (`WinitInput`)
  and `headless.rs` (`AttachedDevices`, the devices a test attaches). A loop
  holds its input through a hook (`InputWindows` in the window class,
  `ActivityInput`, `HostInput`, `LoopInput`, `SimulatedDevices`), and the
  table's input position names the loop
  (`type Input = <Os>Loop`), whose `host::Input` (the keyboard layout and its
  change) the input file implements. The modules the tables in `tests/` decode
  with are public and hidden from the documentation (`assembly`,
  `availability`, `keys`, `apple::keys`, `android::keys`, `web::keys`,
  `windows::direct_manipulation::manipulation`): the native recordings
  replay through the registry, and the allocation count of
  `pointer_assembly_allocates_nothing_per_event` needs a binary of its own,
  whose global allocator counts every allocation, so neither can reach a
  `pub(crate)` module; the other pure halves (`android::motion`,
  `web::pinch`, `web::wheel`) are `pub(crate)` and tested beside them.
- **The platform calls a handler at one point, with no lock or borrow held
  (ADR-0151 §10).** `owner::call_point::call` takes the handler out of its
  `HandlerSlot` (a field of a host's state mutex or `RefCell`, a window's
  `Mutex<Option<_>>`, the input callback's owner slot) under the slot's own
  lock or borrow, releases it, runs the handler in a panic boundary and puts it
  back unless one was installed meanwhile: the newer one stays and the one that
  ran is retired once the lock is released. A slot can refuse the handler back
  (a window that cleared, a loop that stopped accepting work), and a one-shot
  call (`Once`: the quit handler) retires it. `install` replaces a handler and
  `retire` drops any other user-owned value the same way, so a capture's
  destructor may call the platform. A handler's panic is the call's `Failure`:
  the caller resumes it where a Rust frame waits (headless, winit, a window's
  FIFO) or contains it where the operating system called (Win32's exit policy
  and session end, AppKit, UIKit); a handler displaced during a failed call is
  retained, never dropped into a second failure (ADR-0127 §2). A shared hook
  (the wake deadline, an `Arc`) is called as a clone and stays installed while
  it runs. `PlatformHandlers` and `WindowCallbacks` call only through it, and
  so does the system-notice channel. Tests: `owner::call_point::tests`; the
  public-API tables in `tests/handler_reentry.rs` (headless, Win32 and winit,
  each case on a worker thread so that a regression fails as a deadlock instead
  of hanging), `winit_handlers_run_outside_the_state_lock`,
  `native_host_events`' `a_hook_that_replaced_itself_and_failed_is_retained`
  and `web_session_end_hook_reenters_the_platform`. Android and the Apple hosts
  run the same call point and are type-checked only.
- **The OS enters the crate through one boundary (ADR-0151 §16).** An entry is
  code the operating system calls, or a library from inside its own entry: a
  function with a foreign ABI (a window procedure, an enumeration callback, a
  JNI native, an Objective-C method a `ClassBuilder` adds, an NDK callback), a
  `define_class!` method, a method of a windows-rs `I…_Impl` (a COM vtable
  entry), an AccessKit handler, and a closure handed to the OS (an `RcBlock` or
  `StackBlock`, a WinRT delegate, a GCD work item: any closure literal given to
  a function of an OS binding crate). Its body is a constant (a literal or a
  path: no code runs), one call of `owner::panic_boundary::os_entry(entry,
  args, after_panic, body)`, or one call of an adapter whose body is that call:
  a free function of the same file not shadowed by an import, or a method
  called as `Type::name` or `Self::name` (`com_method`, `com_hresult` and
  `TsfStore::document_method` for COM, `jni_native`, `Self::scene_event` for
  UIKit scenes). Out of an `extern "C"` frame a panic would abort the process
  unannounced, out of an `extern "C-unwind"` one unwind through AppKit's or
  libdispatch's frames; `os_entry` logs it with its text, under the entry's
  name and with `args` (what the OS called with, `Copy + Debug`: a message
  and its window, the delegate), contains the log, retains the payload, then
  answers what `AfterPanic` says: `Answer(fn(A) -> R)`, a failure the OS
  understands made from the arguments only after a panic and capturing
  nothing (a broadcast receiver grants a session-end query it could not
  answer; `applicationShouldTerminate:` lets a session end go on and cancels
  any other quit); or `Abort`, where the state behind the entry may be torn
  (FLUI's window procedure, the cloak hook, a UIKit configuration or init, the
  macOS owner lane's guard). An answer that panics too ends the process.
  Steps inside an entry are contained apart, each logged with its text
  (`contain_os_entry` for a broadcast subscriber or an owner-window hook,
  `contain_owner_callback` elsewhere): a TSF method that poisons its document
  keeps its services' entry depth as a guard, so a panic past the poisoning
  cannot leave host operations queued forever, and a view's `dealloc` still
  runs the super `dealloc` after its context's teardown failed. A GCD work
  item's body is a `move` closure, so its captures drop inside the boundary; a
  block's or delegate's captures live as long as the OS keeps it and are
  values whose destructor runs no user code. `cargo xtask panic-boundary`
  finds every entry and holds it to that shape, with no allowlist. It reads
  functions and constructors of OS crates by their path, but methods by name,
  since a receiver's type is out of its reach: a new OS method that takes a
  closure is added to its GCD list (`exec_async`, `exec_sync`, `exec_after`,
  `after`, `barrier_*`, names the crate keeps for GCD), and a constructor a
  glob import hides to its list of callback types. `wasm_bindgen` closures are
  out of its scope because wasm32 aborts on panic, winit's and
  android-activity's procedures contain their own, and `flui-hot-reload`'s
  plugin ABI is outside this crate. Tests: `owner::panic_boundary::tests` (the
  first failure is logged before the answer, with the entry's arguments, and a
  contained step with its text; a panicking log stays inside),
  `a_failing_subscriber_is_logged_and_a_session_end_still_granted`,
  `a_panicking_listener_stays_inside_its_handler` (AccessKit, Windows and
  Linux), the TSF row `a_tsf_call_failing_past_its_poisoning_leaves_no_depth`
  and, on macOS, `a_tick_whose_hook_panics_leaves_no_tick_queued`,
  `a_tick_whose_hook_rearms_then_panics_keeps_the_newer_tick` and
  `a_session_end_is_under_way_from_its_announcement_to_its_answer`.
- **`Platform` and `HostWindow` are sealed (ADR-0151 §5).** Each has a private
  supertrait: `Platform`'s is implemented for `HostPlatform<H>` alone,
  `HostWindow`'s for every type that implements the crate-private
  `CoreHostWindow`, which each host's window implements beside its
  `HostWindow`. An outside implementation does not compile, and the error
  names the seal (`tests/ui/platform_and_host_window_are_sealed.rs`). The
  window seal goes through one generic implementation so that the error lists
  the same implementations on every target and feature set. `PlatformWindow`
  stays open for test doubles.
- **The winit fallback is a desktop backend.** Its module, public re-exports
  and `arboard` clipboard dependency are available only on Windows, macOS and
  Linux under `winit-backend`. The `desktop` compatibility selector enables no
  dependency; native desktop backends are selected by target OS. Enabling every feature on Android or iOS keeps the native backend;
  it does not enable desktop clipboard code. The required-target and
  all-features `cargo xtask cross-typecheck` configurations compile this
  boundary, including the selected packages' tests, examples and benches.
- **A window leaves a backend as an `Arc<dyn HostWindow>`.**
  `HostWindow: PlatformWindow` adds the one AccessKit-speaking accessor,
  `accessibility()`. `Platform::open_window`, `WindowOpen::Ready` and
  `PendingWindow` return it; every backend window implements it next to its
  `PlatformWindow` impl, and its accessibility bridge is fixed when the window
  is built, so a runner may read it once. `dyn HostWindow` has its own
  `HasWindowHandle`/`HasDisplayHandle` impls: without them an `open_window`
  result would not be a `flui_engine::WindowTarget` until upcast.
- **A window publishes a state snapshot and takes commands (ADR-0237).** There
  is one window trait. `PlatformWindow::state` returns an immutable
  `WindowState` with a revision, and `apply` takes a `WindowCommand`; the
  snapshot's `capabilities` say which commands the window honours, each as an
  `Availability`: `apply` answers `Unsupported` exactly when the matching
  capability is `Unavailable`, `Emulated` marks what the host builds itself
  (the Win32 borderless fullscreen, the Web's close, which retires the
  canvas), and the capabilities of each backend are what its `apply` carries
  out today. Showing, activating and closing are commands like the others
  (`Show`, `Activate`, `Close`); there is no method twin. The snapshot
  describes what the OS reported, not what a command asked for, and holds
  every fact of the window (ADR-0254 §1): the drawable `WindowMetrics` (a
  report that is not a drawable area keeps the last), appearance, display,
  safe area, execution state, focus, activation, visibility, hover and the
  capabilities.
- **The snapshot is the one source of a window fact.** No getter and no
  per-fact callback exists beside it: a consumer reads `state()` and hears
  `on_state_changed`, and one that needs to know which fact changed compares
  the snapshot with the last one it saw (`flui-app`'s
  `RuntimeEvent::of_state_change`). Whether a window is seen is
  `WindowState::is_seen`: visible and not minimized. Each host sets a
  `StatePublisher` on the window's `WindowCallbacks` when the window opens
  (`publish_through`, over a weak reference to the window). The publisher
  reads the window again and answers `Publication` (changed, unchanged,
  held) without announcing anything; `publish_state` runs it and
  raises `on_state_changed` when the revision grew, so each fact the OS
  reports is one revision and one notification. This happens in the OS
  callback that reported the fact (each `WM_SIZE` of a Win32 live resize
  inside its modal loop). While a window holds its publishing back (Win32
  through a DPI change or a fullscreen switch), the change publishes and
  announces once, when the hold ends, so a callback reads the finished new
  state. A `StateChanged` queued inside a callback that panics is still
  delivered: its revision is already published. The Win32 publisher reads
  only a context whose `WindowIdentity` is its window's, so a recycled
  handle publishes nothing of another window; every fact comes from the
  window procedure on the owner thread, and a read the context refuses
  leaves the snapshot stale for the owner's next read. A fact another
  feature reports reaches the snapshot the same way: the hover is what the
  last `WindowCallbacks::report_hover` said, whether Win32 or web input,
  AppKit's tracking area or winit's cursor events reported it. An
  observation committed at its delivery point (the Web's focus and
  visibility) is published right after its commit. The facts beyond
  geometry (metrics, appearance, safe area, execution, hover, activation,
  capabilities) are one value, `window::state::ReportedFacts`, that every
  host builds. Activation is Win32's `WM_ACTIVATE` (a window whose owned
  popup has the keyboard is active and not focused); the other hosts have
  no notion apart from focus (AppKit's key window, winit's `Focused`, the
  canvas focus, the scene and the Activity), so their activation is their
  focus. `window_facts_arrive_only_in_the_snapshot` pins one revision and
  one notification per fact; a `compile_fail` doctest on `PlatformWindow`
  pins that the getters are gone.
  Every backend keeps its last snapshot in a `window::state::WindowStateCell`
  (built like `DisplaySource`): an OS event marks the cell stale, the next read
  samples the OS with no lock held, and a sample equal to the published one
  keeps the revision. `on_state_changed` is raised after the publish, outside
  the cell, only when the revision grew. Win32 and AppKit refresh from their
  window messages and delegate callbacks, winit from its window events, the
  Web from the layout, focus and visibility observers and on each owner
  wake, which is how a screen change reaches it; the headless window
  keeps a model that stands in for the OS (`HeadlessWindowPolicy` makes it clamp a size
  or decline a maximize) and publishes after each command or `simulate_*`
  call. The iOS window rereads its metrics and lifecycle on each `state` call
  and publishes from its view's metric updates and each lifecycle step.
- **The macOS backend enforces the window thread-affinity default
  mechanically.** `PlatformWindow`'s rule is that a method driving a window runs
  on the thread that owns the platform event loop. Every window-driving
  `PlatformWindow` body and extension command on `MacOSWindow`
  re-enters the owner lane through `route_on_owner`, so a call from any thread
  is marshaled onto the window's owner lane before any AppKit message is sent.
  Two carve-outs: the class-E raw-handle accessors
  (`raw_window_handle`/`window_handle`, and `display_handle` which carries no
  pointer) take their NSView outside the routing — `!Send` outputs whose
  enforcement is upstream (raw-window-metal's main-thread hard panic plus
  `debug_assert_appkit_main_thread`-guarded platform entries, ADR-0039); and
  the `enable_tiling`/`disable_tiling`/`is_tiling_enabled` trio never routes
  (recorded for observability only until the native API is adopted).
- **Linux host events reach the owner through an inbox, and a machine that
  waits for the application waits for delivery, not for receipt.** Two sources
  run beside the owner and never run application code: an async logind task
  (`lifecycle::linux::logind`) and a kernel-pressure thread
  (`lifecycle::linux::pressure`). Each posts a `HostEvent` to one `Inbox` and
  wakes the owner, which drains it in its next turn through the winit loop's
  `HostNotices` hook. logind's sleep and shutdown carry a
  completion: the delay inhibitor is held until the owner has delivered the
  event (a panic in a subscriber drops the completion too) and is released
  after at most four seconds, inside logind's own `InhibitDelayMaxSec`.
  Shutdown on Linux is a notification: the answer to the session-end query is
  not read, because logind cannot be asked to wait for the user. The decisions
  (signal decoding, trigger requests) are pure rules beside their sources
  (`lifecycle::linux::logind::signals`, `lifecycle::linux::pressure::triggers`),
  and the X11 atoms to a window mode are the window feature's
  (`window::linux::x11::mode`), each checked on every host.
- **The X11 window probe uses two connections on purpose.** One queries from
  whichever thread asks and sets the properties a client sets on an unmapped
  window; the other is read only by the watch thread, which records which
  windows' `WM_STATE` or `_NET_WM_STATE` changed. A reply wait on the
  watching connection would read events into its queue while the thread sleeps
  in `poll`, so no request there waits for a reply. A window that opens
  maximized or fullscreen is created unmapped, gets its mode written into
  `_NET_WM_STATE`, and is mapped afterwards, because a window manager reads
  the mode from the property when it maps a window.
- **An OS version is compared only inside `version::gate` (ADR-0244).**
  Windows, iOS and Android each have one version type (`WindowsBuild`,
  `AppleVersion`, `AndroidApi`), each in a module of `version/gate.rs` built
  for its target and for tests and forbidding `unsafe`, read once when the
  platform is built and kept in it, with its number private; reading it is the
  one part that needs the system, under `cfg(<os>)` (`read_build`, the
  `RtlGetVersion` call behind `WindowsBuild::read`; `AppleVersion::read`; the
  Android level is read where the NDK is looked up, `owner::android::ndk`).
  What a version unlocks lives with the code that uses it: the virtual-mode
  display query's gate and its flags are the Windows display source's
  (`display::windows`). The floors are
  `version/floor.rs`, generated from `platform-support.toml` by `cargo xtask
  platform-floor`. An API newer than the floor is a marker type implementing
  `Gate`, whose doc says what the code does without it; `version.since::<F>()`
  returns a `Since<F>` proof or `None`, and the gated call takes the proof. A
  gate at or below the floor fails to compile, so raising a floor turns the
  gates it makes dead into errors, and an unreadable version unlocks nothing.
  Clippy refuses `objc2::available!` and `AndroidApp::sdk_version`. `version`
  is a `base` of `module-dag`: it names no feature, in its tests too. Each
  module that declares markers checks them, at the version before `SINCE`, at
  `SINCE` and at an unknown version: `os_version_gate_table` the markers of
  `version::gate`, `ndk_gate_table` the NDK functions' (`owner::android::ndk`)
  and `frame_rate_range_gate_table` the display link's
  (`display::apple::link_rate`), through `version::gate::testing::row`, and
  `virtual_mode_gate_table` the display query's (`display::windows::sample`).

---

## Mapping decisions

### Android captures safe-area geometry on the Java view thread

System-bar and display-cutout insets are sampled from the Activity decor view.
Packaged applications require API 23, where `View.getRootWindowInsets` becomes
available; both SDK-tools packaging and generated Gradle configuration enforce it.
The owner accepts a reply only while its window remains live and the sampled
physical size matches the current native window; a reply taken at an older size
is asked for once more. The window asks on every resize, configuration change,
new native window, `Resume` and inset change `FluiView` reports from
`onApplyWindowInsets`; every 500 ms while resumed only until the view is in the
window. From API 30 the insets are the
visible bars (`getInsets`), so hidden bars reserve no space; below it the stable
insets are the only reading without keyboard occlusion. One query may be
outstanding; a request while it is out asks once more after its reply settles,
since that reply may predate the change. Accepted changes dispatch callbacks after
cache guards are released. `android_safe_area_native_metrics` pins density
conversion, stale orientation rejection and invalid conversion inputs; native
rotation and resume additionally require a device run.

Android printable keyboard keys come from the input device's `KeyCharacterMap`
using the event's native meta-state. Physical codes and named keys keep their
existing translation. A dead key remains `NamedKey::Dead`; unavailable maps
retain the legacy US keycode fallback, including Shift and Caps Lock. A successful
map reporting no character remains unidentified. `android_native_key_map_preserves_logical_keys`
pins the projection, while actual layout and Shift mapping require live input.

### Web subscriptions belong to one actual canvas registration

The canvas owns an exact DOM-object registration claim. Another live owner of
that same node is refused before modifying its layout or title; identifiers
and labels are not ownership. Keyboard listeners on the canvas require both
that source target and effective document/root focus before reading key data.
The existing dispatch result independently controls ancestor propagation and
cancelable defaults. A moved canvas retains ownership inside open or closed
shadow DOM; unrelated host inputs remain outside it.

Each private immutable Fn token owns its body and exact removal identity. JS
wrappers hold weak body references. Close fences delivery before detaching all
subscriptions and disconnecting the resize observer, and preserves admitted
callback/close order through the existing window FIFO. Final owner Drop uses
the same idempotent retirement. A refused detach transfers only its inert
weak wrapper to wasm-bindgen's managed JS lifetime; application captures still
retire. Normal removal drops the token without page-lifetime retention.
Installation rolls back its prefix before Created. ResizeObserver absence uses
the explicit window-resize fallback; failure of that mandatory fallback refuses
opening. The platform frame slot retires by callback-set identity, so stale
owners cannot erase replacements or publish redraws.

Web visibility follows the canvas document's `hidden` value, independently of
focus and CSS layout visibility. Construction samples it; an owned mandatory
Document `visibilitychange` listener updates it. Successful registration
reconciles both focus and visibility before Created, including source changes
during earlier listener installation.

Native focus and visibility observations carry their cache commit in the existing FIFO. A
queued callback therefore observes its own delivered value through getters,
even when input or focus callbacks synchronously trigger opposing transitions.
Accepted native focus and visibility observations also finish their commits and notifications
after a dispatch unwind. Recovery retains the same FIFO so a reentrant
observation follows the already accepted tail; ordinary aborted events are
discarded. `the_owner_thread_machinery_honours_its_contracts` pins single,
opposing and reentrant observations, a competing notification panic and the
next healthy observation through the private backend commit seam.

Native pointer capture custody is published on the exact canvas claim, apart
from its Rust buttons cache. A live body rechecks its admission after DOM
getters, translation and native capture calls. A host call that closes before
actually capturing cannot leave an outgoing capture behind. Outgoing release
checks whether a different current claim admitted that same pointer and does
not release the newer owner's capture. Retirement removes all of its native
admissions before the first release, which itself may reenter the host.
Terminal close, clear and retirement notices finish after an admitted callback unwinds;
ordinary nested events from the aborted transaction remain discarded. The
portable `panics_in_callbacks_and_tasks_leave_the_dispatcher_usable` family
pins capture release, the original failure, a competing retirement failure and
a subsequent healthy owner. Terminal close preserves the application's actual
cancellation hook before clearing captures; ordinary aborted events are not
replayed. An opaque FnOnce environment that double-panics during invocation
before reaching the containment boundary cannot be rescued. These checks do
not establish wasm panic-abort recovery.

On the Web the containment is inert. wasm32 builds with `panic=abort`, so a panic
in a browser callback traps the module before `contain_owner_callback`, the
provider containment or any drop-based cleanup runs; the first failure is the
panic hook's console message and the page stays dead until reloaded. Everything
above that recovers after a caught panic (retained first failure, capture
release, the next healthy owner turn) holds on the targets with unwinding and is
not claimed for browser builds. Web code therefore reports a failure it expects
as a typed error and never relies on a caught panic. Building the target with
unwinding is not an option yet: it needs a nightly toolchain and
`-Zbuild-std`, which the pinned stable toolchain does not use.

Browser compilation alone does not prove the DOM delivery contracts. Headless
source-focus controls are distinct from real system/page focus transitions.
A controlled `Document.hidden` source is not a real background-tab transition.
Constructor events do not establish physical keyboard layouts or OS IME.

### Apple displays are resampled on events and keyed by display, not by connection

`display_snapshot` runs on every owner wake, so the macOS and iOS sources keep
the last sample and read the screens again only after something marked it
stale.

**macOS.** `NSApplicationDidChangeScreenParametersNotification` (attach,
removal, mode, refresh rate, arrangement, scale) is the invalidation: the
handler stores one atomic flag and wakes the owner, and the owner samples on its
own turn. A window publishes its state, the display among it, from
`windowDidChangeScreen:` and from a backing-property change, and marks the
display snapshot stale first because
the snapshot carries each display's scale. A window that resolves its screen to
a display the published sample does not hold (a monitor plugged in since)
marks it stale too. The key is `CGDisplayCreateUUIDFromDisplayID` under
`DisplayKey::MacosUuid`, not the `CGDirectDisplayID`, which changes when the
same monitor is replugged. A screen with no `NSScreenNumber`, or no UUID, is
left out of the snapshot (a placeholder key would be shared by two such
screens), and a window on it reports no display. The function lives in
ColorSync, so the backend links that framework as winit does.

**iOS.** The source retains each `UIScreen` it numbers (`ScreenLedger`) and
keys the display `DisplayKey::IosScreen(number)`. A screen is recognised by
object identity against the retained ones, never by address: a released
screen's address can be reused, and its number is not. Screen and scene
notifications invalidate the snapshot; a window re-reads its scene's screen
when layout runs, since a scene can move to another screen without a new
attachment. The period in the snapshot is the cadence the window's
`CADisplayLink` measures (`targetTimestamp - timestamp`), published for the
screen the window is on only when it leaves a 5% band around the last
published value, so an adaptive ProMotion panel does not republish every tick.
Nothing is reported before the first tick (not the maximum), and a link that is
invalidated takes its period back.

**ProMotion.** An iPhone caps the link at 60 Hz unless the bundle sets
`CADisableMinimumFrameDurationOnPhone`, which `flui build` writes into the
`Info.plist`. On iOS 15 and later the link is asked for
`preferredFrameRateRange` 60 to `maximumFramesPerSecond` (a maximum under 60
lowers the minimum with it); before iOS 15 the property does not exist and the
link gets `preferredFramesPerSecond` instead. The platform reads
`NSProcessInfo.operatingSystemVersion` once into a value it owns (no `static`),
and the range call takes a `Since<FrameRateRangeProperty>` proof that only the
version check makes, so it cannot be written without the check. The 60 Hz
floor and the 5% band are choices to confirm on a device; no Apple code runs in CI.
macOS has no version-gated call here, so it reads no version.

The rules (UUID keying, screen ledger, version proof, frame-rate request, band)
are pure modules of `display::apple` and `version::gate` built for tests on
every host, so Linux CI runs them; the UIKit and AppKit glue around them is only
type-checked: no Apple code runs in CI.

### Native text sizing captures conversion inputs on the owner lane

`HostWindow` and `OwnerPlatform` acquire an owner-local `CapturedTextSizing`;
native capsules remain in the backend and app composition root. Foundation
requests identify validated authored size and growth profile, and replies carry
only validated logical sizes. A retained capsule does not authorize publication
after presentation or capture replacement (ADR-0183).

Android copies `DisplayMetrics` before applying the native SP converter and
dividing by the captured density. UIKit retains the complete trait collection
and uses each profile's `UIFontMetrics` with a regular system reference font to
obtain numeric point sizes; portable shaping still selects the authored face.
Neither backend samples a sparse curve or substitutes a scalar for missing
native answers.

Each host maps the system text size the way its OS exposes it, and the
mapping claims no more than that:

| Host | Observation | Policy the runtime applies |
|:--|:--|:--|
| Android | the SP converter for each authored size, which carries the OS's own nonlinear growth | exact answers |
| iOS | `UIFontMetrics` for each growth profile | exact answers |
| Windows | one `UISettings::TextScaleFactor`, 1 to 2.25 | `TextSizing::linear` |
| Linux, GNOME | the portal's `text-scaling-factor`, which GTK applies to every font size alike | `TextSizing::linear` |
| macOS, other Linux desktops, web | none | `TextSizing::linear(1.0)`, the authored size |

Windows publishes only the scalar. WinUI scales larger text less, by a curve
it does not document, so FLUI's large text grows more than a WinUI control's at
the same setting. FLUI applies the scalar as published rather than guess the
curve, and does not count the linear mapping as native parity. GNOME's factor
is linear in GTK too, so there the mapping is the native one. An exact host
never falls back to the scalar for a missing answer (ADR-0183). A subtree
or a style opts out with a fixed policy or a fixed intent; the paragraph
resolves each span once, from its own authored size, which
`the_system_text_scale_grows_each_span_once` pins.

The private `android_capture_delivery_receipt` matrix exercises dropped and
stale receipts, bounded retry, settled inboxes, deadline expiry and late healing
through the acquisition protocol. It also runs on a non-Android test host, which
does not execute JNI conversion. Native Android and UIKit conversion require
their SDK and device verification; portable receipt tests do not establish it.

### Owner capability boundaries have external compiler witnesses

`OwnerPlatform` stays owner-local, and callers obtain its thread-safe residual
through `shared`. A text-store host is reached through that owner capability;
the hidden token cannot be imported or forged with `Default` outside this crate.
`trybuild_ui::ui_tests` pins the external caller diagnostics alongside valid
owner-local access and `SharedPlatform` thread bounds. It also checks each
retired tracker, embedder and timestamp name independently (ADR-0082 §5).

### Win32 mouse samples preserve their queued generation times

Each window rebases `GetMessageTime` onto the shared process epoch using
`GetTickCount64`. The 32-bit message tick is extended by a wrapping subtraction
from current uptime, so an uptime rollover cannot compress a queued gesture.
Messages older than an entire 32-bit wrap cannot be distinguished by Win32's
timestamp. Sent messages inheriting an older retrieved timestamp cannot move
the window's clock backwards. The sample timestamp is committed before hover
callbacks or native capture changes can dispatch another message.

Keys are stamped from the same clock, read before the keydown drains its
characters, so a key and a click queued behind a long frame keep their order.
The clock keeps one floor for every stamp the window delivered: a `WM_POINTER`
reading or a touchpad step stamped from the performance counter raises it, so
a key in the same system tick, whose millisecond tick reads earlier, is not placed before the
reading delivered ahead of it (`a_coarse_stamp_never_precedes_a_delivered_precise_one`,
and the key rounds of `focus_loss_ends_a_touch_and_silences_its_tail`), and a focus
loss stamps the cancels of the other contacts after the touchpad's
(`a_focus_loss_stamps_its_cancellations_in_order`).

`queued_mouse_samples_keep_native_message_time` drives the public native window
with delayed queued mouse samples, `queued_key_events_keep_native_message_time`
with a key down, a mouse move and the key up. The private arithmetic row
`message_clock_preserves_wrapping_samples_and_window_epochs` covers rollover,
older sent timestamps, epoch rebasing and saturation: there is no public API
that advances the OS uptime by 49.7 days. These checks do not establish physical
mouse delivery, pen/touch translation, or other backend clocks.

### A Win32 mouse is one pointer whichever message carries it

Under `EnableMouseInPointer` a mouse's presses arrive as `WM_POINTER` with the
mouse's pointer id, but a cursor moved by `SetCursorPos` (or a host that keeps
the legacy route) arrives as `WM_MOUSEMOVE`, and the wheel always arrives as
`WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`. Every one of them reads as the one reserved
mouse contact (`MOUSE_CONTACT` in `events.rs`), so a consumer sees the hover, the
presses and the scrolls of one pointer. The injected recording
`win32_sent_mouse_and_wheel.jsonl` pins it: run on a Windows host, its live
session (`a_captured_native_mouse_session_replays_to_what_it_delivered`) fails
against the golden when a `WM_POINTER` mouse id is its own contact. A wheel turn
goes through the window's registry (`ContactRegistry::wheel`) only so the native
capture journals it; it belongs to no contact sequence.

The window's hover (the snapshot's `hovered`, the immediate cursor apply)
follows the mouse on either route through one `WindowContext::set_hovered`,
which publishes (`WindowCallbacks::report_hover`) only when the hover changed: `WM_MOUSEMOVE`/`WM_MOUSELEAVE`
on the legacy route, and on the `WM_POINTER` route every mouse message but a
capture change enters and `WM_POINTERLEAVE` leaves. The hover is settled before
the input of its message reaches user code, so a callback that pumps a message
bringing the cursor back is not undone (`a_leave_callback_that_hovers_again_keeps_the_hover`).
The system captures a mouse press on
the `WM_POINTER` route itself, so a drag released outside the client area still
delivers its release. A press dragged outside sends no leave while it is held,
and no pointer message follows a `WM_POINTERCAPTURECHANGED`, so on a capture
change the hover is read from the cursor (on the client area of this window,
the window on top there). `a_pointer_routed_mouse_hovers_the_window` drives both with
`SendInput` moves (never `SetCursorPos`, which arrives as `WM_MOUSEMOVE`).

Losing focus (`WM_KILLFOCUS`) or being minimised (`SIZE_MINIMIZED`) ends every
open contact on the window as `FocusLost` and cancels an open touchpad
manipulation without pausing the viewport, since an inactive window scrolls
under the cursor; the releases may go elsewhere, and what still arrives for an
ended contact is an orphan. `focus_loss_and_minimising_end_the_held_press`,
`focus_loss_ends_a_touch_and_silences_its_tail` and the viewport row
`focus_loss_cancels_without_pausing` pin it.

### Win32 pointer readings keep counter resolution

`POINTER_INFO.dwTime` advances with the ~15.6 ms system tick, so a coalesced
120–240 Hz pen or touch burst from `GetPointer*InfoHistory` shares one value and
a velocity fit would see zero intervals. Each reading is stamped from its
`PerformanceCount` instead, rebased onto the shared epoch through a counter
reading taken beside the window clock's epoch (`Instant` is the same counter on
Windows). `dwTime` stamps a reading only when its source reports a zero count
or the counter could not be read. Conversion runs in 128-bit arithmetic and
saturates at the epoch range.

The current reading is the newest history entry and is decoded with it, so the
event and its last coalesced sample carry one time.

A pen is placed from `ptHimetricLocationRaw` through its device's
`GetPointerDeviceRects`, read once per message, so a stroke keeps the fraction
of a pixel the integer `ptPixelLocationRaw` drops. A mapping that is missing,
empty, or more than a pixel from the reported pixel (a rotated display maps the
axes otherwise) leaves the pixel (`native_pen_position_keeps_himetric_precision`;
`synthetic_touch_and_pen_reach_native_pointer_dispatch` holds each injected pen press
against its own himetric reading and device rectangles within 1e-6).

Each pointer of a multi-touch frame is still decoded from its own message, so a
two-finger frame reaches a recognizer as two steps; one step per frame
(`GetPointerFrameInfoHistory`, `SkipPointerFrameMessages`) needs a frame unit in
the shared pointer vocabulary first.

The private `native_pointer_decoding_contracts` rows
`coalesced_burst_is_ordered_by_performance_count` and
`current_reading_is_stamped_from_its_performance_count` drive the producer's
history decoding with one shared `dwTime`; the `reading_without_*` and
`count_*_epoch_*` rows pin the fallback and range. No public API synthesizes a
native pointer history. They do not establish physical pen or touch delivery.

### Browser capture ends each admitted contact once

The canvas requests native pointer capture before delivering Down. Its
owner-local admission set is updated before release or input callbacks:
Up and browser Cancel consume admission, and a capture loss cancels only a
still-admitted contact. The loss following a normal terminal event stays inert,
and a later contact can reuse the pointer identity. Native capture refusal is
reported through diagnostics without discarding the original Down. Synthetic
DOM events do not create native capture admission.

The DOM's floating-point `offsetX`/`offsetY` getters supply logical pointer and
wheel positions. Local wasm-bindgen getters preserve their CSSOM View double
precision where web-sys exposes the historical integer signatures. Some
browsers round wheel offsets themselves. For a canvas whose whole ancestry
has no CSS coordinate transform or zoom, viewport coordinates minus its border
box origin and border widths recover the padding-edge position, including
fractional origins. A transformed canvas retains the browser's local offsets;
its wheel precision remains browser-limited rather than applying an incorrect
bounding-rectangle inversion.

Browser cancellation, duplicate capture loss and recovery, fractional
pointer/wheel positions, and a trusted drag whose release occurs outside the
canvas are not covered by automatically executed CI tests; a wasm compile does
not prove them. Browser touch-action policy remains page-owned.

### Web events are answered while the browser waits on them

A DOM event's default can be cancelled only inside its listener, so every
canvas listener that dispatches applies the result there: `propagate: false`
stops propagation to the canvas's ancestors (never same-target listeners),
`default_prevented` cancels a cancelable default. Wheel scrolling follows the
result instead of always cancelling page scroll; the FLUI runtime answers
pointer input handled, so a FLUI page still never scrolls under its canvas,
while an embedder whose callback declines lets the page scroll.

A trackpad pinch reaches Chromium and Firefox pages as Control-held wheel
events and WebKit pages as `gesture*` events (`input::web::pinch`). A
Control-held sequence is classified by its first event (a pixel `deltaY` with
no `deltaX` that is a whole number neither of CSS pixels nor of device pixels
is a pinch, anything else a scroll: a mouse detent at a page zoom is
fractional CSS pixels but whole device pixels) and each step
continues the sequence as its first was. The step factor inverts Chromium's
`-100 · ln(scale)` encoding. Control is stripped from the pinch's modifiers,
since the browser synthesizes it. The End is a deadline on the event
timeline, the last step plus `PINCH_GAP`, delivered by a registration-owned
timer or by the next input that passes it, whichever comes first, and stamped
with the deadline either way. A Control-less wheel ends it at its own time.
When the application does not prevent the default of a sequence's first
event, the pinch it started is cancelled and the rest of the sequence reaches
neither the application nor `preventDefault`, so the browser zooms the page.
The capability is `Emulated` where ctrl+wheel is the source and `Native` where
`GestureEvent` exists. WebKit's cumulative scale and degrees become per-step
factors and clockwise radians; web-sys has no `GestureEvent`, so the crate
binds its getters. A gesture that starts while a touch is open is left to the
touches: WebKit on a touch screen reports a two-finger pinch both ways. A pen
whose browser reports only `tiltX`/`tiltY` (Pointer
Events 2) keeps them as tilt planes, read through fractional getters.

The mouse is one contact whatever `pointerId` an engine gives it (1 in
Chromium and WebKit, 0 in Gecko), reserved pointer identity one, and a wheel
event, which names no pointer, scrolls as that pointer; touches and pens take
identities from two. A mouse or pen entering or leaving the canvas outside a
press is an `Enter` or `Leave`, and a pen that leaves is out of range and its
contact ends. When the page loses the OS focus (`blur` at the window) or is
hidden, every open sequence is cancelled with `FocusLost` and its capture
released: the browser may never deliver its release. The browser's context
menu is answered as the application answered the secondary press that opened
it; one no secondary press opened (the menu key) is prevented.

`web_pinch_table` pins the decoders in Linux CI. `tests/web_browser.rs` runs
in headless Chrome:
`a_wheel_scroll_is_answered_with_its_dispatch_result`,
`a_ctrl_wheel_pinch_reaches_the_application_as_a_pan_zoom` (End from the
timer, stamped with the deadline), `a_refused_pinch_is_left_to_the_browser`,
`webkit_gesture_events_reach_the_application_as_a_pan_zoom`,
`a_touch_pinch_is_delivered_once`, `a_press_the_page_loses_is_cancelled`,
`a_wheel_scrolls_as_the_mouse_and_never_as_a_touch`,
`a_hovering_pen_ends_where_it_leaves`,
`the_context_menu_is_answered_as_its_secondary_press` and
`a_pen_reporting_only_tilt_planes_keeps_its_orientation`. Those events are
constructed, not trusted: no trackpad, Safari or page zoom has run
(`a_mouse_detent_at_a_page_zoom_stays_a_scroll` pins Chromium's documented
zoomed detent in the decoder).

### Web drag and drop is HTML drag and drop, answered in its events

`drag::web` holds the platform's one `DataTransferSource`,
`WebTransfer`, and each canvas's `WebDragSource` (ADR-0211 §11). A drag over
a canvas mints an offer from `DataTransfer.types` and its file items: the
browser hides the data until the drop, so a hover reads only types, and
FLUI's session identity travels as a type with its parameters. `dragenter`
is cancelled when the input callback prevents its default, which makes the
canvas the drag's target; `dragover` additionally needs the target's cached
feedback to accept, and its `dropEffect` names the one accepted action. The
drop copies every string representation out and keeps its files before the
event returns; requests are then served from memory, a file through
`Blob.arrayBuffer`. `conclude_drop` and `dragleave` retire the offer.

A browser starts an HTML drag only from its own drag gesture on a draggable
element and allows no data after `dragstart`. A canvas with a source is
draggable; its `dragstart` asks synchronously whether the runtime started a
session on this mouse or pen press, before the browser's drag threshold (a
long-press trigger, typically), and then stores every representation the
request names through `setData` and its actions as `effectAllowed`. Otherwise
the `dragstart` is cancelled and the press continues as pointer input, which
Chrome keeps delivering, and a session the runtime starts later in that press
is refused as `DragError::Unsupported` (`Unavailable`) so it stays in its window. A waiting
session no `dragstart` takes by the release ends `Cancelled`; `dragend`'s
`dropEffect` is the end it reports. A provider that panics at `dragstart` is
contained and reported (`drag::web::mapping::serve_stored`), its session
ends `Cancelled` and the `dragstart` is cancelled, since no `dragend` follows a
refused drag. Close restores the canvas's draggable attribute and ends a
waiting or running session cancelled.

`html_drag_drop_table` pins the mapping in Linux CI. In headless Chrome,
`an_html_drag_over_the_canvas_is_offered_and_dropped`,
`a_declined_drag_is_refused_and_its_leave_retires_the_offer`,
`an_unprevented_dragenter_leaves_the_drag_to_the_page` and
`dragstart_runs_a_waiting_session_and_refuses_otherwise` use constructed
`DragEvent`s with a real `DataTransfer`, whose effects a constructed object
ignores, so the harness supplies them as plain properties. With trusted input
over WebDriver BiDi Chrome itself runs both
directions within one page: a session started from the press's dispatch
becomes an HTML drag a page element takes the text of, and a page element's
drag drops onto the canvas, whose target reads it after the drop. The drag
source's press listener precedes the dispatching one, since the runtime's
quiescent point is still inside that dispatch.

### The web window serves the focused field's store to the browser's input method

`text::web::WebTextInput` is the web window's
`TextStoreHost` (ADR-0090, ADR-0135), reached through
`OwnerPlatform::text_store_host` as Win32's text services are. The focused
field's store is mirrored into an `EditContext` on the canvas where the
browser has one (Chromium; web-sys binds none, so the crate declares it) and
into a hidden `<textarea>` beside the canvas where it does not. The browser
edits its copy and reports each edit (`textupdate`, or the textarea's
`input`, diffed against the text the host last gave it by
`text::ime::diff`, the mirror's one edit); the host applies it to the store under an
asynchronous lock, the text a composition inserted marked composing until
`compositionend`. The application's changes reach the copy through the store's
observer. Offsets are UTF-16 code units throughout. A request the presentation
makes from inside a store call the host started (focus, completion) waits and
runs, in order, when that call returns. The browser has no call that commits a
composition, so `complete_composition` re-attaches the surface, which makes
the input method let go, and answers `Abandoned` for the caller to commit in
place; a completion that waited commits in place itself.

A key the input method processes is the input method's: it reaches the window
marked `ImeComposition::Active` when its event composes, when it carries the
input method's key code 229 (WebKit dispatches the key that ends a composition
after `compositionend`, with `isComposing` false), or while the surface
composes (`input::web::keys::key_composition`). No shortcut, focus traversal
or drag cancellation takes such a key, and its default stays the input
method's, so Escape cancels the composition first. A key the application
handles has its default prevented, so the browser does not also type it into
the surface.

The textarea belongs to the canvas's focus area: it is named on the canvas's
claim (`EditorSlot`), focus moving between the two is not a window focus
change, its keys reach the window through the canvas's key route
(`CanvasKeys`), focusing the canvas forwards focus to it, and detaching gives
focus back to the canvas.

`the_purpose_picks_the_input_mode`,
`a_key_of_the_input_method_is_marked_composing` and `mirror_text_table` pin
the rules in Linux CI, and the conformance table's
`a_key_during_composition_is_the_input_methods` holds them for the web
profile. In headless Chrome, `an_edit_context_serves_the_focused_field`,
`escape_while_the_field_composes_is_the_input_methods` and
`a_textarea_serves_the_field_without_edit_context` drive constructed events;
a manual run drove Chrome's own input-method path
(`Input.imeSetComposition` and `Input.insertText` over ChromeDriver's DevTools
channel, a trusted Escape and key over BiDi) through both surfaces. No OS
input method, candidate window or soft keyboard has run.

### The Windows display snapshot is cached and invalidated, not sampled per turn

The host reads the snapshot on every owner turn, and one sample costs an
`EnumDisplayMonitors` pass, a `QueryDisplayConfig` and a few
`DisplayConfigGetDeviceInfo` calls. `WindowsDisplays` therefore answers from its
last sample until a flag set by `WM_DISPLAYCHANGE` or a `SPI_SETWORKAREA`
setting change (both on the hidden broadcast receiver, whose
`DisplayBroadcasts` subscriber handles nothing else of the display: it sets
the flag and wakes the owner, because the message can arrive inside a COM
call), or a window's `WM_DPICHANGED` says the displays may differ. The flag is cleared before the read, so an invalidation that lands
during a sample survives it. winit on Windows takes its snapshot from the same
source (its monitor list and GDI-name key are not read) and re-samples where
winit reports a move, a scale change or a resume.

The refresh period is the path's `targetInfo.refreshRate` rational, converted
through `RefreshPeriod::from_ratio` so 60000/1001 stays exact. A monitor whose
path has no rate (0/0) or has no path takes `EnumDisplaySettingsW`'s
`dmDisplayFrequency`, where 0 and 1 mean unknown. `QueryDisplayConfig` passes
`QDC_VIRTUAL_MODE_AWARE` only with the `Since<QdcVirtualModeAware>` proof `WindowsBuild`
hands out for build 18362 and later; the build comes from `RtlGetVersion` once
and lives in the source, not in a `static`.

A window's display is the sampled monitor whose handle equals
`MonitorFromWindow(MONITOR_DEFAULTTONEAREST)`, resolved through that monitor's
native key. The lookup reads the last sample and never takes one, because the
host calls it while deriving window displays from a snapshot it already
accepted. A handle the sample lacks (a monitor plugged in since) answers `None`,
never the primary, and invalidates the snapshot once per revision (a repeated
miss costs nothing), so the next owner turn samples it. The window's display
is a field of its state snapshot, read again on `WM_WINDOWPOSCHANGED` (the
default procedure derives `WM_MOVE` and `WM_SIZE` from it) and on
`WM_DPICHANGED`, which holds publishing until the new rectangle has landed.
An equal sample keeps the revision, so a drag on one monitor announces no
display change, and a move across monitors of different DPI announces one
snapshot per change. Rows: `os_display_classification` and `os_capability_table`
(pure), `display_change_message_invalidates_the_snapshot_and_wakes_the_owner`
(in `src`, it counts samples, as do the DPI and work-area rows) and the
native contract rows `refresh_is_the_rational_active_mode`,
`window_display_is_a_display_of_the_snapshot`, `a_dpi_change_publishes_the_new_scale_once` and
`moves_within_one_monitor_keep_the_display`. A transition between two monitors needs a second monitor and
is not a CI row; it ran by hand (see
`docs/platform-parity.md`). Unplugging a monitor on Windows was not run.

### Windows preferences have a window-independent owner lifetime

`Platform::preferences` reads text scale, animation preference, high contrast,
double-click interval, discrete wheel distances and ordered UI languages before
any user window exists.
Unknown observations remain absent. The owner context holds one WinRT STA entry,
native interfaces and subscriptions. Foreign reads are refused. Direct
`RoActivateInstance` avoids generated process-wide factory caching: recreating a
host after its apartment ended reproduced a stale-factory access violation with
`UISettings::new`. `windows_reads_preferences_before_a_user_window_exists` in
`preferences_contract` exercises successive host lifetimes, refusal, repeated
native reads and subsequent native window creation. Its independent `UISettings`
reader compares the public motion observation with the current Windows animation
setting; merely returning a present value cannot satisfy the assertion.

UI languages use `GetUserPreferredUILanguages`, sharing this source's broadcasts,
cache and retry lifetime (ADR-0173). The private
`preferred_language_buffer_contract` injects capacity races and malformed UTF-16
lists that cannot be produced deterministically through the public OS API. It
pins ordered complete tags, count/terminator validation and bounded retries;
failed query outputs never become a preference observation.

The same source samples mouse rectangle/drag metrics at an explicit 96-DPI
bootstrap context without user windows. Those raw observations only invalidate
consumers. Each presentation queries `GetDpiForWindow` and
`GetSystemMetricsForDpi` for its live HWND; scaling a representative window's
metrics cannot reproduce this query. Full double-click dimensions and drag
half-extents remain distinct, and legitimate zero axes are retained. The public
native row compares the projection with independent Win32 reads and checks
foreign-thread and closed-window refusal.

The source's hidden top-level HWND receives setting broadcasts outside user-window
membership and exit policy. WinRT text-scale and animation observers and the HWND
mark a refresh obligation and wake the owner; they never invoke runtime code.
Registering a new owner hook also wakes it if a refresh remains pending. Reads
clear the prior obligation before sampling so an invalidation during a read is
retained; error or unwind restores the obligation. A clean source reuses its last
observation. Reentrant reads are refused while native getters run, including when
an older cached observation exists; a failed refresh is returned as an error,
never as a successful stale observation. The private getter-injection table
`native_preference_read_recovery` covers cache reuse, in-read invalidation,
reentrant refusal and recovery after errors and panic. This private seam injects
failures that the public OS getters cannot deterministically produce.
An unfinished read also arms a 100ms source retry deadline. The source
admits failed-read attempts no more often than that interval even when
unrelated wakes arrive. Early and reentrant reads return `PreferencesDeferred`,
which publishes no observation. A retry wake does not postpone the read's
eligibility; healthy changes are not delayed. The deterministic row
`unrelated_wakes_do_not_repeat_failed_native_reads` covers this admission boundary.
The native message loop includes the retry in its timed wait independently of frame deadlines and wakes
the owner when due. It rearms before posting and disarms after a clean read;
shutdown excludes the deadline. `preference_failure_retries_without_a_user_window`
injects native-getter errors and panic, runs the actual Win32 loop without user
windows, and checks recovery followed by quiet idle.
Shutdown closes notification admission before removing subscriptions, whose
captures contain only ref-counted inert state.

`setting_messages_wake_the_owner_without_a_user_window` sends a message to the
platform's own native receiver and checks owner delivery and teardown. It does not
change OS preferences or establish actual OS-generated notifications, value-change
delivery or retry after a failing owner consumer. Those remain
pending under ADR-0172. The headless row checks that absent observations do not
claim OS defaults.

`native_setting_message_replaces_the_cached_observation` strengthens the native
message proof: a private getter seeds a distinguishable cached scale, then a
message to that exact receiver must cause a real OS read on the registered owner
callback. Removing only invalidation leaves the stale value and fails the test.
The OS configuration is never changed; this proves message-to-observation delivery,
not an actual OS preference change or a rendered native application's update.

### Native gesture sampling follows public API refresh limits

Browser motion observation and owner delivery follow ADR-0196. One host-owned
media query invalidates the existing signal; its callback runs on a later
microtask and remains independent of canvas creation. The public WASM case
`browser_motion_observations_wake_replace_and_retire_their_owner` covers the
adapter's initial observation, delivery, replacement and retirement through a
controlled JS boundary. It is not an actual OS-setting change test.

The same host observation set also reads motion. AppKit samples
`NSWorkspace::accessibilityDisplayShouldReduceMotion` on its existing owner-lane
timer. The same timer also serves the macOS winit fallback: `WinitApp` owns the
source and the platform stores only a weak lookup. Reads lease it outside platform state;
no AppKit query is added to `about_to_wait`. Shutdown and unwinding fence the
signal before retiring that source. Initial native observations,
foreign-thread refusal and reentrant retirement without a user window need a
macOS main-thread process; local cross-compilation does not establish them.

Android reads `Settings.Global.ANIMATOR_DURATION_SCALE` through the
Activity's ContentResolver during preference sampling, validates it before
publication, and leaves geometry-only queries independent of that setting.
UIKit retains both bold-text and reduced-motion notification tokens on one
owner; notifications only wake that owner, which samples the getters and checks
admission before publishing. Successful registrations acquire ownership
immediately so partial registration unwinds retire earlier tokens. Closure makes
all callbacks inert before taking tokens out of the borrow and removing them.
These sources feed the existing ADR-0172 delivery path and ADR-0195 projection.
Local cross-target clippy compiles these native paths; it does not execute their
OS getters or external-setting notifications.

Linux winit owners use one Settings portal connection on the existing background
executor. Setting and service-owner subscriptions precede the initial read;
the accepted snapshot is committed before waking the owner. Initial bootstrap
waits at most 500ms before `on_ready`; unavailable or stalled observations stay
unknown and can recover later. No D-Bus call runs in the frame path. Successful
unsigned `org.freedesktop.appearance/reduced-motion` values map 1 to Reduce and
other values to NoPreference. A missing key permits the GNOME
`org.gnome.desktop.interface/enable-animations` fallback; transient errors
preserve the last accepted snapshot. Settings v1 uses its double-variant `Read`
reply. Healthy observations use notifications; failed reads retain a bounded
retry that unrelated notifications cannot postpone. Service-owner changes
refresh the same observation set. Shutdown fences owner admission before
cancelling the task, whose state holds no strong owner/source cycle.

The same connection reads `org.freedesktop.appearance/color-scheme`: 1 is dark,
everything else (no preference, prefers light, a missing key) light. It is kept
beside the snapshot, not in it, because brightness is presentation-specific
(ADR-0172): the owner's wake hands the accepted value to each window, and a
window with no winit theme of its own (X11 always, Wayland without an app
override) publishes it in its window state, a new revision only when it
changed. One read failing neither discards nor delays the other's accepted
value.

The snapshot's `appearance` is the user's theme on every backend, never a
value the backend chose for its own chrome, and `on_state_changed` announces
it only when it changed. Windows reads `AppsUseLightTheme` and sets the DWM dark
title-bar flag to match it on creation and on each `WM_SETTINGCHANGE` that
changed it, until the app picks the chrome through `Win32Window` or opens the
window with a theme of its own (`Win32WindowOptions::theme`;
`set_theme(System)` hands it back); reading the flag back would report the
backend's own choice. macOS
resolves the window's `effectiveAppearance` against Aqua and DarkAqua, with the
content view's `viewDidChangeEffectiveAppearance` as the change source. iOS
samples `userInterfaceStyle` on construction, on `traitCollectionDidChange:` and
on `didMoveToWindow`, apart from the window metrics: the theme needs neither a
laid-out size nor an attached window. Android reads the
configuration's night mode on `ConfigChanged`, and the Web listens for
`prefers-color-scheme`'s `change`.

The process-isolated `linux_preferences` table runs through public
`WinitPlatform` owner capabilities with a private session bus. It checks initial
sampling without a window, live changes, unknown values, unsupported observations,
GNOME and legacy portal reads, service replacement, failed-read recovery without
another notification, foreign-thread refusal and reentrant capture retirement.
`libtest-mimic` keeps execution on winit's required main thread and supplies the
Cargo/nextest protocol. These tests execute Linux D-Bus and winit, but do not change a desktop's
actual OS settings or establish live behavior of the other native platforms.

Thread-safe residual handles may outlive the event-loop owner. The background
executor's final shared owner requests nonblocking runtime shutdown, so releasing
a retired proxy inside another async runtime or inside the executor's own task
cannot invoke Tokio's blocking runtime destructor. This retains normal runtime
cancellation of queued async work; already executing blocking work finishes
independently. `background_executor_retirement` exercises both contexts through
the public executor, and the Linux table's `late-proxy` case covers the actual
preference source and stopped host.

AppKit observes `NSEvent::doubleClickInterval` on the application owner lane.
Android observes public `ViewConfiguration` timeouts and physical touch/fling
metrics using the Activity context and its resource density; its presentation
query samples that context again. Neither backend invents unsupported mouse or
touch geometry. iOS and web retain unknown numeric gesture observations.

AppKit posts nothing when the double-click interval changes. The macOS host
wakes its owner on the workspace's accessibility display-options notification
(reduce motion, increase contrast), the current-locale notification (preferred
languages) and the application becoming active, and the owner samples every
value again. The interval therefore refreshes when the user comes back from
System Settings; a change written while the application stays active waits for
its next activation. The observers hold only a weak owner signal and end before
the source, with the platform or at the winit loop's shutdown. A failed or
unwound read schedules one owner wake at its retry deadline, since no timer
would otherwise come back for it.
Android has no single notification for its observations either. Configuration
values (locales, font scale and weight, density) invalidate on `ConfigChanged`,
whose accepted changes are delivered before subsequent input. `FluiView` reports
the rest: content observers on the animator duration scale, touch feedback,
long-press timeout and high-contrast-text settings, the `UiModeManager` contrast
listener from API 34, and `onApplyWindowInsets`. Each reaches the owner as a view
event that reads again. The view observes only while attached to the window: it
registers in `onAttachedToWindow` and then reports once, so a change made before
observation began is read too, and unregisters when detached and in `detach`,
which clears its handle first. A view that never reaches the window, or that the
native side detaches because it could not keep it, leaves nothing that could call
a freed handle. Until the view is in the window, or if it never gets there, the
host reads every 500ms while resumed, so no change is lost. The long-press
timeout is read from its secure setting, since `ViewConfiguration`'s copy can
still be old when the observer fires. A failed read owes a retry at its deadline,
which bounds the loop's wait like the cadence does, so errors preserve the
accepted observation and the next refresh attempt.
AppKit registers its observers before its first native getter. A cold source has
no accepted observation; an initial query error cannot abort platform bootstrap,
publish a fabricated default, or discard its retry. The shared production read
path's `cold_native_observation_recovers_without_forged_defaults` covers failed
initial observation, deferred retry, first successful publication and recovery
without a stale-success response. This portable seam does not execute AppKit's
native constructor or its notification observers.
Successful recovery requests owner delivery even when the comparison value is
unchanged: a prior failed consumer read must not strand the accepted observation.
Healthy unchanged observations remain quiet.
Failed or unwinding native reads restore a 500ms admission deadline; unrelated
owner wakes cannot repeat the getter early. Reentrant reads return
`PreferencesDeferred` before native work, and foreign calls do not affect that
admission. `bounded_native_read_admission` exercises this portable production
guard through failure, competing admission and recovery after unwind.

Android API26 public horizontal/vertical scroll factors normalize fractional
axis values to logical pixel deltas exactly once. API21–25 resolve the public
`listPreferredItemHeight` theme attribute through the Activity's display metrics,
without hidden API reflection; this is the public
[AndroidX compatibility path](https://android.googlesource.com/platform/frameworks/support/+/34ede8799a022385ca94a1bc111f978ac2e65f45/core/core/src/main/java/androidx/core/view/ViewConfigurationCompat.java).
These pixel packets bypass discrete wheel preferences. A failed refresh keeps
the accepted logical factors; when no factor was ever observed, an explicit
authored compatibility policy treats each axis unit as one line. That fallback
does not claim an OS factor. `android_axis_factor_policy` runs the actual portable
cache/normalization seam and covers API selection, fractional values, failed
refresh, recovery and invalid projection. It does not prove execution on an
Android device. The AppKit/Android native sources require device validation in
addition to cross-target type checking.

### Android text weight shares the Activity preference source

The preference aggregate reads `Configuration.fontWeightAdjustment` through the
Activity's resources on API31 and later. Earlier SDKs never query the missing
field. Android's undefined sentinel is unavailable, zero is known no preference,
and signed adjustments remain exact until the text consumer applies its policy
(ADR-0174). JNI errors use the existing aggregate admission and retry path;
geometry-only reads skip this text observation entirely.

`android_text_weight_query_contract` exercises the private production query seam
with unsupported SDKs, undefined and zero values, signed extremes, query failure
and recovery. The public OS interface cannot deterministically inject SDK levels
or field-query failures, which is why this seam is tested privately. Removing
either SDK admission or sentinel handling makes its corresponding case fail.
This portable test and Android cross-target checking do not prove JNI execution
or configuration-change delivery on a device.

### Native Win32 owner delivery survives message-post refusal

Owner turns ordinarily use the internal HWND's posted message, including inside
native modal loops. If posting fails, the transport signals an unnamed auto-reset
kernel event. The FLUI message loop waits on that event alongside its message queue
and frame/source deadlines, including an infinite idle wait without user windows.
Both paths drive the same owner-turn slot and quit policy. A strong `OwnedHandle`
owner spans every wait; closing admission prevents a retired proxy from posting.
The event contains no callback or UI state and requires no polling thread.
`failed_native_posts_still_deliver_owner_turns_and_quit` injects physical post
refusal and verifies worker admission, reentrant continuation and quit through
the actual native loop. If both the post and kernel signal fail, the caller still
receives an error and OwnerSignal retains its debt. A third-party modal loop does
not wait on this private event; fallback delivery resumes when FLUI regains its loop.

### Native Win32 delivers admitted idle deadlines through live window paints

The owner loop keeps admitted deadline delivery separate from the next hook
query. When due, it invalidates each still-current window before a new query can
abandon a late deadline. A finite message wait rounds upwards, reserves the
infinite timeout sentinel, and drains input and quit messages without parking
again after readiness. Hook replacement is checked by allocation identity;
observing no deadline permits later admission of the same absolute instant.
The count of frame callbacks run at delivery is the delivery generation: the
same instant from the same hook re-arms after every frame callback that ran
since its delivery, and never when none did. A frame request no callback
answers (no window, or a window with no frame callback registered, visible or
hidden) is not counted, so a delivery that reaches no frame cannot spin.
Queries and outgoing capture retirement run outside handler borrows. Incoming
unwind retains the queried hook before hostile captures can replace the first
failure; healthy aggregate destruction retains its ordinary Rust limitations.

The bounded native rows `deadline_rearms_independent_windows_without_input`,
`deadline_hook_replacement_rearms_without_input`,
`deadline_query_can_close_an_independent_window`,
`deadline_none_allows_same_instant_readmission`,
`deadline_queued_three_times_at_one_instant_gets_three_frames`,
`deadline_no_frame_can_service_does_not_spin`,
`deadline_window_without_frame_callback_does_not_spin`,
`deadline_hidden_window_without_frame_callback_does_not_spin`, and
`deadline_query_unwind_retains_replaced_hostile_captures` belong to
`test_window_lifecycle_contract`. They exercise the native wait and paint
producer, cancellation, replacement, independent windows and recovery. They do
not establish GPU presentation, UIA timing or other platform backends.

### Windows identity admission precedes native window acquisition

Backend window identities admit the final nonzero value once, then latch zero
as permanent exhaustion. Both normal windows and owner-control windows reserve
identity before creating a native HWND, so a capacity failure cannot leak an
already acquired window. Catching that failure cannot reissue an old identity.

The Windows row `window_identity_exhaustion_permanently_refuses_retries` joins
`the_owner_thread_machinery_honours_its_contracts`. It drives
`WindowIdentity::admit`, the admission `WindowsWindow::new` runs around
`CreateWindowExW`, from a local counter with an acquisition that records
native creation, checks repeated refusal and verifies that refused admission
never acquires. This tests the admission boundary, not recycled HWND behavior.
The platform reserves its owner-control identity before it initializes COM or
creates its message-only window; the row `platform_identity_exhaustion_acquires_nothing`
retries an exhausted platform admission and checks that the thread's COM
apartment and the message-only windows are unchanged.

### The Win32 clipboard opens with a message-only owner window on a dedicated pump thread

`WindowsClipboard` has no single owning thread: any thread may read or
write, and the winit backend has no FLUI-pumped loop. Opening with a `NULL` owner, as `arboard` and FLUI used to,
does not exclude other threads of the process, since all `NULL` openers count
as one owner (see "Win32 clipboard: cross-thread sessions" in
[`docs/safety-review.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/safety-review.md)).

`ClipboardSession` therefore opens with one message-only window
(`HWND_MESSAGE` parent), created lazily on the first session and never
destroyed. The window lives on a dedicated `flui-clipboard-owner` thread whose
only job is `GetMessageW`/`DispatchMessageW`; every message goes to
`DefWindowProcW`. The owner must be pumped because `EmptyClipboard` in any
process *sends* `WM_DESTROYCLIPBOARD` to the current owner and waits for it.

Rejected: reusing the owner thread's loop. It exists only on the Win32
backend, not while a winit loop or a headless test drives the process, and
not before `run`. A session on a worker thread would also block on a
`SendMessage` to the UI thread, which deadlocks whenever the UI thread waits
on that worker. A thread per process costs one idle stack and removes all of
this.

The process-wide `clipboard::lock` stays: `arboard` sessions still open with a
`NULL` owner, and a `WindowsClipboard` session would be let in beside one.
`OpenClipboard(NULL)` also leaves `SetClipboardData` without an owner to
record, which Win32 documents as liable to fail; with an owner it is on the
documented path. If the window cannot be created, sessions fall back to a
`NULL` owner and log an error.

Test: `a_null_owner_open_on_another_thread_fails_while_a_session_is_open`
(fails with a `NULL` owner). The owner thread's message pump is not covered by
a test.

### Win32 callbacks live in the window's owner-thread context; off-owner registration is refused

Registration methods take
`&self` on `Send + Sync` traits, so any thread can call them. ADR-0082 §4 step
one for Win32 makes that safe without changing a signature.

- **Storage.** A window's callbacks (`WindowCallbacks`) live by value in its
  `WindowContext`; the platform-level handlers (`on_quit`,
  `on_keyboard_layout_change`) live as `Rc<RefCell<PlatformHandlers>>` in the
  `OwnerControlContext` of the owner message window, shared by every window
  context; the owner-turn callback lives in that context's `OwnerTurnSlot`.
  Both contexts sit behind `GWLP_USERDATA`, are reached only through the
  owner-thread gates (`with_window_context_checked`, `OwnerControl::shares`),
  and are freed by `WM_DESTROY` / `WM_NCDESTROY` on the owner thread. A
  platform dropped off its owner thread without running keeps the owner window,
  so its `OwnerControlContext` and the handlers in it are leaked, never freed
  on the wrong thread. Both are pinned `!Send + !Sync`.
- **Refusal.** A registration from any thread but the owner, on a window that
  is gone, or on a handle the OS recycled for another window, is refused. The
  callback is dropped right there, on the registering thread, so it never runs
  on the owner and is never freed by a thread that did not create it. A
  foreign-thread refusal logs an error ("registration refused"): the caller has
  just lost a callback. An off-owner `open_window` returns
  `OpenWindowError::Backend`.
- **Restore through the call point.** Dispatching the window-event or
  keyboard-layout handler goes through `owner::call_point::call`: it is taken
  out of its slot, run with no borrow held, and put back only if the slot is
  still empty. A handler registered while it ran — including by itself — wins,
  the rule `OwnerSignal::drive` already follows.
- **Quit order.** The run's teardown keeps a handle to the handler set while
  the owner context exists, destroys the owner window, runs the quit callback
  once (its panic contained), then retires the handler set: the other handlers
  drop contained, or stay alive when the teardown runs during an unwind.

The other backends still store `Send` callbacks behind locks until their own
steps.

Tests (CI only type-checks Win32): the owner-affinity refusal order is pinned
by `refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot`
(run by `the_owner_thread_machinery_honours_its_contracts` in `owner/handlers.rs`), and the callback panic and re-entrancy rules by
`tests/window_callback_unwind.rs` and `tests/handler_reentry.rs`.

### Each Win32 window owns its text services, activated at creation and deactivated by `WM_DESTROY`

A window's TSF connection (`text::windows::TextServices`, ADR-0135 §3) lives in the text
feature's hook on FLUI's window class (`text::windows::Tsf`, a `WindowTable` by the window's
identity), activated on the owner thread once the window's context is installed (the class's
hooks hear the hook's `created` first, before the drop target and the precision touchpad; TSF's
calls read the window's `WindowScale`), so the host exists
before any field can focus; a lazy activation would put the first `ITfThreadMgr::Activate`
inside a focus change, under owner code. A failed activation leaves `None` and the `WM_CHAR`
path; a window a message of the activation destroyed fails its open
(`a_window_destroyed_during_activation_fails_the_open`). The hook deactivates it at the
window's `Closed` step, which `WM_DESTROY` tells after the close callbacks (the presentation's close unfocuses its field through the still
active host) and before the context retires; the shutdown is a host operation, so one that
arrives inside a TSF call into a store waits for that call. Its application-code failures are
logged, contained: the window procedure aborts on a panic. Test:
`a_window_offers_its_text_services_as_its_host`, and the `window destroyed inside a TSF call`
row of `the_text_services_answer_a_completion_for_its_store`.

A field's screen rectangles move with its window, so `WM_MOVE`, `WM_EXITSIZEMOVE` (heard by
the `Tsf` hook before the window's procedure answers them; TSF reads the window's position
from the system) and `WM_DPICHANGED` (told from the window's own handling, once the new scale
TSF reads is applied) tell the focused field's store (`TextStore::screen_moved`), which reports
`OnLayoutChange` by its own notification rule: not while a lock is held or its gate is shut,
since TSF answers with a synchronous lock request. An input method then moves its candidate
window with the text; failures are logged, contained
(`a_moved_window_tells_tsf_its_layout_changed`, a move under a held lock included).

### AppKit reopen signals use a loop-owned serialized callback pump

The owned application delegate implements
[`applicationShouldHandleReopen:hasVisibleWindows:`](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldhandlereopen(_:hasvisiblewindows:)).
It always returns false: FLUI owns window policy and does not ask AppKit to create
an untitled document. Both visible-window flag values deliver the same existing
`on_reopen` signal. Apple's flag counts miniaturized windows as visible; it is
not a substitute for presentation visibility or frame eligibility.

The Objective-C method only records a signal and schedules weak loop control.
A pending count, queued flag and active guard serialize owner-lane delivery,
including callbacks that pump nested native events. Starting-phase signals wait
until the loop runs. The registration current at delivery receives each signal;
a replacement installed before that turn receives it. During delivery, callbacks
are called through the call point outside locks and restored only if no
replacement exists and quit has not been requested. Active remains set through
callback destruction, so a nested signal is retained until its predecessor's call
and cleanup finish.

Explicit quit fences registration, delivery, restoration and window admission
immediately, including before deferred termination evaluation. Old delegates
reference only their old loop; stopped loops cannot act on a later run. Handler
replacement/destruction happens outside locks, preserving the existing
handlers-then-state lock order. Callback, destructor and native/dispatch boundary
panics use hostile-payload-safe containment; a panicking registration does not
prevent another pending signal from reaching its replacement or restored slot.

A manual macOS run separated direct delegate routing from actual LaunchServices
reopen AppleEvents. The latter invoked `open -a` on the exact running temporary
bundle and required callback and normal return in the original PID. That run
covered the platform signal, not a rendered resident application or a framework
root-window creation API. It needs a macOS host with an active GUI session.

### Headless explicit quit consumes its callback outside platform state

`HeadlessLoop::quit` uses the same discipline as its window close and
exit-policy re-evaluation paths: it marks the loop stopped, then calls the
current quit callback once through the call point (`Once`), which takes it out
under the state lock and invokes and drops it after the lock is released. Recursive or repeated quit does not re-invoke that
registration. This is consumption of the existing callback slot, not a new
process-wide latch; registering a later callback still replaces the slot.
Callback panics propagate to the Rust caller after releasing state, and the
consumed callback is not restored. Tests exercise the actual `Platform::quit`
seam with a reentrant getter/quit, a captured-data drop sentinel, and an original
panic payload. Their first nonblocking lock assertion makes a regression fail
immediately instead of hanging while trying to re-enter the same mutex.

### Standalone AppKit stops its loop and returns through Rust cleanup

The existing `Platform` exit-policy hook remains the boundary: `flui-app`
owns UI runtime/service policy; `flui-native` owns window bookkeeping and native
loop actuation. AppKit now implements both policy installation and coalesced,
any-thread re-evaluation. Close callbacks finish before the deferred owner turn
consults the hook. Hooks run outside locks, and both loop phase and window count
are checked again afterward because a hook may open a replacement window.
Reentrant requests get another owner turn; a replaced hook is never overwritten
by restoring the old leased callback. No public policy variant is added.

`LoopOwnership` accepts only a standalone NSApplication: a running application
or existing delegate is rejected before bootstrap or activation mutation. The
constructor merely acquires the singleton. The run scope retains its delegate,
sets Regular activation, and restores the previous policy only while its own
delegate and installed policy are still current. External replacements survive.
A terminate request nobody announced goes into the same deferred quit path and
is answered TerminateCancel, so AppKit does not exit the process itself; a
session end is answered differently (see the section after this one).

Explicit quit bypasses the last-window veto. `stop(None)` plus an application
NSEvent wakes the native wait; quit during bootstrap skips `run()` entirely.
This follows [Apple's stop contract](https://developer.apple.com/documentation/appkit/nsapplication/stop(_:))
and the installed winit 0.30.13 `stop_app_immediately` implementation. This is the
standalone main loop contract; nested modal loops and foreign-loop embedding are
not supported by this owner. Physical keyboard/menu wiring is a separate input
concern, not proved by sending `terminate:` in a probe.

Shutdown first closes admission, clears callback cycles and closes remaining
windows on main, then invokes quit once outside locks. Cleanup also runs on
bootstrap error/unwind. User callback/destructor panics are contained per cleanup
step and logged; exceptional panic payloads are deliberately leaked rather than
risk a second panic during unwinding. Resource cleanup after such a panic is
best effort, while the guard still detaches its own delegate.

Native window release now executes inline on its owner, including after the
loop returns, through the same panic boundary used by lifecycle cleanup: a
panic payload whose destructor also panics is retained without destruction
while ordinary local resource destructors still execute. `windowWillClose:` pins its receiver with an extra retain transferred
to the surrounding autorelease pool before callbacks can drop the final wrapper.
That prevents deallocation under the native close stack. Worker drops remain
nonblocking queued tails; if the owner no longer services them, that existing
fallback can leak native resources and never releases them off-main.

**Verification:** a manual macOS run staged a fresh bundled subprocess per case
with an eight-second kill/reap limit. Cases required post-return, quit,
callback-drop and stack-drop markers, and weak NSWindow references had to be nil
after an explicit autorelease pool drained. Replacement windows had to survive a
later owner turn before being closed; replacement hooks had to actually execute.
Foreign ownership, pre-run/bootstrap quit, bootstrap failure/unwind and quit
panic each had their own case.

### An AppKit window's mode, the host's notices and a session end (ADR-0237)

AppKit has no mode property. `window::apple::geometry::mode` reads a window's mode
from its flags (full screen outranks miniaturized, which outranks zoomed, and a
zoom counts only for a resizable window) and plans the toggles that reach
another mode from those flags. `ModeSequence` orders the commands: a full-screen
or Dock step animates, AppKit ignores a toggle sent meanwhile, and what the
window looks like afterwards is known only afterwards, so a command that
arrives during one, and the rest of a mode whose first step is one, are held
and planned again from the real flags at the step's `windowDid…` notification.
The newest command wins. A step whose notification never comes stops holding
after three seconds. AppKit keeps no rectangle to return to from a zoomed,
minimized or full-screen window, so the backend records the outer rectangle
just before the window leaves normal mode.

A position is the outer top-left in global logical points: AppKit measures from
the bottom-left corner of the primary screen, so `window::apple::geometry::frame`
flips through the primary screen's height. The display snapshot is measured the
same way (device pixels of that screen from the primary screen's top-left
corner), so a position and a display origin share one space. `Platform::open_window`
turns a position measured on a display into a global one before the window is
built.

The notice sources (`NSWorkspace` sleep and wake, the distributed screen lock
notifications, the memory-pressure dispatch source) start in `run`, after the
application has installed its hooks, and only forward to `SystemNotices`; a wake
marks the displays and every window's state stale first. The distributed centre
is asked to deliver while the application is inactive, and is put back when the
sources drop. The memory-pressure source's handler holds the source and
`Drop` cancels it, which ends that cycle.

A session end is two events. `NSWorkspaceWillPowerOffNotification` asks the
session-end hook and `TerminateGate` remembers the answer; the terminate request
that follows is answered `NSTerminateNow` (after the hook hears `Ending`), or
`NSTerminateLater` while the application holds the session end, with the reply
sent when it releases its blocks or after four seconds. The release reaches the
gate as a task of its own on the main queue, never from the stack that dropped
the block. AppKit says nothing when the user calls a log out off, so an
announcement no terminate request follows within ten seconds is reported
`Cancelled`; a Cmd-Q inside those ten seconds is taken for the session end,
which the live checks record. A terminate request with no announcement is an
ordinary quit. The quit callback is not run for a session end: the process ends
with the session, and the application was told what to save.

### Win32 decides the exit policy on an owner-window turn

A `WM_DESTROY` that leaves the platform tracking no top-level window does not
consult the exit-policy hook itself: it posts one coalesced message to the
owner message-only window, and `request_exit_policy_reevaluation` posts the
same message from any thread (refused once the owner window is closed). The
owner procedure then checks that no window is tracked and no quit is under way,
calls the hook through the call point with no borrow of the handler set held,
restores it unless a newer hook replaced it, and checks the window count again,
since the hook may have opened a replacement window. A hook that panics is
contained as a veto, and a hook it displaced is retained. An allowed exit ends the loop the way
`quit` does. No hook allows the exit, as on winit. Deferring the decision keeps
the hook, which re-enters the embedder, out of a `WM_DESTROY` nested inside
embedder code that closed the window. The native rows in `tests/contract.rs`
(`closing_the_last_window_ends_the_loop`,
`exit_policy_veto_holds_until_a_reevaluation_allows_exit`,
`window_opened_by_the_exit_policy_keeps_the_loop`) run the real message loop.

### The winit backend delegates the whole keyboard event to `ui-events-winit`; Win32/AppKit keep hand-written tables

**Rule:** every native keyboard event this crate receives must be normalized
into FLUI's owned keyboard vocabulary (`Code`, `Key`,
`Location`) at the platform boundary — see the module doc of `crates/flui-platform/src/input.rs`.
`Code::Unidentified` must mean the backend genuinely could not identify the
physical key, never that a conversion table was incomplete (issue #1092).

**Choice:** the winit backend's `keyboard_event` (`input/winit.rs`)
converts the whole `winit::event::KeyEvent` (code, logical key, location,
down/up state, `repeat`, and modifiers) through
`ui_events_winit::keyboard::from_winit_keyboard_event` (crates.io
`ui-events-winit`, the same bridge Masonry and Xilem use) instead of
hand-assembling any `KeyboardEvent` field itself; `owner/winit.rs`'s
`ModifiersChanged` handler stores winit's raw `ModifiersState` rather than a
pre-converted `keyboard_types::Modifiers`, and each read site (pointer paths
and the keyboard path alike) converts through the crate's
`from_winit_modifier_state` at the point of use. The Win32 (`input/keys.rs`)
and AppKit (`input/apple.rs`, `keys`) backends keep their own hand-written
`Code`/`Key` tables, because no equivalent bridge crate exists for
`WM_KEYDOWN` scancodes or `NSEvent.keyCode` — those two native id spaces are
FLUI-specific translation work with no upstream crate to delegate to.

Location is also sourced differently than before this issue: winit's own
`KeyEvent.location: KeyLocation` field feeds `from_winit_keyboard_event`
directly, rather than being re-derived from the physical `Code` (the
pre-#1092 code matched a hand-picked list of `KeyCode` variants against
`Location::Numpad`/`Left`/`Right` — exactly the incomplete-table failure
mode this issue fixed for `code` too). There is no "does `Code::X` imply
`Location::Numpad`" property left to hold; winit already computes location
per-platform and exposes it, so nothing here re-derives it.

This asymmetry between backends is deliberate, not inconsistent: the rule is
"delegate to an audited bridge when one exists for this native surface," not
"every backend must look the same." `tests/backend_conformance.rs`'s
`physical_keys_agree_across_backends` cross-checks a curated set of
physical keys against both hand-written tables so the two authored
translations and the delegated one cannot silently drift apart.

The Winit and Android ecosystem mappings remain boundary dependencies:
`input::keys::adapter` converts their results to owned `KeyEvent` values.
Win32 and AppKit tables return owned enums directly, and Web parses the DOM's
W3C spellings against the generated owned tables. No pointer event uses the
keyboard adapter. `keyboard_adapter_contract` preserves all named/physical
key spellings, legacy Meta aliases and every event field. This conversion
test lives beside the private adapter because Winit's complete `KeyEvent`
contains private platform state and cannot be constructed by an integration
test; native backend probes cover actual input delivery separately.

**Alternatives considered:**

- Complete the crate's own hand-written `winit::KeyCode → Code` table (kept
  as a dev-dependency oracle test against `ui-events-winit`, never shipped).
  Rejected: it is a second, unaudited copy of exactly the table the bridge
  crate already maintains and tests upstream — an ownership cost with no
  behavioral upside, and the class of bug this issue exists to fix (a
  hand-rolled table quietly falling behind the enum it mirrors).
- Delegate only the three field-level conversions (`from_winit_code`,
  `from_winit_key`, `from_winit_location`) and keep hand-assembling
  `KeyboardEvent`'s remaining fields (down/up state, `repeat`,
  `is_composing`, and a separately hand-written modifiers conversion) in
  `keyboard_event`. Rejected: `from_winit_keyboard_event` and
  `from_winit_modifier_state` already perform exactly that assembly, so a
  hand-written half is byte-identical duplicate logic — the same
  re-derived-seam risk the issue exists to close, just smaller.
- Vendor `ui-events-winit`'s tables directly into this crate (a `// PORT
  NOTE`-style copy). Rejected for the same reason as the first bullet, plus
  it would need re-syncing by hand on every winit/`ui-events` bump instead
  of `cargo update` doing it.

**Trade-off:** this crate's winit backend now tracks `ui-events-winit`'s
release cadence for keyboard fidelity — a regression or gap introduced
upstream reaches FLUI without a code review here. The `.github/dependabot.yml`
`ui-events-cohort` group (with `ui-events` itself) and the
`every_winit_keycode_maps_to_a_canonical_code` regression test are the
mitigations: a bump that drops coverage for a `KeyCode` variant fails CI
immediately rather than shipping silently — though that test can only pin
the dependency's own completeness, not a regression inside `keyboard_event`
itself, since `winit::event::KeyEvent` cannot be constructed outside winit
(`platform_specific` is `pub(crate)`) and so `keyboard_event` can never be
called directly from a test; see that test's doc for the full reasoning. If
the workspace's `winit` pin ever needs to move ahead of what
`ui-events-winit` supports (it pins `winit ^0.30`), the escape hatch is a
`[patch.crates-io]` pin at the upstream `ui-events-winit` git repo (plus
adding that repo to `deny.toml`'s `sources.allow-git`), not reviving a hand
table — see the dependency comment in `Cargo.toml`.

**One observable behavior change for already-working input:** a winit
`Key::Dead(_)` (a dead-key composition in progress) now reports
`Key::Named(NamedKey::Dead)` instead of `Key::Named(NamedKey::Unidentified)`
— `from_winit_key` distinguishes the two where the deleted hand table
collapsed both to `Unidentified`. Verified inert today:
`crates/flui-widgets/src/text/editable_text.rs`'s key handler matches
specific `NamedKey` variants and falls through everything else, `Dead`
included, to `Key::Named(_) => KeyEventResult::Ignored` — no consumer in the
workspace currently branches on `NamedKey::Dead` specifically (`rg
'NamedKey::Dead'` across `crates/` has no hits outside this record). Named
here so a future consumer that starts caring about dead-key state — an IME
composition indicator, for instance — knows this signal already exists on
the winit backend and does not need a new one.

**Further reading:** [`.rust-studio/specs/1092-winit-physical-key-map/survey.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/.rust-studio/specs/1092-winit-physical-key-map/survey.md)
is the market/reference survey that motivated this decision (`ui-events-winit`
coverage measurement, a generated physical-key catalog as
the completeness precedent, and the three options evaluated before choosing
production delegation) — read it for context, not as the source of a fact;
figures cited in this entry are measured directly against this repository
and `ui-events-winit`'s own source, not against the survey.

**Replacement coverage:** `input/winit.rs`'s
`keyboard_conversion_tests` module (`every_winit_keycode_maps_to_a_canonical_code`
over all 194 winit 0.30.13 `KeyCode` variants — length- and
duplicate-checked against the winit source) and
`tests/backend_conformance.rs`'s `physical_keys_agree_across_backends`
(winit vs. Win32 vs. AppKit on a shared physical-key set). Before this change, `convert_physical_key`'s hand
table mapped 71 of winit 0.30.13's 194 `KeyCode` variants to `Code` (the
count measured at issue #1092's baseline) and `convert_winit_key` mapped 37 of 306
`NamedKey` variants to `Key`, falling through to `Unidentified` for the rest
in both cases.

### The surface-availability signal is a per-window `bool` callback, not a polled query

**Rule:** a backend that learns the GPU surface's native backing is about to go
away must say so through `PlatformWindow::on_surface_status_change` **before** the
handle behind any surface built from it dies, so the presentation can drop that
surface at the right moment. A backend that never emits the signal is harmless:
the surface is never released, which is the pre-#1146 behavior. A backend that
emits `false` and never
`true` is not harmless, because the presentation stays released, every later frame
is a skipped one, and the window paints nothing forever.

**Choice:** `on_surface_status_change(Box<dyn FnMut(bool) + Send>)`, a defaulted
no-op, following the window callbacks' shape: stored in
`owner::handlers::WindowCallbacks`, dispatched through
the same FIFO and the call point. `false` means "release the surface before this
callback returns"; `true` means "a surface valid for the handle available now must
exist", which the runner acts on unconditionally rather than by comparing it
against what it already holds — that statelessness is the point, since a `false`
can be lost and an "already present, do nothing" rule would then keep a surface
built from a dead window for the rest of the process's life. The parameter is a
`bool`, not an enum: a third state of the *surface* belongs in its own callback,
per ADR-0035's split of this family by signal rather than by arity. Android and the Web
emit it. Android's `Pause` and `TerminateWindow` send `false`, `Resume` and `InitWindow`
send `true` — `owner/android/looper.rs`'s module doc carries why that pair, and not
`Pause` alone, is the correct mapping. The Web sends `false` on `webglcontextlost` and
`true` on `webglcontextrestored`: the canvas stays, the context behind it is gone, and
whatever was built on it must be rebuilt once the browser gives one back. The loss is
cancelled with `preventDefault` — without that the browser never restores the context —
and a browser that never restores it leaves the page without a surface, which is all the
page can know. A lost WebGPU device is not a surface event, so it does not use this
signal; the renderer reports it to its owner directly.

**Alternatives considered:**

- A polled query on the window (a `surface_is_available()` the runner reads each
  iteration). Rejected: a poll cannot carry this deadline. The drop has to happen
  while the native handle is still valid, which on Android means inside the
  `TerminateWindow` callback; a query consulted at the next loop iteration reports
  a window that is already gone.
- A `#[non_exhaustive] enum WindowSurfaceStatus` parameter instead of `bool`.
  Rejected for the parameter, not for the concept: it is the shape this crate uses
  for input and owner status, but every consumer of this signal branches on two
  states, and a callback's parameter type is a one-way door that changing later
  breaks for every registrant.
- Delegating the surface to the platform, the shape Leptos reaches on Android by
  hosting a Tauri v2 WebView. Rejected as unavailable rather than inferior: it
  requires giving up the adapter, the swapchain, and any ability to composite
  native-rendered content into the framework's own UI. It would not escape the
  problem either, since a wgpu surface inside a WebView is canvas-backed and
  wgpu's wasm backend does not recover from `webglcontextlost` (gfx-rs/wgpu#3679).

**Market lineage:** [bevy#6830](https://github.com/bevyengine/bevy/pull/6830), where
an `android-activity` maintainer states that only the top-level render target needs
recreating, not the wgpu stack; [bevy#9937](https://github.com/bevyengine/bevy/pull/9937),
which first despawned the window on suspend and was revised to "keep Bevy window,
only recreate Winit window and wgpu surface"; and
[winit#3786](https://github.com/rust-windowing/winit/pull/3786), which forwards
Android's `suspended()`/`resumed()` and is a named second emitter for this same
`false`/`true` pair, since winit is already a workspace dependency with a backend
here. Flutter asks for the same contract under another name:
[flutter#160933](https://github.com/flutter/flutter/issues/160933) requested an
`onSurfaceDestroying` that precedes the engine's `cleanup()`, having found the
after-the-fact callback fires once the native surface is gone. The spelling that
landed upstream is not that one: the API that exists is
`SurfaceProducer.onSurfaceCleanup`, the before-signal that replaced
`onSurfaceDestroyed` because another thread could still touch the surface after
the fact, so a grep for the requested name finds the request rather than the
implementation (ADR-0063 names both spellings). One divergence from
winit and bevy is deliberate: winit 0.30.13 keys on the window events alone —
`src/platform_impl/android/mod.rs` maps `MainEvent::InitWindow` to `Resumed`
and `MainEvent::TerminateWindow` to `Suspended`, while its `Start` and `Stop`
arms are `warn!("TODO: forward …")` stubs that forward nothing — and bevy
consumes those same two events. This backend keys on `Pause`/`Resume` *as
well as* `TerminateWindow`/`InitWindow`: the window pair is where the handle
actually dies and returns, and the lifecycle pair is where the framework is
told the app is going away, so emitting on both is the more conservative
choice, at the cost the trade-off below books.

**Trade-off:** releasing on `Pause` unconditionally makes the cost per-edge rather
than per-defect. Android maps `false` to both `Pause` and `TerminateWindow` and
`true` to both `Resume` and `InitWindow`, and the seam attempts a recreation on
every `true`, so every `Pause`/`Resume` pair pays a release, a create/configure,
a lane generation mint and a full repaint — including the pauses where the native
window was never destroyed, such as a dialog over the activity or a multi-window
deactivation. A second `true` over a surface still held on the same window is not
paid twice but refused: Vulkan allows one `VkSurfaceKHR` per `ANativeWindow`, so
the second create fails, the held surface stays, and no second mint or repaint
follows (ADR-0063 decision 6 books what that refusal costs under the vendored
`wgpu-hal`, and marks the ordering that could produce it as unverified). Dropping
a configured `wgpu::Surface` is
not cheap either: it reaches `vkDeviceWaitIdle` before destroying anything, which is
an unbounded wait on whichever thread runs the callback. That is why the release is
a synchronous, surface-only operation that performs no lane handoff, no probe and no
submit, and why the `TerminateWindow` release that follows `Pause` in the ordinary
cycle is an idempotent no-op — while its caller holds the raster lane's guard across
it by design, so the wait does happen with the lane held.

**Unasserted:** no test pins this. A covering test has to be a wire test —
registration through the `PlatformWindow` trait method, the `simulate_surface_status`
affordance driving the platform's own dispatch, and both edges asserted in the
closure — because a backend left out of `impl_window_callback_setters!` still
compiles the registration and silently drops it, which a direct
`dispatch_surface_status_change` test cannot see. The Android arms themselves are
**type-checked by `cargo xtask cross-typecheck` and executed by nothing**: no gate
on this host runs the Android backend, so their mapping is an inference from `android-activity`'s
documented contract, recorded rather than measured.

### AppKit's frame source stays `drawRect:`, and its re-arm crosses one owner-lane turn

**Rule:** a display-driven backend's produce signal must be able to re-arm itself
from inside the frame it just produced. AppKit discards a `setNeedsDisplay:`
issued while it is displaying the view, so a backend whose only frame source is
`drawRect:` and whose re-arm happens there is circular — one frame, no wake, no
frame. A re-arm AppKit silently drops is worse than a missing feature: the pump
stops with no error, no log line, and a perfectly healthy-looking request behind
it.

**Choice:** `drawRect:` stays the frame source. It is the signal AppKit actually
delivers — the view is live and the frame carries its own dirty rect — and the
produce contract the engine consumes is dispatched from it (`owner::apple::macos::content_view`'s `draw_rect`
→ `WindowCallbacks::dispatch_request_frame`). What changed is the re-arm.
`owner::apple::macos::display_pass` marks the thread for the duration of the frame
AppKit asked for (an RAII guard, nesting-safe, thread-local), and
`window/apple/macos.rs::dispatch_redraw_request` — the single decision point `request_redraw`
calls — sends the `setNeedsDisplay:` inline at any other moment, but hands it to
the next owner-lane turn when the caller is inside that pass. This is ADR-0039
§4(c) realised on AppKit: the frame transaction is a region the drain gate is
closed for, and a wake arriving while it is closed defers rather than draining.
The deferred turn re-checks the marker before messaging AppKit and re-defers,
bounded at `MAX_DEFER_HOPS`, because a display pass that opens a nested run loop
services the same lane from inside itself — which the deferral would otherwise
reproduce silently.

Measured on macOS 15.7 (Darwin 24.6), a 100 Hz panel, isolated probe arms of 4 s
each, every arm with the window fully visible (`occlusionState` carries the
visible bit):

| re-arm issued from inside `drawRect:` | draws over 4 s |
|---|---|
| `[view setNeedsDisplay:YES]` | 1 |
| nothing at all (control) | 1 |
| `[view.layer setNeedsDisplay]` | 1 |
| `setNeedsDisplay:` deferred to the next main-queue turn | 396 |

**Alternatives considered:**

- Re-arm through the backing layer (`[view.layer setNeedsDisplay]`). Rejected by
  measurement, not by taste: the layer's `needsDisplay` reads `YES` right after
  the call while the view is still never redisplayed — the flag that survives is
  not the flag AppKit acts on, so it is not a usable signal to test against
  either.
- Move the frame off the display pass entirely (`CADisplayLink`, i.e.
  `NSView.displayLink(target:selector:)` on macOS 14+, or `CVDisplayLink`).
  Deferred, not rejected: ADR-0044 names `CADisplayLink` as macOS's intended
  produce signal, and ADR-0045 (Proposed) keeps macOS on `RasterMode::Inline`
  until the locked `wgpu-hal`'s Metal path stops messaging `NSView`/`NSWindow`
  from the calling thread. Until that lands, the frame has to be able to re-arm
  from where it runs.
- A diagnostic redraw tick in the backend (an `NSTimer`/thread poke behind a
  `FLUI_MACOS_REDRAW_POKE_MS`-style knob, which was implemented while diagnosing
  this). Deleted, not deferred: it contradicts ADR-0058 — the platform paces
  production, a sleep never does — and it pre-empts the `PlatformProxy` redraw
  verb ADR-0045 decision 5 scopes to #559 by shipping a second, private frame
  source. The primer that starts a measurement belongs to the measuring harness.

**Market lineage:** nobody shipping a Rust macOS UI builds its frame loop this
way. gpui-ce (`crates/gpui_macos/src/display_link.rs`, `main` @ `8e36ac0`,
2026-09-15) and upstream Zed (the same crate, `main` @ `c24e309`, 2026-09-17)
both pace on `CVDisplayLink` — one immortal link per `CGDirectDisplayID` in a
`static Mutex<Registry>`, windows subscribing rather than owning, driven by
`NSWindowOcclusionState::Visible` as the master start/stop switch and re-armed on
screen change — and render **directly** from the link's callback on the main
queue, with `displayLayer:` as the second, AppKit-initiated entry; both paths
stop the link, render, and restart it. `CADisplayLink` /
`NSView.displayLink(target:selector:)` appears nowhere in either repository, and
**neither calls `setNeedsDisplay:` at all** (the sole hit is
`setNeedsDisplayOnBoundsChange(true)` on the `CAMetalLayer`), nor has any
`drawRect` path (`0` hits). That last point is the one worth keeping straight:
it is evidence that the path this backend was on is a path the market left, not
evidence about AppKit's behaviour — the discarded in-pass `setNeedsDisplay:` is
this entry's own four-arm measurement, and the market never reaches the
question. Their link is also leaked by design, because `CVDisplayLinkStop` is
asynchronous and releasing raced its io thread (upstream segfaults #32116 /
ZED-7XR) — the wart `CADisplayLink` does not have, which reads as support for
ADR-0044's choice with the caveat that the market sits on the API macOS 15
deprecates. Their deferral is a different one and not to be confused with the
one above: upstream Zed PR #3592 (`f12510b8`, 2023-12-11) defers *drawing* until
CoreAnimation calls `displayLayer:`, i.e. defers to a platform signal rather than
around a discarded request; core gpui's `deferring re-entrant window draw
request` is its own comment's description of a Windows-only re-entrancy case.
One behaviour of theirs this backend still lacks is the fix for the stall that
keeps a cold start from beginning: a single synchronous frame on re-activation,
gated on `activated_least_once`.

Basis for the above: source reading at the two commits named (clone grep plus
`gh search code`, which agree), not by running either project — the clones are
shallow, so an older revision containing a `setNeedsDisplay:` call is not ruled
out, and no Apple documentation was consulted for the API-deprecation claim.

**Trade-off:** this deferral is AppKit-local, and the same defect had a second
instance on another backend with the opposite sign. Web's `request_redraw`
dispatched the frame immediately and `WindowCallbacks::drain_events` drains
synchronously until the queue is empty, so a frame callback that re-armed
recursed *within* the originating call rather than reaching a browser frame turn;
in the interaction gallery an animation started from a click (a swiped row
closing) re-armed every frame at the same frame time and held the page's thread
for good. Both were the same contract failing: **a redraw request issued from
inside a frame request must be deferred to the platform's next frame turn, not
serviced in place.** Web now serves `request_redraw` from its
`requestAnimationFrame` loop alone, which requests a frame on every animation
frame (`a_redraw_waits_for_the_next_animation_frame` in `tests/web_browser.rs`,
run in headless Chrome). The concept still lives per
backend rather than in a shared place, so a third backend writing a frame loop
has to rediscover it.

**Replacement coverage:** three tiers, because no single one reaches the claim.
`display_pass.rs`'s four tests pin the marker (set inside a pass, restored on
drop, nesting, thread grain). Two always-run window tests pin the decision
without an NSWindow
(`a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline` and its
complement — deleting the deferral branch fails the first, checked by mutation:
with `dispatch_redraw_request`'s body replaced by an unconditional inline send,
that test fails and the other five in scope still pass), because a bare test
process cannot construct an NSWindow at all. The behavioural end-to-end pin was a manual macOS run: a real visible window
on the real AppKit run loop, whose frame callback re-arms the way the engine's
frame does, behind a primer that stops at the first frame so the measurement
cannot be explained by it. Measured 2026-09-17: 301 frames in 3.010 s (100.0 fps
on the 100 Hz panel) with the deferral, against 1 frame total and 0 in the
measurement window with the deferral branch removed. Both runs' marker lines
(§4 of its `plan.md`) and the raw run logs are kept with the
[pacing evidence](https://github.com/vanyastaff/flui/tree/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/.rust-studio/specs/macos-native-vsync-pacing-evidence).

### macOS reports its display period as the current mode's rate, in the display snapshot

**Rule:** a backend that can tell an embedder the display's refresh period should
report the rate of the mode that display is *in* — not a capability of the panel
and not a generic default. The value is a floor for pacing ("never produce faster
than this", the display snapshot's `refresh_period`), so a rate that is too high paces
against a cadence the display is not running, and a rate that cannot be
determined must be reported as unknown rather than guessed at.

**Choice:** `Platform::display_snapshot` carries, for every `NSScreen`, a period
from `CGDisplayModeGetRefreshRate` over the display's *current* mode
(`display/apple/macos.rs::refresh_rate_of_screen`, made a period by
`RefreshPeriod::from_hz`), reached from the
screen via `NSScreenNumber` — the same display id `display.rs` reads for its
bounds. A rate of 0 is `None`: CoreGraphics reports 0 for modes it cannot
describe and some displays do, and a 0 turned into a period is infinite. The
window names its display in its state snapshot, **cached** in
`MacOSWindowState` beside `scale_factor`: re-read in the handler that already
updates the scale (`handle_backing_properties_changed`, reached from
`handle_screen_changed`), which then publishes the window's state, and seeded when
the platform binds its display registry to the new window, which is safe there
because the window is created at the origin — a point on a screen — so
`-[NSWindow screen]` answers rather than returning nil. Caching is the one place
this diverges from the winit implementation it otherwise mirrors: winit queries
the monitor per call, but this backend's AppKit traffic belongs to the owner lane
while `display_id` is a plain `&self` accessor any thread may call, so the query
happens on the lane and the accessor reads the lock. The screen key is the
`CGDirectDisplayID` (a label, stable while the display stays attached), a stopgap
the display-UUID source replaces.

**Alternatives considered:**
- `NSScreen.maximumFramesPerSecond` — rejected twice over. It is macOS 12+, above
  this crate's 11.0 deployment floor, so an unguarded call would crash on 11;
  and it reports the panel's hardware *maximum*, so it over-reports on a
  ProMotion display running a 60 Hz mode — exactly the too-high rate the rule
  above forbids. winit does not use it.
- Querying per call, the way winit does — rejected here for lane safety, above.
- A `CVDisplayLink` fallback for displays whose mode reports 0 (winit has one) —
  **deferred, not dropped**: it needs a `core-video` dependency, and CVDisplayLink
  is this backend's intended produce signal anyway (ADR-0044), so it belongs with
  that tick source rather than ahead of it. Until then `None` is honest and
  observable: `flui-app`'s runner logs `reported=false` and paces against its
  documented default.

**Market lineage:** winit 0.30.13's macOS `MonitorHandle::refresh_rate_millihertz`
(`src/platform_impl/macos/monitor.rs:260`) reads `CGDisplayCopyDisplayMode` →
`CGDisplayModeGetRefreshRate`, returns it when it is `> 0.0`, and only then falls
back to `CVDisplayLinkGetNominalOutputVideoRefreshPeriod`. That is the source
read here and the reason for the `> 0` guard. Read at the version pinned in this
workspace's `Cargo.lock`, not recalled.

**Trade-off:** the cached display follows the window across screens and backing
changes. A mode change that leaves the window where it is (a user switching
refresh rate in System Settings) is observed too: `ScreenObserver`
(`display/apple/macos.rs`) marks the snapshot stale on
`NSApplicationDidChangeScreenParametersNotification` and wakes the owner, and the
host derives every window's display again on that wake. What stays open is a
display whose current mode reports 0 Hz, which some built-in panels do: its period
is unknown and the host paces at its 60 Hz default, because the
`CVDisplayLinkGetNominalOutputVideoRefreshPeriod` fallback winit takes is not
written ([gaps](../../docs/gaps.md), entry 4). Neither a live refresh switch nor
such a panel ran: the display this was measured on has one fixed mode.

**Replacement coverage:** the arithmetic is free-standing and AppKit-free
(`RefreshPeriod::from_hz`, the contract's constructor the macOS source hands
CoreGraphics' rate to), pinned by `refresh_value_classification_table`
(non-cadence inputs — zero, negative, NaN, ±infinity — are unknown rather
than an infinite or panicking period) and by
`macos_shim_keeps_the_period_it_reports_today`, which holds the common panel
rates to the value this backend reported before the snapshot. The read
itself is FFI, so `every_display_source_reads_a_cadence_alike` has no macOS
column. The live path is
pinned behaviourally by the same manual run that pins the frame pump,
which reported the refresh period from
the real backend on the real display: measured 2026-09-17 as `period_us=10000
hz=100.0`, matching the independent AppKit probe's `CGDisplayModeGetRefreshRate`
of 100.000 Hz on the same panel. Like every macOS-gated test here, these run
locally only — CI has no macOS test job and `cross-typecheck` is lint-only.

### AppKit's input method is a route for a `keyDown:`, not a second producer of one

**Decision.** `FLUIContentView` (the owner's `content_view`) conforms to
`NSTextInputClient` (all 11 methods plus `add_protocol`), and `keyDown:` becomes
a gate rather than a direct conversion: while the view's text input does not
compose (`ViewText::takes_key_presses`, `TextInputState.ime_allowed` **false**)
it hands the event to the view's input exactly as before; while it does it
hands the event to `interpretKeyEvents:` and emits nothing itself, with
`doCommandBySelector:` handing the events the input method declines back to
the input. The methods turn AppKit's arguments into Rust values and call the
view's `TextView`, which keeps the composition. `PlatformTextInput` is
implemented by `MacOSTextInput`, whose two setters route through the owner
lane to the same `TextView`.

**Why.** AppKit's composition pipeline is entered only by `interpretKeyEvents:`,
which calls *back* into the same view (`setMarkedText:` while composing,
`insertText:` on commit). Calling it alongside the existing conversion would make
one physical press reach the application twice — a `Key::Character` **and** an
`ImeEvent::Commit`. That is ADR-0044 §3's double-producer defect class, and
ADR-0069 states the contract it violates. The gate makes attachment the *only*
variable: the pre-existing path is byte-identical at the default, so the
conformance adds no regression surface to a window that never attaches a text
input.

**Market lineage.** winit 0.30.13 documents the same rule in the same terms
(`Window::set_ime_allowed`): with IME allowed the window receives `Ime` events
and, during the preedit phase, no `KeyboardInput`; with it disallowed the window
receives no `Ime` events and a `KeyboardInput` for every keypress; and IME is
**not** allowed by default. The winit backend inherits this by delegation
(`WinitTextInput`); the native backend owns the pipeline instead of delegating,
so it implements the protocol rather than wrapping one. The documentation covers
the *press*; reading winit's macOS implementation shows the release is gated too
and on a different variable — `key_up` queues a `KeyboardInput` only from its
`Ground` and `Disabled` states, so an open preedit suppresses the release while a
committed character's release still arrives (`ImeState::Committed` is reset to
`Ground` inside `keyDown:`). `key_up` here gates on the same condition expressed
in this backend's own state (`TextInputState::reports_key_release`), so a release
inside an open composition is suppressed for the same reason the press was: the
application never saw that key go down.

**Teardown drops the composition.** `set_ime_allowed(false)` **drops**
the composition (ADR-0069) and `unmarkText` emits
`ImeEvent::Preedit { text: String::new(), cursor: None }` rather than nothing.
It is grounded in the client-side bug class
`flui-platform/src/ime.rs` records — a client left holding composition state it was
never told ended suppresses `Key::Character` for the rest of the focus session
and keeps the cancelled slice in its buffer — and in winit's own macOS
implementation, which drops the marked text on `set_ime_allowed(false)`, so
ADR-0069 cites that implementation for the drop-vs-commit half. AppKit's header does not say whether an empty
`setMarkedText:` always precedes `unmarkText`, so both paths are covered; the
event is inert when nothing is composing, which is why the callback carries no
`hasMarkedText` check.

**Rejected.** Unconditional `interpretKeyEvents:` (the two-producer defect).
Gating on `-[NSTextInputContext handleEvent:]`'s `BOOL` — the routing decision is
already correct before the call, and consulting AppKit first would take the
keyboard path in exactly the states where both producers are live. A pre-call
predicate on the key, which is undecidable before the input method runs, and
wrong per layout: on a U.S. layout a plain letter *does* reach `insertText:`.

**Marked-range convention, a choice a document-less view cannot do better than.**
`markedRange` answers `(0, utf16_len)` while composing and `{NSNotFound, 0}`
otherwise; `selectedRange` echoes what `setMarkedText:` delivered. `NSRange` is
UTF-16 and `ImeEvent::Preedit.cursor` is a **byte** range, so the conversion is
`utf16_range_to_byte_range` — AppKit-free and tested.

**Trade-off, named rather than assumed.** The class-registration block cannot run
under a bare `cargo test` (an unbundled `NSWindow` throws
`_CFBundleGetValueForInfoKey`), so every encoding and selector here was
hand-checked against `objc-0.2.7`'s `add_method` preconditions until the bundled
a bundled run executed them — which it now has, on a real Mac, with all five assertions
passing. What that run measured about the rect is that it is non-zero and
screen-relative; the **direction** of the `firstRectForCharacterRange:` Y-flip is
still designed rather than measured, because a flip of the wrong sign produces a
non-zero screen rect too. Measuring which side of the caret the candidate window
would land on needs a real input method to ask, which is the same gap as for real composition below.

**Replacement coverage.** The conversion carries seven always-run tests
(ASCII identity, multi-byte divergence from UTF-16 offsets, CJK unit-vs-byte, a
range splitting a surrogate pair, past-the-end, empty text, and AppKit's real
`NSNotFound` location), and an eighth pins the `keyUp:` gate across all four
states it distinguishes — unattached, attached-but-idle, composing, and a
composition that has just ended. It is mutation-checked against the gate a
reader would first reach for (`!ime_allowed` alone fails it on the
attached-but-idle case). A manual run drove the live path from a bundled
`.app` on a real Mac and passed: the inverse pair
(attached → one `Commit` and no key event; detached → one key event and no
`ImeEvent`), the `Preedit` → `Commit` sequence with the byte cursor asserted, the
empty-`Preedit` that ends a composition, a non-zero cursor rect whose
`actualRange:` is answered rather than left not-found, and the `keyUp:` gate
end-to-end — a release reported while a text input is attached with nothing
composing, and the same call suppressed while a composition is open. The gate's
arm is mutation-checked against the ungated `key_up`: removing the check leaks a
`KeyboardEvent { state: Up, key: Character("a") }` into that arm and fails the
run. A synthesized `NSEvent` proves the
*routing* and is not a genuine input method, so real composition (press-and-hold
or a CJK source) remains **not driven**.

### iOS binds UIKit through `objc2`, and its loop-exit signal is `applicationWillTerminate:`

**Decision.** The iOS host (`owner::apple::ios` and the `ios` files of its
features) binds UIKit through `objc2`
0.6 / `objc2-ui-kit` 0.3 / `objc2-quartz-core` / `block2` /
`dispatch2`, and takes its framework loop-exit signal from
`applicationWillTerminate:`. ADR-0071 carries the full record.

**Why `objc2` and not the macOS backend's `cocoa`/`objc`.** The choice is not
consistency-versus-modernity: `objc` has not released since 2019 and `cocoa`
has **no UIKit surface at all**, so the macOS stack cannot express this platform
even in principle. `icrate`, which this module's stub doc once named, is a
deprecated alias split into the `objc2-*` crates. Every shipping Rust Apple
stack is on `objc2` (winit's iOS backend since 0.30, wgpu-hal, egui, slint,
gpui), and the macOS module's own header already commits to migrating there.

**Why `applicationWillTerminate:`.** `UIApplicationMain` never returns, so
there is no "after `Platform::run`" for the runner to use — the desktop and
Android runners call `teardown_platform_ui_runtime()` there. Without a deliberate
choice the framework never receives its loop-exit signal on iOS and leaks every
UI runtime, service pool and the clipboard for the process's life. That delegate
method is the only pre-exit notification iOS sends, so the platform fires its
quit handler from it and the runner runs the teardown.

**Alternatives considered.**
- *A process-global for the delegate's session state* — rejected: it would add
  a new process-global to the runtime's ambient reach, and the state is
  main-thread-only anyway. A thread-local is the owner-affine
  scope ADR-0027 prefers and matches winit's own `ACTIVE_EVENT_LOOP`.
- *`UIScene` adoption* — deferred, not dropped. It is the multi-window iPadOS
  feature and would make `UIScreen.mainScreen` (deprecated in the scene era)
  scene-relative; this backend presents one full-screen window, so the app-wide
  accessor is the honest spelling and the module carries an
  `expect(deprecated)` with that reason.
- *`objc2-ui-kit` with default features* — rejected: it enables all ~456
  header features, compiling the whole framework's bindings for a handful of
  classes. The backend names the classes it messages, as `winit-uikit` does.

**Trade-off.** Two `objc2` generations are in the tree: this backend uses
0.6.4/0.3.2 (already there via `wgpu-hal`), while `winit` 0.30 and
`accesskit_macos` 0.27 still pull 0.5.2/0.2.2. The duplicate resolves when
those move (H10 / winit 0.31), not by anything here. A real device is not
covered — the simulator slice is, by a manual run — and no CI job boots a
simulator; `cross-typecheck` gained an `aarch64-apple-ios` clippy line so a
broken iOS build is at least loud.

**Replacement coverage.** A manual simulator run (a Metal device and a rendered
frame from the app's own log, plus a screenshot) for the
end-to-end path, and three host-run unit tests on `display.rs`'s bounds
arithmetic. The engine-side portability fix this uncovered —
`required_limits` clamped to the adapter's own, because the simulator's Metal
adapter caps `max_inter_stage_shader_variables` at 15 where
`wgpu::Limits::default()` asks for 16 — carries a comment at the clamp.

### macOS and iOS share the `objc2` binding stack; the `cocoa`/`objc` pair is gone

**Decision.** Both Apple backends bind their platform frameworks through the
`objc2` family (`objc2`, `objc2-app-kit` for macOS/AppKit, `objc2-ui-kit` for
iOS/UIKit, `objc2-foundation`, `objc2-quartz-core`) at the
versions `wgpu-hal` already pins. The `cocoa` 0.27 / `objc` 0.2 dependency pair
and the `build.rs` that existed only for its `cfg` macros are removed from the
crate; neither appears in `Cargo.lock` any more. ADR-0071 carries the full
record (ADR-0071 chose the stack for iOS first).

**Why.** `objc` has not released since 2019 and points at `objc2` as its
successor; `cocoa` deprecated its whole surface in the same direction and has no
UIKit bindings at all. Every shipping Rust Apple stack is on `objc2` (winit since
0.30, `wgpu-hal`, egui, slint, gpui). Carrying both stacks was a two-generation
wart with a `build.rs` to feed.

**The raw `msg_send!` shape is kept on purpose, not left un-migrated.** objc2's
macro accepts a raw `*mut AnyObject` receiver, `Bool` arguments and a manual
`release`, so `window.rs`'s already-reviewed safety shape survives. The concrete
reason: `NSWindow`/`NSView` are `MainThreadOnly` in objc2's typed API, while the
backend constructs test windows on a caller-supplied off-main serial lane
(`MacOSWindow::for_test`), which a `MainThreadMarker`-gated method would refuse.
`ClassBuilder` replaces `objc` 0.2's `ClassDecl` for the two runtime classes
(`FLUIContentView`, `FLUIWindowDelegate`).

**Two defects the stricter macro caught.** objc2's `msg_send!` verifies return
types against the selector encoding at run time and found `makeFirstResponder:`
declared `void` where AppKit returns `BOOL` — silently accepted by objc 0.2. It
also made the cursor-icon match's explicit `arrowCursor` arm list
`clippy::match_same_arms`-visible (kept as documentation under a scoped allow).
The newer macro is a stricter oracle; that is part of the migration's value.

**Trade-off.** A second `objc2` generation is still in the lock via
`accesskit_macos` 0.27 and winit 0.30 (both on 0.5); resolved when those move
(H10), not by anything here.

**Replacement coverage.** The four bundled macOS probes are unchanged in what
they assert and all PASS on `objc2`, driving the migrated `msg_send!` sites
through the production launch path; the five real-`NSPasteboard` tests and the
`display.rs` arithmetic run in the normal suite.

### Window-independent owner turns

`owner::signal` owns coalescing, callback leases and quit admission for
native desktop and headless. Physical posting is serialized with queued-state
acknowledgement: a concurrent sender cannot receive success for another sender's
failed post. Callbacks and capture drops execute outside locks; active delivery
covers both, so nested owner dispatch cannot recursively invoke a replacement.
Quit fences admission before posting and remains retryable after posting failure.
Per-run weak proxy stamps retain the original owner thread even after expiry.

The signal holds admission and scheduling state; the callback waits between
turns in a `TurnSlot` the caller passes to `register_in` and `drive_in`. Win32
keeps an `OwnerTurnSlot` (`!Send`) in its owner control context, so closing or
dropping the last `Arc<OwnerSignal>` on another thread never drops the
callback there; the owner clears the slot on quit, or `WM_NCDESTROY` frees it
with the context, and a platform dropped off its owner leaks it with the
context. **Unasserted:** no test pins this. macOS, UIKit, winit and headless
still use `register` and `drive`, backed by a shared slot inside the signal,
until their own steps of ADR-0082 §4.

A posted turn is acknowledged when it begins (`OwnerSignal::begin_turn`), not
when the application's turn runs. A loop that reads what wakes asked of it
before driving (the winit loop's host notices, portal appearance and X11 window
properties) begins the turn, reads, then drives the `OwnerTurn`; a wake that
lands while it reads posts the next turn, which reads again, instead of being
settled by the drive with its change unread. `drive` and `drive_in` begin and
drive at once, for loops that read nothing first (Win32, AppKit, UIKit, web,
headless) or read only after the drive (Android's view events, UIKit's scene
installation). A begun turn dropped before it is driven (a read that unwinds,
a loop that returns early) posts the next turn for the work still owed. A read
on the owner thread that finds work for the application's turn owes it to the
turn it runs in (`OwnerTurn::owe`, the winit loop's Windows preferences) rather
than waking, which would post a turn with nothing left to read. A wake that
finds no callback stays owed, and a registration posts it. Tests:
`a_wake_while_the_loop_reads_gets_a_turn_that_reads_again` (the winit loop and
the signal itself), `an_undriven_turn_posts_the_next_one`,
`a_turn_owed_by_its_read_posts_nothing`,
`a_change_with_no_turn_callback_reaches_the_next_one_registered`.

macOS posts through GCD, winit through its existing EventLoopProxy, and Win32
through a dedicated message-only class with its own typed userdata. Headless
provides `HeadlessOwnerTurns`, an owner-local driver and one-shot posting failure
injection for deterministic app recovery tests. Successful headless run return
leaves the retained logical owner alive; failed bootstrap, quit and destruction
close it. The app uses this transport for pending-window completion without a
window or UI runtime frame. It is not a generic closure executor.

A live macOS run needs a GUI session. External counters assert owner affinity, nonrecursive delivery, shutdown capture
release and rejected work after quit. On Windows, a turn through the real
message loop runs on the owner and is released there. **Unasserted:** no test
pins this. CI only type-checks Win32. Mobile/web registration remains explicitly
unsupported; proxy window creation support is separate from wake/quit support.

### Reveal without changing window mode

`WindowCommand::Show` is distinct from a restore: restoring also removes
maximization, while revealing a resident application's existing main window must
preserve maximized/fullscreen mode and geometry. The command reveals and
requests focus; operating-system focus policy still decides whether focus moves.
AppKit deminiaturizes then orders the window forward. Win32 queries
`WINDOWPLACEMENT` fallibly and interprets `WPF_RESTORETOMAXIMIZED` only for minimized
placement, using `SW_SHOW` for non-minimized windows. A winit 0.30.13 Wayland
window leaves `shows` and `activates` `Unavailable`, so `admit` refuses the
command before any mutation: unminimize and focus are unsupported there. A
closed window refuses every command with `Closed`.

Sources: [winit Window](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html),
[Win32 WINDOWPLACEMENT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-windowplacement).
Headless tests model minimized independently from maximized/fullscreen.
Windows evidence is strict cross-compilation, not a native runtime claim.

Win32 show verifies retained callback-Arc identity under the owner context guard
before reveal and again after callback-capable show/foreground calls. Recycled
HWND or context addresses cannot redirect a retained wrapper's show operation.
The portable operation-sequence test asserts that stale identity causes neither
reveal nor focus. This does not change the separate legacy teardown route or
claim native Windows runtime verification.

### The first reveal waits for the first presented frame, behind alpha 0

AppKit shows a window the moment it is ordered front, and the first frame
reaches the compositor only once the GPU stack behind it exists and the first
present lands — 2.81 s on a measured cold launch — so ordering an opaque window
at open shows its bare background for that whole gap (the launch blank-window
observation in [`docs/BETA.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/BETA.md)). The deferral is the caller's to ask for,
through `WindowOptions::reveal`: with `WindowReveal::AfterFirstFrame`, macOS
orders a `visible: true` window front at `alphaValue` 0 and restores alpha 1
when the embedder reports the first presented frame through
`PlatformWindow::reveal_after_first_frame`, or when the window is shown
explicitly (`show`, `set_visible(true)`, `activate`). The flag is settled under
the window's state lock, never inside an owner route, so exactly one caller
performs the reveal.

Opt-in rather than the backend's default, because the reveal report is a
promise only a frame-loop owner can keep, and the promise is selected at the
one place that keeps it: `flui-app`'s desktop runner asks for
`AfterFirstFrame` at its two open sites (the main window and a content-bearing
secondary window, through `runner::desktop::rendered_window_options`), whose
frame closure performs the reveal. Its shared `From<&AppConfig> for
WindowOptions` conversion stays at `AtOpen` on purpose — `run_direct` and the
bare `open_secondary_window` use it and have no frame loop that would reveal.
A direct consumer of this crate — the platform probes — likewise has no first frame to report, and a window
whose reveal waits for a call that never comes is a window nobody sees. The
default `WindowReveal::AtOpen` therefore leaves alpha untouched and those
windows appear at open, as they always did. A backend that cannot defer treats
`AfterFirstFrame` as `AtOpen`.

Transparent rather than hidden, because a hidden alternative was tried and
measured on 2026-09-21: an un-ordered window gets no Metal drawable — wgpu
reports the surface occluded on every acquire (30+ withheld frames in the log)
— so a reveal that waited for a present into a hidden window waited for
something that cannot happen and fell through to the embedder's 1 s fallback.
Ordered at alpha 0, the first acquire still comes back occluded once (a
post-configure quirk of the Metal surface) and the second presents; the reveal
followed the first frame by 85 ms, and the window's first on-screen sighting
through the CoreGraphics window list was already painted.

The embedder's half is `flui-app`'s `FirstReveal` policy: the first presented
frame earns the reveal, and a frame that ran and presented nothing arms a
bounded fallback (one second from that outcome, not from install, so a slow
cold frame still inside its render arms nothing) after which the window is
revealed regardless — a surface that never presents yields a window the user
can see and close rather than a process with no window. The fallback deadline
joins the desktop wake-deadline hook and the frame closure's dirty predicate
like every other wake-deadline source there.

The winit backend reveals at open, unchanged: whether a hidden X11 or Win32
window hands out swapchain images could not be verified here, and a wrong guess
would cost every Linux launch the fallback's second. Wayland maps a surface at
its first commit, which is the first present, so the deferral there is the
protocol's own. Win32's native backend is likewise unchanged.

### Per-window execution eligibility

`WindowExecutionState` separates native execution permission from focus, visibility
and GPU readiness (ADR-0072). The callback slot is private. iOS serializes native
observations in a separate queue and delivers effects through immediate calls
through the call point, independently of the general input/frame FIFO. Invocation and capture
destruction are contained separately, outside storage locks. Only resource intents
advance the generation: focus reentry cannot cancel a resource transition, while
superseded resource continuations and queued restorations are rejected. A queued
background pauses native ticks immediately, including during nested UIKit loops. Temporary inactivity preserves the surface;
true background suspension pauses the display link and releases it. Foreground
restores execution while unfocused; duplicate foreground preserves established
focus. Headless simulation exercises the same observation contract.

Android publishes `Resume`/`Pause` as a shared execution snapshot before invoking
embedder callbacks. Its window reports `Running` only while resumed and otherwise
reports `Suspended`; focus and native-window availability remain separate facts.
This makes redraw-backed owner continuations honest: the loop consumes redraw
flags only after polling confirms execution is still running. A queued `Pause`
therefore preserves the flag rather than acknowledging a continuation whose frame
will be refused; `Resume` makes the callback deliverable again. The app also retains
a logical continuation when the initial actuator refuses it and retries from the
next fresh native root.

A manual run on a dedicated booted iOS simulator covered the cases
foreground-reentered-background, background-reentered-foreground,
duplicate-foreground, foreground-callback-panic, nested background and
foreground transitions while active or inactive, nested-foreground-then-panic,
superseded-foreground, and background-nested-run-loop. The last verified that a
queued background stops frames before the observer pumps a nested native run
loop.
The run terminated only its owned process after marker completion; this is not a
claim of normal UIKit loop return or a real OS background transition. Xcode 26.2
is the verified SDK. UIScene ownership and migration remain separate work.

### UIKit scene attachments

UIKit process bootstrap and scene connection are separate ownership boundaries
(ADR-0073). `IOSPlatform::on_scene_event` registers the fallible, owner-thread
consumer before UIKit connections. A stable logical view backs a session; native
windows/controllers and display links belong to individual attachments. A
reversible disconnect retains the logical owner but denies new raw handles.
Terminal close fences callbacks and retires the attachment before disposal.

`MainThreadBound` protects native storage. Final worker drops enqueue retirement
on main rather than synchronously waiting for it. Plain cached metrics remain
readable without touching UIKit. A native weak-view check verified both
retention through logical close and eventual release after a worker's final drop.
OwnerSignal/GCD carries typed process wake/quit independently of display links;
it grants no operating-system background execution entitlement.

### A ready task imposes no executor bounds on its result

`Task<T>` moves its result out and never exposes a pinned borrow to it.
The handle is therefore `Unpin` independently of `T`, as is the native
`JoinHandle<T>` variant. Its `Future` implementation accepts local, borrowed
and non-`Unpin` results; `Send + 'static` belongs to executor admission,
where work crosses threads, rather than to polling a completed value.
Dropping a spawned handle detaches the Tokio task; it does not cancel it.
Pinned by `ready_tasks_return_results_without_executor_bounds` through the
public API.

### Browser display units, held buttons and callback reentry

Browser screen dimensions are CSS pixels; display bounds expose rounded device
pixels after applying the device pixel ratio. Pointer coordinates retain the
platform input contract of logical pixels, including the upstream
`PhysicalPosition` storage name. DOM button masks preserve X1/X2 on down, up,
move and wheel events.

Platform window-event handlers are called through the call point outside the
state mutex and restored unless the callback installed a replacement. Setter
retirement also occurs outside the mutex. Quit commits stopped state and consumes
its callback in a one-shot call, so reentry cannot invoke the same quit callback
again. A failed displaced callback is retained rather than destroyed while
propagating its first failure.
Source inspection and type checking do not prove browser execution
or arbitrary aggregate destructor containment.

### The Web display period is measured from animation frames and is an estimate

**Rule:** a browser page cannot ask for its display's refresh rate, so the Web
backend reports one only as a measurement of what the page can see, says so, and
reports unknown until it has measured. Capabilities are detected by asking for the
function, never by reading a browser version.

**Choice:** `display::web::source` owns the snapshot. The period comes from the
timestamps `requestAnimationFrame` hands the platform's existing frame loop
(`owner/web.rs`), fed to `display::web::refresh_estimate::RefreshEstimator`: the trimmed
mean (without the fastest and slowest tenth) of the last 30 intervals, with
intervals longer than three times the window's lower quartile left out (a background
tab, a debugger pause, throttled frames: a window that is mostly throttled has a slow
median, so the baseline is the fast end, not the median). A mean and not a median
because a clock clamped to whole milliseconds reports 120 Hz as 8 and 9 ms
intervals, whose median is 8.0; the trim keeps a few dropped frames out. The first
value waits for a full window. A replacement is published only when it has left a 3%
band around the published value and then stayed within 1% of where that run began for
five consecutive frames, while the mean of the latest ten intervals agrees with it
(within 6%): while the window holds two rates the estimate glides from one to the
other at a value the display never ran at (a live run published 95 Hz on the way from
144 to 100 Hz before the stability rule), and a gliding estimate is never still. The band keeps 59.94 and 60 Hz one value and 60, 75, 90,
120 and 144 Hz apart. The estimate restarts when the tab becomes visible and when
the pixel ratio changes (the intervals no longer describe the cadence; the published
period stays until a fresh estimate replaces it), and is forgotten when the
`currentScreen` object changes or, without screen access, when `window.screen` raises
`change` (the only sign of a move to another monitor, indistinguishable from a
metrics-only change), whose rate the old value does not describe. A
published change invalidates the cached snapshot and wakes the owner through the
`OwnerSignal`; the owner's next turn reads the new snapshot.

`Platform::display_snapshot` runs on every owner wake, so the snapshot is cached and
read from the browser again only after an event that can change it: a pixel-ratio
change, a published period, `change` on `window.screen`, a change of the screen list,
of a listed screen or of the window's screen.
The pixel ratio is watched with `matchMedia("(resolution: Xdppx)")`
(`display::web::dppx_query`): the query matches while the ratio is exactly `X`, so it
fires on any change and is spent by it, and the next one is armed for the new ratio.
Arming checks `matches()` and reads the ratio again, a bounded number of times, when
it moved between the read and the listening (a query that does not match would never
fire for a change that already happened); a refused query is retried for as long as it is refused, after a growing wait
(one frame, two, four, up to 120), and also at once on a window `resize` or a
`window.screen` change, which zoom already fires: a watch that stays unarmed would
miss every zoom and scale change (accepted work must stay deliverable). Every query is listened to with one retained JS function, so the callback
that is running never frees itself. `window.screen` raises `change` for a size, ratio
or position change in Chromium without any permission, detected by `onchange` being
present (`WebFeature<ScreenChange>`); the browser calls that need a capability are
methods on its proof, defined in `display::web::source` beside `BrowserFeatures`.

Several screens are listed only when `getScreenDetails` exists and the
`window-management` permission already reads `granted` (the older
`window-placement` name is tried when the current one is rejected). The platform
never requests the permission: a prompt is the application's explicit act, and a
permission granted later is noticed through the `PermissionStatus` change event.
Each `ScreenDetailed` object is the screen's identity, held in a retained list
(`display::web::screen_identity`): a screen found in the list keeps its `DisplayId`, an
object that matches nothing gets a fresh one, and an id leaves the list with its
screen and is never issued again. Labels are not keys; two identical monitors share
one. A `change` on a listed screen (it is followed with the list) refreshes the
snapshot. The estimate belongs to the screen the window is on (`currentScreen`); the
others report unknown. The current screen reports `window.devicePixelRatio` as its
scale, which includes page zoom, exactly as in the single-screen case. Bounds do not
move with zoom (a live run showed them inflating): with access they use the screen's
own `devicePixelRatio`, origin included, so a layout of mixed-ratio screens is the
browser's coordinate space expressed per screen, not one global device grid; without
it they use the page's ratio when `window.screen` was first seen (Chrome's
`screen.width` ignores zoom and `devicePixelRatio` is OS scale times zoom, so the
zoom cannot be separated afterwards), taken again on a `change` of `window.screen`.
A page that starts zoomed overstates its bounds until then. The first time a listing
appears, its `currentScreen` adopts the id of the single-screen display, so the
window sees the display gain a name, not change.

**Limits:** in a browser whose `screen` has no `onchange`, moving the window between
two screens of equal pixel ratio but different size raises no event, so the bounds are
stale until the next one. The estimate is the presented cadence: a power saver, a
throttled frame or a variable-refresh panel lowers it. Both are documented on the
module. The window's state names the current screen. Every event above wakes
the owner, and the wake reads the window's state again before the owner's turn,
so the turn sees the window's display and the display snapshot of the same
moment (`web_window_names_the_display_the_snapshot_describes`).

**Alternatives considered:**
- Reading `screen.refreshRate` or any version-keyed table: no such property is
  standard, and a version says nothing about what the user enabled. Rejected.
- Requesting Window Management access on startup: the prompt would appear in every
  application. Rejected; the application asks, the platform follows.
- Keying a screen by its label or position: both repeat or move. Rejected for the
  object, the only identity the API offers.

**Coverage:** the rules are free-standing and run on every host
(`web_refresh_estimate_table`, `dpr_change_arms_the_query_for_the_new_ratio`,
`retained_screen_keeps_its_id`, `web_feature_absent_gives_no_handle`); the browser
glue runs on Node in the wasm lane against a controlled boundary
(`tests/wasm32/display.rs`) and by hand in a browser.

### Web logical keys validate their source grammar

Web keyboard and MouseEvent-based pointer/wheel input preserve positive
canonical getModifierState observations, including AltGraph and toggle states,
alongside the four convenience booleans. Supported legacy Hyper/Super modifiers
normalize to Meta. No source inference from Ctrl+Alt, AltRight or a named-key
sequence is made; raw facts remain distinct from widget command interpretation
(ADR-0201). Native observation completeness is not implied by the owned DTO.

The browser page's `observed-modifiers`, `modifier-getter-reentry` and
`modifier-getter-retirement` controls tested registered DOM delivery through public
input, source metadata, same-family reentry and retired-owner silence followed
by healthy replacement delivery. FnLock/SymbolLock and legacy source overrides
are explicitly controlled-getter cases. Constructor delivery is not a physical
keyboard/layout or native-IME witness.

The private DOM getter retains the original UTF-16 string until admission.
`JsString::is_valid_utf16` refuses an unpaired surrogate before conversion can
replace it with U+FFFD. The literal replacement character and valid surrogate
pairs remain valid inputs. The getter runs once, including during reentry.

Both DOM keyboard states resolve the owned named-key vocabulary and accepted
Hyper/Super aliases before admitting a character value. A Web character is a
nonempty NFC string with zero or one non-control base followed by combining
characters, as defined by
[W3C key strings](https://www.w3.org/TR/2025/REC-uievents-key-20250422/#key-string).
Control means Unicode category Cc; combining means Mc or a nonzero canonical
combining class. A scalar or grapheme count does not establish that grammar.
Refused values become Unidentified without discarding physical code, location,
timestamp, modifiers, composition or the Down/Up observation. No normalization
rewrites source meaning. `Key::character` and native producers keep their
nonempty-text contract (ADR-0143).

The private Web conversion uses ICU4X `icu_properties` and `icu_normalizer`
2.3 compiled data. General categories, canonical combining classes and NFC
therefore come from one library cohort; combining classes reuse normalization
data. Dependencies are wasm-target-only, and admission adds no owning service,
mutable cache or public parser. Browser execution establishes
constructor-driven translation; it does not establish physical layouts or IME
text commits.

### Off-owner accessibility adapter retention precedes diagnostics

The Win32 and AppKit accessibility adapters cannot retire their native subclass
state off its owner thread. Their existing exceptional retention fallback moves
the adapter into permanent retention before attempting diagnostics. A subscriber
failure is contained independently; it cannot restore a destructor obligation on
the wrong thread or replace an unwind already in progress.

The private shared `retain_off_owner_resource` seam is used by both adapter
Drop paths. `the_owner_thread_machinery_honours_its_contracts` exercises the same
production boundary with an opaque aggregate resource surrogate: successful
reporting, an opaque reporting failure, an earlier teardown failure alone and
competing with reporting, and a subsequent callback. These rows do not execute real UIA or
NSAccessibility adapters; they prove the shared boundary. Native wiring remains source-reviewed and subject to
platform type-checking.

### Input processing has no unused platform-side duplicate

ADR-0082 §5's helper retirement removes the zero-consumer `PlatformEmbedder`
abstraction and platform velocity/timestamp helper types. Backends retain their
actual event clocks and translation paths; interaction owns velocity tracking.
The compile-fail cases `basic_velocity_tracker_is_retired`,
`platform_embedder_is_retired`, `system_timestamp_is_retired` and
`timestamp_provider_is_retired` check, one name each, that the retired types
are unavailable at the crate root. This does not adopt the ADR's
remaining backend consolidation or capability removals.

### Android has no separate page-aligned container

Android exposes no framework allocator or page-aligned vector. The removed
container had no production consumers, while its generic element retirement
could leave already-dropped elements marked initialized after a destructor
panic. Framework buffers use standard owning containers; wgpu owns GPU
allocation and its backend alignment requirements. A device's native page size
does not require every application-side vector to use a custom allocator.
This removes an unused unsafe surface rather than asserting native allocation
behavior: Android execution remains unavailable on this host.

### AppKit tab joins borrow live windows

`MacOSWindowExt::add_tab_to_window` takes `&dyn HostWindow` and answers whether
AppKit received the join (`None` when the receiving window is not an AppKit
window). A numeric ID cannot establish ownership of an
Objective-C object. The backend rejects a different backend, different owner
lanes, non-main lanes, closed windows and joining a window to itself before
messaging AppKit. Both borrowed wrappers own their NSWindows, which are created
with `releasedWhenClosed:NO`; explicit native retains span the callback-capable
join on their shared main owner lane. No state or registry mutex spans that call.

The method's public compile-fail doctest rejects numeric IDs, and its compiling
example pins the borrowed-window signature and its `Option<bool>` answer. These
compiler checks do not prove native tab grouping or close/reentry behavior.
AppKit runtime execution remains unverified on the Windows audit host.

### Win32 observations and closed-window tracking

A window context holds weak references to the wrapper's observation cache and
platform registry. Native size, position, visibility, focus and DPI messages
publish cache changes before invoking callbacks; no cache mutex spans a user
callback or native call. Initial dimensions are the native client dimensions,
and setters do not overwrite a synchronous native observation with their
requested outer dimensions. Minimization keeps the last non-minimized size.

`WM_DESTROY` clears userdata and retires its context ledger before removing the
matching window identity from tracking. The removed owning Arc is dropped outside
the registry mutex. Consequently its destructor cannot recursively destroy the
in-flight native window, and an old wrapper cannot remove a reused HWND's newer
registry entry. Weak context references do not alter the wrapper's final-state
owner count or introduce another cycle. WNDPROC callback failures retain the
existing fail-stop policy; this does not claim recoverable native unwinding.

`test_window_lifecycle_contract` adds bounded Windows subprocess rows
`native_close_retires_final_registry_owner`, `closed_window_retires_tracking`,
`close_callback_releases_external_owner`,
`resize_callback_observes_current_client_bounds` and
`hidden_popup_is_natively_hidden` and
`move_callback_observes_full_native_coordinates`. These create actual hidden native windows,
check owner retirement (including the registry's final owner during native
`WM_CLOSE`) and subsequent opening, compare getters inside and after
real native resize delivery, query native popup visibility and compare full
client-origin observations across a native move beyond signed16 coordinates
and a subsequent ordinary move. Cache and Moved events use `ClientToScreen`;
packed message coordinates remain a fallback only if that query fails. These rows do not
exercise monitor DPI migration or interactive focus changes.

`resize_callback_preserves_large_native_dimensions` pins unsigned `WM_SIZE`
decoding in the same bounded native family. A hidden popup is resized separately
to a width and height above 32767, comparing `GetClientRect` with callback sizes
and public getters inside and after delivery, followed by an ordinary resize.
The row requires the actual native dimension to reach the unsigned range; an OS
limit reports an unavailable witness instead of allowing a false positive.
Zero-size clamping and minimized-window handling retain their existing behavior.

### Win32 keyboard delivery preserves permitted native defaults

Keyboard down/up messages run the input callback first and reach `DefWindowProcW`
unless its result prevents native default handling. This includes the conservative
`DispatchEventResult::DEFERRED` returned during callback reentry. System-key defaults
provide native actions such as Alt+F4, whose close command still passes through the
existing close-veto hook. A callback may close its window; the outgoing context's
guard pins its allocation while a userdata comparison refuses native defaults on
a retired context or a recycled HWND.

The bounded Windows subprocess rows `unhandled_system_key_closes_window`,
`allowed_system_key_closes_window`, `prevented_system_key_preserves_window`,
`system_key_close_honours_veto`, `system_key_callback_can_close_window`, and
`deferred_system_key_preserves_window` send the system-key message through an actual
owned HWND and dispatch the resulting native system command. These are native
message-protocol checks, not physical keyboard input or rendered application checks.

### Linux displays come from the compositor on a connection of their own, and a winit window names its display by native number

`display/linux/` reads the outputs of a Wayland compositor or an X
server without a window system: `wayland_model` and `x11_model` are pure (they
take events or a RandR state and answer `OutputView`s, so every host checks
them), `wayland.rs` and `x11.rs` are thin adapters behind the `wayland` and
`x11` features, and `source.rs` resolves the views into the snapshot and keeps
them current from the thread `flui-display-linux`. The winit backend takes its
snapshot from it on Linux; Windows and macOS take theirs from their native
sources, and winit's own monitor list is read nowhere.

**Why a connection of its own.** winit enumerates monitors only through a live
event loop and reports no change outside it, and its Wayland connection
belongs to winit. A second connection costs a socket and a thread and gives a
snapshot before the loop runs, with the same global names winit sees. The
thread only waits (`poll` on the connection and a stop `eventfd`), swaps the
new list in, marks the snapshot stale and wakes the owner through a `Weak`
signal; application code never runs on it. Shutdown closes the owner signal's
admission first, then writes the stop descriptor and joins; the thread drops
the connection before it ends. A source that cannot connect, or loses its
connection, answers every snapshot with that error until a watcher starts
again, instead of reconnecting on every owner wake.

**Which protocol.** The rule is winit 0.30.13's, read from its source:
`WAYLAND_DISPLAY` or `WAYLAND_SOCKET` set and non-empty means Wayland, then a
non-empty `DISPLAY` means X11. winit 0.30 no longer reads `WINIT_UNIX_BACKEND`.
The winit backend then checks the choice against the display winit actually
opened (the event loop's `RawDisplayHandle`, which is the one its windows use)
and replaces the watcher on a mismatch. `WAYLAND_SOCKET` hands over a single
connected descriptor that winit consumes, so the source connects by
`WAYLAND_DISPLAY` only.

**Wayland.** An output is listed after its first `wl_output.done`, and each
later burst replaces what is listed only at `done`
(`wayland_output_applies_only_after_done`). `wl_output` is bound at version 4
where offered; version 1 has no `done` and is not bound. `zxdg_output_v1`
supplies the logical position and size, and its own `done` closes a burst only
below version 3. Bounds are in the compositor's logical space (Wayland has no
global device-pixel space), the scale is the mode's width over the logical
width (1.5 where `wl_output.scale` says 2), and `refresh` is millihertz, zero or
less meaning unknown. There is no primary output and no work area. The key is
the version 4 `name`; an output without one gets a fresh id that it keeps while
its global exists (`wayland_below_v4_mints_fresh`), and two live outputs that
report one name are two displays, the later keyed fresh
(`two_outputs_reporting_one_name_are_two_displays`).

**X11.** RandR 1.5 monitors are the displays; an older server is an error. A
monitor is keyed by the XID of its **first** output
(`x11_monitor_is_keyed_by_its_first_output`); a monitor with no output (made by
`xrandr --setmonitor`) is not listed. The refresh rate is `xrandr`'s:
`dot_clock / (htotal * vtotal)` with `vtotal` doubled for a double-scanned mode
and the interlace factor folded into the numerator, in 128 bits
(`x11_big_dot_clock_does_not_overflow`), and zero totals are unknown
(`x11_zero_htotal_or_vtotal_is_unknown`). The scale is `Xft.dpi / 96`, the usable
area the monitor cut to `_NET_WORKAREA` of the current desktop. A whole
reconfiguration (screen, CRTC and output notifications) is one read of the
state, and a state whose displays are unchanged wakes nobody.

**Native number.** winit 0.30.13's `MonitorHandleExtX11::native_id` is the
**CRTC** (not the RandR output), and
`MonitorHandleExtWayland::native_id` is the `wl_output` global name. A display
therefore answers to the CRTCs behind all of its monitor's outputs and to its
global
(`winit_window_matches_its_output_by_native_id`: mirrored outputs of one
rectangle stay two displays, a tiled monitor is one). A monitor handle with no
match, or a window with no monitor, has no display.

**What reaches the window.** `Moved`, `ScaleFactorChanged` and `Resized` ask
the window whether its display changed. A change of the monitor list needs no
event of its own: the thread wakes the owner and the host derives every window's
display from the new snapshot. On Wayland winit reports no event when a window
enters an output: none for its first (a window is on no output until its first
frame enters one), and only a `ScaleFactorChanged` for a later one of another
scale, so a window moved to an output of equal scale but another refresh rate
would keep the old one. Every Wayland window is therefore asked its monitor again
each time the loop goes idle (`about_to_wait`, `sync_unannounced_displays`),
which follows the compositor's `enter`; a change publishes the window's state
and then raises the display hint. An X11 window reports moves with `Moved`, so
only one with no display yet is looked at there. The `wl_surface.enter` events
themselves belong to winit's connection.

**What was not run.** The pure rows run on every host. The thread is tested
against a scripted session on Linux (`display_thread_contract`: change, wake,
stop-and-join, failure, panic, closed owner, dropped signal). The Wayland and X11
adapters were run live against WSLg (Wayland and XWayland), a headless weston and
an Xorg dummy server whose modes `xrandr` changed; the macOS winit source is
compiled and type-checked (`aarch64-apple-darwin`) but not run, and a window
moved between two real monitors was not observed.

### The file store replaces a value with `std::fs::rename`, and probes an access refusal

`storage::FileStore::write` stages the bytes in a file `tempfile` names uniquely
in the target's own directory, but opens it with `std` and moves it with
`std::fs::rename`, never `NamedTempFile::persist`. On Windows `tempfile` passes
`MoveFileExW` and `SetFileAttributesW` a path without the `\\?\` prefix, which
fails past 260 characters, and marks the file it creates
`FILE_ATTRIBUTE_TEMPORARY`, which the replaced value would keep; `std` prefixes
the path and falls back to POSIX-semantics replacement. A write based on a
version holds an exclusive `File::try_lock` on `<root>/.flui-storage.lock` from
comparing the stored version to the rename, so two processes on one directory
cannot both succeed from the same base. The comparison checks the stored
length against the base's (`StoredVersion::byte_len`) before reading, so a
value of another length is never read for it. A file system without locks
refuses such a write as `Unsupported` (`NotOnThisDevice`) rather than writing unguarded:
`std` maps only some of its codes to `ErrorKind::Unsupported`, so
`ERROR_INVALID_FUNCTION` (1), `ERROR_NOT_SUPPORTED` (50) and `ENOLCK` are
recognised by code.

Each write stages its bytes in a `.flui-staged-*` file. A writer killed before
its rename leaves that file behind; a based write, holding the root's lock,
removes the store's own staged files older than 60 seconds. A live write
finishes its file within one call, so it is never that old; another program's
files never carry the prefix.

Windows reports a sharing violation (32) and a lock violation (33) only by code;
both are `Busy`. `ERROR_ACCESS_DENIED` (5) is both lasting (a read-only file,
an ACL, a directory in the file's place) and passing (a holder that did not
grant delete sharing, a file pending delete), so a refused rename probes the
target. A directory or a read-only file is `Inaccessible`. A target that
refuses even an open requesting no access is pending delete and `Busy`: an ACL
does not refuse that open, and `fs::metadata` cannot tell, because on that
refusal `std` reads the attributes from the directory listing. A target that
then refuses to be opened for writing with code 5 is `Inaccessible` and never
retried; one that opens, or is held without write sharing, is `Busy`.
Machine-local values always live in `<local root>/.machine-local/`, so they
stay apart from roaming values when both roots are one directory (Linux),
however the two paths are spelled: the store never compares paths. No
`StorageName` starts with `.`, so neither that directory, the lock file nor a
staged file collides with a value.

`storage::file_store::tests::file_store_interruption_matrix` stops a write at
each step and reads the old whole value until the rename and the new one after
it; `two_threads_interleave_and_exactly_one_conflicts` holds one writer between
its comparison and its rename until the other has tried, and fails without the
lock; `a_based_write_sweeps_stale_staged_files` ages one orphan and keeps a
young one and a foreign file. `storage::os_error::tests::os_error_classification_table`
pins the mapping of codes to `StorageError`, including the lock codes. The
`file_store_contract` table in `tests/file_store.rs` covers the directory
created on the first write, a directory in the file's place, the lock, the
read limit, the two scopes under separate roots, one root and one root spelled
two ways, a long non-ASCII path; on Unix only, a stale base refused against a
sparse terabyte without reading it; on Windows only, a read-only target
(`Inaccessible`), one held without delete sharing and one pending delete
(both `Busy`, reached through code 5 and the probe). `sync_all` matters only on
power loss, which no test causes; a killed process (`taskkill /F`) and two
processes on one directory are left to the dated native run.

### Every backend assembles pointers through one registry

Backends copy each native event into a plain `input::assembly::NativeReading`
(no `windows::*`, `objc2`, JNI or DOM type) and `decode` it against a
`ContactRegistry` (ADR-0217 §3). The registry keys a contact by surface, device
label and contact label, except the backend's reserved contact (a mouse on
Win32, AppKit and the Web, the Pencil on UIKit), which is one cursor whichever
device drives it: a reading naming another device or none moves it and it
carries the device that drove it last; mints pointer and device identities in order and refuses
permanently after the last; keeps a mouse's contact for the life of its device on
the surface; returns a lifted pen to hovering on the same identity until the
platform's leave, so its `Enter` and `Leave` pair up (a touch ends at `Up` or a
cancel, and any contact a cancel or a retirement removes, a hovering pen at a
window's focus loss among them, delivers the `Leave` for an `Enter` it delivered,
after which the platform's own leave is a silent orphan; a hovering pen whose label returns as
another kind leaves first); drives touch and pen sequences by their contact (a barrel button
changes within it, or while the pen hovers) and mouse sequences by any held
button; counts clicks only from OS settings a backend supplies (Win32 today, AppKit
reports its own count, winit reports none); folds trackpad components into one
`PanZoomAccumulator` session; and chooses each sequence's clock when it starts:
the performance counter when the device and the host report one, otherwise the
coarse stamp, after which that device stays on the coarse stamp. Stamps within a
sequence never run backwards. A reading is measured completely before anything is
committed: an undecodable one cancels its open contact as `InvalidInput`, an orphan
(a release, leave, cancel, touch move or gesture update for no open contact) emits
nothing, and both log a diagnostic. `MoveBatch` stamps a native batch's coalesced
readings on the contact's clock in order and fails the whole batch on one bad
reading; a failing prediction is only dropped. The reading that began the current
press sequence stays on the contact for a native drag start. Decoding allocates
nothing per event once contacts are open; a coalesced batch allocates the
vocabulary's sample buffer.

`pointer_assembly_contracts` in `input::assembly::tests` pins label reuse after
Up, primary admission, the mouse cursor's identity and cancellation, the
reserved contact across devices (`the_reserved_contact_is_one_cursor_across_devices`), the pen
contact rule, a lifted pen's hover until its leave, an entered touch's leave at its end, the originating reading, device hot-plug, identity exhaustion, click
counting across the OS interval and rectangle, gesture accumulation including the
session end, move batching, undecodable and orphan readings, a press before any
position, and clock rebasing, wrap, saturation, monotonic order and per-device
choice; `pointer_assembly_allocates_nothing_per_event` in
`tests/pointer_assembly_allocation.rs` counts allocations over every per-event
path with a positive control. The backends' own rows (Win32
`native_pointer_decoding_contracts`, winit `pointer_translation_tests`, AppKit
`native_pinch_and_rotation_share_one_cumulative_gesture`, UIKit
`native_touch_readings_keep_sensor_presence_and_pen_angles`, Android
`android_motion_translation`) pin only the copy into readings. Scroll phases
go through the shared `ScrollPhases` grammar, and a two-finger pan is a phased
scroll on every backend, winit's `PanGesture` included; a pinch's translation is
its focal point, which every `Update` refreshes (ADR-0217 §4;
`two_finger_pan_is_phased_scroll_on_every_backend`,
`pan_zoom_carries_only_scale_and_rotation`, `winit_pan_gesture_is_a_phased_scroll`).

### The winit translation makes up for what winit reports alike on every session

`input/winit.rs` decides what winit's shared shapes hide
(`session_quirks_translate_to_one_grammar`):

- A winit device id is labelled per kind of input (cursor, touch, trackpad
  gesture): Wayland gives every pointer and touch event one id and X11 the
  master device's, so a mouse and a touchscreen behind one id are two devices
  of known kind, both present, rather than one of mixed kind that reports
  none. `DeviceEvent::Removed` retires every label of the id.
- winit 0.30 drops the protocol's event time, so every reading is stamped at
  dispatch. A move is held until the next event: a move of the same contact
  replaces it, anything else (another window event, an owner wake that may
  close its window, the end of the loop turn at `about_to_wait`) delivers it
  first (`programmatic_close_runs_the_full_teardown_and_exits_the_loop` holds a
  move across a close request). The moves a late turn dispatches back to back are then
  one move, and velocity never sees their microsecond spacing.
- On X11, winit reports a `CursorMoved` on the master device at the first
  touch's position before its press and moves; a held move that any event of
  a touch at that position follows (its lift and cancel included) is dropped,
  so nothing hovers under the finger. A
  fractional `LineDelta` is a touchpad's smooth scrolling and is precise; it
  carries no phases.
- On Wayland, a leave with a button held cancels the mouse's sequence: the
  compositor that took the pointer sends no release. On X11 the implicit grab
  keeps delivering the sequence after a leave, and it continues.

### Android motion events decode through a cfg-free translation

`input/android.rs` only copies each `MotionEvent` out of the NDK
into plain facts (`input::android::motion::Motion`: source, action, buttons,
and each pointer's tool, axes and history) and discovers its device's motion
ranges through Java; `input::android::motion::AndroidMotion` turns the facts
into readings, so Linux CI runs what the device runs
(`android_motion_translation` in `input::android::motion::tests`). Two decisions are
the translation's own:

- Android ends a hover with `ACTION_HOVER_EXIT` at the instant a pen or a
  cursor goes down and starts it again with `ACTION_HOVER_ENTER` when it lifts.
  The exit is held: a press from the same device at the same time continues the
  hovering contact, so a pen keeps one pointer identity from hover through
  contact and back, as on Win32 and UIKit; any other event, or an owner turn
  `EXIT_GRACE` (40 ms) after it, delivers it as a leave, since the press can
  reach the queue a turn after its exit and a pen leaving range sends nothing
  more. An enter for a contact still open is a move.
  (`a_pen_keeps_its_identity_from_hover_through_contact`,
  `an_exit_no_press_continues_is_a_leave`.)
- A touchpad in pointer mode reports finger tools from `SOURCE_MOUSE`. Its
  hovering finger is the cursor and is delivered as the mouse; its gestures
  arrive as fake fingers around the cursor and are never touches. A press
  whose first pointer carries `AXIS_GESTURE_PINCH_SCALE_FACTOR` opens a pinch,
  delivered as `PanZoom` scale about the cursor; one without opens a
  two-finger scroll, delivered as phased `Scroll` from the cursor by
  `AXIS_GESTURE_SCROLL_*_DISTANCE`, already in the owned sign; one with
  `AXIS_GESTURE_SWIPE_FINGER_COUNT` is a three- or four-finger swipe, which the
  system navigates with and FLUI does not translate; a press with a button
  held is a click. The axes stand in for `getClassification`, which
  `android-activity`'s event wrapper does not expose; both arrived in API 34,
  where the availability table starts reporting precise scrolling and
  trackpad gestures. (`a_touchpad_pinch_is_pan_zoom_about_its_cursor`,
  `a_touchpad_scroll_is_phased_from_its_cursor`,
  `touchpad_clicks_swipes_and_touches_stay_what_they_are`.)
- A pointer lifted with `FLAG_CANCELED` (API 33: the system rejected a palm)
  is cancelled, not released, so a tap under it never accepts
  (`a_cancelled_pointer_up_is_a_cancel_not_a_lift`).

The Java device query (`InputDevice.getDevice` and its motion ranges) runs
for every event but a move (`ACTION_MOVE`, `ACTION_HOVER_MOVE`, the events at
the sample rate), for a device id the cache does not hold, or for a source it
was not read for: a device the OS reset or removed is seen at the press that
starts a sequence and at the lift, exit or cancel that ends it (a reset
cancels its sequences) (`the_device_is_queried_for_every_event_but_a_move`).

Android has no counterpart to `PointerRevision`: a `MotionEvent` carries no
estimated properties to revise, so the backend never emits `Revise`.
A run on a device held the translation (see the
native recording section below).

### A native recording is the journal of one pointer registry

A native recording (ADR-0215 §4) holds what a backend hands the shared
decoder, not OS structures: a header with the backend, the clock anchors
(epoch, uptime, performance-counter count and frequency) and the registry's
setup (reserved contact, first identities, click settings), then one row per
registry call — a reading, a coalesced batch, a device hot-plug, a surface or
contact cancel. Replaying the rows through the same calls reproduces the
backend's `PlatformInput` on any host, so Linux CI exercises Win32 QPC
stamping, AppKit and UIKit uptime clocks, Android history batches and DOM
predictions. The format is the `HostInput` recording's framing (JSON Lines, a
major version, unknown fields skipped, an unknown variant a typed error); its
codec lives in `tests/native_recording/codec.rs`, so serde never enters the
backend graph. The header declares every device label its rows read, with the
kinds they read it as, and the reader refuses a row that names another.

Recordings are captured in debug builds with the `native-capture` feature:
`input::assembly::capture::capture` installs a sink on a thread, and
every registry created there while it is installed — a backend window's
included — reports each call before acting on it. A registry reports to the
capture installed when it was created, identified by session rather than by
index, for as long as that capture's guard lives; the sink runs with its own
reentrant calls skipped, and a panicking sink is dropped and leaves the
registry unchanged. A call made on the registry from outside a reading is
journaled by its caller: a mouse wheel turn (`ContactRegistry::wheel`), a
scroll step from an open contact with its phase already resolved
(`ContactRegistry::scroll`: Android's wheel and two-finger scroll), and a
touchpad viewport's callback or abandonment (`TouchpadAssembler`), whose
pinch readings are decoded unjournaled because replaying the viewport row
derives them again. The codec reads every number back to the bits it was
written from (serde_json's `float_roundtrip`): a cumulative pinch scale read
one ulp off replays a different transform than the device delivered.

On a Windows host the injected goldens are recorded: each child row of the native window contract
(`a_captured_native_pointer_session_replays_to_what_it_delivered` for a touch
and a pen from `InjectSyntheticPointerInput`,
`a_captured_native_mouse_session_replays_to_what_it_delivered` for mouse buttons
and wheels from `SendInput`) checks that its journal replays to exactly what the
window delivered, then holds it against its checked-in golden by shape
(`native_recording::capture::hold_against_golden`: the replayed events with
times left out and positions in device pixels). `FLUI_NATIVE_RECORDING_DIR`
also writes the fresh recording there, and `FLUI_NATIVE_RECORDING_BLESS`
(`--bless`) writes it over the golden. On another host, install `capture` on
the owner thread before the backend opens its windows.

The Android goldens were recorded on a device by a `NativeActivity` library
that installed the capture before the Android platform created its registry and
shared the test codec and sink (`tests/native_recording/journal.rs`) through
`#[path]`. Each session, in a
fresh launch, is injected by the system (`adb shell input`, and `uinput`
devices the real `InputReader` reads: a pen with pressure, tilt and hover, a
touchpad in pointer mode), and the library wrote its journal and the `Debug`
of every pointer event its window delivered. The host test
`pulled_device_recordings_replay_to_what_they_delivered` requires the replay
to equal what the device delivered exactly, then holds the recording against
its golden by grammar (`capture::grammar`: who entered, pressed, moved,
pinched or scrolled, in order, with each press's sensors), not by shape: the
device batches and resamples moves by its frame timing, so their number and
positions change from run to run.

A browser page has no file system, so the web goldens were recorded by
splitting the same steps between the page and the host. The page opened the web
backend under a `Recorder` (the capture, kept installed across the event-loop
turns the driver's input arrives on) and reported the recording as JSON Lines
with the pointer events the window delivered and the recording's replay; the
driver sent trusted mouse, pen and touch input over WebDriver BiDi, required
the replay to equal what was delivered (DOM wheel scrolls excepted: they have
no contact, so the registry never sees them), wrote the recording on the host
and held it against
`web_bidi_mouse_pen_touch.jsonl` natively
(`browser_sessions_hold_against_their_goldens`, given
`FLUI_NATIVE_RECORDING_FRESH`). Its moves are single steps between pauses
longer than a frame, and touches move one at a time, since Chrome
interpolates a move over its action tick and its touch predictions depend on
timing; the shape is then the same on every run.

`native_recording_contracts` in `tests/native_recording/mod.rs` replays one
recording per backend (`win32_pointer_info_recordings_decode_to_their_host_input`
and its AppKit, UIKit, Android, winit and web rows, the web's Chrome capture
`web_bidi_recording_decodes_to_its_host_input` among them) against hand-computed
transcripts, writes each back byte for byte, refuses unknown variants, other
versions, out-of-range values and undeclared devices, and, with the feature,
pins the capture's journal, session identity, panic containment and reentrant
release and replacement.

### A window's input waits behind its capability snapshot

`WindowCallbacks` owns the ADR-0213 §3 feed: a backend sets the initial
`InputCapabilities` (its availability table from `input::availability`
with the presence its device registry reports) when it builds the window, and
`set_input_callback` commits the registration before delivering that snapshot,
then the input held while no snapshot had been delivered. Input admitted before
the first registration is held, not dropped; its native default is not
suppressed, since nothing claims it yet. A pass delivers only the entries it
started with and contains a callback panic itself: a failed snapshot stays at
the head (three failed passes per registration, then held until re-registration
or close), a failed ordinary entry is not re-delivered, and the tail keeps its
order. Remaining work is resumed by an owner turn the backend's `OwnerSignal`
schedules (`OwnerSignal::input_wake`), so an idle application still drains it.
The feed holds at most 256 entries, as many as the runtime holds while it waits
for a tree: past that the oldest ordinary input is evicted (a queue of
capability changes only gives its head to the snapshot's baseline), so a window
whose callback is never registered keeps only its newest input. Once a
registration's snapshot has failed its passes nothing can deliver, so new input
is refused with the default result, keeping its platform default, until the
callback is registered again; the first loss is reported once.
The rows of `input_capabilities_contracts` in `tests/input_capabilities.rs` pin
the order, retries, the bound and the refusal
(`input_held_before_any_registration_is_bounded`,
`input_after_the_snapshot_attempts_run_out_is_refused`), reentrant admission
and a replaced callback whose capture panics. Presence changes reach a window from the headless device handle and the
winit backend's registry; the native Win32, AppKit, UIKit, Android and web
backends report their initial table without presence changes yet.

### Every backend's haptics table is a pure function

`haptics::degradation`, the haptics model, holds each backend's ADR-0210 §7 row as a
function from the device's facts (OS version, API level, primitives, pen
waveforms) to the native call and its `HapticOutcome`; the native backends call
it, and Linux CI runs every row (`haptic_degradation_table`). A source the backend
does not know plays on the window's actuator (`actuator_for`,
`stale_haptic_source_plays_on_the_window_actuator`), and the conformance table's
haptics rows hold every backend to that and to its claimed capabilities. Where a
backend can only step between values, a step takes the lower of its two ends, so
no table raises an intensity.

Where an Android 36 device has envelopes, the whole pattern plays as one
`BasicEnvelopeBuilder` envelope: silent steps join the events, and a continuous
event ramps to its first point in one millisecond, the shortest step an envelope
takes, then holds it. A pattern of continuous events plays exactly; one with
transients reports `Approximated`, because a transient becomes a 10 ms pulse in
the envelope: the public `Composition` takes only primitives, so no single
effect mixes the two. Transient-only patterns are compositions of primitives
chosen by sharpness and intensity; overlapping events, and devices without
envelopes, take the amplitude waveform. Composition delays count from the
previous primitive's end, so the backend subtracts the durations API 31 reports.

iOS plays tier 1 through the view-bound generators from 17.5 and the view-less
ones before, and patterns exactly through Core Haptics. Each event is its own
`CHHapticPattern`, started at its time on the engine's clock, because a
parameter curve scales every event of its pattern; a curve longer than Core
Haptics' sixteen control points is split into curves that start where the one
before ended (`ios_pattern_parts`). The engine is created on the first pattern;
its stopped and reset handlers only record the restart obligation and wake the
main thread, which restarts it, and a pattern requested meanwhile plays its
fallback (ADR-0210 §8). The macOS and iOS paths have only been compiled. The
Web path ran in headless Chrome: before the
page's first trusted click `navigator.vibrate` is refused for want of user
activation and the window's actuator reports `Unavailable` with no
capabilities; after it a request is `Submitted`. Chrome's own answer is all
that ran: a headless desktop has no vibration motor.

Android tier 1 posts `performHapticFeedback` to the Java main thread, which
owns the decor view, and the owner never waits for it (ADR-0247 §5): a busy
Java thread, or one blocked on the native thread in a lifecycle callback,
never stalls the owner. One call waits at a time (`AndroidViewAnswers`): a
request made while it waits posts nothing and reports `Suppressed`, since a
call queued behind a stalled thread would only play late. The Java thread
answers each call as it makes it; `false` (feedback off for the view or the
system, or no window) is a state of the view, so later requests report
`Suppressed` until a call plays again, and otherwise a request reports the
table's outcome. A JNI failure is logged; it, or a posted call dropped unrun, is
counted as answered, one call's event that changes nothing after it.

Android pattern vibrations go to the `Vibrator`, which needs the install-time
`VIBRATE` permission (the CLI's manifest template declares it). Without it the
backend reads the vibrator as offering nothing, so its patterns report
`FallsBack` and play their fallback through `performHapticFeedback`, which needs
no permission, rather than claiming what the vibrator would refuse. Every
pattern carries the touch usage, `VibrationAttributes.USAGE_TOUCH` from API 33
and sonification audio attributes before it (`android_vibration_usage`), so the
touch-feedback setting and intensity govern it. On a Galaxy S21 (Android 15,
amplitude control, no primitives) a device run played the
semantic feedback and a continuous swell as touch vibrations, and
a two-tap pattern falls back, as the table says. Android 15 also files a short
effect sent without attributes as touch, so the usage is not observable there;
`haptic_degradation_table` pins it.

A Win32 window has no actuator of its own (`InputHapticsManager` is
unpublished), so it plays on the pen in range of it (ADR-0210 §5): the pen a
request names while the window's pointer registry holds it open, touching or
hovering, otherwise the pen read most recently (`ContactRegistry::pen_contact`,
`a_pen_in_range_is_its_haptic_contact`). Its pointer id gives the
`SimpleHapticsController` through `PenDevice::GetFromPointerId`, read on each
request since the pen behind a pointer is the OS's; with no pen in range, or no
controller, both tiers report `Unavailable`. The controller's calls come from
`pen_timeline`, which hands a continuous waveform its length where the
controller takes a play duration and stops it at its end otherwise
(`haptic_degradation_table`'s `pen_timeline` row), and a per-window worker
thread, in the multithreaded apartment, makes each at its time; a newer request
replaces what is left of an older one, and dropping the window's actuator stops
what still plays. The worker's mailbox holds one request: one the worker has not
taken while a controller call stalls (a Bluetooth pen's HID write leaving range)
is replaced unplayed, and a waveform reached more than two frames after its time
is skipped, so feedback never plays late in a burst
(`a_stalled_pen_worker_plays_only_the_newest_request_and_never_late`). `a_window_without_a_haptic_pen_in_range_reports_unavailable`
drives a real window on a Windows host, with no pen and with a pen pointer the
OS has no pen device for; no haptic pen has played through it yet.

### Win32 precision touchpads are DirectManipulation viewports

`WM_POINTER` reports a touchpad's contacts but not a two-finger pan, its
inertia or a pinch. Each Win32 window owns a manual-update
DirectManipulation viewport over its client area, created beside its text
services from the platform's COM apartment; a window whose viewport does not
initialise reports `precise_scroll` and `trackpad_gestures` `Unavailable`
(`input::availability::windows`), otherwise `Native`. The system offers a
contact in `DM_POINTERHITTEST` to the window under it; only a `PT_TOUCHPAD`
pointer is handed to the viewport, so touch and pen stay raw `WM_POINTER`
contacts. The viewport's handler only appends a plain `ViewportReading`
(status or content transform) to a queue; `Update` runs it synchronously and
the window drains the queue afterwards, so no user code runs inside a COM
callback. The viewport is shared as `Rc<ComViewport>` and every COM call runs
with no `RefCell` borrow of the window's touchpad held. Delivery goes through
a per-window outbox: an input callback that runs a nested message loop (a
dialog, a menu) can pump again from its `WM_TIMER` or `WM_PAINT`, and that
nested pump appends behind what the outer pump has not delivered yet, so the
outermost delivery keeps decode order (a nested `Changed` never overtakes the
outer one, a cancellation never precedes the steps it ends).

`input::windows::direct_manipulation::manipulation` is the cfg-free decoder, replayed from a
checked-in recording on Linux CI. A manipulation is a pan or a pinch, decided
by whichever passes its slop first (2 device pixels of translation, 2% of
scale), since a pinch's centroid drift is not a scroll. A pan is phased
`Scroll` (device pixels, positive when the offset grows, so fingers moving up
scroll down); `Inertia` ends the finger phase and starts
`ScrollPhase::Momentum*`; `Ready` ends everything and asks the window to
return the content to the origin, and the content update that causes arrives
while nothing is open and is not input. A pinch is `PanZoom` on a trackpad
contact at the cursor; the viewport is not configured for rotation, which a
touchpad does not report. A pinch whose fingers lift with the centroid moving
ends at `Inertia`, and the coast that follows publishes nothing but is still
reset at `Ready`, so scale never accumulates across pinches. A status that is
not `Running`, `Inertia` or `Ready` (`Disabled`, `Suspended`) cancels what is
open. Every reset re-anchors the decoder at the origin, so the next
manipulation is measured from where the reset put the content even when the
window discarded the reset's own content update (a paused window does).

Pumping is the window's own: `WM_PAINT` pumps a running or coasting viewport
before the frame callback, `WM_TIMER` (8 ms, clamped by the system) paces a
manipulation no frame serves and still fires inside menu and drag loops, and
a pump that delivered input invalidates the window so the next step is drawn
without the application asking for a frame. A resting viewport has no timer
and costs a frame nothing. `WM_ENTERSIZEMOVE` pauses the viewport (open
phases are cancelled, the content reset, contacts declined, the timer
stopped) and `WM_EXITSIZEMOVE` resumes it. A
failing `Update`, or `WM_DESTROY`, cancels what is open and releases the
viewport (disable, remove the handler, abandon, deactivate) before the
window's callbacks go; the COM apartment outlives its interfaces. After a
failing `Update` the window republishes its capabilities without
`precise_scroll` and `trackpad_gestures`, since two-finger scrolling then
reaches it as wheel ticks (`a_failed_viewport_withdraws_the_touchpad_capabilities`).
A frame takes the cursor where it last read it when it cannot be read, and a
viewport that has never read it stops and drops the manipulation (its next
contact starts it again) rather than placing it at the origin. The
viewport's content rectangle is far larger than the window so the system
never rubber-bands a manipulation.

The decoder rows (`direct_manipulation_contracts`), the recording and the
capability table run everywhere. On Windows,
`touchpad_viewport_initialises_and_survives_its_messages` initialises a real
viewport and sends it its messages, and the in-crate rows
`the_window_pumps_paces_and_pauses_its_viewport` and
`destroying_the_window_cancels_what_is_open_before_it_closes` drive a real
window's pump, frame request, pacing timer, nested delivery, size-and-move
pause and teardown with readings appended to the viewport's own queue. No
precision touchpad has driven a manipulation: that run is
`docs/platform-parity.md`'s condition for ✅.

The recording `win32_direct_manipulation.jsonl` is a regular native recording:
the codec's `viewport` row carries one viewport callback (a status or the
content's scale and offset) with the cursor, scale and clocks of its frame, its
`viewport_abandoned` row one abandonment (the window or its viewport going
away, or a size-and-move pause), and `native_recording_contracts` replays them
through the same `TouchpadAssembler` and the same registry (the pinch is the
registry's gesture reading). Its header's `provenance` says it was written by
hand from the documented callback order. The capture journals both calls as
these rows (`a_captured_registry_journals_the_recording_it_replays`), so a
`native-capture` run of the window on a precision touchpad records the golden
that certifies it; no injector reaches the touchpad, so that run needs the
hardware.

Hardware verification still owed (the Windows device run, on a machine with a
precision touchpad; none of these has run):

1. Sign convention: two fingers moving up scroll the content down (scroll
   offset grows, `dy > 0`), moving left gives `dx > 0`, with Windows'
   "scrolling direction" setting flipped both ways; the viewport must not apply
   that setting a second time.
2. Slop and mode choice: a slow two-finger pan never starts a pinch, a pinch
   whose fingers drift never scrolls, and the first scroll step is not lost.
3. Inertia feel: a flick coasts as `MomentumBegan`, `MomentumChanged`,
   `MomentumEnded` at frame cadence with no stall at the first step, a touch
   during the coast ends it (`MomentumEnded`) and begins a new pan, and the
   8 ms timer plus the self-invalidation keep frames coming with an otherwise
   idle application.
4. Reset: after `Ready` the next manipulation starts from the origin (no
   jump), at 100%, 150% and 200% display scale and after a window resize.
5. Modal-loop pause: starting a window drag or resize with a manipulation
   open cancels it (`Cancelled` or `MomentumEnded`), contacts during the loop
   are declined, and scrolling works again after `WM_EXITSIZEMOVE`.
6. Contact routing: with two windows, the manipulation reaches only the window
   under the cursor; touch and pen contacts never reach the viewport; unplugging
   the touchpad mid-pan cancels it.
7. Capabilities: `precise_scroll` and `trackpad_gestures` read `Native` on
   such a machine and `Unavailable` when DirectManipulation cannot start.

### Win32 drag and drop runs `DoDragDrop` on the owner window's next turn and paces frames from a thread

The platform owns one `OleTransfer`, its only `DataTransferSource`. Every window
registers an `IDropTarget` with `RegisterDragDrop` as it is created (after its
text services, so OLE needs the apartment `OleInitialize` entered, not a bare
`CoInitializeEx`) and revokes it in `WM_DESTROY`, leaving a drag still over it
first. A drag entering a window mints an offer in the transfer's table, keeps
the drag's `IDataObject` in an owner slot for lazy fetches until the target
concludes the drop or the drag leaves, and answers `DragEnter`/`DragOver` from
the feedback the window cached, narrowed to one permitted action. The drop's
action is that cached answer; a refused drop leaves instead. Formats map one to
one (`CF_UNICODETEXT`, CF_HTML, `CF_HDROP`, `PNG` and a registered format per
media type); the session identity travels as the format registered under its
bare media type, with its parameters in the data, because Win32 cannot register
a format per session. The mapping, the byte layouts, the effect mapping and the
drop source's decision are the platform file's pure module,
`drag::windows::crossing`, checked on every host; the COM side is the rest of
`drag::windows`, its COM objects in `drag::windows::com`.

A window's drag source starts the session the runtime asks for, but not inside
the call. The host calls it once its outermost runtime callback ends, which is
still inside the window's own callback drain; a modal loop there would queue the
source window's input and frames until the drag ended. So the start records the
request (one at a time; a second is refused with `DragError::Unsupported`, `Unavailable`) and
posts `START_DRAG` to the owner window, whose procedure hands it to the
transport's own `OwnerMessages` hook, which runs `DoDragDrop` on a fresh turn of
the message loop; a request whose message never runs is told `Cancelled` when
the owner window is destroyed (the hook's `destroyed`) or the platform goes. `DoDragDrop`'s loop follows the mouse
alone, and a pen or touch contact the window takes as `WM_POINTER` is never
promoted to mouse input, so it sets no mouse button in the key state the loop
reads: a start whose contact is a pen or touch (by the window's pointer
registry), or with no mouse button held, is refused with
`DragError::Unsupported` (`NotOnThisPlatform`) and the drag stays in its window (ADR-0211 §11,
`a_drag_the_os_loop_cannot_carry_stays_in_its_window`, the pure rule
`os_loop_buttons`). The drop source drops when the buttons held at the start
are released and cancels on Escape or on another button.

`DoDragDrop`'s loop waits for input and for messages other threads send, and
nothing else: while the pointer rests it dispatches no posted message, timer or
paint, so the main loop's deadline delivery and a `WM_TIMER` would both stall.
A pacing thread sends `MODAL_FRAME` to the owner window instead, which the
loop's wait does dispatch; the transport's hook asks the owner
(`OwnerWindow::deliver_modal_frames`), which delivers a due wake deadline as the
main loop would (once per frame generation), sends `WM_PAINT` to every
invalidated window with `UpdateWindow`, and answers how long to wait by the
owner's modal pace (`owner::modal_pace`, which `DefWindowProc`'s move
and size loops share). The session's
callbacks run with no `RefCell` borrow or lock held across a COM call or a
dispatch: the provider is leased out of the data object while it runs, so a
fetch from inside it is refused rather than reentered, and every COM method
contains a panic as `E_UNEXPECTED`.

A drag that leaves its window on the window's mouse capture asks
`PlatformWindow::window_at` what is under the pointer before it escalates: the
point goes to the screen with `ClientToScreen`, `WindowFromPoint` and
`GetAncestor(GA_ROOT)` honour the desktop's stacking order, and only a window
whose context this thread reaches is the process's own; a point over its frame
rather than its client area, over another application's window, or past the
device coordinate range answers `Outside`. The runtime then reaches a sibling
window of its own without starting `DoDragDrop` (ADR-0211 §2).

`ole_drag_and_drop_on_real_windows` runs on a Windows host: the registration,
OLE's calls on a window's registered target (answers, drop, fetches of every
format, a refusal, a provider fetching from inside itself, a window its drag
callback closes), the deferred start through a real `DoDragDrop`, a second
window's frames while the loop rests, a drop into a FLUI window through the
real loop, woken by posted mouse moves rather than the system pointer, and
`window_at` across two placed real windows.
`ole_drag_drop_contracts` pins the pure half everywhere, and
`the_modal_loop_waits_for_the_next_deadline_within_a_frame` the pace. Not run: a drag to
or from another process (Explorer, a browser), a held mouse button, pen or
touch, and a whole `flui-app` with two runtimes.

### Android's Java side is `FluiView`, loaded from the native library

`owner::android::java_view` loads `FluiView`'s dex (`android/classes.dex`, compiled from
`android/java` and checked against it by `cargo xtask android-dex`) on the Java main thread at
platform start: `InMemoryDexClassLoader` from API 26, a read-only, byte-compared code-cache copy
through `DexClassLoader` below, either parented to the Activity's loader. The view's native
methods are bound with `RegisterNatives` on the class that loader returned, and one full-window
view is laid over the `NativeActivity`'s content (ADR-0247). Every owner-to-view call is posted
(`JavaView::post`, or `post_or` when a refusal must still discharge an obligation, as a drag the
view could not start must still end); every native method only queues a `ViewEvent` and wakes the
owner, which delivers them in order each loop turn, and contains a panic before it reaches the
JVM. The view holds a `Box<Weak<JavaView>>` as its handle; `detach` clears the view's copy and
then frees it, both on the Java main thread, the only thread that calls the view's natives.

The window's `TextStoreHost` is `text::android::AndroidTextInput`. The input method edits the
view's mirror; `text::android::ime::Mirror` turns each report into one edit against the
snapshot the view holds, and the host replays it under an async lock, dropping one made on an
older snapshot than it published last (`replay_current`). A key the native queue delivers while
the view reports a composition is marked composing (`key_composition`): Android pre-dispatches
every key to the input method, so such a key is one it passed on. `drag::android::AndroidTransfer` is the
platform's one `DataTransferSource`: drags over the view mint offers from their
`ClipDescription` (`drag::android::clip::offered`) and drops are answered from the cached
feedback; `drag::android::AndroidDragSource` serves the whole clip at the start (`clip_parts`) and maps
`ACTION_DRAG_ENDED` to `OsDragEnd` (`source_end`). Without the view, or below API 24, the
window reports no drag source and `outgoing_drag()` `Unavailable`.

The view names no text item: its input method calls (`nativeImeState`, `nativeImeAction`,
`nativeImeCursorUpdates`) go to the `java_view::InputMethod` the host's table hands it
(`owner::android::AndroidHost::input_method`), the text feature's `ViewInputMethod`, on the
Java main thread. Each answers an `InputMethodReply`: whether the input method composes now,
which the view applies, and the work the owner runs against the window in order with the
view's other reports (`ViewEvent::RunOnWindow`): an edit replayed into the field, the editor
action committed and delivered as the key it stands for, a cursor update request. What the
view reports of its editor (`EditorState`) is the view's own vocabulary. A pen tip the loop
reads is offered to `InputMethod::pen_down`; stylus handwriting has not run on a device (the
S21 has no S Pen).

`android_ime_adapter` (`src/text/android/ime/tests.rs`) and `android_drag_mapping` (beside
`drag::android::clip`) pin the pure halves in Linux CI, with the Android rows of
`backend_conformance` and `input_capabilities_contracts`. A run on a device
covered the JNI half: a drag started there dropped on its own window, and the default
keyboard composed, committed, passed Escape on while composing and sent its action key.

### The platform names the data roots; the Web keeps values in IndexedDB

`Platform::data_dirs` answers where an application keeps its data, so the host
asks the running backend instead of guessing from the target. The default is
`storage::data_dirs` (the `dirs` crate). Android overrides it, because `dirs`
has no root there: data under `internal_data_path()`, which Auto Backup copies,
and machine-local values under its sibling `no_backup`, which Android never
backs up. On wasm32 `FileStore` keeps the native methods, returning futures:
one record per name in the IndexedDB database `flui-storage`, keyed
`<root>/<name>`, the value a `Uint8Array`. Each call is one transaction on a
connection it opens and closes, so an idle connection never blocks another
tab's upgrade. A based write reads, compares and puts inside one readwrite
transaction; the browser runs readwrite transactions over one store one at a
time across every connection of the origin, so of two tabs writing from one
base exactly one succeeds. `Replace` skips the comparison, as on native. A
quota abort is `Full`, a page without IndexedDB `Unavailable`, a record that is
not a `Uint8Array` `Inaccessible` on read and never a base. Each value's
length is a record in a second object store, `lengths`, put in the same
transaction as the value, so it can never pair with another tab's value. A
read over the limit and a based write whose base has another length
(`StoredVersion::byte_len`) are decided from that record alone, without a
cursor over the value; a value with no record (another script wrote it) is
loaded and measured. The JavaScript values stay
in a task of the page (`spawn_local`) and the answer crosses a oneshot, so the
returned futures are `Send` and the host's IO lane awaits them.

A Web Lock around a compare-and-replace in `localStorage` was rejected:
`localStorage` has no lock across tabs, but neither is it consistent across
them. Each tab reads a cache the browser updates asynchronously, so a tab
granted the lock next can still compare against a stale value. IndexedDB's
transaction scheduling covers ordering and visibility at once.
`tests/wasm32.rs` (`page_store`) covers a based write from its base and from a
stale one, two writes from one base started together (exactly one lands), the
read limit and a base of another length decided without a value cursor (the
stand-in counts them), and a page without IndexedDB. Node has no IndexedDB, so CI runs
them against a stand-in that runs one transaction at a time; with
`WASM_BINDGEN_USE_BROWSER=1` they run against a browser's own. Storage needs no Cargo feature:
`AppConfig::with_storage_dir` is the one switch.

### Android samples its displays through JNI and refreshes them on announced change only

**Rule:** `Platform::display_snapshot` runs on every owner wake, so a backend whose
source costs a native round trip keeps the last snapshot and reads again only after
something that can change it has announced itself. Nothing polls.

**Choice:** `display::android::source` reads every `Display`
(`DisplayManager.getDisplays`): the real size and density (`getRealMetrics`,
scale `densityDpi / 160`), the rate of the current `Display.Mode`
(`getRefreshRate` when the mode is absent), the highest rate among
`getSupportedModes`, and `getDisplayId`, which keys the identity
(`DisplayKey::AndroidId`; `getUniqueId` is hidden). The Activity's display is
`Activity.getDisplay` from API 30 and `WindowManager.getDefaultDisplay` below; it
is what `AndroidWindow::display_id` answers. The snapshot goes stale on
`ConfigChanged`, on `Resume` and on the choreographer's refresh-rate callback
(API 30+: an adaptive 60/120 Hz switch raises no configuration change); each of
those wakes the owner, whose next wake does the read. Below API 30 the rate is read
at the first two events only. The kept snapshot is the shared `DisplaySource`
cache (`invalidate`, `snapshot_with`); an Activity whose display is absent from
a sample is `note_miss`ed, which allows one more sample per snapshot revision.
`getDisplays()` also lists virtual displays, and a display id is never reused
within a boot, so a re-plugged display gets a new identity. The callback runs in
the app thread's own `poll_events`, invalidates the source and wakes the owner; it is taken down
(`AChoreographer_unregisterRefreshRateCallback`) on that thread when the loop
exits, before the data it points at is freed, and a registration dropped on any
other thread leaks that data instead of freeing what a late call could reach.
The loop has no timer of its own (ADR-0249): every producer wakes the looper
(`request_redraw` on the request that sets its flag, `OwnerSignal`, the Java
view's queue, the inset reply, this callback), and `poll_timeout` blocks until
the next wake deadline or 500 ms preference sample while resumed, and while
paused until a deadline still ahead or a wake. A deadline already behind while
paused waits for a frame, so it does not cut the wait to zero.

`owner/android/ndk.rs` reads the API level once, when the platform is
built, and keeps it in the platform (no `static`): `android_get_device_api_level`
looked up by name, then `__system_property_get("ro.build.version.sdk")`. The
function is exported by `libc.so` only from API 29 and is an inline function in the
NDK header below it, so linking it directly would break `minSdk` 23; the property
reader is exported from API 23. NDK functions newer than `minSdk`
(`ANativeWindow_setFrameRate`, `AChoreographer_registerRefreshRateCallback`: API
30) come back as `NdkFn<N>`, a typed handle that exists only when the level and the
symbol both do, and that keeps the `dlopen` handle of its library alive. `N` is a
marker of the sealed `NdkFunction` trait, which pairs the symbol name, the first
level (as its `Gate`) and the pointer type taken from the NDK header, so resolving is
safe code. The Java-side gates (`Context.getDisplay`, `fontWeightAdjustment`, the
scaled scroll factors) take the same level, and an unreadable level is
`AndroidApi::UNKNOWN`, which unlocks nothing.

`PlatformWindow::set_frame_rate_hint` is `ANativeWindow_setFrameRate(window, max, DEFAULT)`
for `High`, with `max` the highest supported-mode rate of the Activity's display, and
`0.0` for `Default`. The window keeps its last hint and sends it again on
`InitWindow`, because the request rides on one `ANativeWindow`, and again when a
sample finds another highest rate for the Activity's display (a `High` hint
skipped while that rate was unknown). The host also sends its demand on
`PrimarySurfaceRestored`; the two overlap on a resume and are idempotent.

**Alternatives considered:**
- `DisplayManager.DisplayListener` — it needs a Java object implementing the
  interface, which a `NativeActivity` app without Java code does not have.
- Polling `Display.getRefreshRate` — the 500 ms preference poll is a bound on
  configuration getters with no notification, and a display read would cost a
  second round trip each time for a value the choreographer announces.
- Linking `android_get_device_api_level` or the API 30 functions directly — the
  binary does not load on an older system.

**Trade-off:** below API 30 an adaptive-rate switch that raises no configuration
change goes unnoticed until the next one or a resume. The JNI path, the
choreographer registration and the frame-rate request run only on a device; the
host tests cover the level lookup (`os_capability_table`), the value
classification (`android_display_table`) and the poll timeout
(`a_paused_loop_blocks_until_it_is_woken`), and CI type-checks the rest for
`aarch64-linux-android`.

### A Win32 window opens hidden, in its mode, and is shown by its first frame

**Rule:** `CreateWindowExW` never gets `WS_VISIBLE`. The window is placed first (`SetWindowPlacement`
with `SW_HIDE` stores the rectangle it returns to), put in its opening mode (fullscreen is entered
while hidden), and only then shown, with the command of that mode (`SW_SHOWMAXIMIZED`,
`SW_SHOWMINIMIZED`, else `SW_SHOW`). Its first sight is therefore the final one: no normal window that
jumps to maximized. `WindowReveal::AtOpen` shows it before `open_window` returns;
`AfterFirstFrame` waits for `reveal_after_first_frame`; `visible: false` waits for `show`.

**Why a deferred window answers `visible`:** the runtime gates frames on visibility, and the frame that
earns the reveal must run. A deferred window therefore reports `visible` (and seeds its visibility edge
filter `true`) while it is natively hidden. A hidden window is never sent `WM_PAINT`, so
`request_redraw` on one posts a frame request to the window instead.

**The first show is one slot** (`WindowContext::first_show`), taken by whichever of the reveal, `show`,
`activate` or a mode that shows the window comes first. A mode set on a window still waiting for its
first frame is staged in the slot, not shown early.

**Frame and limits:** `resizable` is `WS_THICKFRAME | WS_MAXIMIZEBOX`, `decorated` is the caption; a
popup has neither, so a window without a frame reports `resizable: false` and gets its wish back with
the frame. `WM_GETMINMAXINFO` turns the app's logical inner-size limits into outer limits in the
window's current DPI with the frame measured for the style the window has
(`window::windows::limits`); a fullscreen window keeps the system's rectangle.

**Fullscreen from another mode:** a visible maximized or minimized window is restored (`SC_RESTORE`)
before the style is replaced, and the style saved to return to has no `WS_MAXIMIZE` or
`WS_MINIMIZE`. A resize does not end fullscreen, and a minimized fullscreen window comes back
fullscreen. `WINDOWPLACEMENT` rectangles are workspace coordinates; a task bar on the top or left edge
moves the work area, so they are converted with the monitor's work-area offset.

**System events** arrive on the hidden top-level receiver (`owner/windows/broadcast.rs`), which a
message-only window could not be: `WM_QUERYENDSESSION` and `WM_ENDSESSION`, `WM_POWERBROADCAST`,
`WM_WTSSESSION_CHANGE`, and a posted message from a pool thread waiting on the low-memory event
(`lifecycle::windows::host_events`; rules in `lifecycle::windows::messages` and
`lifecycle::windows::memory_watch`). A session-end query the host answers with a block is refused and
registered as a shutdown reason on the first visible window of the thread (the receiver otherwise);
`Platform::release_session_end` withdraws it. The host's answer carries no reason text, so the system is given
a fixed neutral one. A critical session end is never blocked, and a panicking hook answers
`Proceed`. The low-memory event is a level, not an edge: the watch reports once, stays deaf for
`HOLD_INTERVAL`, and only then waits again. `WTSRegisterSessionNotification` asks for this session only.

**Verified:** the pure rules by host tests; the window and the receiver against the real system by
`windows_window_commands` and `windows_host_events` (messages sent to the real windows). A real
log-off, sleep, lock or low-memory state, and the first frame seen by eye, have not run.

**Modes on a window that is not shown:** `Normal` keeps a hidden window hidden and decides how it is
first shown; `Minimized` and `Maximized` show it, as `ShowWindow` does; only a window waiting for its
first frame stages a mode without showing it. Size limits also hold a window without a sizing border
(`WM_WINDOWPOSCHANGING`), since the system asks for them only while sizing one that has it. A change
that takes several steps (entering fullscreen, changing the frame) publishes one snapshot.
`WTSRegisterSessionNotification` is asked once at startup; if it fails (an app started before
Remote Desktop Services at logon), lock and unlock stay unreported for the process.

### Android reads the Activity's lifecycle as three facts and a cycle

**Rule:** an Activity callback is mapped by one pure rule
(`lifecycle/android.rs`, behind the looper's `ActivityCallbacks`) and delivered through `ActivityTarget`, which
`AndroidLoop` implements; nothing in the event loop decides what a callback
means. The window's published state is refreshed before its observer is told.

**Choice:** `onStart`/`onStop` give visibility, `onResume`/`onPause` the execution
state, `onWindowFocusChanged` focus; none implies another. NativeActivity has no
`onTrimMemory`, so `lastTrimLevel` is read through JNI (`owner/android/memory.rs`) on
`LowMemory`, `Pause`, `Stop` and `Resume`, never on a timer, and a level is
remembered when it is read: notices are not replayed, so a level seen before the
host listens is not announced later. The session ends once per start-to-stop
cycle, by whichever of `Stop` and `SaveState` comes first (the order differs
below and above API 28; a Back that finishes the Activity never saves state),
so the notice repeats on every trip to the background and asks for saving. The
session-end hook is called through the call point: out of its slot while it
runs, and put back even when it panics.

**Verified by:** `android_lifecycle_mapping*` and the call point's
`a_panicking_handler_keeps_its_place_and_its_payload` and
`a_handler_installed_during_a_call_replaces_the_one_that_ran` on the host; the
`MainEvent` match and the JNI read run only on a device.
