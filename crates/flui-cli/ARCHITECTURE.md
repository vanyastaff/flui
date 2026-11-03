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

### Android runs build for the selected device's native ABI

The run session reads the selected device's ordered ABI list before invoking
cargo-ndk. It chooses the first supported ABI instead of forcing ARM64 on x86
emulators. Environment validation checks that ABI's exact Rust target in the
project's toolchain and reports its `rustup target add` command before invoking
cargo-ndk if it is missing. A failed build with hot reload disabled exits as a build failure;
there is no watcher that could repair it. `android_run_builds_the_selected_device_native_abi`
pins the target argument, missing-target admission and bounded failure through the CLI process.

Both SDK-tools APK packaging and generated Gradle projects require API 23,
matching the platform's [View.getRootWindowInsets](https://developer.android.com/reference/android/view/View#getRootWindowInsets()) safe-area capture boundary.
`apk_link_requires_supported_window_insets_api` pins the aapt2 invocation;
`scaffolded_platforms_ignore_and_clean_what_builds_write` checks the generated
application configuration through the CLI.

The Android package comes from `flui.toml`'s `[app] id` on every build, whichever packages the
APK. The SDK-tools path writes it into the manifest it links; for a Gradle project `flui build`
writes it into `gradle.properties` as `flui.applicationId` beside `flui.minSdk` and
`flui.targetSdk`, and the scaffolded `app/build.gradle.kts` reads it. A changed identifier
therefore reaches the APK that `flui run` then starts and copies scene plugins into by that same
package. A literal `minSdk` or `applicationId` in the script that differs, or one the build cannot
read, is a second statement and is refused before Gradle runs
(`a_build_script_states_what_flui_toml_states_or_is_refused`). The `namespace` stays as scaffolded:
it is the package of the app's own Java or Kotlin code.

### Manifests declare what the linked crates declare

A crate states what it needs from the application's manifests in
`[package.metadata.flui.declares]` (ADR-0307): a `schema` version, then per-platform keys, and
under `features.<feature>` what a feature adds while it is enabled. `declarations.rs` reads every
package of the graph `cargo metadata --filter-platform` resolves for each target built, with the
features that graph enables, and joins them. The table is strict: an unknown key, a schema newer
than the CLI reads, a feature the crate lacks or a malformed value fails the build naming the crate
(`a_malformed_declaration_is_refused_by_crate`). The CLI holds nobody's needs as literals:
`flui-native` declares `VIBRATE` for its haptic patterns, `flui-app`'s `http` feature declares
`INTERNET` for `HttpURLConnection` (`declarations_are_joined_over_the_graph_the_target_links`,
`the_manifest_declares_what_the_linked_crates_need`).

Android's `AndroidManifest.xml` is generated whole on every build (ADR-0255 decision 6) at
`<project>/platforms/android/app/src/main/AndroidManifest.xml`, where both packagers read it: FLUI's base
(`templates/android/AndroidManifest.xml`), the joined permissions, the label from `flui.toml` and
the application's `cdylib`. It is written only when it changed. A file there without the
generated-file header is refused untouched rather than overwritten
(`the_manifest_is_rewritten_only_when_flui_wrote_it`).

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
`flui::hot_reload`; the generated host installs its `WorkerReloadHook` with
`AppConfig::with_dev_reload`. Sibling app
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

### `flui run --agent` and `flui mcp` share a session file and a contract crate

The two commands meet at the app's driver agent (ADR-0239) through a small
credentials file, `target/flui-agent.json` (`CARGO_TARGET_DIR` honoured), and
the contract crate `flui-protocol`, the CLI's one FLUI dependency: the launch
variables and the shortest token (`launch`), the hello, the method names and
each method's parameter type.

- **Launch.** `flui run --agent` is admitted on this desktop and on an Android
  device. A debug
  build needs nothing of the application; a profile other than `dev` (whose
  debug assertions it cannot be sure of) builds with the `driver` feature of
  the `flui` facade, or of `flui-app` for an app that names the runtime crate,
  read from the same Cargo metadata that admits the project; a package with
  neither is refused. It creates a `Session`: a `0700` temp directory holding
  the socket (a pipe name on Windows), `MIN_TOKEN_LEN` random bytes as hex for
  the token, and the session file written `0600` and removed with the session
  unless a newer run replaced it. The same feature goes to `cargo build` and
  `cargo run`, or cargo would build twice. The token is in the app's
  environment and the file, never in the `--json` stream.
- **Android.** The app binds the abstract socket `@flui-agent-<hex>` (ADR-0288
  §6), which only the device reaches, so the session holds an
  `adb forward tcp:0 localabstract:<name>` and the file names that loopback
  port (`Endpoint::Forwarded`) where a desktop session names the app's own
  endpoint (`Endpoint::Local`). `am start` hands the app the endpoint and the
  token as the intent's string extras; `adb`'s connections arrive as the
  shell's user, which the app admits. The forward is removed with the session,
  after the app is stopped. A release build gets the `driver` feature through
  `cargo ndk`. The token stays out of every rendered `adb` command.
