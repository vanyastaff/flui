# flui-driver architecture

The client half of the driver protocol (ADR-0238): the app's agent (ADR-0239, ADR-0288) knows the
app's trees, frames and input, and this crate composes that with what only the OS can do.

## Invariants

- **A client, never linked into an app.** The crate reaches neither `flui-native` nor `flui-app`
  (`reach-forbid`), and only `flui-cli` takes a normal edge to it. Its OS code (the lanes) is a
  client's, which is why ADR-0082's rule for framework crates does not bind it.
- **One conversation, three transports.** `connection::socket`, `connection::bidi` (feature
  `chrome`) and `connection::in_process` carry the same lines (`flui_protocol::envelope`): a web
  app's page export is reached through the Chrome lane's BiDi session (`connect` with a channel as
  `onLine`, each line a `script.message`, each request a `send` through `script.callFunction`),
  and the headless target runs the agent's own `flui_runtime::agent::Session` through
  `flui_testing::agent_service::HeadlessAgentService`, so the in-process answers are the ones a socket carries.
  The transport trait, `connection::Transport`, is private. The driver is a
  `flui_protocol::client::Connection` (`client.rs`, ADR-0400 §1) over the window that had content
  when the connection was first used, kept for the session: every call, wait, input and end
  addresses it, a call whose parameters name no window is sent to it and one naming another is
  refused (`invalid_argument`), and once it closes each operation answers `gone`, the end's
  release counting a closed window as holding nothing.
- **Virtual time on the headless target.** The headless app's host runs on a thread of its own and
  pumps no frame by itself: a call that waits asks for one frame at a time, each 16 ms of the
  host's clock unless the launch names a longer period (`Launch::frame_period`, a window
  presenting more slowly),
  and the call's budget is counted in that time. A run is the same on every host.
- **The headless app keeps a native app's edges.** A failure a frame contained is dropped and the
  app serves on; the frame that closes its window ends it, and what the agent had not written goes
  with it; the call waiting and every later one fail as the process gone.
- **The socket goes with the driver.** One thread owns the stream, non-blocking; dropping the
  connection (a close, an error returned, a panic unwinding past it) closes its channel, and the
  thread shuts the stream down and is joined, so the app sees the driver leave on every exit. At
  most `INCOMING_ROOM` lines wait for the caller; past that the thread stops reading, and drops
  nothing. Why the connection broke is recorded in one place (`SocketConnection::take`), the first
  failure kept for every later call. Before why it ended, the thread hands over every complete
  line it read and those the stream has ready, reading at most `MAX_REPLY` bytes past the
  failure and one more to learn whether anything followed, and why it ended names what it did
  not hand over (an unfinished last line, a line too long, what the app sent past the bound); a
  send that finds the thread ended keeps the lines it
  reads to learn why.
- **A timed-out act is never resent.** A call with no answer in its budget is `timeout`, with
  `may_have_run` for a method that acts; a late answer is passed over by its id. `Error::Timeout`
  says what it awaited (`Awaited`): only the app's agent leaving a call unanswered is a
  `Connection`'s `CallError::Unanswered`, which releases the input of the connection's
  window; a tool on the host that hung is the lane's failure, `CallError::Failed` with its
  message.
- **OS input rules are held at the seam, not by each lane.** `compose::choose_path` never falls
  back to the OS path; `lane::admit` (over `os_input::admit`, the guard's one source being
  `Lane::guard`) refuses OS input in process, and on the shared desktop or a physical device
  without a guard, before the run is announced and again at every `lane::send_guarded`, which
  asks the guard before every event and releases everything on every exit, answering
  `ReleaseFailed` when the release leaves input held, whatever ended the run: the run's own
  failure is its `first`, whose effect and verdict it answers with (`Error::effect`,
  `Error::is_cannot_verify`); an input-method composition reaches only a lane whose capabilities
  say it composes (`LaneCapabilities::compositions`, the Chrome lane alone), refused before the
  desktop is locked or the run announced, at `send_guarded`, and by every other lane's own `send`;
  `OsInputLock` holds one session per user's desktop (a headless browser, the session's own,
  takes none), its file in a directory only that user can write (`$XDG_RUNTIME_DIR` where it names an
  existing directory, trusted as the XDG specification requires, or else a `flui-driver-<uid>`
  of mode 0700 in the temporary directory on Unix), `busy` only while another
  handle holds the OS file lock (a lock file this user may only read is opened for reading, one
  it cannot open is `Error::Io`, never retried); the first OS
  event of a session is announced once; once OS input went out, a failed read of what it did is
  `may_have_run`, never not run.
