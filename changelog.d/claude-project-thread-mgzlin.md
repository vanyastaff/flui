### Security

- A release build of `flui build` (every platform) no longer writes the build machine's directories
  into the binary: the source paths rustc records for panic locations name a registry crate or a
  path dependency as `<name>-<version>/…` and a generated file as `target/…`, through
  `--remap-path-prefix` joined to the project's own rustflags. A debug build keeps the full paths.
