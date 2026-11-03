### Added

- `flui_foundation::panic::commit` and `commit_into` change an owner's state in a `RefCell` and
  run the user code the change owes (`Outbox::call`, `Outbox::wake`, `Outbox::release`) after
  the borrow is released, each item contained and the first failure resumed after every item ran.
  A queued call is an `FnMut` called at most once, so its captures are destroyed after it,
  contained, even when it panics. A mutation that panics runs none of its calls but still wakes
  its wakers (ADR-0287).
- `flui_foundation::panic::Obligation` claims an owed delivery from a `Cell<bool>` and sets it
  again unless the delivery calls `discharge`, so a failed or abandoned delivery is owed on the
  next turn (ADR-0287).
- `flui_foundation::TicketSource` and `Ticket` match an asynchronous answer to the request that
  asked for it: an answer is accepted only for the current ticket, once, and a write or a newer
  request makes older tickets stale (ADR-0287).