- **A person's interruption followed by a failed release is cannot-verify, the held input
  named.** When the guard sees the person's input, the run stops and is cannot-verify: the input
  that reached the app is no longer the scenario's, so neither a pass nor a failure of the app can
  be read from it. A release that then fails does not turn that into a failure of the app, which
  did nothing wrong; nor may it go unsaid, since input stays down on the person's desktop.
  `send_guarded` answers `Error::ReleaseFailed` with the keys, buttons and contacts still held in
  `held` and the interruption as `first`, from which `Error::is_cannot_verify` and
  `Error::effect` answer: the report says cannot-verify, and the error says what to release by
  hand. The same holds for any first failure: a timed-out act stays `may_have_run`.
- **What a lane cannot send is refused before anything is sent.** `lane::send_guarded` asks
  `Lane::check` over the whole sequence before it arms the guard: a key a lane has no code for, a
  device or button it lacks, a chord the OS shell takes. Nothing is pressed, so nothing is
  released.
- **A lane imports no other lane.** What more than one lane uses is a module of `lane`:
  `lane::walk` (the bounded accessibility walk), `lane::ax` (the macOS accessibility API's rules,
  such as an element's label, on every host, and its client on macOS, which the macOS lane and
  the iOS Simulator lane read through),
  `lane::pacing` (the guard-watched slices of a pause and the echo deadline), `lane::screen` (a
  window's geometry from its device origin) and `lane::tool` (a host tool, `adb` or `xcrun`, run
  to its end within a deadline and a bound on what it prints).
- **A lane's OS code builds on its OS only; its tables build everywhere.** `lane::windows` keeps
  the key, role, coordinate and activity tables, the safety rules and the contact frames pure, so
  Linux CI runs them; the modules that call Windows (`win32`, `guard`, `uia`, `capture`, `job`)
  and `WindowsLane` are `cfg(windows)`. `lane::macos` does the same: its key, role, screen,
  activity, pointer, frame and release tables, its safety rules and its callback containment are
  pure, and `quartz`, `ax`, `guard`, `capture` and `MacosLane` are `cfg(target_os = "macos")`.
  `lane::x11` keeps its keysyms, keymap and held input, safety rules, echo count and image layout
  pure, `lane::atspi::roles` and `lane::wayland::mapping` are pure, and the parts that reach an X
  server, AT-SPI or libei are Linux only (libei with the `wayland` feature). `lane::android` is
  host code that drives `adb`, so all of it builds on every host, and its tests script the device
  (`testing::ScriptedAdb`); `lane::ios_simulator` drives `xcrun` the same way
  (`testing::ScriptedXcrun`) and reads the Simulator's window through `HostAccessibility`
  (`testing::ScriptedSimulatorScreen`; the Mac's `MacAccessibility` alone is
  `cfg(target_os = "macos")`), and only starting an app needs a Unix host, whose socket the
  simulator reaches. `lane::chrome` (feature `chrome`) is host code on every desktop: its
  keys, actions, input-method steps and screenshot decoding are pure tables, and what reaches a
  browser goes through its session.
