### Added
- A precision-touchpad pan now drives a `GestureDetector`'s pan and drag slots, and a `Scrollable` inside a draggable is ordered against it by the same ownership table as a touch drag. A scrollable with nowhere to go on its axis sits the contest out.
- `RenderObjectContext::{declare_scroll_contact_feed, withdraw_scroll_contact_feed, declare_scroll_target_axis, withdraw_scroll_target_axis}` and `GestureRecognizer::feeds_scroll_contact`.

### Changed
- `InteractionDispatchHandle::declare_scroll_target_axis` takes a `can_scroll` probe.
