# flui-native Architecture

The crate-wide invariants of the native host. Module documentation records the
contract of one backend or mechanism; cross-crate decisions live in ADRs.

The contracts this crate's backends implement — `PlatformWindow`,
`PlatformTextInput`, `PlatformHaptics`, the clipboard's text vocabulary, the
display snapshot vocabulary (`DisplaySnapshot`, `DisplayId`, `RefreshPeriod`),
the `data_transfer` vocabulary with its `DragTransport` (ADR-0312) and the
window and input vocabulary — live in `flui-platform` (ADR-0082), and this crate does not re-export them: each item
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
  (`error.rs`), the table-test runner (`table_test.rs`), `executor.rs` (the
  runtime the Linux and Windows hosts marshal their own I/O on, not part of `Platform`) and `task.rs`. Its directories are the
  tables (`hosts/`), the owner thread and each platform's loop (`owner/`), the
  OS versions (`version/`), the storage position's vocabulary (`storage/`), the
  OS objects and protocol machines two kinds of transfer share (`transfer/`:
  the Wayland seats' data devices, which the clipboard and drag both reach,
  and `linux::x11::selection`, the ICCCM whole/`INCR` reader and writer beneath
  the X11 clipboard and XDND), plus `transfer::intake`, the drag backends' one
  policy for admitted type entries, name length, concurrent fetches and fetch
  deadlines. The X11 machine owns no payload, selection
  ownership, feature queue or completion: its adapters keep those and their
  error vocabularies. Reads measure announced and accumulated bytes and reserve
  only a bounded amount from an announcement; writes use bounded chunks, while
  each adapter bounds live writers and gives every read and write a deadline.
  Beside these shared objects there is one directory per feature of the closed
  list: `window`, `input`, `text`,
  `display`, `clipboard`, `drag`, `accessibility`, `haptics`, `fonts`, `http`, `preferences`,
  `lifecycle`. Beside `transfer` there is no directory of code shared across
  features, and there is no
  directory per platform: what two platforms of a feature share is that
  feature's model, and each platform is a file (or, where one OS object or the
  3 000-line limit needs it, a directory) in every feature it fills, named in
  std's words (`windows`, `apple` with `macos`/`ios`, `android`, `web`,
  `linux` with `wayland`/`x11`, `headless`). A public name a host
  offers beyond `Platform` (a window, the headless test controls, the UIKit
  scene events) is named at the crate root, except one OS's window
  extensions, which are named in that OS's module of `os` (`os::windows`,
  `os::macos`), the modules the facade publishes as `flui::native::<os>`.
  `[package.metadata.flui.modules]` gives the roles `cargo xtask module-dag`
  checks: the features; `owner`, `version`, `storage` and `transfer` as
  `base`, which
  names no feature; `hosts`, which nothing names; and the crate-level files
  (`crate-level`: `host`, `platform`, `host_window`, `error`, `executor`,
  `task`, `os`). A top-level module with none of these roles is a finding, so a
  directory the layout does not name cannot come back unchecked
  (`table_test`, built for tests only, needs none). A platform file names its
  model, a base and other features' models; every other edge is a
  named, dated exception whose `reason` says what removes it, and none is
  left. No window file names another
  feature's: a window's text input, accessibility bridge, actuator and
  chrome reach it through the host table; a feature keeps its own state of a
  window, keyed by the window's identity and set up and dropped by its own
  window hooks; a window reads a display, preference or lifecycle fact
  through an owner hook; and the features that answer one AppKit or UIKit
  view join it through the owner's hooks.
- **A window's id is issued, never derived (ADR-0039 §4).** Each host issues
  the `WindowId` of every window it opens from a `flui_platform::WindowIdMint`
  in its own state (the Win32 `WindowSet`, the AppKit `MacOSWindows`, the X11
  `X11Windows`, the Wayland loop's table, the UIKit scene state,
  the Android loop's `ActivityWindowIds`, the Web loop's state), never from an
  HWND, an `NSWindow` address or an X id, which the OS reuses. The headless
  host's mint is one for the process (`HEADLESS_WINDOW_IDS`), since a test
  builds several headless platforms whose windows meet in one application
  registry. A host keyed by a native handle looks the id up in its
  table of open windows, whose entry goes as the window is destroyed; a
  handle that misses names no window. A mint that issued its last id refuses
  the open with `OpenWindowError::IdsExhausted`. The window conformance check
  `a_window_opened_after_a_close_has_an_id_of_its_own` holds a host to it.
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
  `owner::android::ActivityRef`, `owner::web::CanvasRef`,
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
  whose loop it is. The application path, the launch values (an
  Android intent's extras, the environment elsewhere; ADR-0288 §7), URL and path opening,
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
  host builds are crate-private: the platform hands out `Rc<dyn HostWindow>`,
  a raw window or display handle leaves through the window's `SurfaceTarget`
  and `raw-window-handle`, and what
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
  one table read both ways. The contract's material
  (`WindowCommand::SetMaterial`) is a window command: the Win32 window set
  reads the build once, and the `SystemBackdrop` gate (build 22621,
  `window::windows::material`) is the one gate of `DWMWA_SYSTEMBACKDROP_TYPE`.
  Below it the window's `material` capability is unavailable on this device,
  so admission refuses with `NotOnThisDevice`, and `Win32Window::set_backdrop`
  answers the same; every write of the attribute takes the gate's proof. A
  refusal of the desktop window manager is `ApplyError::Platform`. A Win32
  window opens on Mica through the same gate. AppKit draws `System` as
  `NSVisualEffectView`'s window-background material behind FLUI's view, for
  windows on the main lane. Where the OS draws a material and the host does
  not yet (UIKit, Android from API 31 with cross-window blur on, an AppKit
  window off the main lane), the capability is unavailable with
  `NotYetOnThisHost`; below the version gate, or with the device's blur off,
  `NotOnThisDevice`. The rules are pure (`window::capabilities`), and their
  table tests run on every host.
  The AppKit backdrop (`AppKitWindow::set_backdrop`)
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
  `tests/main.rs`); `tests/os_extensions.rs`, its own harness so AppKit gets the main
  thread, pins that a headless window has no Win32 view and a Win32 window has one, that a
  Win32 window opens with
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
  platform calls a handler, `owner::app_delivery` what a loop delivers to the
  application's handler, `owner::window_delivery` what a window delivers to
  its own, `owner::handlers` what a loop asks of the windows it holds,
  `owner::notices` the trait a loop pushes system notices through
  (`NoticeChannel`, which the lifecycle feature implements over the loop's
  delivery; a wake marks the display and window caches that registered
  `StaleOnWake` first) and the release of a held session end a loop answers
  (`LifecycleHooks`),
  `owner::sources` what a loop keeps of a feature's source
  (`SystemPreferenceSource`), and `owner::watch` an observation a feature
  started for a loop. `owner` is a `base` of
  `[package.metadata.flui.modules]`, so `cargo xtask module-dag` refuses an
  import from it into a feature's directory. Its public items are named at the
  crate root only. Its tests name no feature or host either: they run over
  stand-ins of the owner's own (`owner::windows::testing::QuietHost<C>`, a
  Win32 host with the window hooks `C` gives, none by default;
  `owner::notices::testing::NoNotices`), and a test that needs a host's real windows is that
  feature's.