- **The Android lane reaches the device through one door.** Every `adb` command runs through
  `lane::android::adb::Adb` with a deadline and a bound on its output, a timed-out command being
  `timeout` with `may_have_run`; a stream (`getevent`, `uinput`) hands over bounded records, at
  most a fixed number waiting; a value from outside the lane enters a device shell line only
  through `adb::quote`. What the device prints is read as untrusted input: sizes checked before
  decoding (`screencap`, `uiautomator dump`), node, depth and device counts bounded.
- **The Android lane holds what `input` holds, and records it once taken.** `input` holds no key
  between commands, so `lane::android::held::HeldInput` sends a key at its release (alone, or as
  one `keycombination` with the modifiers held) and holds only the one `motionevent` contact and
  the pen's range and touch, each recorded when its command returned (the pen's when its `uinput`
  device answered a sync); a timed-out press is held and cancelled at the release, and a key a
  timed-out command may have left down is named in `Error::ReleaseFailed`, never pressed again.
  It refuses a second contact, a sequence that ends with a key down (which would never be sent),
  the keys and chords that leave the app or change the device (the fallbacks of AOSP's
  `Virtual.kcm` and `Generic.kcm` among them, `safety::FALLBACKS`), and a press of any pointer,
  the mouse included, going down at the display's edge; before each press, key, text and scroll
  one `dumpsys window -a` must show the window the lane located holding the focus, compared by
  its hash, frame and rotation, on the display holding the device's focus: another display is
  `android_default_display`, another window of the app's title is `NotForeground`, the same
  window moved or turned is `Busy` until the lane locates it again. The whole dump runs under the
  window manager's lock, which every window change on the device waits for, once a locate and
  once before each such command; `-a` is what prints the frames. Whatever ends a sequence
  after part of it went out answers `Error::PartlySent` around its own failure: the commands
  that went out may have run, and no retry is invited, which would send them again.
- **The Android lane takes no device nobody named.** `adb::HostAdb` has no constructor without a
  serial, so every command it runs names one; `lane::android::host_device` builds it from
  `FLUI_ANDROID_SERIAL` and asks `adb` nothing without it. What the lane leaves on the device is
  its own: a window dump at a path of the session's, and a forward from a loopback port the lane
  picked, asked for with `--no-rebind` (a port another session holds is refused and another
  picked) and removed by that port at its end once `adb` made it or may have.
