### Changed

- A `flui create --hot-reload` project's host links the logic crate by default (its `linked`
  feature): a release build calls the build function directly, loads no worker library, installs
  no reload hook and needs no library beside the binary. `flui run` builds the host with its
  `reload` feature and its default features off, as `flui.toml`'s new
  `[hot_reload] reload_feature` names it. The logic crate builds as `cdylib` and `rlib`, and
  `CounterApp` carries where its build function comes from (`CounterBuild`).
