### Changed

- `flui run` of a desktop application starts the executable its build wrote, with the library
  path `cargo run` sets, instead of running `cargo run`, which checked the sources again and
  could start a build nobody reported.
- A rebuild of a desktop `flui run` that produced the build the app already runs leaves the app
  running: a save and a `flui_reload` of the same edit restart it once. Its report is
  `"unchanged"` (protocol 0.34), and the `r` key reads "Rebuild, and restart if it changed".