- **The Android lane finds the app's nodes by matching, never by guessing.** `accesskit_android`
  sets no `viewIdResourceName`, so `Lane::read_tree` is handed the app's own tree and
  `lane::android::tree` matches the dump to it: the root is the child of the decor view that fits
  the app's root, where `FluiView.install` puts it, and a node fits an app node by structure and
  the order the adapter adds children in (those `accesskit_consumer`'s filter keeps), the class
  `lane::android::roles` gives its AccessKit role, its content description, its toggled state
  where the app's read reports it (a radio button's selection included; a grid cell's, and that
  of a list item, list box option, menu list option, tab or tree item publishing a selection, it
  does not report, and the match does not read the dump's: `tree::Unobservable::UnreportedToggle`)
  and enabled state, and its bounds at the surface's origin; two candidates the order does not
  tell apart are `Error::Ambiguous`, and bounds off the display place a node nowhere. Only a
  matched node carries the app's role and identifier; what a dump cannot show
  (`tree::Unobservable`) projects as the app published it. Only the read of the tree dumps: the
  window is located from `dumpsys window` (`device::parse_windows`): the focused window by its
  hash, on the default display, located again after the dump, which a replaced or moved window
  or another turn makes `Busy`; so input and capture pause no accessibility service, and no dump runs while
  any service does (`safety::check_dump`). `invoke` is refused, since UI Automator performs no
  accessibility action.
- **The OS tree is compared with the app's tree of its own revision, on every lane:**
  `Window::compare_projection` reads the app's tree again while it moves, by the rule its
  rustdoc states, and answers `Busy` when it never held still.
- **The iOS Simulator lane sends no input** until a supported channel exists (the flui-driver
  spec's rule; `lane::ios_simulator::xcode` has the evidence): `Lane::check` refuses every input
  event as `not_supported` naming `ios_simulator_hid`, with the Xcode and runtime, before anything
  is sent. It reads the screen through `simctl io screenshot`, whose file length and declared size
  are checked against the window before a pixel is decoded, and takes no simulator nobody named
  (`FLUI_IOS_SIMULATOR_UDID`). A session holds its simulator (`SimulatorLock`), since the launch
  and the terminate name the app by its bundle identifier; it always ends the app it started with
  `simctl terminate`, since an iOS app does not exit when its driver goes, reading an answer that
  nothing runs as an app that already exited. The launch values reach the app as
  `SIMCTL_CHILD_` variables in `simctl`'s environment, never in a command's text or debug form,
  and no such variable inherited from the driver's environment reaches it.
- **The iOS Simulator lane reads what the Simulator serves the Mac, and matches it.** The
  Simulator vends the iOS app's elements to AppKit as elements of the device's window (Apple's
  private translation framework), which the public `AXUIElement` API reads; the lane takes no
  private framework. Apple documents none of it: the roles the bridge gives each trait are
  inferred, whether it carries the app's identifier is unconfirmed, and until runs on a Mac pin
  both the read stands for VoiceOver on iOS only as far as they confirm it (`docs/gaps.md`).
  The read hands back the elements as served (`accessibility::ScreenElement`), and everything
  after it is pure code both the Mac and the scripted window go through (`ios_simulator::tree`).
  The window is one on screen titled `<device> – <runtime>`, found without looking the
  Simulator's process up by name, and taken by an allow-list that every host goes through, the
  scripted window included (`tree::choose_window`): the one window naming the device's UDID is
  taken, whatever the others name; else the one naming no UUID, only while no other listed
  simulator, in any state, shares the title; one naming any other UUID never; two naming the
  UDID, or two naming none when none names it, are refused. A device renamed or removed since
  the lane connected is refused (`accessibility::screen_title`). The screen's elements are read
  by a pure walk over a source (`accessibility::walk_screen`): a level the node budget or a
  vanished element cuts is marked unread on its parent, the screen included, and a group gone
  mid-search cuts the screen search short. Every walk over an OS tree takes a step that finds an
  element gone as its level cut, never ended (`lane::walk`); `ax::child_at` answers
  `kAXErrorInvalidUIElement`, the parent gone, as that. Every element the read asks is given the
  client's deadline and carries it, so a timeout, a press's included, names the deadline of the
  element its call was asked of, or says that element refused it. The screen is found by walking every group of the
  window within ten levels (`tree::find_screen`), each group's children listed by the bounded
  listing the screen's walk shares (`accessibility::list_children`): an `iOSContentGroup` when
  there is one, else the shallowest group of the app's window's shape, which a search left
  partial by a group gone or its budget does not take. `Lane::read_tree` and `Lane::invoke` are handed
  the app's own tree, and each element is matched among the descendants of the node its nearest
  matched ancestor fits (the bridge serves only the elements VoiceOver reads), an app node once,
  by the role `lane::ios_simulator::roles` gives `accesskit_ios`'s trait, its label (a static
  text's from its value, the rule `lane::ax::label` the macOS lane shares), its checked and
  enabled state, an identifier it carries, and its frame on the window; two candidates alike are
  `Error::Ambiguous`. A press finds its element again by the facts the read gave, refusing none,
  two, and one on a read that stopped short (`tree::locate`), and presses it through `AXPress`
  (`accessibility::pressed`). A failure while the process is no longer trusted answers the
  Accessibility permission, wherever it comes, except a press already sent: one not answered,
  or whose element went away, keeps `may_have_run` and names the permission in its message.
- **The X11 lane rechecks the server before every event,** as the Windows lane does the desktop:
  the window is the app's (by the process X-Resource reports for its client, or its
  `_NET_WM_PID`) and mapped, it has the keyboard focus, the window under the point is its own, and
  no button or key it did not press is held (`lane::x11::safety::Refusal`). XTEST fakes a core
  pointer and keyboard only, so touch and pen are refused before any event.
- **Every X11 key goes through one plan over the server's keymap.** `lane::x11::held::HeldInput`
  plans a key or text against the keymap and what the lane holds, records a press once the server
  took it, and owns the StickyKeys count across sends; a character on no key is typed through a
  spare key mapped for the text, with a pause after mapping and before emptying, since an app
  reads its keymap again only when told it changed. The release of everything empties every
  mapped key too, and completes no desktop shortcut: while a modifier is held it presses and
  releases a spare key with nothing on it first (`HeldInput::mask`), which ends a run of lone
  Shift taps, and a release that would still complete StickyKeys is not sent. A wheel click is a
  press and a release held as a button's are, so a click left down is released with the rest, and
  a scroll of more than `held::MAX_WHEEL_CLICKS` clicks is refused before any event.
- **The X11 guard tells the lane's XTEST input from anyone else's by counting.** It reads XInput
  2's raw events on its own connection; an event from a device other than the XTEST devices is the
  person's, and one from them past the count of events the lane faked is another program's, which
  ends the run as the person's does. Each faked event waits for its echo, so a guard that stopped
  seeing the lane's input ends the run.
- **The Wayland lane sends OS input only into a compositor of its own.** Wayland shows a client
  no input it did not send, so the lane has no guard: on the shared session `lane::admit` refuses
  its input, and it only reads. Keys and text go through EI's text interface as keysyms and UTF-8,
  so no keymap is read; a portal that refuses or a compositor that does not answer is a lane the
  host cannot run, and the portal of the driver's own session bus, the person's desktop, is
  refused for a compositor of the run's own. A Wayland client is told nothing of where its window is: the lane maps from the
  origin its owner gives it, or refuses naming `window_position`.
- **AT-SPI is read, never switched on.** Both Linux lanes read the app through
  `lane::atspi::client` on the accessibility bus the session bus names, finding the app by the
  process owning its bus connection; AT-SPI off in the session is a lane the host cannot run, and
  the lane leaves the person's setting alone.
- **A launched Linux app dies with its session.** It leads a process group of its own, which the
  session kills when it ends, what the app started included even after the app exited: the app
  is read without being reaped (`waitid` with `WNOWAIT`) until then, so its id names its group and
  no other process's, and the group is killed only while `waitid` still answers for the app (a
  driver that ignores `SIGCHLD` has its children reaped by the kernel, and kills no group then). It also dies when the thread that started it ends
  (`PR_SET_PDEATHSIG`): that thread is the session's own (`launch::linux::Starter`), so a driver
  moved to another thread keeps its app.
