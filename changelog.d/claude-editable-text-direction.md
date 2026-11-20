### Fixed

- `EditableText` lays its line out in the ambient `Directionality`, as `Text` without a direction does: in a right-to-left field the typed text has a right-to-left base, starts at the right edge, and the caret, selection and input-method rects follow. A direction change relays the mounted field out. Its selection handles and magnifier are built for the field's direction rather than the overlay's.

### Added

- `RenderEditable::set_text_direction`.
