### Changed

- **`.gitignore`** lists what the repository's builds, tests and documented workflows write,
  a secrets set, and the common editor and OS files. One contributor's tool state (an agent
  runtime, an MCP server) goes in `.git/info/exclude` or user-level configuration, as
  [`CONTRIBUTING.md`](/CONTRIBUTING.md) now says.

### Removed

- **`tools/web-server`** (`flui-web-server`): `flui run --device browser:<id>` builds, serves
  and reloads a project `flui create` generates; the wasm-pack demos under `examples/` are
  served with any static HTTP server.
- **`examples/android_demo`, `android_scene` and `android_app`**, which no command could build.
  An Android app is a project `flui create` generates, built with `flui build android` and run
  with `flui run --device <id>`.
- **Per-crate `CHANGELOG.md` files** of flui-animation, flui-engine, flui-interaction,
  flui-painting, flui-scheduler, flui-view and flui-widgets: the root changelog is the record
  (flui-cli keeps its own, which its release archives ship).
- **The root `platforms/` tree**, a Flutter project template with committed Gradle output that
  nothing built, and the project-wide `.mcp.json` and `.lsp.json`.
- **Historical planning records**: `.rust-studio/`, `docs/{archive,audits,brainstorms,ideation,superpowers}`,
  `specs/`, finished design documents, and the dated plans and research no live document relies
  on. Git history keeps them, and the references that remain point at a permalink.
