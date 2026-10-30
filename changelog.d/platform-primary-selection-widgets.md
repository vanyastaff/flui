### Added

- **`flui-widgets`**: `EditableText` and `SelectionArea` offer their selection as the primary
  selection on X11 and Wayland. The selection is read only when another application pastes it, a
  growing selection keeps one claim, an empty selection lets go, and an obscured field offers
  nothing. A middle press in an `EditableText` pastes the primary selection where it lands.
- **`flui-testing`**: `Harness::primary_selection`, `LaidOut::primary_selection` and
  `Harness::dispatch_pointer_down_with_button`.
