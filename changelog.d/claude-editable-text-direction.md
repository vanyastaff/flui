### Fixed

- `EditableText` lays its line out in the ambient `Directionality`, as `Text` without a direction does: in a right-to-left field the typed text has a right-to-left base and starts at the right edge, and the caret, selection and input-method rects follow. A direction change relays the mounted field out.
- In a right-to-left field Left and Right step the caret, and with Shift the selection, toward the side of the line they name: Left toward the text's end, Right toward its start.
- A field's selection handles and magnifier are built for the field's direction rather than the overlay's, and a shown handle is built again when that direction changes.

### Added

- `RenderEditable::set_text_direction`.
