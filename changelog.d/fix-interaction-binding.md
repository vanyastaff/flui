### Fixed

- **`flui-interaction`**: a wheel packet that arrives while its pointer holds a button belongs to
  its wheel latch everywhere: `GestureArena::dispatched_sequence` names the latch rather than the
  held contact, and `GestureArena::dispatched_latch_end` reports when the latch ends, so a scroll
  consumer ends its sequence with the latch. Haptics requested inside
  `GestureBinding::dispatch_contactless` carry that contactless sequence.

### Removed

- **`flui-interaction`**: the crate root no longer re-exports `ScrollSignal`, `ScrollDispatch` and
  `ScrollSequenceEnd`, which are gone; `ScrollCause`, `ScrollDevice` and `ScrollEndReason` are
  named from `flui_widgets`. `RoutePanic`, which `InteractionDispatchHandle::invoke_pointer_route`
  returns, is named `flui_interaction::pointer::RoutePanic`.
