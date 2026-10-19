### Added

- **Gesture observers** (`flui-interaction`): `GestureArena::observe` registers a
  `GestureObserver` that receives each `GestureObservation` the arena commits (a member joining,
  the contest closing, a claim, each win and rejection, a cancellation with its `ContestCancel`
  cause, an elapsed deadline) after the operation or binding dispatch that made it. Delivery runs
  in batched passes with no arena or binding borrow held; each call is panic-contained with the
  first failure reported; the returned `#[must_use]` `ObserverHandle` removes only its own
  registration and can `replace` its observer from the next observation; registration refuses
  with `IdExhausted` instead of reissuing an identity. With no observer the arena records
  nothing and allocates nothing for it.
