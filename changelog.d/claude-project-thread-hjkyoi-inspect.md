### Changed

- `flui_foundation::inspect::Value::Debug` holds an `Untyped`, built by the deprecated
  `Untyped::new`: a field described only by its `Debug` text is an
  `#[expect(deprecated, reason = "…")]` at its site, which the compiler reports once the site
  is gone (ADR-0303 §7). `#[inspect(debug)]` on a derived field is unchanged.
