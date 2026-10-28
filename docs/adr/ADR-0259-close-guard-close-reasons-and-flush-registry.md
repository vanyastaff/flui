# ADR-0259: A presentation's close guard records closes by reason, and a host-owned flush registry is the one way persisted state reaches storage

- **Status:** Proposed. The contract is in place and not wired: `CloseReason`, `CloseGuard`,
  `CloseHold`, `PendingClose`, `CloseChanged` and `CloseGuardSource`
  (`crates/flui-view/src/close_guard.rs`), `FlushRegistry`, `FlushPublisher` and `FlushHost`
  (`crates/flui-view/src/flush_registry.rs`), `OwnerNotify` (`crates/flui-view/src/owner_notify.rs`),
  the close-delivery claim of `LifecycleSource` (`crates/flui-view/src/lifecycle.rs`) and
  `CloseRequest::reason` (`crates/flui-app/src/app/close_request.rs`). No presentation hands out a
  guard, no hold refuses a close, and the registry keeps published bytes without writing them.
- **Date:** 2026-10-28
- **Related:** [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (the exit order
  that flushes the registry; what a panic on a close path does),
  [ADR-0260](ADR-0260-session-end-flushes-before-it-tears-down.md) (a session end consults the guard
  with `SessionEnd` and flushes the registry inline),
  [ADR-0254](ADR-0254-window-contract.md) decision 4 (`WindowHandler::close_requested` answers a
  `CloseDecision`), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md) (a capability is acquired
  in a lifecycle hook), [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §4 and §6
  (closing delivers `Detached` while the tree lives; begin-close fences subscriptions),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) and
  [ADR-0127](ADR-0127-exceptional-path-retention.md) (what a close that holds a failure retains),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the registry is a host value, not a
  `static`), [ADR-0152](ADR-0152-capability-seam-revised.md) (storage reaches a UI runtime through
  the capability seam)

## Context and problem statement

A window closes in several ways: the user's close button or shortcut, the application quitting
through its own code, the operating system ending the session. Today a close request carries only
its presentation's address; the application's `CloseRequestHandler` answers `Close` or `KeepOpen`,
and `KeepOpen` leaves no record: nothing finishes the close when the reason to keep the window is
gone. `AppHandle::request_quit` asks no one.

Persisted state has the opposite problem. A document that a widget writes has to reach storage at
the end of the application and of the session, including when its UI runtime is busy or already
gone. A write loop inside the UI runtime's `AsyncDriver` cannot do that: at loop exit the UI runtime
is destroyed before the services stop, and at a session end the UI runtime may be checked out by the
operation the session message interrupted.

Close and persistence meet in one place: a write in flight, or a failed write with unsaved edits,
is a reason to keep the window, of a different strength for each reason it is closing.

What does a presentation hold its close with, what is recorded when it does, and what path takes
persisted state to storage when the application or the session ends?

## Decision drivers

- a close the application refused is not forgotten: when the reason to refuse it is gone, the close
  happens, once, without asking the handler again;
- a close for one reason is not confused with a close for another: a cancelled log-off withdraws
  the log-off, not the user's close button;
- application code runs only in a UI runtime that is in its slot; what runs while one is checked out
  touches only state outside it;
- a transition never runs waiters or application code where it happens: it signals the owner, and
  the owner settles it inside its one containment boundary;
- one write path to storage, which the host can drive without any application code;
- no `static`, no lock held across application code, nothing on the frame path that waits for IO.

## Considered options

1. A per-presentation close guard in `flui-view` with records keyed by reason, and a flush registry
   owned by the host outside every UI runtime.
2. A boolean "dirty" flag per presentation that the close handler reads, and a write loop in each UI
   runtime's `AsyncDriver`.
3. An asynchronous close handler (`async fn close_requested`) that the platform awaits.

## Decision

Chosen option: 1, because it is the only one that keeps a refused close and writes persisted state
when the UI runtime cannot run.

### §1. Close reasons

