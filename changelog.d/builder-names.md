### Changed

- A description's builder method has the parameter's bare name (ADR-0325): `AppConfig`'s
  builders drop `with_` (`.title(…)`, `.size(…)`, `.dev_reload(…)`, `.storage_dir(…)` and the
  rest), `Application::with_config` is `config` and `with_startup_window` is `startup_window`, and
  `OverflowBox`, `SizedOverflowBox` and `IntrinsicWidth` take `.alignment(…)`, `.max_width(…)`,
  `.step_width(…)` and their siblings.
- `RouteSettings` is read by its fields: `name` and `arguments` are `pub` on a `#[non_exhaustive]`
  type, the `name()` and `arguments()` getters are removed, and `with_arguments` and
  `with_arguments_shared` are `arguments` and `arguments_shared`.
