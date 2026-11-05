### Changed

- The animation controller's status ledger is the notification channel: the
  controller's own listener vector, its listener mint and its delivery turn's
  callback snapshot give way to a `Notifier<AnimationStatus>`, whose snapshot
  tickets name the committed delivery turns and whose
  `notify_selected_with_recovery` resolves them at the turn. The per-ticket
  liveness check, the delivery-time containment and the controller's first
  failure arrive with the channel; a committed status whose turn outlives a
  dispose silences there. Withdrawal keeps ADR-0180's guard: a take through the
  channel inside the state borrow, a delivering round sending the capture to the
  FIFO tail, and recovery inheritance outside one.
- Refused (disposed) status admission retires its capture through the
  controller's containment as before, now from a refusal lane of its own; the
  exhausted-identity refusal is the channel's `IdMint` contract, and the
  controller-level counter-injection identity rows end (the foundation's
  `IdMint` suite pins the channel's exhaustion). The controller's value
  refusals pin their own inert, terminal behavior in
  `disposed_value_registrations_retire_their_captures`.
