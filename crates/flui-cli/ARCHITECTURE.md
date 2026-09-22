# FLUI CLI architecture

`flui-cli` owns command parsing, project template selection, generation of
consumer dependency declarations, platform scaffolding and build
orchestration (`src/build/`), and the dev-loop source watcher
(`src/watch.rs`). It links no FLUI crate: the runtime half of hot reload
lives in `flui-hot-reload`, which the *app* links, and the only contract
between the two is a pair of environment-variable names pinned by a
dev-dependency test.

## One output policy, no logging framework

Everything the CLI prints goes through `ui::`. Human narration is styled
text on stderr; `--json` is NDJSON on stdout; warnings and errors survive
`--quiet`; diagnostics (the commands run, the probes made, the paths skipped)
are `ui::debug` lines shown under `-v` only. There is no `tracing`
subscriber and no `RUST_LOG`: with no framework crate in the graph, a log
filter would have nothing to select but the CLI's own lines, and a second
voice on stderr would compete with the narration. `flui-log` is the
application's logging backend, not the CLI's.

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
SDK-required build-pipeline tests (`src/build/tests/`) verify actual application bundle delivery.

### One output policy, two audiences

Every command narrates through `src/ui.rs`, never through `cliclack` directly.
Human text goes to **stderr**; stdout is reserved for payloads a user would
pipe (completion scripts) and, under `--json`, for one NDJSON object per line
with an `event` discriminator (`doctor.check`, `device`, `run.app.log`, …).
`--quiet` drops narration but keeps warnings, errors and tool output. The policy
is a process-global `OnceLock` because a CLI has exactly one terminal.

This follows Flutter's `--machine` and Dioxus's `--json-output`, with two
deliberate differences: every listing and the dev loop
emit JSON, not only a daemon, and JSON mode never mixes human text into stdout.
The alternative — a `Context` parameter threaded through every helper — was
rejected as pure noise for commands that are free functions.

### Interactivity is decided once, up front

`ui::is_interactive()` requires a terminal on stdin and stderr and none of
`CI`, `FLUI_NON_INTERACTIVE`, `--non-interactive`, `--json`. Anything that
would prompt (the `create` wizard) or read hot-keys (`run`) consults it and
fails with `CliError::NonInteractive` (exit 7) and a hint instead of blocking.
A CI job that forgot the project name gets a one-line fix, not a hung runner.

### Exit codes are a contract

`CliError::exit_code` maps every variant to a documented table (0 success,
2 usage, 3 environment, 4 build/test failed, 5 device not found, 6 not a FLUI
project, 7 needs a terminal, 130 interrupted). Scripts branch on the code;
the text is for people. Flutter documents only 64/1; `dx` and `tauri` document
none.

### External tools run with a deadline

`src/proc.rs` bounds every environment probe. `flui devices` once hung forever
on macOS because `Safari -v` launches Safari rather than printing a version;
browser versions are now read from `Info.plist` and every probe is killed at
its deadline and reported as a timeout. A `Problem` row, not an error, is the
result of a missing or hung tool: discovery never fails the command.

### The dev loop multiplexes four sources

`flui run` drives one `dev_loop` over a `ReloadStrategy` (process restart, or
worker host for the Flutter-parity layout). The loop polls, in priority order:
the Ctrl-C flag (set by a `tokio::signal` listener on its own thread), hot-keys
from a `console::Term::read_key_raw` thread (`r`/`R`/`c`/`h`/`q`), the child's
exit status, and the debounced source watcher. The child never shares stdin
(the key reader owns the terminal), and in `--json` mode its stdout is piped
and forwarded as `run.app.log` so the machine stream stays pure. The child is
stopped on every exit path, including errors, so a failed `flui run` never
leaves an orphaned app; a real SIGINT with nobody at the keyboard exits 130.

## The build pipeline is a module, not a crate

`src/build/` has one consumer, this binary, and no framework dependency, so
it is a module rather than a crate: every builder, scaffold and cargo helper
in it exists because a command calls it. Its artifact-selection tests live
in `src/build/tests/` as unit modules; they build real Cargo fixtures and
re-execute the test binary as a worker.

## The watcher is dev-machine code

`SourceWatcher` (`src/watch.rs`) watches files on the developer's machine;
nothing in a running app does. Keeping it here is what lets
`cargo install flui-cli` compile no framework code: the runtime half of hot
reload, `flui-hot-reload`, is linked by the app, and the two halves share
only a pair of environment-variable names.
