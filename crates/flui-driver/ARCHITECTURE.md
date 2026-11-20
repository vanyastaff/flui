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
  most `INCOMING_ROOM` lines wait for the caller; past that the thread stops reading. Why the
  connection broke is recorded in one place (`SocketConnection::receive`) and kept for every later
  call.
- **A timed-out act is never resent.** A call with no answer in its budget is `timeout`, with
  `may_have_run` for a method that acts; a late answer is passed over by its id.
- **OS input rules are held at the seam, not by each lane.** `compose::choose_path` never falls
  back to the OS path; `lane::admit` (over `os_input::admit`, the guard's one source being
  `Lane::guard`) refuses OS input in process and on the shared desktop without a guard, before the
  run is announced and again at every `lane::send_guarded`, which asks the guard before every
  event and releases everything on every exit; `OsInputLock` holds one session per user's
  desktop; the first OS event of a session is announced once; once OS input went out, a failed
  read of what it did is `may_have_run`, never not run.
- **What a lane cannot send is refused before anything is sent.** `lane::send_guarded` asks
  `Lane::check` over the whole sequence before it arms the guard: a key a lane has no code for, a
  device or button it lacks, a chord the OS shell takes. Nothing is pressed, so nothing is
  released.
- **A lane's OS code builds on its OS only; its tables build everywhere.** `lane::windows` keeps
  the key, role, coordinate and activity tables, the safety rules and the contact frames pure, so
  Linux CI runs them; the modules that call Windows (`win32`, `guard`, `uia`, `capture`, `job`)
  and `WindowsLane` are `cfg(windows)`.
- **The Windows lane rechecks the desktop before every event.** It sends nothing off the desktop
  that takes the user's input (`UOI_IO`), and that refusal is a lane the host cannot run
  (`cannot_verify`); then the window, the foreground, the window under the point and the physical
  input held, each refusal with its wire code (`safety::Refusal`). Every event carries the lane's
  tag, which is how its guard tells the lane's input from anyone else's.
- **No trampoline for the guard.** A low-level hook procedure gets no context, and a process-global
  cell would be a trampoline only `flui-app` and `flui-native` may hold (ADR-0097): the procedure
  posts what it saw to its own thread's queue, and the loop that owns the state classifies it.
- **A lane stays `Send` without a new `unsafe impl`.** OS handles a lane keeps (a window, a
  synthetic pointer device) are kept as integers; a COM object (UI Automation, Direct3D, a
  capture session) and its apartment live for one call on the lane's thread.
- **A launched Windows app runs in a kill-on-close job.** The job is made before the app starts,
  and its handle closes with the session or with the driver's process, however that ends.
- **The OS tree is read against the app's through one projection.** A lane projects its OS tree
  onto the wire's nodes, and `projection::Projection` compares the two through
  `flui_protocol::tree::normalized_outline` (ADR-0095 §4), the one definition of what only one
  backend reports.
- **Mapping refuses, never clamps.** `coordinates::to_device` maps in `f64` with every step checked
  and refuses a point outside the surface; `steps::os_events` hands a lane single-step moves, each
  followed by a pause, at most `steps::MAX_STEPS` per tick, the moving sources of a tick stepping
  together.
- **Waits are on the connection's clock.** An action's actionability wait ends at a deadline on
  the clock the connection counts budgets in, or as soon as the window presents no frame before
  it.
- **The verdict rule is one function.** `Report::verdict`: `fail` when a check failed, else
  `cannot_verify` when a check could not be made or nothing was checked, else `pass`.
  `Error::is_cannot_verify` decides which failures cannot be verified.
