### Security

- A release build of `flui build` (every platform it builds) no longer writes the build machine's
  directories into the binary: the source paths rustc records for panic locations name a registry
  crate, a path dependency or a vendored source outside the workspace as `<name>-<version>/…` and
  a generated file as `target/…` or `build/…`, through `--remap-path-prefix` joined to the
  project's own rustflags. A debug build keeps the full paths.
