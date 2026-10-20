### Added

- **`flui-widgets`**: the nested scroll channel
  ([ADR-0209](/docs/adr/ADR-0209-nested-scroll-connection.md)). `NestedScroll` puts a
  `NestedScrollConnection` (`sequence_started`, `pre_scroll`, `post_scroll`, `released`,
  `post_fling`, `sequence_ended`) into the chain of every scrollable beneath it; steps carry their
  `InputSequenceId`, `ScrollCause` and `ScrollDevice`, and sequences end with a `ScrollEndReason`.
  `Scrollable::overscroll_behavior` (`OverscrollBehavior::Contain` keeps a remainder) and
  `Scrollable::snap_policy` (`SnapPolicy`, `PageSnap`) are new; `ScrollStep::for_test` and
  `FlingStep::for_test` sit behind the `testing` feature.
- **`flui-interaction`**: `GestureArena::dispatched_sequence` names the sequence of the input being
  dispatched (a contact's, a wheel latch's or an assistive action's),
  `GestureArena::begin_contactless_sequence` and `GestureBinding::dispatch_contactless` issue and
  publish contactless sequences; `DragDownDetails`, `DragStartDetails`, `HoldDetails` and
  `ContactSnapshot` carry the contact's `sequence`, and all but `HoldDetails` its `device`.
- **`flui-widgets`**: `Scrollable` scrolls by the keyboard when it, or something inside it, has
  the focus: arrows along its axis, PageUp/PageDown, Space/Shift+Space, Home and End, each press a
  keyboard sequence in the nested scroll chain; `Scrollable::autofocus` takes the focus on mount
  and a drag on a scrollable focuses it. It is not a Tab stop.
- **`flui-testing`**: `TrackpadScript::momentum` scripts platform inertia after a pan.

### Changed

- **`flui-widgets`**: every offset change of a `Scrollable` — drag, wheel, assistive action, fling,
  controller jump or animation, reveal — is a step through its nested scroll chain, and its drag
  runs on its own axis, so a scrollable inside one on the other axis shares a contact's motion by
  axis. A drag that starts at an inner list's edge moves the enclosing list for the rest of the
  contact.
- **`flui-interaction`**: a wheel or trackpad latch ends after the preset's
  `contactless_latch_idle` (1500 ms by default, was 500 ms), and a phased latch that never sent
  its end phase after `phased_latch_idle`.