- **Each platform's loop is a module of `owner/` (ADR-0151 §10).**
  `owner::windows` is the Win32 message loop and its wake deadlines, the
  owner's message-only window (`control`), the thread's COM apartment, FLUI's
  window class with its raw window procedure, and the hidden top-level window
  that hears the system's broadcasts (`broadcast`); `owner::apple` is
  `NSApplication`'s loop with the owner lane and the wake pump (`macos`) and
  `UIApplicationMain` with its delegates and scenes (`ios`); `owner::android`
  the looper over `android-activity`, whose frames the choreographer paces
  (`frame_clock`, ADR-0269), the JNI `FluiView` and the NDK lookup;
  `owner::web` the `requestAnimationFrame` loop; `owner::linux::x11` the X11
  host's loop over its libxcb connection and an eventfd doorbell;
  `owner::headless` the test
  double's owner turns. The
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
  `updateLayer` asks the window for. On iOS the window's view hands its
  touches to the input's `ViewTouches` (`IosHost::view_touches`, one per
  view), and its display link's cadence and its screen to the loop's
  `HostDisplays` (`observe_link`, `move_window`, `forget_link`). A
  loop keeps a feature's state the same way: each `owner` module declares one
  host trait (`WindowsHost`, `MacosHost`, `IosHost`, `AndroidHost`,
  `WebHost`, `X11Host`, `HeadlessHost`) whose methods the table implements
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
    the receiver detaches it before destroying its window. Tests: `broadcast_subscribers_hear_a_message_in_order`,
    `the_displays_and_the_preferences_hear_every_change_in_either_order`,
    `display_broadcasts_mark_the_monitors_stale_and_wake_until_detached`.
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
    `a_due_deadline_gets_one_modal_frame_per_generation`,
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
  `WaylandHost::drag`
  (`WaylandDrag`: the drops on the windows, which the seats' data devices hear,
  and the sessions the windows start),
  `AndroidHost::clipboard`,
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
  its caret is: headless) or `Store` (the platform reads or
  mirrors the field's store: AppKit, UIKit, TSF, Android, web, Wayland's
  `zwp_text_input_v3`), and `Absent<position::Text>` answers none; each
  window's accessor goes through it, as do its text sizing
  (`capture_sizing`, `invalidate_sizing`, `fence_sizing`) and its going
  (`detach`). Where the text feature keeps a
  window's state itself the handle is a key to it: Android's input method
  keeps each window's field by `owner::android::ActivityWindowId`, and the
  Web's text input lives in the canvas window's owner-thread slot. On
  Wayland the seats' input binds each seat's `zwp_text_input_v3` through the
  owner's `SeatText` seam, which `text::linux::wayland` implements and the
  table hands the input, and records its events in the seats' queue in order
  with the keys around them, a seat or manager that goes queued behind what
  its text input reported, so that reaches the field first; a window's text
  reaches the input as an opaque
  `SeatWindowText`, so neither the owner nor the input names the text
  feature's platform file. On X11 the input method is a D-Bus daemon,
  IBus or Fcitx5 (the portal's names in a flatpak), asked in the order
  `XMODIFIERS`, `GTK_IM_MODULE` and `QT_IM_MODULE` name: every key the loop
  reads goes through the owner's `LoopText` seam, which `text::linux::x11`
  implements and the table hands the input and the loop. A key of a window
  whose field composes is asked of the daemon and held, keys behind it wait
  in order, and each goes as typed unless the daemon took it; a key
  unanswered within `ANSWER_TIMEOUT`, the keys of a window losing the focus
  or closing, and every key once the daemon is gone go in their original
  order (`input_context::KeyHold`, answers matched by ticket). The daemon is
  spoken to by a task of the host's executor (`x11::bus`) holding one
  context per window, whose replies and signals reach the loop in the
  daemon's order through an inbox the doorbell wakes; the field is served
  by the same `text_input` client as on Wayland, over a wire that turns its
  requests into the daemon's calls and the daemon's signals into its
  batches. A window's text reaches the loop as an opaque `X11WindowText`.
  Beside it,
  one file per platform with its pure half a module inside it:
  `windows/mod.rs` (the pure `geometry` and `acp`, TSF's `ITextStoreACP`
  answers over the field's store, and the `Tsf` class hook) with `tsf/`,
  `apple/{macos, ios}.rs`, `android.rs` (the pure `ime`, `input`, `sizing`),
  `web.rs` (the pure `ime`, `input`), `linux/{text_input, input_context, wayland, x11}.rs` (the
  pure `text_input` client of `zwp_text_input_v3` over a wire trait, the pure
  `input_context` key hold and D-Bus input-method values, the Wayland
  objects and the X11 daemon's wire and bus task) and `headless.rs`. `cargo
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
  `text::web::ime::tests`, `text::windows::geometry::tests`,
  `text::linux::text_input::tests`, `text::linux::input_context::tests`);
  `text::linux::x11::bus_tests` serves a window to a stand-in IBus on a
  private `dbus-daemon`; `tests/wayland_text_input.rs` drives
  the Wayland client from a real input method on sway.
  AppKit and UIKit use `text::apple::client` over that same store; neither
  keeps a document of its own. UIKit's before and after notices bracket the
  store's publication, with synchronous reads seeing the old and new
  documents respectively (`application_notices_bracket_the_document_the_keyboard_reads`).
  Focus requests wait until publication finishes; replacement or retirement
  of the context invalidates its remaining notices, without retargeting a
  later notice to the new context (`a_will_notice_keeps_its_recipient_until_publication_or_retirement`).
  Callback failures preserve the first panic while draining accepted work;
  retirement withdraws the store and context before callbacks and still
  delivers an accepted completion in place
  (`retirement_delivers_a_completion_accepted_inside_a_notice`).
  The macOS context's `SecureHold` owns its release operation, a function
  pointer in production. Its destructor makes one final attempt after a
  refused release, without claiming the OS must accept it
  (`dropping_a_hold_retries_one_refused_release`,
  `rebinding_gives_the_previous_context_its_secure_hold_back`).
  UIKit's character-range queries keep UTF-16 surrogate pairs whole, including
  in protected fields: offset errors precede `Protected`, so bounded probes
  need no plaintext (`uikit_ranges_keep_supplementary_scalars_whole`). A point
  hit-test and its scalar range share one read session. Character ranges use
  the containing box's start; an outside point has no range, while the
  separate closest-position query keeps nearest-caret behavior
  (`point_derived_ranges_stay_with_the_queried_field`); read failure drains
  accepted focus requests before resuming the first panic
  (`a_failing_point_read_delivers_reentrant_focus_and_keeps_its_first_failure`).
  Status, edit and composition-completion calls release their queue guard
  and drain accepted host operations before resuming a store failure
  (`failing_store_calls_deliver_focus_before_the_next_edit`). Queued completion
  reads and fallback commits keep newer focus requests behind earlier ones
  (`a_queued_completion_keeps_newer_focus_after_older_requests`). An accepted
  deferred completion still attempts its fallback when the platform's read
  panics, before resuming that first failure
  (`a_queued_completion_finishes_after_its_read_panics_without_moving_focus`).
- **`fonts` answers which system faces fall back, for a locale (ADR-0271
  §3).** `fonts::system_fallback` is the one entry; the composition root hands
  it to the font feed, which calls it on its own thread, never the owner's
  and never in a frame. A platform that cannot answer gives
  `SystemFallback::none`, never a panic. `fonts::linux` opens fontconfig with
  `dlopen` for the query and closes it after; `fonts::android` reads
  `fonts.xml` below API 29 (its parse runs on every host over AOSP's recorded
  files) and asks `AFontMatcher` from API 29, looked up like the other NDK
  functions (`owner::android::ndk::FontMatching`, all seven or none).
  `fonts::windows` hands DirectWrite the sample through an
  `IDWriteTextAnalysisSource` of its own, whose methods cross `os_entry`;
  `fonts::apple` looks `CTFontCreateForStringWithLanguage` up at run time,
  since the macOS floor predates it. What two platforms share and can be
  decided without the OS (naming a face by file or family, reading a sample
  around a position) is the model's, tested on every host.
- **Clipboard, drag and drop, accessibility and haptics are feature
  directories (ADR-0151 §10).** Each holds its model beside one file per
  platform, and `[package.metadata.flui.modules]` lists the four as
  `features`: `cargo xtask module-dag` holds a platform file to its own model,
  `owner/`, `version/` and other features' models. The models are
  `clipboard::lock` (the process-wide lock every Win32 clipboard session
  runs under), `drag::offers` (the offer table every data-transfer source mints its
  ids in, `TransferUri`,
  a URI list as the hosts that read paths or URIs write it, and what a media
  type offers and its bytes carry, shared
  by Android and the Web), `accessibility::bridge` and `haptics::degradation`.
  A platform is one file. One whose decisions need no OS keeps them in a
  module of the file under `forbid(unsafe_code)`, named for what it decides
  (`drag::windows::crossing`, what crosses OLE; `drag::android::clip`, what a
  clip offers, reads and answers; `drag::web::mapping`, how `DataTransfer`
  types and effects map); the file is compiled under `cfg(any(<os>, test))`
  and its OS side under `cfg(<os>)`, so every host tests that module. A child
  file is a module's tests, the mechanism of one OS object
  (`drag::windows::com`, OLE's COM objects), or a split past 3 000 lines. A
  position a platform does not have is `Absent`: drag on macOS and
  iOS, accessibility on the Web,
  Android's clipboard is `ClipboardManager`, reached
  through JNI from the owner thread.

  Accessibility has no Cargo feature (ADR-0151 §15, as amended).
  `accesskit_windows` and `accesskit_macos` are unconditional dependencies of
  their OS, and every Win32 and AppKit window builds its adapter. On Linux
  `accesskit_unix` and `zbus` are unconditional dependencies, as the native
  Wayland host brought them, and `accessibility::{bridge, linux}` compile under
  `target_os = "linux"`. `UnixAccessibility` is the accessibility position of
  every Linux table for any window reference (it needs nothing of the window),
  so the Wayland and X11 windows announce themselves on the AT-SPI bus
  alike. Every Android Activity window builds `accesskit_android`'s low-level
  adapter, which lives on the Java main thread: `FluiView`'s node provider
  answers from it there, and a publish is posted there and applied in order
  (`accessibility::android`). The view reaches it through a `BridgeSlot`
  (`owner::android::bridge_slot`), which a closing window empties only of
  its own bridge.

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
  rule the estimate also judges its frames' agreement by). Each platform
  file's pure half also maps what its OS reports of a display's colour to the
  contract's `DisplayColor` (DXGI colour spaces and luminances, Apple's gamut
  and potential EDR headroom, Android's wide-gamut, HDR and highest HDR/SDR
  ratio, a Wayland image description, the browser's colour queries), and a
  report that is not a finite multiple of white publishes no headroom. Where
  the OS reports nothing the colour is an `Unsupported` whose reason says why:
  X11 `NotYetOnThisHost`, a compositor without `wp_color_management_v1` or a
  device below the API that reports it `NotOnThisDevice`, the browser's
  headroom `NotOnThisPlatform`. The Wayland model numbers each image
  description it asks for and takes over only the newest one's `done`.
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
  `linux/` (Wayland and X11 outputs) and `headless.rs`. The
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
  `IosHost`, `AndroidHost`, `WebHost`, `WaylandHost`, `X11Host`,
  `HeadlessHost`); the windows a loop opens reach the displays through the same hook, whose
  implementations hold the bodies. Every observer a display source starts for
  the loop (AppKit's and UIKit's screen notifications, Android's refresh-rate
  callback, the browser's listeners, the Linux connection thread)
  comes back as an `owner::watch::Watch`, which is `#[must_use]` and which the
  loop keeps until it stops, so an unwind cannot skip a stop; the macOS loop
  keeps its watch in a `NativeOwner`, which releases it on the main thread.
  The Android loop's poll rule is the loop's
  (`owner::android::poll`). The iOS source reads the OS version as it is built
  (`version::gate::apple::AppleVersion::read`).
- **The preferences and the lifecycle are feature directories,
  `preferences/` and `lifecycle/` (ADR-0151 §10).**
  - `preferences`: the model is `read` (the bounded read admission the Android
    and AppKit sources take) and `locales` (preferred languages from the forms
    the systems spell them in); the platform files are `windows` (WinRT and
    Win32, told of changes by the broadcast receiver), `apple::{macos, ios}`,
    `android` (`configuration`, and the rules `contrast` and `text_weight`),
    `web` (`browser`, and the rule `media`), `linux` (the Settings portal,
    whose watch names no host type, so the Wayland and X11 hosts both start it) and
    `headless`.
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
    activity}`, `WebHost::{preferences, page}`, `WaylandHost` and `X11Host::{notices,
    preferences, host_notices}`, `HeadlessHost::notices`): the preference
    source as `owner::sources::SystemPreferenceSource` or a loop's own trait
    over it (`OwnerPreferences`, `UIKitPreferences`, `ActivityPreferences`,
    `owner::linux::LinuxPreferences` for the
    two native Linux loops), the notice channel as
    `owner::notices::NoticeChannel` (`lifecycle::notice::SystemNotices`
    implements it), the Windows lifecycle as `OwnerLifecycle`, and what the
    Activity's callbacks mean as `ActivityCallbacks`. A source the loop starts
    only once it runs is an object the table makes with a method that starts
    it (`NoticeSources::listen` and `PageLifecycle::listen`, which return a
    `Watch`; the Wayland and X11 tables' `preferences` and
    `host_notices`, which take the executor and the signal). The native Linux
    loops share one shape for the session (`owner::linux`): the portal as
    `LinuxPreferences` behind a `PortalSlot` their windows read the colour
    scheme from, the logind and pressure sources as `LinuxNotices`, which
    deliver to either loop as a `SessionLoop`, and `xdg_open` for the
    services.
  - Each loop is its table's preference position, and its lifecycle position
    on every host but headless, whose position is its test handle
    (`HeadlessPreferences`, `HeadlessHostEvents`). The loops that hear the
    lifecycle in their own callbacks (AppKit's and UIKit's delegates, the
    Activity's, the native Linux loops' turns) implement
    `owner::notices::LifecycleHooks`, and one generic impl in
    `lifecycle/mod.rs` makes them the position; the Windows and web loops
    answer their own.
  - What the Activity's callbacks mean is the lifecycle feature's
    (`lifecycle::android`, `onTrimMemory` from `FluiHost`'s queue included), behind the looper's
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
  `ModeFlags`: minimized before fullscreen before maximized;
  AppKit's flags, the X11 window manager's properties and the Win32
  window's tracked mode each read their report into the flags and nothing
  else), the desktop window configuration
  (`window::config`, crate-private: its windows open with F11 as the
  fullscreen key) and the acknowledgement
  a host waits for before it publishes a window it attached
  (`window::installation`, public at the crate root as `WindowInstallation`,
  `PendingWindowInstallation`, `InstallationCancelled`).
  `window::state::mapping` reads one table through the X11
  window manager's properties, AppKit's raw flags (which keep full screen
  above a miniaturized flag, a row of their own) and the Win32 tracked mode.
  Whether a window is seen is three facts of its snapshot, `visible`, its
  mode and `occluded`, which `WindowState::is_seen` reads together; each
  window reads them from its own OS, the occlusion through `ReportedFacts`
  so no host leaves it out. Shown is the show state: Win32's `WS_VISIBLE`,
  an X11 window mapped, waiting for its first frame or iconic, AppKit's
  `isVisible` or `isMiniaturized`, and on Wayland, Android, iOS and the Web,
  where the application cannot hide its window, open. Occluded is what
  each window system tells of a shown window kept off the screen: a DWM
  cloak or a locked session, an X11 `VisibilityNotify` of `FullyObscured` or
  an unviewable map state, AppKit's `occlusionState` without its visible
  bit, a Wayland toplevel unconfigured, on no output or
  `suspended`, an Activity stopped, a scene in the background, a hidden
  document. The pure `held_to` holds a size to the window's constraints
  for the hosts whose window system applies a size as given (AppKit's
  `setContentSize:`, the headless model). Each platform's
  window is one file beside it (`android`, `web`, `headless`, and
  `linux/{wayland, x11}`, the live Wayland and X11 layers), or a directory where
  its halves or one OS object need one: `windows/` (the pure `limits` and
  `mode`; the window, the set and position, the
  procedure and the backdrop under `cfg(windows)`) and `apple/` (the pure
  `geometry` with its `frame`, `mode`, `surface` (the occlusion bit) and
  `screen_edges`, `macos` with its `delegate` and its public `ext`, and
  `ios`). The platform's system chrome (the table's `Chrome` position) is in
  the same file. Production headless windows use no-op text and chrome sinks,
  retain only accessibility activation state and keep current window-system
  facts without command histories. `HeadlessPlatform::with_test_doubles`,
  available with `testing`, explicitly selects recording facets and histories
  for windows opened afterwards; the feature alone selects none.
  - **The host seam.** The window position names what a platform's windows
    share, `host::Window::Shared`: the Win32 handle map and configuration
    (`WindowSet`), the macOS configuration, the UIKit window factory, the headless window model,
    and on every host the table's window-level positions
    (`host::WindowPositions`, all Android and the Web share). The host's
    constructor builds it with
    `HostPlatform::from_parts(|positions| shared, |shared| Loop::new(...))`
    (or `try_from_parts` for a loop that can fail): the platform keeps the
    one instance and the loop gets that instance as the hook it declares, so
    the two cannot see different windows; the platform passes it to every
    call of the position.
  - **The loops' hooks.** Each loop declares what it asks of its windows, and
    the window file implements it: `owner::handlers::OpenWindows` (whether
    any window is open), shared by the hooks that extend it, and a window's
    delivery to its handler reached through `HostWindow`
    (`owner::window_delivery::WindowDelivery`); `owner::windows::ClassWindows` (the
    deadline redraws and modal paints); `owner::apple::macos::WindowRegistry`
    (the open AppKit windows, for the wake pump's frames and the stop);
    `owner::apple::ios::{SceneWindow, SceneWindows, SceneConsumer,
    SceneInstallation}` (a scene's window, made and driven by the scene
    delegate; the application's handler of `IOSSceneEvent`s, which carry the
    window's installation, adapted by the window feature, whose accepted
    connection returns what the loop watches of that installation, so a
    connection cannot leave the loop with nothing to publish or roll back);
    `owner::android::ActivityWindow` (the Activity's one window);
    `owner::linux::wayland::LoopWindow` and `owner::linux::x11::LoopWindows`
    (what the Wayland and X11 loops ask of the windows they hold); and
    `owner::headless::TrackedWindows`.
  - The Win32 window-thread affinity rules (`owner::hwnd_affinity`) are
    owner machinery: the owner control and the loop apply them as the
    window does; they sit beside the Win32 loop, under `cfg(any(windows,
    test))`, so every host tests them.
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
  touchpad viewport's readings, which report to the crate-level `journal`; it names
  no platform file and forbids `unsafe`), `availability` (each backend's
  input capabilities table), `keys` (the Win32 virtual keys and the mouse
  buttons' codes, `MouseButtonCodes`, the `adapter` from `ui-events` key
  results that Android reads, with keyboard-types' locations
  matched variant by variant, and the one table of key locations, whose
  named rows give the number the DOM reports for each location and the
  codes that imply it, which Win32 and AppKit read it from; a character no
  key typed is `CHARACTER_WITHOUT_A_KEY`), `regions` (a window's input
  regions projected to device pixels, rounded outward, and handed the OS only
  when the pixels, the scale or the display changed; every window that takes
  them, the headless one too, projects through it), `scroll` (the one wheel step: a
  platform's amounts in the unit it picks, each axis turned to FLUI's
  direction by whether it moves the offset or the content, an idle axis
  `+0.0` whichever zero the platform reported, and a non-finite amount
  refused and dropped by every backend before its phase can commit; a finite
  zero reported by the OS remains an event) and `event_timestamp_ns` (public
  at the crate root).
  `input::keys::mapping` reads one table of keys through the Win32 scancode
  table, the AppKit keycode table, the DOM's number and the keyboard-types
  adapter; `input::scroll::mapping` reads one table of turns through the
  Win32 registry, the Web's, Android's and AppKit's steps. Beside it, one file per platform, each pure
  half forbidding `unsafe`: `windows/` (`mod.rs`: `InputWindows`, the
  class's input hook, which keeps each window's `InputWindow` keyed by its
  identity, and `raw`, the messages decoded; `direct_manipulation.rs`, the
  touchpad's DirectManipulation viewport, one OS object, with its pure
  `manipulation` decoder), `apple.rs` (the pure `keys` and `wheel`; `macos`,
  `ViewInputs`, the input of one content view, and its events; `ios`,
  `TouchInputState`, the touches of one view), `android.rs`
  (the pure `keys`, `motion` and `scroll`; `AndroidInput`), `web.rs` (the
  pure `keys`, `wheel` and `pinch`; `events`, `WebInput`),
  `linux/` (the Wayland and X11 hosts' input) and `headless.rs` (`AttachedDevices`, the devices a test attaches). A loop
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
  `HandlerSlot` (the application's or a window's handler in an owner slot, a
  field of a host's state mutex or `RefCell` for the callbacks a host still
  keeps) under the slot's own lock or borrow, releases it, runs the handler in
  a panic boundary and puts it back unless one was installed meanwhile: the
  newer one stays and the one that ran is retired once the lock is released. A
  slot can refuse the handler back (a window that cleared, a loop that stopped
  accepting work). The application's and a window's handler are `Rc`s called
  as a clone, so they stay installed while a method runs and may be called
  again from inside it. `install` replaces a handler and `retire` drops any
  other user-owned value the same way, so a capture's destructor may call the
  platform. A handler's panic is the call's `Failure`: the caller resumes it
  where a Rust frame waits (headless, a window's FIFO) or contains it
  where the operating system called (Win32's exit policy and session end,
  AppKit, UIKit); a handler displaced during a failed call is dropped,
  contained, its panic discarded (ADR-0281). `AppDelivery` and
  `WindowDelivery` call only through it. Tests: `owner::call_point::tests`;
  the public-API tables in `tests/handler_reentry.rs` (headless and Win32, each case on a
  worker thread so that a regression fails as a deadlock
  instead of hanging) and
  `web_session_end_handler_reenters_the_platform`. Android and the Apple hosts
  run the same call point and are type-checked only.
- **The application's handler is passed to `run` (ADR-0254 §4).**
  `AppHandler` (in `platform.rs`) has one method per obligation and no
  default: `ready(OwnerPlatform)`, `quit_requested`, `reopen`, `session_end`,
  `host_notice`, `exit_policy` and `wake_deadline`, each borrowing the owner
  capability. Every loop keeps one `owner::app_delivery::AppDelivery`, which
  its sources report to from construction on: `run` binds the handler on the
  owner thread in an owner slot, with the platform and the owner hooks held
  weakly (the platform owns the loop that owns the delivery), and each call
  mints the capability it borrows. A report (quit, reopen, notice) goes
  through one FIFO: accepted before the handler is bound, while `ready` or
  another method runs, or on another thread, it waits and is delivered in
  order on the owner thread (an owner turn of the loop's signal for a report
  from another thread); the quit is heard once per run. A question (session
  end, exit policy, wake deadline) is asked at once, re-entrantly if need be,
  and a report it raises is delivered right after its answer; with no handler
  bound the host answers it itself (proceed, its own exit default, no
  deadline), and so it does for an exit the handler leaves to it
  (`ExitAnswer::HostDecides`). A panicking method is contained, the tail behind
  it still delivered, the first panic returned. The loop releases the handler
  when its run ends; a release made while a delivery runs (the web page and the
  Android Activity end from inside a method) waits for that delivery, so the
  quit it queued is heard first. One released while an unwind is under way is
  retained, never left in the owner thread's table to be dropped when the
  thread exits. The handlers tests run a platform with are
  `testing::app` (the `testing` feature). The
  lifecycle position keeps only the release of a held session end and the
  activation; the keyboard layout callback stays a registration until nothing
  above the host reads it. Tests: `owner::app_delivery::tests::app_delivery_contracts`,
  `tests/app_handler.rs::app_handler_obligation_table` and the application
  rows of `tests/handler_reentry.rs`.
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
  and its window, the delegate), contains the log, discards the payload (ADR-0281 §2), then
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
  out of its scope because wasm32 aborts on panic,
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
- **A window leaves a backend as an `Rc<dyn HostWindow>`, and stays on its
  owner thread (ADR-0254 §5).** `HostWindow: PlatformWindow` adds the one
  AccessKit-speaking accessor, `accessibility()`. `Platform::open_window` and
  `WindowOpen::Ready` return it; a deferred open (`PendingWindow`) resolves to
  an `OpenedWindow`, whose identity any thread may hold and whose window only
  the owner thread reaches. Every backend window implements it next to its
  `PlatformWindow` impl, and its accessibility bridge is fixed when the window
  is built, so a runner may read it once. Neither `dyn PlatformWindow` nor
  `dyn HostWindow` is `Send` or `Sync`; what other threads hold of a window
  is `Send + Sync` and does not keep the window open:
  - its `SurfaceTarget` (`surface_target()`), from which the renderer takes
    the raw window and display handles. It answers `Unavailable` once the
    window's `Retirement` started (`WindowDelivery::clear`, where every host
    retires its native window) and, per host, when Win32's identity probe no
    longer finds the window behind its handle, on any thread but the main
    one for AppKit and UIKit, on any thread but the looper for Android, or
    when the Android activity has no native window. A target keeps alive
    what its handle names, so a handle it gave out never dangles:
    the Apple targets a retained reference
    to the content view (`MainRetainedView`, taken and released on the main
    thread, a drop elsewhere posts the release there), Android's a counted
    `NativeWindow`, the Wayland target an `Arc` of the window's `wl_surface`
    (destroyed when the last holder drops) and a clone of the connection. Its
    `pre_present_notify` arms the Wayland frame callback on the
    native Wayland host, and returns at once elsewhere. It is also where a
    presentation report starts: the native Wayland target asks
    `wp_presentation` for a feedback per commit, whose `presented` event the
    window rebases from the compositor's clock (`clock_id`) onto the event
    clock, and the Win32 target rebases the DXGI frame statistics the renderer
    read (`SwapChainStatistics::Dxgi`) from the performance counter, measuring
    the refresh period between two reports (`frame_timing::PresentTracker`:
    a read naming the present or refresh already reported is no report, and
    `SwapChainStatistics::DxgiDisjoint` forgets the last refresh). The
    AppKit window reports from its view's backing layer, not the target:
    `owner::apple::metal_layer::FluiMetalLayer` (from `makeBackingLayer`)
    is the `CAMetalLayer` wgpu takes as the view's root layer and gets every
    drawable from, and its `nextDrawable` adds a presented handler to each,
    which Metal calls on a thread of its own with the drawable's
    `presentedTime`, rebased from `CACurrentMediaTime` onto the event clock,
    with the refresh of the window's display.
    Each reports into the window's
    `handlers::PresentationReports`, which the `WindowDelivery` keeps and
    `dispatch_frame` empties into the next frame request
    (`FrameRequest::presented`): the newest report wins, a report no later
    than one kept or carried is dropped, and one still ahead of the request's
    delivery waits. The headless window reports what a test injects
    (`HeadlessWindow::simulate_presented`). The web and X11 windows answer
    `NotOnThisPlatform` (a browser names the frame's start, never its showing;
    an X11 present completes on the GPU driver's connection),
    and UIKit and the Activity
    `NotYetOnThisHost` (`window::capabilities::PRESENTATION`: a UIKit view
    backed by its own `CAMetalLayer` showed nothing on the iOS Simulator, and
    Android's swap chain that knows is wgpu's). Only the web target and
    `MainRetainedView` carry an `unsafe impl Send`/`Sync`, whose SAFETY
    names the invariant;
  - its `RedrawRequester` (`redraw_requester()`), how a waker, a signal
    written on a worker or an accessibility adapter's thread asks for a
    frame: Win32 and Android ask the window system directly, AppKit
    hops to the owner lane, the headless host waits for an owner turn, UIKit
    and the web serve every frame from their display links.
  A host table of open windows (`WindowSet`, the AppKit
  registry) holds them in `owner::slot::{OwnerCell, OwnerMap}`, which only the
  owner thread reaches; `OwnerWeak` is the `Send` weak reference a queued
  close or state publisher carries. A cell or map belongs to the thread that
  made it. A slot dropped on another thread leaves its value to the owner,
  which drops it at its next use of any slot; values still held when the
  owner thread exits are leaked, never dropped during thread-local teardown.
- **A window publishes a state snapshot and takes commands (ADR-0237).** There
  is one window trait. `PlatformWindow::state` returns an immutable
  `WindowState` with a revision, and `apply` takes an `AdmittedCommand`
  (ADR-0254 §2); the snapshot's `capabilities` say which commands the window
  honours, each as an `Availability`: `WindowCommand::admit` refuses with
  `Unsupported` exactly when the matching capability is `Unavailable`,
  `Emulated` marks what the host builds itself (the Win32 borderless
  fullscreen, the Web's close, which retires the canvas), and the capabilities
  of each backend are what its `apply` carries out today. Every host's `apply`
  opens the token against the capabilities of its last published snapshot
  (`WindowStateCell::published_capabilities`), never a constant of its own, so
  a capability change the snapshot published (the headless `simulate_capabilities`) answers
  `ApplyError::Superseded`. `WindowStateCell::new` stamps the window's first
  snapshot with a `CapabilitiesOwner` from one process-wide counter
  (`window::state::NEXT_CAPABILITIES_OWNER`, checked, never reissued), so a
  token admitted for another window, or against capabilities built by hand,
  never opens on this one. The conformance check
  `a_command_admitted_before_a_revision_is_superseded` holds a host to it
  where its fixture can revise capabilities. A command no host arm carries
  out answers `ApplyError::Platform` (`window::state::not_carried_out`), and
  Win32 numbers beyond its device range are `PlatformFailure::OutOfRange`:
  admission held them finite and positive, the range is the window system's.
  Showing, activating and closing are commands like the others
  (`Show`, `Activate`, `Close`); there is no method twin. The snapshot
  describes what the OS reported, not what a command asked for, and holds
  every fact of the window (ADR-0254 §1): the drawable `WindowMetrics` (a
  report that is not a drawable area keeps the last), appearance, display,
  safe area, execution state, focus, activation, visibility, hover and the
  capabilities.
- **The snapshot is the one source of a window fact.** No getter and no
  per-fact callback exists beside it: a consumer reads `state()` and hears
  the handler's `state_changed`, and one that needs to know which fact changed compares
  the snapshot with the last one it saw (`flui-app`'s
  `RuntimeEvent::of_state_change`). Whether a window is seen is
  `WindowState::is_seen`: shown, not minimized and not occluded. Each host sets a
  `StatePublisher` on the window's `WindowDelivery` when the window opens
  (`publish_through`, over a weak reference to the window). The publisher
  reads the window again and answers `Publication` (changed, unchanged,
  held) without announcing anything; `publish_state` runs it and
  raises the handler's `state_changed` when the revision grew, so each fact the OS
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
  last `WindowDelivery::report_hover` said, whether Win32 or web input,
  AppKit's tracking area reported it. An
  observation committed at its delivery point (the Web's focus and
  visibility) is published right after its commit. The facts beyond
  geometry (metrics, appearance, safe area, execution, hover, activation,
  capabilities) are one value, `window::state::ReportedFacts`, that every
  host builds. Activation is Win32's `WM_ACTIVATE` (a window whose owned
  popup has the keyboard is active and not focused). A Win32 window asks for
  the foreground in one place (`request_foreground`), which Windows grants
  only to the process the user works with or one it handed the right
  (`AllowSetForegroundWindow`); `Activate` refused it flashes the taskbar
  button (`FLASHW_TIMERNOFG`) and never forces the foreground, and opening a
  link, file or folder hands the right only to the handler process the shell
  started, by its id (`ShellExecuteExW`, `foreground_recipient`): a handler
  already running gets nothing and comes forward under the OS's own rules,
  as no process is ever handed the right to take the foreground from any
  other (`ASFW_ANY`). The other hosts have
  no notion apart from focus (AppKit's key window, the
  canvas focus, the scene and the Activity), so their activation is their
  focus. `window_facts_arrive_only_in_the_snapshot` pins one revision and
  one notification per fact; a `compile_fail` doctest on `PlatformWindow`
  pins that the getters are gone.
  Every backend keeps its last snapshot in a `window::state::WindowStateCell`
  (built like `DisplaySource`): an OS event marks the cell stale, the next read
  samples the OS with no lock held, and a sample equal to the published one
  keeps the revision. the handler's `state_changed` is raised after the publish, outside
  the cell, only when the revision grew. Win32 and AppKit refresh from their
  window messages and delegate callbacks, the
  Web from the layout, focus and visibility observers and on each owner
  wake, which is how a screen change reaches it; the headless window
  keeps a model that stands in for the OS (`HeadlessWindowPolicy` makes it clamp a size
  or decline a maximize) and publishes after each command or `simulate_*`
  call. The iOS window rereads its metrics and lifecycle on each `state` call
  and publishes from its view's metric updates and each lifecycle step.
- **The macOS window is reached on its owner thread only, by type.**
  `MacOSWindow` is neither `Send` nor `Sync`, so every window method and
  extension command runs on the thread that holds the window, the AppKit main
  thread, and messages AppKit directly. What leaves the thread is the window's
  `MacOSRedraw`, which queues a redraw on the owner lane
  (`exec_async_on_owner`) when asked from another thread, and its
  `MacOSSurface`, which hands out the content view's pointer while the window
  is open and never messages it (raw-window-metal checks that the Metal layer
  is made on the main thread).
- **The native Wayland host waits on two descriptors and nothing else**
  (`owner::linux::wayland`, the rule of ADR-0249). The owner thread blocks in
  `poll` on the compositor's socket (libwayland's `prepare_read`, so a GPU
  driver reading the same display on its own thread is never starved) and an
  `eventfd` every other producer writes: the owner signal and a redraw asked
  for on another thread. A turn dispatches what the compositor sent (the
  dispatch of a window's objects only records), then lets each window settle
  with no lock of the loop held (acknowledge a configure and commit it, publish
  the snapshot, ask about a close, deliver a frame that is due), then drives
  the owner turn and asks the exit policy when the last window closed. A wake
  deadline that came due is offered to the windows as frame requests once and
  armed again only after a frame ran (`owner::wake_deadline::WakeDeadline`,
  the X11 and Win32 loops' rule), so a window that declines its frames, or one
  the compositor does not draw, leaves the loop idle; a loop that no longer
  accepts work asks for no deadline. A turn whose dispatch recorded events
  ends before it would block, so they are applied first, and a flush the socket cut short makes the
  wait watch the socket for writing (`wait::before_poll`). A frame callback a
  skipped present never committed is given up after `frame::CALLBACK_GRACE`.
  The host and the display source connect to the one socket `WAYLAND_DISPLAY`
  names. The window (`window::linux::wayland`) sizes its surface
  through `wp_viewport`, so a buffer of any size is valid during a resize and
  each configure takes effect with a commit; its capabilities follow the
  compositor's `wm_capabilities` where it sends them (version 5); one that
  sends none (sway 1.9) is taken to offer every mode. A window tiled and in
  no mode takes the tiling's size, so `SetSize` is `NotOnThisDevice` while it
  is. Its buffer is drawn at the compositor's fractional scale
  (`wp_fractional_scale_v1`), else the surface's preferred whole scale, else
  its output's, and at scale 1 without a viewport: `scale::buffer_scale` is
  the one place that decides. Its frame is the compositor's answer through
  `xdg-decoration`, never the request; a configure after a request that names
  no mode answers it with the mode unchanged (wlroots sends no mode twice).
  Where the compositor offers none, or leaves the frame to the client, a
  window that wants a frame draws its own
  (`window::linux::wayland::client_frame`); only a compositor that draws a
  frame although asked not to (sway frames every tiled window) makes
  `SetFrame` to the system frame or none `NotOnThisDevice` (`decoration`). A
  suspended window is occluded: it is given no frame and gives up no frame
  callback until the compositor shows it again, when the frame asked for
  meanwhile is delivered (`frame::Pacing`). Where the compositor offers
  `xdg-foreign` v2 the window is exported as it opens, and the export lives
  until the window closes. Its portal parent `wayland:<handle>`
  (`foreign::PortalParent`) is a cross-platform fact (X11 names a window
  `x11:<XID>`), so it reaches the capability crates through the contract's
  window reference, not through an OS extension; until that reader exists the
  handle is kept and read under `testing` only. The window tells its AT-SPI
  bridge each change of the compositor's activation, which
  `UnixAccessibility::focus_changed` passes to the adapter (the X11 window
  already called it); the bridge reaches the adapter through `AtSpiAdapter`,
  the two calls it makes of `accesskit_unix::Adapter`. `Activate` is a request
  with an `xdg-activation` token (`activation`): the window asks for one naming
  the last user input (its seat, serial and surface, `UserInput`, kept by the
  loop in `LastInput`, which the windows read; no input records into it until
  the host binds its seats, so every token asks for attention) and hands it
  back once the compositor has shown the window (a frame callback came, or the
  surface entered an output), each token once and in order. The compositor
  decides: a token with no input behind it asks for the user's attention, so
  `activates` is `Emulated`, and `NotOnThisDevice` without `xdg_activation_v1`.
  The launcher's `XDG_ACTIVATION_TOKEN` is read once by the loop and given to
  the first window its table holds (`WindowTable::register`), so a window that
  failed to open leaves it; the variable stays set, as unsetting it races every
  thread reading the environment, and `xdg_open` leaves it out of the opener it
  starts. `current_host` picks the native Wayland host for a Wayland session
  (`choose_linux_host`).
- **Linux host events reach the owner through an inbox, and a machine that
  waits for the application waits for delivery, not for receipt.** Two sources
  run beside the owner and never run application code: an async logind task
  (`lifecycle::linux::logind`) and a kernel-pressure thread
  (`lifecycle::linux::pressure`). Each posts a `HostEvent` to one `Inbox` and
  wakes the owner, which drains it in its next turn (`HostNoticeSource::take`).
  logind's sleep and shutdown carry a
  completion: the delay inhibitor is held until the owner has delivered the
  event (a panic in a subscriber drops the completion too) and is released
  after at most four seconds, inside logind's own `InhibitDelayMaxSec`.
  Shutdown on Linux is a notification: the answer to the session-end query is
  not read, because logind cannot be asked to wait for the user. The decisions
  (signal decoding, trigger requests) are pure rules beside their sources
  (`lifecycle::linux::logind::signals`, `lifecycle::linux::pressure::triggers`),
  and the X11 atoms to a window mode are the window feature's
  (`window::linux::x11::mode`), each checked on every host.
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
- **Every collection the peer fills is bounded.** A Wayland compositor and an
  X server are peers, not trusted parts of the process: what one of their
  events adds and only another of their events removes has a fixed cap, named
  as a constant with why its number was chosen, and the event past it is
  refused, delivered as a partial frame, or replaces what it supersedes —
  whichever is true to it — never held without end. The caps: open touches per
  device (`MAX_TOUCHES`, in the shared registry, so X11 and every backend too),
  a Wayland touch frame's steps and a tablet frame's button changes (delivered
  in parts), tablet tools per seat, seats, outputs, the keys a keyboard holds
  (in the shared `xkb::Keyboard`, which also refuses a key code outside its
  keymap), one queued keymap per seat, and the X11 devices and valuators a
  query lists. A pointer frame's axes are fixed fields and bounded by their
  type. A Wayland window's own frame keeps one pointer state and one action
  asked for, which a later press replaces, and draws each of its five pieces
  at most once a turn, none wider or taller than `client_frame::MAX_EXTENT`
  whatever size a configure gives. The Wayland seats' queue of recorded events holds what one read of the
  socket dispatched, since the loop delivers it before it reads again; a
  keymap is queued as the compositor's file, and a newer one replaces the
  queued one and closes its file unread, so delivery reads one keymap of up to
  8 MiB per seat. Each cap
  in the pure translation has a test that feeds 10 000 distinct events without
  the one that clears them and fails with the cap removed; the seat, output
  and tablet-tool caps sit where a global or a tool is bound, which only a
  compositor reaches.

  Every object a Wayland compositor can make the client hold is one of three
  kinds. **Destroyed on arrival:** a `zwp_tablet_v2` and a `zwp_tablet_pad_v2`
  (FLUI reads neither), and a pad's group, ring or strip the moment an event
  of its own or its parent's reaches it. **Capped:** outputs (64), seats (16),
  tablet tools per seat (16), and one of each manager the input binds (`wl_shm`,
  `wl_compositor`, the tablet, gestures and cursor-shape managers; a second
  announcement is not bound); the loop's own globals are bound once, from the
  list at start. **Tied to a request of ours:** a seat's pointer, keyboard and
  touch, a pointer's pinch and cursor-shape device, a window's surfaces and
  frame callbacks, an activation token (destroyed at its `done`), a
  presentation feedback (ended by its own `presented` or `discarded`), and the
  display watch's colour objects: an output's `wp_color_management_output_v1`,
  and one image description with its information at a time per output — a
  change announced while one is read is asked for once it is done or failed,
  and the description it replaces is destroyed (the information destroys
  itself at its `done`). The display watch, on its own connection, reads at
  most the outputs the loop binds. A data offer follows `Intake::DEFAULT`:
  repeated and overlong names still consume its bounded source-entry budget,
  and only the first short name of each type is admitted.
- **Work per peer event is bounded.** What a compositor's or an X server's
  event costs is a constant, or linear in that event's own bytes; never in
  data the peer sent before (a keymap's size, a list's length), so a burst of
  small events cannot cost a burst of large reads. Where an event needs such
  data, it is read once when the data arrives and looked up after, or read at
  most twice a loop turn. The handlers:
  - a Wayland group change names its layout from where the keymap's text
    names it, found once when the keymap was taken (`KeymapSource::Compositor`'s
    `layouts`, ranges into the text rather than copies of it); a keymap is read
    and compiled once per seat per delivery, its file at most 8 MiB
    (`a_layout_switch_reads_no_keymap`, `layout_names_are_held_in_the_keymap_text`);
  - an X11 hierarchy change removes what it names removed or disabled with no
    read, and asks the server for each device it names added, enabled or
    attached, by that device's id, as GTK's `handle_hierarchy_changed` and
    winit's `xinput2_hierarchy_changed` do: one query per device the change
    carries, at once, so a device's events later in the same batch find it
    (`a_pen_plugged_in_mid_batch_draws_as_a_pen`,
    `a_hierarchy_change_asks_only_for_what_it_names`); an `XIDeviceChanged`
    applies the classes it carries, as GTK's `handle_device_changed` does, with
    no query (`a_device_change_brings_its_own_classes`); an XKB keyboard or map
    change reads the keymap again, bounded work since an X keymap is bounded by
    the protocol (key codes 8 to 255, four groups); an XKB state change names
    the layout from the names read with the keymap;
  - the scans an event makes run over the bounded collections above: held keys
    (128), a device's touches (32) and a touch frame's steps (256), a tablet
    frame's buttons (32), the seats (16), outputs (64), the X11 devices (128)
    and valuators (36), and the registry's contacts and devices, which those
    bound; a window lookup scans the application's own windows;
  - an X11 event's valuator mask is read in the event's own length;
  - a pointer event over a Wayland window's own frame looks its surface up
    among the five pieces of each window's frame and hit-tests the five
    pieces; the frame's title is shaped and rasterized again only when the
    title, the room it has, the scale or the fonts changed, and a turn draws
    each piece at most once (`client_frame`).
