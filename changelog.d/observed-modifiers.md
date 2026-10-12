### Fixed

- Preserve reported Web keyboard, pointer and wheel modifiers, including AltGraph
  and lock states, without rewriting raw source flags.
- Interpret explicit AltGraph glyphs consistently in text fields, shortcuts and
  Slider controls while retaining Meta/Shift and produced-character bindings.
- Leave source-composing keys to the input method instead of editing, submitting
  or invoking ordinary shortcuts; keep Slider navigation usable with lock states.
