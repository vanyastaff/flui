### Added

- `InlineSpan::semantics_text_eq` and `InlineSpan::semantics_text_is_empty` answer what
  comparing or testing `to_semantics_text` would, without building the strings.

### Changed

- A paragraph whose text changes no longer builds its old and new spoken text to compare them:
  `text_edit` allocates about a fifth less per step.
