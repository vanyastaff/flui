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
