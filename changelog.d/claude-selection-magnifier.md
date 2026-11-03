### Added

- A touch drag of a selection handle in a `SelectionArea` or an `EditableText` shows a magnifier over the dragged end: its bottom centre a gap above the end's line, following the end, taking no touch and hidden from assistive technology. `SelectionMagnifier` is its look, built from `MagnifierInfo`. `PlainMagnifier` gives a rounded rectangle on Android and Fuchsia, a capsule on iOS, and none on the desktops. `DefaultSelectionControls::magnifier` and `no_magnifier` set it for a subtree.