- **The macOS lane posts to the app's process from a private, tagged source.** Every event goes
  out through `CGEventPostToPid` from a `kCGEventSourceStatePrivate` source whose user data is the
  lane's tag, and carries the modifiers the lane holds as its flags (a modifier is a flags-changed
  event with the flags it leaves). Its events never reach the hardware state, so anything that
  state shows held is someone else's and refuses the event; its guard, a listen-only session
  tap, takes an event for the lane's only with the private state, the tag and the driver's
  process id together, and a tap the system turned off ends the run.
- **A privacy permission is asked, never awaited.** Before every event, release and read that
  needs Accessibility, Input Monitoring or Screen Recording, the macOS lane preflights it (from
  macOS 10.15, the release that brought them) and refuses with `not_supported` naming
  `macos_permission_<name>`, since the person can take one back mid-run and an event posted
  without it is dropped with no word; a release refused so leaves its key or button held and
  named (`Error::ReleaseFailed`). It never shows or waits on a prompt.
- **Code macOS calls back into is contained.** The guard's tap callback and ScreenCaptureKit's
  completion handlers run under `FirstPanic` (`lane::macos::callback`): a panic never unwinds into
  the system's frame, the guard stops watching (which ends the run), and a capture answers a
  typed failure.
- **A newer Apple API is looked up, never linked.** FLUI runs on macOS releases older than
  ScreenCaptureKit, so the lane loads that framework by path when a capture is asked for and
  finds its classes by name: a release without them answers `not_supported`, and the driver links
  only frameworks every release has.
