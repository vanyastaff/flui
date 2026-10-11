### Changed

- Tap Down callbacks deliver `TapDownDetails`; successful Up and completed-tap callbacks deliver `TapUpDetails`, including secondary and auxiliary button families. Movement and cancellation retain `TapDetails`.
- Long-press movement delivers `LongPressMoveUpdateDetails` with displacement from the admitted Down in root and local logical units. Completed release delivers `LongPressEndDetails` with finite measured local velocity at release and the admitted device kind. Import callback payloads directly from `flui::interaction` or `flui_interaction`.
