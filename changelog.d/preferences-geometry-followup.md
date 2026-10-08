### Fixed
- Derive default editable caret height from shaped text, including inherited
  text scaling and empty fields, while retaining explicit logical overrides.
- Keep inherited device-pixel ratios consistent with the renderer at startup
  and reject invalid direct or native updates before publishing them.
