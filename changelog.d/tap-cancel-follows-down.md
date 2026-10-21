### Added
- `GestureDetector::on_tap_down` and `GestureDetector::on_tap_up`, with the contact named in `TapDownDetails::pointer` and `TapUpDetails::pointer`.

### Changed
- `on_tap_cancel` fires only for a press whose `on_tap_down` was reported, so a down-minus-cancel count never goes negative after a scroll takes the contact. A contact lost before the press showed reports nothing. This applies to `TapGestureRecognizer` and `GestureDetector`.
- `TapDownDetails::new` and `TapUpDetails::new` take the contact's `PointerId`.
- `InkWell` starts its wave at the tap's down instead of the raw pointer down, so a second contact beside a pending tap grows no wave.
