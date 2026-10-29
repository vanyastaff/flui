### Added

- Semantics nodes publish `aria-current` (`Current`), `aria-haspopup` (`HasPopup`),
  `aria-autocomplete` (`AutoComplete`), `aria-orientation` (an `Axis`), keyboard shortcuts,
  language, URL and role description, each with a `Semantics` builder method.
- `SemanticsRelation` gains `DescribedBy`, `Details`, `FlowTo`, `ErrorMessage` and
  `ActiveDescendant` (ADR-0263), with `Semantics::described_by`, `details`, `flow_to`,
  `error_message` and `active_descendant`.
- `NumericRange::with_jump` declares a range's large step, published as AccessKit's
  `numeric_value_jump`.
- `SemanticsRole::Generic` and `SemanticsRole::Presentation`: a node declared to mean nothing,
  published as a container whose label never makes it static text.

### Changed

- `SemanticsConfiguration::set_invalid` and `Semantics::invalid` take `Invalid` (`False`, `True`,
  `Grammar`, `Spelling`); a declared `False` counts as a declaration when configurations merge.
  `is_invalid()` is replaced by `invalid() -> Option<Invalid>`.
- `NumericRange::new` is renamed `NumericRange::try_new`; `NumericRangeError` is
  `#[non_exhaustive]` and gains `NonPositiveJump`.