- **The Windows lane rechecks the desktop before every event.** It sends nothing off the desktop
  that takes the user's input (`UOI_IO`), and that refusal is a lane the host cannot run
  (`cannot_verify`); then the window, the foreground, the window under the point and the physical
  input held, each refusal with its wire code (`safety::Refusal`). Every event carries the lane's
  tag, which is how its guard tells the lane's input from anyone else's.
- **Every key goes through one checked plan.** A key event and typed text become keystrokes
  planned against what the lane holds (`lane::windows::held::HeldKeys`): the shell-hotkey rule
  reads those strokes before a sequence, and the lane sends them, reading the desktop again before
  each. What the lane holds (keys, buttons, contacts) is recorded as soon as Windows took the
  press, before the guard confirms its echo, and forgotten once it took the release, so a press
  whose echo never came and a refused release are both released again. `HeldKeys` is also the
  one owner of what the shell's shortcuts count across sends (the modifiers of a chord, the run
  of lone Shift taps): every key edge Windows took goes through it, a release of everything
  included. Only another key restarts the run or breaks a chord: Windows documents five presses
  of Shift and nothing about a pause or pointer input between them, so neither resets the count.
  The lane never completes a shortcut the shell takes on a release: a key event that would
  (letting go of Control+Shift or Alt+Shift, the fifth lone Shift press or its release, since
  Microsoft does not say which one turns Sticky Keys on) is preceded by a press and release of
  the unassigned virtual key `0xE8` (`held::MASK`), as AutoHotkey masks one, so the app still gets
  its keys, the mask among them. The release of everything masks whenever a modifier is held, and a release that
  would still complete a shortcut (the mask not sent) is not sent. Only a press the shell takes
  (the Windows key, Alt+Tab, Ctrl+Shift+Esc, a lock key) is refused before a sequence.
- **Nothing stays pressed without a word.** Where no caller is left to retry (the lane's worker
  ending, a job that panicked), the worker releases once more after a failed release and logs
  what is still held at error level; the error (`Error::ReleaseFailed`, the lane's own failure,
  apart from `input_held`) names every key, mouse button and touch contact still down and a pen
  down or in range, gathered from the lane's parts by `lane::release_outcome` with each part's
  first failure, so one part's failure never hides another's. It never waits, and a panic in that release (the
  lane's, a log subscriber's) is held behind the job's panic (`FirstPanic`), never in its place.
- **An accessibility provider's answers stay apart.** A UI Automation read or invoke tells a
  provider that timed out (`timeout`), an element that went away (`gone`) and a pattern the
  element does not offer (`action_unsupported`) from each other; a timeout or a gone element
  during an invoke carries `may_have_run`.
- **A pause is waited out whole,** in slices with the guard looked at between them; a guard whose
  watcher is gone, or whose hook no longer sees the lane's own input, ends the run, never reads as
  a quiet desktop.
- **No trampoline for the guard.** A low-level hook procedure gets no context, and a process-global
  cell would be a trampoline only `flui-app` and `flui-native` may hold (ADR-0097): the procedure
  posts what it saw to its own thread's queue, and the loop that owns the state classifies it.
- **A lane stays `Send` without a new `unsafe impl`.** OS handles a lane keeps (a window, a
  synthetic pointer device, a window number) are kept as integers; a COM object (UI Automation,
  Direct3D, a capture session), an accessibility element or a Quartz event source lives for one
  call on the lane's thread.
- **A launched Windows app runs in a kill-on-close job.** The app starts suspended and joins the
  job before its first instruction, and the job's handle closes with the session or with the
  driver's process, however that ends. On Unix a launched app leads a process group of its own,
  which the session ends whole while its leader runs unreaped.
