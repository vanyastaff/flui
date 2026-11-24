### Changed

- `DynLib::open`, `ScenePlugin::load` and `WorkerPlugin::load` in `flui-hot-reload` return
  `Result<_, LoadError>` instead of `Option`. `LoadError` says why a plugin library was refused:
  not built yet, the platform loader's own text (`dlerror` or the Windows message), no plugin
  symbols, a missing symbol, an ABI-token mismatch with both tokens, or a worker init that
  panicked. The reload hooks log each refusal once per build of the file, at `warn`, where the
  reason used to go to `trace` or be lost.
