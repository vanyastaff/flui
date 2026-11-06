### Changed

- The focus node layer's listener ledgers are the notification channels: the
  node's hand-rolled listener list with its own id mint and `FocusSubscription`
  guard give way to a `ChangeNotifier`, and the manager's listener list with
  its id pair gives way to a `Notifier<FocusChange>`. The public surface
  narrows to the owned `Subscription`: `FocusNode::subscribe` and the new
  `FocusManager::listen` return it; `FocusManager::add_listener`,
  `remove_listener` and `clear_listeners` are gone. The facade stops
  re-exporting `FocusSubscription` and exports the callback alias
  `FocusNodeChangeCallback` instead. Membership snapshots, delivery ordering,
  the removed-earlier-in-dispatch skip and containment are the channels', once,
  as ADR-0314 states.
- Focus rounds join the foundation receipt: a live transition round is one
  `PanicRecovery` whose recovery scope every channel notify borrows, and the
  round finishes by resuming the first caught failure after the accepted
  reentrant queue drains. `FocusClosePanic` remains only for closed-owner
  admission refusals. A close that owes a failure omits the optional
  notifications, retires every outgoing capture under the round's containment,
  and keeps the tombstone's preserving policy through a flag bridge from the
  round's failure state — preserving both the omission an ordinary close kept
  after its first caught failure and the discarding of everything a preserving
  close catches.
