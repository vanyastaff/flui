### Fixed

- Android: the input method is told the new selection and composition after each of its own edits (`updateSelection`), which keyboards rely on for suggestions and autocorrect revert.
- Android: a character the input method committed on a snapshot the owner has since replaced with a selection-only update no longer leaves the input method's text ahead of the field; the input method restarts on the field's text.
