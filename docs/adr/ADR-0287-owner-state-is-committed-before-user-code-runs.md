# ADR-0287: Owner state is committed before user code runs

- **Status:** Accepted (2026-11-03). Partly implemented: `flui-foundation` has the primitives and
  the capability seam's `Completion` uses them; each other seam moves to them in its own change
  (see Migration).
- **Date:** 2026-11-03
- **Related:** [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)
  (what a destructor that panics costs, and the containment after a caught failure);
  [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (where a contained user panic
  ends up); [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) §4 (`IdMint`, which the
  ticket source issues from)

## Context and problem statement

An owner (a clipboard host, a persisted document, a selection area, a capability seam) changes
its state and calls code it does not own in the same operation: a listener, a waker, the `Drop`
of a value it replaced, a `tracing` event. Each owner orders those calls by hand. A review of
about 400 findings on 103 pull requests (October 2026) put roughly a third of them in three
classes that come from that order:

| Class | Share | Example |
|---|---|---|
| A panic in the middle of an operation leaves the state half done | ~18% | a frame hook panics and a `watching` flag stays set, so later edits are never saved |
| User code re-enters while a borrow or lock is held | ~6% | a waker cloned under a mutex; the `Drop` of an old value under `borrow_mut` |
| Accepted work or an obligation is lost on a failure path | ~8% | a re-arm only when the inbox was not empty; a flag cleared before the delivery that failed |
| A late answer is taken for the answer to a newer request | part of ~12% | a read issued after a write joins the read issued before it |

Each fix closed one call site, and the next review found its neighbour. The workspace has about
670 hand-written `catch_unwind` calls outside tests, most of them in owners that order this
themselves.

How does an owner change its state and call user code so that the four classes cannot be
written?

## Decision drivers

- Rules are types (AGENTS.md): the order has to be the only one an owner can write, not a
  convention review checks at each site.
- A borrow or lock is never held across user code, and user code never sees half-changed state.
- Accepted work is delivered or refused on every exit, a panic included (AGENTS.md, "Recurring
  defects").
- The first failure of an operation propagates; delivery to the rest continues (ADR-0281 §4).
- An asynchronous answer is matched to its request by a ticket, not by "the current one".
- The frame path is synchronous and owner-local: no `Send` bound, no lock, no allocation per call
  beyond the queued work itself.

## Considered options

1. Keep the per-site discipline, documented in a checklist and enforced by review.
2. A queue of deferred user code filled while the state is borrowed and run once the borrow is
   released (`Outbox`), with a guard that puts an owed delivery back unless it completed
   (`Obligation`), and an opaque ticket for asynchronous answers (`Ticket`).
3. An actor per owner: every operation is a message, and user code runs only between messages.

## Decision

Chosen option: 2, because it turns the order into the only way the closure can be written: the
state is reachable only inside `mutate`, user code is reachable only through the queue, and the
queue runs after the borrow ends.

§1. **Commit, then call.** `flui_foundation::panic::commit(&RefCell<S>, mutate)` borrows the state
mutably and runs `mutate(&mut S, &mut Outbox)`. `Outbox` has no public constructor, so user code
an owner wants to run is queued, never called, while the state is borrowed: `Outbox::call` (a
`'static` `FnMut` called at most once, so its captures outlive the call and are destroyed after
it, contained, a panic included, never during its unwind: ADR-0281 §3), `Outbox::wake` (a
`Waker`, woken by value) and `Outbox::release` (a value whose destructor is user code, such as a
replaced listener). After `mutate` returns, the borrow is
released and the queue runs in order, each item contained (`FirstPanic`); the first panic is then
resumed, after every item ran; while the thread already unwinds (a `commit` run from a `Drop`), it
is discarded behind that unwind (ADR-0281 §4). A queued call may commit on the same cell again:
its own queue runs before the rest of the outer one. `commit` on a cell that is already borrowed
panics with `BUG:`, and the borrow it was refused does not change.

§2. **A failed mutation still wakes.** When `mutate` panics, its queued calls do not run, since
they announce a change that did not complete; its released values and calls are destroyed, each
contained; its queued wakers are still woken, since a spurious wake costs a poll and a lost one
hangs a task. The mutation's panic is then resumed: it is the operation's first failure.

§3. **Into an operation's recovery.** `commit_into(&RefCell<S>, &mut PanicRecovery, mutate)` does
the same inside an operation that already owns a failure context: each failure goes into that
recovery, so an earlier failure stays first, and it returns `None` when `mutate` panicked.

§4. **An owed delivery is put back unless it completes.** `Obligation::take(&Cell<bool>)` claims a
set flag (`Some`, and the flag is cleared) or finds nothing owed (`None`). The obligation is
`#[must_use]`; `discharge()` ends it. Dropped without `discharge`, after a panic or an early
return, it sets the flag again, so the next turn delivers again. Because the flag is cleared when
the delivery starts, an obligation raised again during the delivery stays raised after
`discharge`: an older delivery cannot clear a newer obligation.

§5. **An answer carries its ticket.** `TicketSource<K>` issues `Ticket<K>`s from an `IdMint`
(increasing, never reissued; exhaustion is `IdExhausted`, a refusal the owner reports). The last
ticket issued is the current one. `is_current(ticket)` answers whether an answer still belongs
to the latest request; `redeem(ticket)` accepts it once and leaves no ticket current;
`invalidate()` makes every outstanding ticket stale (a write, a focus move, an owner retiring).
`Ticket<K>` is opaque: no accessor exposes its number, and the kind `K` keeps the tickets of two
kinds of request apart at compile time.

### Consequences

- Good, because an owner written with `commit` cannot hold its borrow across user code, cannot let
  user code see half-changed state, and cannot drop a queued wake on a failure path.
- Good, because the first-failure rule, the delivery-continues rule and the containment of each
  destructor (ADR-0281 §3) live in one function instead of in each owner.
- Good, because a stale answer is a type-checked comparison, not a reasoning step in review.
- Bad, because each queued call allocates one box. The owners that queue on every frame keep a
  hand-written path until a measurement shows the box matters.
- Bad, because a queued call runs after the mutation, so a listener that needs to veto the change
  cannot be one; a veto stays a synchronous call made before `commit`, with the state not yet
  changed.
- Neutral, because state behind a `Mutex` on a `Send` path is not covered yet; a seam that needs it
  adds the guard-taking variant when it migrates.

### Confirmation

- `Outbox` has no public constructor and `commit` takes the cell, not a borrow, so state and user
  code cannot meet in one scope through this API (compiler).
- The table test `commit_then_call_contract` in `flui-foundation` (`tests/commit.rs`) covers each
  rule above through the public API: the borrow is released before a queued call, a re-entrant
  call commits again, the first failure propagates after every item ran, a failing call's captures
  are destroyed after its panic is caught, a queued failure during an unwind is discarded behind
  it, a failed mutation wakes and runs no call, a re-entrant
  `commit` panics with `BUG:`, an obligation dropped by a panic is put back and one raised during
  a delivery survives `discharge`, a redeemed ticket is not accepted twice and an invalidated one
  is stale.
- Each migrated seam deletes its hand-written ordering in the same change. Once the first seam
  has moved, a `cargo xtask` gate counts the remaining `catch_unwind` calls in owner code, and the
  count only goes down.

## Pros and cons of the options

### 1. Per-site discipline

- Good, because it adds no type and no allocation.
- Bad, because it is the state the review data measured: each site is right or wrong on its own,
  and the class keeps producing findings until the last site is rewritten.

### 2. Commit-then-call primitives (chosen)

- Good, because the order is the shape of the API, and the remaining rule (what to queue) is local
  to the closure.
- Good, because the primitives are owner-local and synchronous, like the frame path.
- Bad, because a closure per call allocates, and the migration is one seam per change.

### 3. An actor per owner

- Good, because no state is shared, and every operation is ordered.
- Bad, because every call becomes a message, which costs a queue hop on the frame path and turns a
  synchronous answer (a hit test, a layout query) into a deferred one.
- Bad, because a re-entrant call from user code is a message to the owner already running, which
  needs its own rule for delivery order.

## Migration

The seams migrate one per change, the costliest first by the review data: the clipboard owners,
`Persisted`, `SelectionArea` and the capability seam. Each change replaces the hand-written
ordering of one owner and keeps its tests; a seam whose tests named an order this ADR changes
says so in its changelog fragment.

The clipboard's reads settle through the capability seam's `Completion`, so it moved first: its
owner-side state is one `RefCell` changed through `commit` wherever the change runs or destroys
user code (the waiting closure, a waker). An owner whose state lives on its own thread behind
`&mut self` (the X11 selection thread) has no borrow to release; it keeps `FirstPanic` for its
fan-out.

## More information

- The review measurement behind the Context is a project retrospective kept outside the
  repository.
- Revisit §1's allocation if a frame-path owner shows the boxed call in a profile; a small inline
  queue of typed events is the next step, not a return to hand-written order.
