### Changed

- `flui-platform`'s descriptions take bare builder names (ADR-0325): `WindowOptions` (`.title(…)`,
  `.size(…)`, `.visible(…)`, `.mode(…)` and the rest), `OsWindowOptions::windows`/`android`,
  `Win32WindowOptions::corner_preference`/`theme`, `AndroidWindowOptions::color_mode`,
  `FrameRequest::target_present`/`presented` and `HapticRequest::source`/`sequence`/`timing`.
- `HttpRequest` is read by its fields: `method`, `url`, `headers`, `body`, `body_limit` and
  `deadline` are `pub` on the `#[non_exhaustive]` type, their getters are removed, and the
  builders are `header`, `body`, `body_limit` and `deadline`.