- **Serve.** `flui mcp` speaks MCP over stdio (hand-written JSON-RPC: four
  tools, no SDK, so the CLI's graph does not grow) and forwards each call as
  one protocol method, its parameters checked against the method's parameter
  type before anything is sent. It connects on the first call and reconnects
  once per call, because the app restarts under `flui run`: a request that
  never reached the app goes again, one that did goes again only when its
  method reads, since one that acts may have run. A reply is awaited on the
  connection's own thread for a bounded time, so a peer that is not an agent
  cannot hang the MCP client. Element handles are scoped to a window, so a
  call without `window` is resolved only when the app has exactly one.
- **Tests.** `agent::mcp::tests` pin the tool-to-method mapping;
  `an_agent_reads_and_taps_the_app_run_with_agent_through_mcp` runs
  `flui run --agent`, `flui mcp` and a fixture app end to end, and
  `an_optimized_agent_build_turns_on_the_driver_feature_and_a_debug_one_needs_none`
  the feature rule (Unix; the Windows pipe is covered by `flui-app`'s agent
  tests and not yet by a CLI run).
  `mcp_reaches_an_android_app_through_its_forwarded_port` drives `flui mcp`
  over a forwarded port, and
  `an_android_session_forwards_a_port_to_a_fresh_abstract_socket_and_removes_it`
  pins the `adb` calls and the intent extras with an `adb` that logs them.

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

Every listing and the dev loop emit JSON, not only a daemon, and JSON mode
never mixes human text into stdout (Dioxus's `--json-output` is the closest
prior art).
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
the text is for people. `dx` and `tauri` document no such table.

### External tools run with a deadline

`src/proc.rs` bounds every environment probe. `flui devices` once hung forever
on macOS because `Safari -v` launches Safari rather than printing a version;
browser versions are now read from `Info.plist` and every probe is killed at
its deadline and reported as a timeout. A `Problem` row, not an error, is the
result of a missing or hung tool: discovery never fails the command.

### The dev loop multiplexes four sources

`flui run` drives one `dev_loop` over a `ReloadStrategy` (process restart, or
worker host for the host/worker layout). The loop polls, in priority order:
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

### Finishing a probe transfers its captured output

The pipe reader owns a mutex-protected byte buffer shared with its drain thread.
At the deadline, finishing takes those bytes under the same lock rather than
cloning an entire output stream. A detached reader may subsequently append to
the empty buffer; it cannot change the returned snapshot. The existing
`grandchild_holding_the_pipe_does_not_hang_the_probe` checks the deadline and
captured prefix on Unix. CLI output integration tests check the delivered bytes;
no allocation-count claim is inferred from those behavior tests.

### Build selectors reach the selected platform unit

Web builds forward `--example` and `--package` through the existing Cargo target
resolver. Android APK builds currently require the current package's cdylib;
unsupported selectors are rejected before SDK probing rather than ignored.
`platform_build_selectors_reach_the_selected_cargo_unit_or_refuse` drives the
public CLI. Its portable fixture delegates metadata and project discovery to
real Cargo, substitutes tool availability, and refuses compilation while
reporting the actual selected Cargo arguments. It verifies selection without
claiming a wasm compilation or Android delivery run.


### Generated crate names respect Rust's reserved edition keywords

Project names become bare crate identifiers in generated application source,
so validation refuses `gen` and `try` along with the other reserved keywords.
`create_with_an_invalid_project_name_is_rejected` includes public CLI rows for both names;
accepting either would create source that the selected Rust edition rejects.

### Worker staging uses durable content identity

Staged worker paths include the full SHA-256 digest of the exact bytes read for
that build. The host can retain a dynamic-loader mapping after a staged file is
pruned, so a later path reuse is valid only for identical content. SHA-256's
cryptographic collision resistance is the deliberate content-identity assumption;
the old 32-bit FNV address space admitted practical collisions and silently
served an earlier image. An existing path is also checked against the built bytes.
Differing or interrupted staging is refused without overwriting a potentially
loaded image. Removing the corrupt staged copy permits the next admission.

`commands::run::tests::run_contract` includes
`staged_worker_content_identity_survives_reuse_and_pruning` with two distinct
images that collide under the previous hash, unchanged-file reuse and a pruned
image's return. `corrupt_staged_worker_is_refused_without_overwriting_a_loaded_path`
pins refusal, preservation of both files and the next staging operation. These
private orchestration seams exercise the actual writer the host consumes without
requiring a compiled plugin or platform loader; they do not certify dyld unloading.

Staging publishes a fully written same-directory temporary file with no-clobber
persistence. Concurrent identical admissions verify and reuse the first complete
snapshot instead of truncating it. The published bytes preserve cargo's source
signature and are never re-signed. `concurrent_identical_workers_publish_one_complete_immutable_snapshot`
coordinates three actual staging calls at the private pre-publication seam and
checks unpublished completeness, shared final identity and next-operation reuse.
