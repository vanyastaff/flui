### Changed

- `FilterConfig` is read by its fields (ADR-0325): `directives` and `env_var` are `pub` on the
  `#[non_exhaustive]` type, the getters of those names are removed, and `env_var` replaces the
  `with_env_var` builder.
