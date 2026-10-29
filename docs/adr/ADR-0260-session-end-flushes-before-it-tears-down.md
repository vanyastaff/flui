# ADR-0260: A session end consults the close guards, writes the flush registry inline, and tears down only with time to spare

- **Status:** Accepted (2026-10-28). Not implemented: the platform delivers the session end to
  `AppHandler::session_end`, and `flui-app` answers it from the notice subscribers alone
  (`answer_session_end`, `crates/flui-app/src/app/runner/host.rs`); no guard is consulted, nothing is
  flushed and nothing is torn down when the session ends.
- **Date:** 2026-10-28
- **Related:** [ADR-0237](ADR-0237-window-state-and-host-notices.md) decision 8 (a session end can
  be held where the OS allows it; this record adds the close guard as a second holder, and the
  writing and the teardown that follow), [ADR-0254](ADR-0254-window-contract.md) decision 4
  (`AppHandler::session_end` replaces `Platform::on_session_end`),
  [ADR-0259](ADR-0259-close-guard-close-reasons-and-flush-registry.md) (`CloseReason::SessionEnd`,
  the guard, the flush registry, the one close delivery, application code only in a UI runtime in
  its slot), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (what a panic on
  this path does), [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §16 (the session
  messages' OS entry answers a panic by letting the session end)

## Context and problem statement

When the user logs off, restarts or shuts down, the platform reports the session end to the host as
`SessionEndPhase::{Query, Cancelled, Ending}` through `AppHandler::session_end`, and takes a
`SessionEndAnswer::{Proceed, Block}` back. On Windows the host's message receiver handles
`WM_QUERYENDSESSION` and `WM_ENDSESSION` (`crates/flui-native/src/lifecycle/windows.rs`): a `Block`
answer registers `ShutdownBlockReasonCreate` on a visible window of the thread, `Ending` asks the
handler and then releases the reason, and the system ends the process soon after the handler
returns. A panic in the handler counts as `Proceed`.

`flui-app` answers today from the `HostNotice` subscribers only (ADR-0237 decision 8): a subscriber
that holds a `SessionEndBlock` makes the answer `Block`, and a UI runtime that is busy when the
query arrives answers `Proceed` with a diagnostic. On `Ending` it delivers `SessionEnded` and
returns. The windows' close guards are not asked, the flush registry is not written, and the
process is ended by the system with whatever its services had written by then.

The time is short and not the application's: Windows shows "this app is preventing shutdown" about
five seconds after the query, and may end the process at any moment after `Ending` returns. The
message may also arrive while a UI runtime is checked out by the operation the system interrupted
with it.

What does the framework do with each phase, in what order, and within what time?

## Decision drivers

- what the application published to the flush registry reaches storage, even if its UI runtime is
  checked out and no application code can run;
- application code runs only in a UI runtime that is in its slot (ADR-0259 §7);
- the framework's own work fits a fixed budget; application code cannot be interrupted, so it is
  started only when the budget can absorb it;
- a cancelled session end withdraws its own record and nothing else;
- the process may be ended at any moment after `Ending` returns, so what matters is written first.

## Considered options

1. Answer from the guards and the subscribers, write the flush registry inline within a budget, and
   tear down only when time is left and no UI runtime is checked out.
2. Deliver the close into every UI runtime, checked out or not, and let each one write its own
   documents from its observers.
3. Do nothing beyond the notices: the application saves from its `SessionEnded` subscriber.

## Decision

Chosen option: 1, because it is the only one that writes the published state whatever the UI
runtimes are doing, and runs no application code where it is not allowed to.

`SESSION_END_BUDGET` is 3 s, counted from the start of the `Ending` phase, for the framework's work.

1. **Query.** For each presentation of a UI runtime in its slot, the close is asked with
   `CloseReason::SessionEnd`: the `CloseRequestHandler`, then the guard. `hold` does not refuse it
   (the registry writes what the hold protects); `require_decision` and a `KeepOpen` do, and record
   `PendingClose(SessionEnd)`. The answer is `Block` when any presentation refuses or any
   `SessionEndBlock` lives (ADR-0237 decision 8), and `Proceed` otherwise. A UI runtime that is
   checked out is not asked; it counts as `Proceed`, because its published bytes are written in
   step 4. The platform asks once per phase on its one receiver, so the answer needs no cache across
   top-level windows.
2. **Cancelled.** Each guard withdraws its `SessionEnd` record only (`CloseGuardSource::withdraw`),
   and its `changed` futures resolve on the owner's turn. A `User` close recorded earlier stays
   pending. The platform releases its block reason when the host releases the session end
   (`release_session_end`).
3. **Ending: delivery.** A UI runtime in its slot gets the one close delivery of ADR-0259 §6 with
   `SessionEnd`, which runs application code (`Detached` observers may still publish). A UI runtime
   that is checked out gets a `SessionEnding` operation in its FIFO, which runs only if the process
   survives; no application code of it runs, and its text field's uncommitted composition is lost.
   Delivery starts only while at least 1 s of the budget is left.
4. **Ending: flush.** `FlushHost::flush_within(start + SESSION_END_BUDGET)`, synchronously, inline in
   the handler: the write in flight is awaited, then the latest bytes of every name are written. It
   always runs, and runs no application code. No `AsyncDriver` takes part.
5. **Ending: teardown,** only when no UI runtime is checked out and at least 1 s of the budget is
   left. Otherwise the UI runtimes are left as they are, and the system ends the process.
6. **Ending: release.** Services are cancelled without being joined, and the pools are shut down
   without grace. The loop is always asked to quit, including for an end that closes only this
   application. The handler then returns, and the platform releases its block reason.

**The 1 s threshold** applies to the steps that run application code (the delivery of step 3 and the
destructors of step 5): they start only when at least 1 s of the budget is left, so that step 4 has
time to write. Application code cannot be interrupted: an observer that blocks longer than the budget
spends the flush's time, which is a limit of the application, not of the framework, and is
documented as such.

A panic in steps 3 and 5 is contained (ADR-0258 §1 and §2); steps 4 and 6 still run. A panic that
escapes the handler reaches the session message's OS entry, which answers by letting the session
end.

### Consequences

- Good, because the published state of every document is on disk before the system can end the
  process, whatever the UI runtimes are doing.
- Good, because a window with unsaved edits that need a decision holds the session end the way the
  OS shows the user, and a write in flight does not.
- Good, because a cancelled log-off leaves the user's own pending close alone.
- Bad, because a checked-out UI runtime runs none of its observers, and its uncommitted composition
  is lost.
- Bad, because the budget is the framework's, not the application's: an application that does slow
  work in its observers can starve the flush.
- Neutral, because platforms without a query (the web, Android, iOS) never refuse; they still flush
  where a phase is reported.

### Confirmation

- `session_end_flushes_the_registry_within_the_framework_budget`: the framework's share of the
  `Ending` phase stays within `SESSION_END_BUDGET`.
- `session_end_with_a_checked_out_runtime_writes_registry_bytes_and_calls_no_user_code`: the session
  end simulated from inside a handler (`HeadlessPlatform::simulate_session_end`) runs no observer and
  leaves the published bytes on disk.
- `a_cancelled_logoff_withdraws_only_the_session_entry`.
- The persistence feature's `session_end_during_a_write_writes_the_latest_registry_bytes` and
  `session_end_never_blocks_for_work_in_progress`.
- A dated native run of the acceptance app through flui-driver's `os` lane: the session messages
  sent to its window, then a restart, find the state in place.

## Pros and cons of the options

### 1. Guards and subscribers answer; the registry is written inline; teardown only with time left

- Good, because the write needs no application code and no UI runtime.
- Good, because the order puts the write before anything that can take long.
- Bad, because a checked-out UI runtime gets no delivery, so its observers miss the session end.

### 2. Deliver into every UI runtime and let each write its own documents

- Good, because every observer hears the session end.
- Bad, because delivering into a checked-out UI runtime runs application code inside a nested
  pump, the re-entry ADR-0259 §7 forbids, and against the text input's assumption that no nested
  access to its UI runtime exists.
- Bad, because there would be two write paths, the documents' own and the registry.

### 3. Notices only

- Good, because nothing new is built.
- Bad, because nothing is written unless an application's subscriber writes it synchronously, and
  a busy UI runtime's subscribers do not run at all.

## More information

The budget rests on an estimate, not a measurement: writing a document of a few hundred kibibytes
with `FlushFileBuffers` and `MoveFileExW` takes tens to hundreds of milliseconds on a spinning disk,
and half a second is kept for the platform to release its reason and quit before Windows' prompt at
about five seconds. Revisit both numbers after a dated run on a slow disk and on a OneDrive-backed
folder, and check that Windows delivers `WM_ENDSESSION` to a nested modal loop.
