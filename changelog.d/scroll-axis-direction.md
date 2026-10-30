### Changed

- `SemanticsConfiguration::set_scroll_axis_direction(AxisDirection)` replaces `set_scroll_axis(Axis)`, and `AxisDirection` moves to `flui_foundation::geometry` (still re-exported from `flui_rendering::constraints`). A scrollable that grows up or to the left now publishes its accessibility scroll position from the top or left edge, the order platforms read it in (ADR-0273).

### Added

- `Scrollable` takes an accessibility `SetScrollOffset` request, so a platform can scroll it to an exact offset.
