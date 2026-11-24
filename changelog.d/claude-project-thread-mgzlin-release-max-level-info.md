### Added

- The `release-max-level-info` feature of `flui`: a release build holds no `trace!` or `debug!`
  event, the framework's or the application's own. `flui create` writes it into an application's
  manifest, which makes a release counter 0.72 MB smaller on Linux x86_64 and 0.41 MB smaller on
  wasm32; an existing project adds it to its `flui` dependency by hand.
- A managed application warns when its log filter (`RUST_LOG` or the diagnostics profile) selects
  a level the build compiled out, instead of delivering nothing without a word
  (`flui_log::filter::compiled_out`, `ResilientSetup::compiled_out`).
