# FLUI CLI architecture

`flui-cli` owns command parsing, project template selection, and generation of
consumer dependency declarations. `flui-build` owns platform scaffolding and
build orchestration. Template dependency resolution stays in the CLI rather
than adding framework-installation knowledge to a platform scaffold.

## Mapping decisions

### Local source dependencies are explicit Cargo paths

`flui create NAME --local` selects the current working directory as the source
checkout. `--local=PATH` selects another checkout. The path belongs to the source
framework; the independent `--path` option chooses the application's destination.
Using `=` for an explicit source preserves the unambiguous positional name in
`flui create --local NAME`.

All templates use the same validated dependency source. Resolve the source to
an absolute path and validate its required manifests before writing a project.
TOML serialization handles path escaping; paths that cannot be represented in a
UTF-8 Cargo manifest are errors rather than silently altered filenames.
Hot-reload host, worker, and types crates use that same source; their sibling
dependencies remain relative within the generated workspace.

This follows [Cargo path dependency semantics](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-path-dependencies)
and [Clap's optional-value support](https://docs.rs/clap/latest/clap/struct.Arg.html#method.default_missing_value),
consulted on 2026-09-19. It replaces fixed `../../crates` paths, which coupled
consumer location to the framework checkout. A compiled-in developer path or
implicit global SDK discovery would make an installed CLI depend on hidden
machine state. Explicit local paths are predictable for developers and agents.

The tradeoff is deliberate: moving the generated application is supported,
while moving the source checkout requires updating the dependency paths.
Registry generation remains the distribution path for published versions.
CLI integration tests cover external destinations and compile all three template
shapes; source-validation failures must leave no partially created application.

### Applications depend on the facade

Basic, counter, and hot-reload templates declare only `flui` as a framework
dependency. Generated imports use its public prelude and modules. A local facade
dependency points at the checkout root; internal crate directories stay an
implementation detail. The root package and required source manifests are still
validated before output creation.

Hot-reload members enable the facade's `hot-reload` feature, which exposes
`flui::hot_reload` and activates the application's reload driver. Sibling app
crates retain relative dependencies. The worker deliberately receives the full
facade dependency graph: this increases its compile graph but keeps application
authoring consistent with the host and ordinary projects. The development-only
reload dependency remains absent from the normal graph when that feature is off.
The generated worker uses the hygienic `hot_reload_worker!` macro; this change
does not alter the separate scene/plugin macro contracts.

This follows the application-facing entry points demonstrated by
[Iced's widget module](https://docs.rs/iced/latest/iced/widget/) and
[Dioxus's prelude](https://docs.rs/dioxus/latest/dioxus/prelude/), consulted on
2026-09-19. Template integration tests assert exact framework dependency names,
local root paths and reload feature selection, then compile every shape outside
the repository.
