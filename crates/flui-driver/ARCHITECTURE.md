# flui-driver architecture

The client half of the driver protocol (ADR-0238): the app's agent (ADR-0239, ADR-0288) knows the
app's trees, frames and input, and this crate composes that with what only the OS can do.

## Invariants

- **A client, never linked into an app.** The crate reaches neither `flui-native` nor `flui-app`
  (`reach-forbid`), and only `flui-cli` takes a normal edge to it. Its OS code (the lanes) is a
  client's, which is why ADR-0082's rule for framework crates does not bind it.
- **One conversation, two transports.** `connection::socket` and `connection::in_process` carry the
  same lines (`flui_protocol::envelope`): the headless target runs the agent's own
  `flui_runtime::agent::Session` through `flui_testing::HeadlessAgentService`, so the in-process
  answers are the ones a socket carries. The `Connection` trait is private.
- **Virtual time on the headless target.** The headless app's host runs on a thread of its own and
  pumps no frame by itself: a call that waits asks for one frame at a time, each 16 ms of the
  host's clock, and the call's budget is counted in that time. A run is the same on every host.
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
  `may_have_run` for a method that acts; a late answer is passed over by its id.
- **OS input rules are held at the seam, not by each lane.** `compose::choose_path` never falls
  back to the OS path; `lane::admit` (over `os_input::admit`, the guard's one source being
  `Lane::guard`) refuses OS input in process and on the shared desktop without a guard, before the
  run is announced and again at every `lane::send_guarded`, which asks the guard before every
  event and releases everything on every exit; `OsInputLock` holds one session per user's
  desktop, its file in a directory only that user can write (`$XDG_RUNTIME_DIR` where it names an
  existing directory, trusted as the XDG specification requires, or else a `flui-driver-<uid>`
  of mode 0700 in the temporary directory on Unix), `busy` only while another
  handle holds the OS file lock (a lock file this user may only read is opened for reading, one
  it cannot open is `Error::Io`, never retried); the first OS
  event of a session is announced once; once OS input went out, a failed read of what it did is
  `may_have_run`, never not run.
- **What a lane cannot send is refused before anything is sent.** `lane::send_guarded` asks
  `Lane::check` over the whole sequence before it arms the guard: a key a lane has no code for, a
  device or button it lacks, a chord the OS shell takes. Nothing is pressed, so nothing is
  released.
- **A lane's OS code builds on its OS only; its tables build everywhere.** `lane::windows` keeps
  the key, role, coordinate and activity tables, the safety rules and the contact frames pure, so
  Linux CI runs them; the modules that call Windows (`win32`, `guard`, `uia`, `capture`, `job`)
  and `WindowsLane` are `cfg(windows)`. `lane::macos` does the same: its key, role, screen,
  activity, pointer, frame and release tables, its safety rules and its callback containment are
  pure, and `quartz`, `ax`, `guard`, `capture` and `MacosLane` are `cfg(target_os = "macos")`.
  `lane::x11` keeps its keysyms, keymap and held input, safety rules, echo count and image layout
  pure, `lane::atspi::roles` and `lane::wayland::mapping` are pure, and the parts that reach an X
  server, AT-SPI or libei are Linux only (libei with the `wayland` feature).
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
- **Nothing stays pressed without a word.** A failed release is tried once more on every exit.
  A guarded sequence starts with nothing held: what an earlier one left is released first, and
  the sequence is refused, nothing sent, while it stays held. After a send that went through,
  the release's failure is the answer; where no caller is left to retry (the lane's worker
  ending, a job that panicked, a send that failed with an error of its own, which stays the
  caller's), what is still held is logged at error level. The error (`Error::ReleaseFailed`, the
  lane's own failure, apart from `input_held`) names every key, mouse button and touch contact
  still down and a pen down or in range, gathered from the lane's parts by
  `lane::release_outcome` with each part's first failure, so one part's failure never hides
  another's. It never waits, and a panic in that release (the lane's, a log subscriber's) is
  held behind the job's panic (`FirstPanic`), never in its place; a panicking send still disarms
  the guard and releases before its panic is resumed.
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
- **The OS tree is read against the app's through one projection.** A lane projects its OS tree
  onto the wire's nodes, and `projection::Projection` compares the two through
  `flui_protocol::tree::normalized_outline` (ADR-0095 §4), the one definition of what only one
  backend reports.
- **Mapping refuses, never clamps.** `coordinates::to_device` maps in `f64` with every step checked
  and refuses a point outside the surface; `steps::os_events` hands a lane single-step moves, each
  followed by a pause, at most `steps::MAX_STEPS` per move; the moving sources of a tick share
  one timeline, each split by its own duration, so they move together and each keeps its own
  speed, as on the synthetic path. Steps due within one millisecond go out together, so every
  pause is whole milliseconds, none under one, and a tick's pauses add up to its length exactly.
  A wheel source carries the part of a device pixel each scroll leaves into the next, and a scroll
  that adds up to no whole pixel yet sends nothing.
- **A hung app costs one timeout.** Reading a UI Automation, AX or AT-SPI element stops at the
  first property read that times out or finds the element gone
  (`lane::windows::walk::Properties`, which all three readers walk through), and the walk ends
  with `timeout`.
- **Waits are on the connection's clock.** An action's actionability wait ends at a deadline on
  the clock the connection counts budgets in, or as soon as the window presents no frame before
  it. The answer is read after the last wait: `compose::actionable` reads the element's facts
  again after its last frame, so an element that turns actionable on that frame is acted on, and
  a refusal reports what the app last showed.
- **The verdict rule is one function.** `Report::verdict`: `fail` when a check failed, else
  `cannot_verify` when a check could not be made or nothing was checked, else `pass`.
  `Error::is_cannot_verify` decides which failures cannot be verified.
