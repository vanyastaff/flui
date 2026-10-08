### Changed
- `Locale` preserves complete language tags, including variants and extensions.
  Its component constructors now return `Result`; display and serde use a
  normalized BCP 47 tag string instead of the former three-field serde object.
  Built-in locale constructors are now `#[must_use]`.
- `MediaQueryData` includes ordered `preferred_locales`. Exhaustive struct
  literals must supply the field or use the default update syntax.

### Fixed
- Windows host language preferences reach mounted application resources through
  `WidgetsApp`, including live host publication and late runtime construction.
  Explicit application locales retain precedence. Resource fallback selects a
  supported full locale rather than forwarding an unsupported extension.