- **The Chrome lane's browser is one session's, headless, behind one socket owner.**
  `lane::chrome::bidi::Session` is the only owner of the BiDi WebSocket: a thread of its own writes
  each command with a fresh id and hands each answer to the caller whose id it carries, so a late
  answer is dropped and an error with no id answers nobody; a message past `MAX_MESSAGE` is
  refused from its frame header, at most `MAX_PENDING` commands wait, every command has a deadline
  and one past it is answered `timeout`, and the page's lines wait for its connection within
  `PAGE_ROOM` lines and `PAGE_BYTES`, past which the page's connection breaks with why rather than
  the socket going unread, which would starve the lane. The first break is recorded once and
  answers every later command. The session owns the browser it runs in (the page's loopback
  server, the ChromeDriver), ended after the socket once its last handle went; Chrome always starts
  `--headless=new`, so its input lands on no desktop and the lane needs no guard. The lane records
  a key, button, contact or composition as held once the browser answered the command that made
  it, an unanswered one (a text's included) as possibly held, and its release cancels a
  composition and sends `input.releaseActions` whatever it recorded, naming what stays held when
  the browser refuses it. What the browser sends is untrusted: a screenshot's base64 length is checked
  before it is decoded and its PNG goes through `pixels::codec` (`lane::chrome::screenshot`), ChromeDriver's and
  the archives' answers are read within bounds, and the page's server answers only `GET` and
  `HEAD` of plain names under its root, for the `Host` it bound, each connection within one
  deadline. A launch reaches no network unless its `ChromeDriverSource` allows a fetch, which is
  HTTPS alone and records the SHA-256 a cached ChromeDriver is checked against before it runs;
  the cache's files are linked into place, never replaced, so parallel fetches cannot tear them.
- **The OS tree is read against the app's through one projection.** A lane projects its OS tree
  onto the wire's nodes, and `projection::Projection` compares the two through
  `flui_protocol::tree::normalized_outline` (ADR-0095 §4), the one definition of what only one
  backend reports.
- **Mapping refuses, never clamps.** `coordinates::to_device` maps in `f64` with every step checked
  and refuses a point outside the surface; `steps::os_events` hands a lane single-step moves, each
  followed by a pause, at most `flui_protocol::input::MAX_STEPS` per move, as
  `flui_protocol::input::plan` lays them; the moving sources of a tick share
  one timeline, each split by its own duration, so they move together and each keeps its own
  speed. Steps due within one millisecond go out together, so every
  pause is whole milliseconds, none under one, and a tick's pauses add up to its length exactly.
  A wheel source carries the part of a device pixel each scroll leaves into the next, and a scroll
  that adds up to no whole pixel yet sends nothing. A scroll by lines is refused: an OS event
  scrolls by device pixels, and no lane knows the line height the app scrolls a line by. What no
  lane sends and what the plan refuses (`steps::admit`, which needs no window) are refused before
  the desktop is locked, the window located or the run announced.
- **A hung app costs one timeout.** Reading a UI Automation, AX or AT-SPI element stops at the
  first property read that times out or finds the element gone
  (`lane::walk::Properties`, which all three readers walk through), and the walk ends
  with `timeout`.
- **Waits are on the connection's clock.** An action's actionability wait ends at a deadline on
  the clock the connection counts budgets in, or as soon as the window presents no frame before
  it. The answer is read after the last wait: `compose::actionable` reads the element's facts
  again after its last frame, so an element that turns actionable on that frame is acted on, and
  a refusal reports what the app last showed.
- **A capture names its source, and a source the driver cannot read is named, never faked.**
  `pixels::Capture` carries its `CaptureSource`, backend, scale and frame. `os` comes from the
  lane (`Lane::capture_window`, one `pixels::Image` type for every lane); `presented` and
  `rendered` are `not_supported` naming the method while the app does not serve it, and naming
  `capture_payload` while the connections read no binary frame.
- **A baseline check passes only by matching.** `pixels::Baselines::compare` answers `Matched`
  or a status that fails (`Missing`, `SizeDiffers`, `Differs`, `Written`): a baseline an update
  wrote fails its check, and `FLUI_DRIVER_UPDATE_BASELINES` is refused under `CI`. A missing or
  differing baseline leaves `.new.png` (and `.diff.png`) beside it; a match or a write removes
  them. A stable capture compares masked images, so what a mask hides cannot keep it unstable,
  and it ends after `MAX_STABLE_CAPTURES` captures whatever its clock does.
- **Every image and trace the driver reads is bounded before it is decoded.** A PNG's file
  length, then its declared size, then the decoder's allocations (`pixels::codec`); a trace's
  file length, then its bytes as they are read (one past the bound stops a file that grows),
  line length and row count (`trace::reader`); a native recording through the format's own
  `Budget` (`trace::native::read`). Native streams are read only as `native/index.json` names
  them, at most `MAX_NATIVE_STREAMS` and `MAX_NATIVE_TOTAL_BYTES` together (every size added up
  before any is read), each under its one name (`<stream>.jsonl`). A trace's files have fixed
  names, each checked from the top down to be no symbolic link or junction (`native/`,
  `evidence/` and the file), so none resolves outside the trace.
- **A trace records the caller's commands, once each.** `Driver::command` is the one place a
  command becomes a row: the outermost call only (`command_depth`), its parameters as written
  (locators, never only the handle they resolved to), written before it runs, its result after;
  a panic out of the command restores the depth and writes no result. The rows of the trace's own
  recording go to `input.jsonl` and `native/`; every other event is written and kept for
  `take_events`. Events the connection dropped are a `gap` row, a `gap` in `input.jsonl` and in
  every native stream (which ones lost lines is not known) while the recording runs, and part
  of the caller's dropped count; once the recording ended, no line of it is left to lose. A write that fails is the trace's first failure, nothing is written after it, and
  `finish_trace` answers it; a session that ends unfinished leaves no `end`, which readers report
  as incomplete, and so does a last line torn mid-write.
- **The app records only while a trace holds its recording.** `start_trace` makes the trace's
  files before it sends `record.start`, and every way out after it (a recording file that cannot
  be made, a panic) sends `record.stop` and removes what it made, never what was there: each
  file is made new (`create_new`), and the directory goes only if the trace made it.
  `finish_trace` takes the trace out of the session before its own stop, so the stop is no row.
- **One writer of native recordings.** `trace::native::NativeFiles` writes a recording's native
  layer for a trace and for `flui record --native` alike, and `trace::native::read` reads one
  back through `flui-protocol`'s schema, without the core.
- **Redaction is by field over the JSON a row carries** (`trace::TEXT_FIELDS`), a command's
  `TextMatch` kept as the caller's own; `trace::every_text_field_of_the_wire_is_redacted_or_named_as_kept`
  holds every free string field of the protocol's schema to one side of the list. A replay never
  sends redacted text: a redacted row ends it, refused there.
- **A trace replays against the protocol it was recorded at.** A replay compares each result
  field by field, so `trace::replay` refuses a trace whose header names another protocol than the
  app speaks (`Error::Version`), before any command.
- **A replay reads handles only where the method's schema has one** (`trace::handles` walks
  each value along the protocol's own schema of its method, made with `schemars`): a text, a
  locator's name or an input source's id shaped like `e1` is text, though a node's `id` is a
  handle. `trace::no_field_the_trace_redacts_holds_a_handle` keeps the redaction, which goes by
  name, off every name that is a handle anywhere.
- **The verdict rule is one function.** `flui_protocol::report::Report::verdict`: `fail` when a
  check failed, else `cannot_verify` when a check could not be made or nothing was checked, else
  `pass`.
  `Error::is_cannot_verify` decides which failures cannot be verified.
