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


### Default counter state and interaction

The counter template uses a stateless application shell with a stateful counter
beneath a Material theme. The element owns `Rc<Cell<usize>>`; `init_state`
acquires its rebuild handle. The button callback changes the value and schedules
a state rebuild, while `build` only describes the current view. This follows
[Iced's owned-state, event, and state-derived-view counter example](https://book.iced.rs/first-steps.html)
(consulted 2026-09-19) through FLUI's retained element lifecycle rather than
introducing a message architecture.

The generated test dispatches real pointer down/up events to the Increment label
and advances only scheduled work with `tick`, checking rendered text from 0 to 1
to 2. A parent rebuild preserves 2; a separate app starts at 0. The external CLI
regression executes that named generated test and checks a normal build whose
dependency graph excludes `flui-testing`. The sole framework dependency remains
`flui`; its `testing` feature is enabled only in development dependencies.


### Run admission uses Cargo package identity

`flui run` asks `cargo metadata --no-deps --format-version 1` for the current
manifest and selects that package by canonical manifest path. Cargo resolves
renamed dependencies and workspace inheritance; a normal declaration of `flui`
identifies an application. Legacy `flui-app` and `flui-widgets` declarations
remain accepted. Comments, similarly named packages, dev/build dependencies and
unrelated workspace members cannot admit a project. A virtual workspace root
needs an application package directory, except for the existing configured
worker hot-reload path. Metadata is declaration evidence; Cargo's subsequent
run still determines whether the selected application builds and executes.

This follows [Cargo's metadata contract](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html).
Integration fixtures use real Cargo metadata and marker-printing binaries with
minimal local dependency identities. They verify admission and execution without
opening a window; they do not substitute for the generated application's own
interaction regression or live platform verification.

### Native iOS selection and launch

`build ios` means a native executable application; `--lib` opts into static-library
XCFramework delivery. Exact simulator UDIDs are resolved before Cargo. The booted
device's `SIMULATOR_ARCHS` must be one recognized architecture, consistent with
runtime supportedArchitectures when present. There is no host-run fallback.
Commands have deadlines and drain both output streams. Install and launch use
the validated bundle's identity and selected UDID, without shutting down devices.
Config SemVer maps to numeric Apple keys while the full value remains in FLUIVersion.
The unavailable-device integration test guards against accidental host execution;
SDK-required flui-build tests verify actual application bundle delivery.
