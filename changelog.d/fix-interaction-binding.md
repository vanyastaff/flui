### Fixed

- **`flui-interaction`**: a wheel packet that arrives while its pointer holds a button belongs to
  its wheel latch everywhere: `GestureArena::dispatched_sequence` names the latch rather than the
  held contact, and `GestureArena::dispatched_latch_end` reports when the latch ends, so a scroll
  consumer ends its sequence with the latch. Haptics requested inside
  `GestureBinding::dispatch_contactless` carry that contactless sequence.
- **`flui-interaction`**: a binding dispatch nested in a pointer callback no longer sends the
  outer contact's `Cancel(Claimed)` to route targets the outer packet has yet to reach: a
  contact's claim settles once its own packet reached its whole route, so a contact claimed and
  ended inside a nested dispatch still cancels the rest of its route.
- **`flui-interaction`**: a packet or `Revise` from another device under an active contact's
  pointer id is refused, without reaching the contact's listeners or reporting its sequence from
  `GestureBinding::dispatch_pointer_on`.
- **`flui-interaction`**: a gesture or haptics deadline past the last instant the platform's
  clock represents schedules no host wake, instead of a wake at the clock's epoch that fires at
  once and again on every turn.

### Removed

- **`flui-interaction`**: the crate root no longer re-exports `ScrollSignal`, `ScrollDispatch` and
  `ScrollSequenceEnd`, which are gone; `ScrollCause`, `ScrollDevice` and `ScrollEndReason` are
  named from `flui_widgets`. `RoutePanic`, which `InteractionDispatchHandle::invoke_pointer_route`
  returns, is named `flui_interaction::pointer::RoutePanic`.
