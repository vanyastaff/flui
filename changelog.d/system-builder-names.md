### Changed

- `OpenRequest` and `SaveRequest` are read by their fields (ADR-0325): `title`, `directory`,
  `filters`, `accept_label` and `suggested_name` are `pub` on the `#[non_exhaustive]` types, the
  getters of those names are removed, and the builders are `title`, `directory`, `filter`,
  `accept_label` and `suggested_name`.