```rust
// flui-view; re-exported as flui::view::CloseReason and flui::app::CloseReason
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CloseReason { User, Program, SessionEnd }
```

`CloseRequest::reason()` carries it to the application's `CloseRequestHandler`, which stays
synchronous (`Close` or `KeepOpen`). A platform with another kind of close adds a variant.

### §2. The close guard

```rust
pub trait LifecycleContext: BuildContext {
    fn close_guard(&self) -> Option<CloseGuard> { None } // in lifecycle hooks only (ADR-0078)
}
#[derive(Clone)] pub struct CloseGuard { /* Rc of the presentation's state; !Send */ }
impl CloseGuard {
    pub fn hold(&self) -> CloseHold;             // refuses User and Program; lets SessionEnd through
    pub fn require_decision(&self) -> CloseHold; // refuses every reason the platform lets it refuse
    pub fn pending(&self) -> Option<PendingClose>; // the earliest record; at most one per reason
    pub fn changed(&self) -> CloseChanged;         // resolves at the next change, on the owner's turn
    pub fn can_veto(&self, reason: CloseReason) -> bool;
}
#[must_use] pub struct CloseHold { /* released on drop */ }
pub struct PendingClose { /* reason + which recording of it */ }
impl PendingClose {
    pub fn reason(&self) -> CloseReason;
    pub fn discard_and_close(self) -> Result<(), StaleClose>; // close past the holds
    pub fn stay_open(self) -> Result<(), StaleClose>;         // withdraw this record only
}
```

- **One guard per presentation**, owner-local: every clone, hold, record and future of one
  presentation shares one state, and none leaves the owner thread.
- **Two strengths of hold.** `hold` covers work that finishes by itself (a write in flight): it
  refuses `User` and `Program`, and lets `SessionEnd` through, because the framework writes the
  latest published bytes while the session ends (§5). `require_decision` covers a state only the
  user can resolve (a failed write with unsaved edits): it refuses every reason the platform lets
  the application refuse.
- **A refused close is recorded**, once per reason: a repeated request for the same reason records
  nothing more. A `KeepOpen` from the handler records the same way as a hold.
- **Releasing the last hold carries the close out once.** When the last hold is released, records
  exist and none is held by `require_decision`, one close operation for that `PresentationAddress`
  is queued on the owner. The handler is not asked again.
- **A record names one recording.** `PendingClose` carries the recording's epoch; once that
  recording is carried out, withdrawn or resolved, `discard_and_close` and `stay_open` answer
  `StaleClose` and do nothing, even if the same reason was recorded again since.
- **Withdrawal is per reason.** `CloseGuardSource::withdraw(reason)` removes that reason's record
  only: a cancelled log-off withdraws `SessionEnd`, and the user's earlier `User` close stays
  pending.
- **Where the platform cannot wait** (`can_veto(reason) == false`: a session that is already ending,
  the web, Android, iOS), holds are not consulted; only the framework's flush writes what was
  published, and a document hidden or paused writes at once.

### §3. Transitions signal the owner and settle on its turn

A transition (a close recorded, withdrawn or due; a write finished) commits its state, releases its
borrow or lock, and calls `OwnerNotify` with a `LifecycleEvent` (`CloseChanged`, `CloseDue`,
`FlushChanged`). The callback only signals: it wakes the owner's loop. On its turn the owner calls
`CloseGuardSource::settle` and `FlushHost::settle`, which hand back the due close and the wakers of
the futures the change resolved; the owner carries the close out and wakes the wakers itself, one by
one, inside its containment boundary. A signal that cannot be delivered (`Undeliverable`) loses
nothing: the transition stays recorded and the next settle finds it. A waker that panics loses
neither the due close, which the owner already holds, nor the wakers after it.

### §4. `Program` is a close the application's code asks for

`AppHandle::request_quit` asks every presentation's guard with `CloseReason::Program`. While any
refuses, the quit is deferred, and it proceeds when the last hold over it is released.
`request_presentation_close` stays the application's unconditional close: it asks no guard.

