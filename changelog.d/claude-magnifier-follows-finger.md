### Changed

- A handle drag's magnifier, in a `SelectionArea` or an `EditableText`, follows the finger between characters along the dragged end's line instead of jumping to the end at each step. Each step of the end over a character plays `HapticFeedback::SelectionClick` with the frame that shows it.
