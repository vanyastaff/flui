### Fixed
- Subpixel splitting saturates extreme inputs without overflowing. Actual glyph placement omits non-finite or unrepresentable device coordinates while preserving valid vertical cancellation and ordinary hinting.