### §5. The flush registry

```rust
// flui-view; not in the widget surface: documents reach it through a sealed accessor
#[derive(Clone)] pub struct FlushRegistry { /* Arc: the host's registry */ }
impl FlushRegistry {
    pub fn publisher(&self, name: StorageName) -> FlushPublisher;
    pub fn committed(&self, name: StorageName) -> Option<(Revision, StoredVersion)>;
    pub fn state(&self, name: StorageName) -> FlushState; // Clean | Pending | Failed(StorageError)
    pub fn changed(&self) -> FlushChanged;
}
impl FlushPublisher {
    pub fn publish(&self, bytes: Arc<[u8]>, mode: WriteMode, revision: Revision); // owner thread
}
// flui_view::__runtime, for the host only
impl FlushHost {
    pub fn new(storage: Arc<dyn Storage>, writer: FlushWriter, notify: OwnerNotify) -> Self;
    pub fn registry(&self) -> FlushRegistry;
    pub fn settle(&self) -> Vec<Waker>;
    pub fn flush_within(&self, deadline: Instant) -> FlushReport; // synchronous, no application code
}
```

- **Owned by the host, outside every UI runtime.** The host builds one `FlushHost` per host from its
  storage; a UI runtime's documents get the publishing side. Because it is outside the UI runtime,
  the host can write what was published when the UI runtime is gone or checked out.
- **The latest bytes win.** A publish replaces bytes of the same name not yet written. The registry
  writes on the IO pool through `FlushWriter`, one write in flight per name; new bytes during a
  write make exactly one write after it.
- **A publisher's edits are one line.** An edit based on the version its own previous write replaced
  is rebased onto that write; an edit based on a version another publisher replaced fails with
  `StorageError::Conflict`. Publishers are compared by identity, held as the publisher's `Arc`
  itself, never an address that a later publisher could reuse.
- **A failed write keeps its bytes** until newer bytes replace them or the host's flush writes them.
  A `FlushWriter` that refuses a write hands it back; the write stays owed.
- **The one path to storage.** A `Persisted` document encodes on the owner thread and publishes; it
  reads its status from `committed` and `state`. There is no write loop in the `AsyncDriver`.
  `flush_within` waits for the write in flight and writes the latest unconfirmed bytes of every name
  before the deadline; it runs at loop exit (ADR-0258 §3) and inline at a session end (ADR-0260).
- **Not a widget capability.** `LifecycleContext` reaches the registry only through a crate-private
  token (`flush_registry_in_crate`), so the registry is part of `flui-view`'s persistence, not of the
  widget surface.

### §6. One close delivery per presentation

Closing a presentation — by close, quit, session end, or from `UiRuntime`'s `Drop` — goes through one
delivery in `flui-runtime`:

1. it claims the presentation's delivery (`LifecycleSource::claim_close_delivery`); a second claim
   while one runs or after one ran gets `None` and does nothing, so a nested pump inside a `Detached`
   observer and the repeat from `UiRuntime`'s `Drop` are no-ops. The token records whether the
   delivery completed or was interrupted (dropped by a panic);
2. a focused text field commits its composition in place, in its controller (the same step its
   `dispose` takes, which is then a no-op);
3. `begin_close`, then `Detached` to observers while the tree is alive; the application may still
   publish to the registry here.

A panic in steps 2 and 3 while the last UI runtime closes is terminal (ADR-0258 §2).

### §7. Application code runs only in a UI runtime that is in its slot

Framework code reached while a UI runtime is checked out (a session message pumped inside one of its
operations) touches only state outside it: the flush registry, the host's map, the platform. It
records what the UI runtime must do in its FIFO, and that runs when the UI runtime is back.

### §8. One containment for the application code teardown calls

Teardown calls application code at several points: the close handler, `Detached` observers,
`CloseChanged` wakers, the registry's writer, the destructors of captures. Each is called with no
lock or `RefCell` borrow held, through one containment helper: the call is caught, the payload and
the remaining values are retained per ADR-0127, the first failure stays first, and the failure is
reported as `CallbackPanic` through `UiRuntime::report_contained_panic`. No point writes its own
catch. The contract leaves room for it: `FlushWriter` hands a refused write back instead of dropping
it, `CloseGuardSource` takes its `OwnerNotify` and calls it outside its state, and
`report_contained_panic` reads the payload by reference and leaves its ownership to the boundary.

### Consequences

- Good, because a refused close is finished by whatever refused it, and a window never stays open
  because a write that has completed forgot to say so.
- Good, because what the application persisted is written at exit and at a session end without
  running its code, whatever state its UI runtime is in.
- Good, because a cancelled log-off and the user's close button do not erase each other.
- Bad, because a document's write is visible to the application only through the registry's state
  and its `changed` future, not as a future the document awaits.
- Bad, because a close recorded for a reason the platform later stops letting the application
  refuse is carried out with whatever was published: a hold cannot outlive a session that is
  ending.
- Neutral, because `CloseRequestHandler` stays synchronous; the asynchronous part is the guard.

### Confirmation

- §2, §3 (`crates/flui-view/tests/lifecycle_tests.rs`): `a_hold_and_a_decision_refuse_by_reason`,
  `a_repeated_request_records_one_close`, `releasing_the_last_hold_queues_one_close`,
  `a_recorded_close_runs_when_the_last_hold_goes_unless_kept_open`,
  `discarding_a_recorded_close_makes_it_due`, `a_cancelled_logoff_withdraws_only_the_session_entry`,
  `an_undelivered_signal_keeps_the_due_close`, `a_close_change_wakes_on_the_owner_turn`.
- §1, §4: `a_close_request_carries_its_reason`, `a_vetoed_close_disposes_nothing`, and the existing
  `a_panicking_handler_vetoes_and_stays_registered`.
- §5: `a_publishers_own_line_rebases_and_another_conflicts`,
  `a_finished_write_wakes_changed_on_the_owner_turn`,
  `a_registry_entry_published_at_detached_is_written_at_teardown`, and the persistence feature's
  `session_end_during_a_write_writes_the_latest_registry_bytes`.
- §6: `close_delivery_is_claimed_once_and_records_how_it_ended`, and
  `close_delivery_is_idempotent` (a nested delivery from a `Detached` observer and the repeat from
  `UiRuntime`'s `Drop` deliver once).
- The rows above are `#[ignore = "contract: …"]` until the code is wired; each counts once its
  `ignore` is removed. `xtask globals`: the registry and the guards add no `static`.

## Pros and cons of the options

### 1. A close guard with records by reason, and a host-owned flush registry

- Good, because the record of a refused close outlives the request that refused it, and its reason
  keeps a log-off apart from the user's close.
- Good, because the host can write persisted state without the UI runtime.
- Bad, because there are two owner-settled states (the guard and the registry) and a signal channel
  for them.

### 2. A dirty flag and a write loop in the `AsyncDriver`

- Good, because it is small.
- Bad, because a flag records no reason and no request: a `KeepOpen` still leaves nothing to finish
  the close.
- Bad, because the write loop dies with the UI runtime, which at loop exit is destroyed before the
  services stop, and cannot run while a session message has the UI runtime checked out.

### 3. An asynchronous close handler

- Good, because the application writes "save, then close" as one function.
- Bad, because the platform must answer a close synchronously (`WM_CLOSE`, `windowShouldClose:`),
  so the await becomes a refusal plus a later close anyway, and the frame path stays synchronous.
- Bad, because a session end gives no time to await application code.

## More information

The persistence feature consumes this contract: `hold` while a write is in flight,
`require_decision` on a failed, conflicting or read-only document with edits, `discard_and_close`
and `stay_open` as the user's answers. Revisit §2's strengths if a platform offers a close that can
wait for some work and not other.
