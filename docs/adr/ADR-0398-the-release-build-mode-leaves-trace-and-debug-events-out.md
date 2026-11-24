# ADR-0398: The release build mode leaves trace and debug events out

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:** [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) §7 only (the
  bullet "Framework crates set no `max_level_*` feature")
- **Related:** [ADR-0397](ADR-0397-an-icon-face-is-in-a-binary-only-where-a-package-or-the-application-asks-for-it.md)
  (the other cut a release application takes from a Cargo feature);
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the facade is the application's one
  dependency, so its features are the application's switches)
- **Sources:** `flutter/flutter@3.50.1` (`ae9b586f34`)
  `packages/flutter_tools/lib/src/runner/flutter_command.dart:760-781` (the tool's `--debug`,
  `--profile` and `--release` flags pick the build mode), and
  `packages/flutter_tools/lib/src/build_system/targets/web.dart:313` (the tool compiles a release
  with `-Ddart.vm.product=true`, which drops debug-only code)

## Context and problem statement

Every `trace!`, `debug!`, `trace_span!` and `debug_span!` call site in FLUI and in an application
is compiled into a release binary, with its callsite metadata, field names and message strings,
and is filtered out at run time. What bears on that:

| Fact | Evidence |
|---|---|
| `tracing`'s `release_max_level_info` feature compiles every call site above INFO out of a build without debug assertions. On a generated counter it took 720,696 bytes off the Linux x86_64 release binary and 449,924 off wasm32 | release builds of `flui create --template counter`, 2026-11-23, with `tracing` named in the application's own manifest |
| The frame's phase spans are DEBUG and per-node spans are TRACE (ADR-0242 §7). `flui-devtools`' capture (its profiler and timeline) and a Chrome or Tracy layer an application adds read them, and the protocol's `perf` methods are to (the runtime does not serve them yet), so a build without them has no frame timeline | ADR-0242 §7 and §8; `packages/flui-devtools/src/capture.rs`; `not_answered!` in `crates/flui-runtime/src/ui_runtime/agent_serve.rs` |
| The `profiling` profile of a generated project inherits `release` and keeps the symbols a profiler needs; it is the build a timeline is taken from | `crates/flui-cli/src/templates/mod.rs`, `[profile.profiling]` |
| A Cargo feature cannot differ by profile: a feature written into the manifest is on for `release` and for `profiling` alike | Cargo's feature resolver |
| An application has one `tracing` and one static maximum level: any crate in the graph that turns the feature on sets it for every crate | `tracing`'s `level_filters` |
| `flui build` chooses `--release` or a debug build on every platform; `flui run` also takes a cargo `--profile`, which its desktop run passes to cargo, and `flui run --agent` adds the facade's `driver` feature | `crates/flui-cli/src/main.rs`, `crates/flui-cli/src/commands/run.rs` |
| Flutter's tool, not the application's manifest, picks the build mode, and a release compiles debug-only code out | Sources |

Who decides whether a binary holds its trace and debug events?

## Decision drivers

- A shipped application carries no code it never runs: 0.72 MB of call sites a release never
  delivers is cost without use.
- A build a timeline is taken from keeps every phase span: the `profiling` profile, a debug build
  and an agent build read them.
- An author gets the release build by asking for one, with nothing written in the manifest and
  nothing to remember per profile.
- The rule lives at one seam, the one every platform's build goes through.
- A filter that asks for events the build does not hold says so, where the author sees it.

## Considered options

1. Keep every call site in every build (today).
2. The application's manifest turns on a facade feature, which `flui create` writes.
3. The facade feature exists, and the CLI's release build mode turns it on.
4. The framework's own call sites check a `cfg` of their own, and the application's stay.

## Decision

Chosen option: 3, because the build mode is the only place that knows a build ships, and the CLI
is where FLUI's build modes are chosen, as Flutter's tool chooses its own.

### §1. The facade has the switch, and `flui-log` owns it

`flui-log` declares `release-max-level-info = ["tracing/release_max_level_info"]`; `flui-app`
forwards its own `release-max-level-info` to it, and the facade's to `flui-app`'s, each through a
normal dependency. No framework crate turns it on, and a library never does: the level is one for
the whole application.

### §2. The CLI's release mode turns it on

`flui build --release` and `flui run --release` add `--features <key>/release-max-level-info` for
every platform, where `<key>` is the name under which the built package depends on the facade
(`flui`, or its rename), else on `flui-app`, both read from the cargo metadata of the target the
build selects, counting the dev-dependencies an example links; an `optional` dependency gets
`<key>?/release-max-level-info`, which turns the
feature on only where the dependency already is. `flui run --profile release` is `--release`,
read once for every target (`Profile::of_run`), and a worker project's run without hot reload
builds its host in the run's mode. One function decides the features of a build mode,
`FrameworkDependency::mode_features` in `flui-cli`: every platform's builder takes them through
`BuilderContext::mode_args`, and the desktop run through the build it names to each of its cargo
commands. They are left out of a debug build, of any other `--profile` (`profiling` keeps its
spans) and of an agent build (`flui run --agent`), a development build that gets the `driver`
feature in their place. A package that depends on neither builds without the ceiling, and its
release build says so.

A plain `cargo build --release` is a cargo build, not FLUI's release mode, and keeps its events;
an application that ships through cargo alone names the feature itself. A release build of an
application that links `flui-devtools` records no frames, as Flutter's release mode has no
profiler: its capture is taken from a `profiling`, debug or agent build, which its crate
documentation says.

### §3. A filter above the ceiling is reported past the filter

When the subscriber `flui-log`'s `setup` installs has a filter that selects a level its build
compiled out, that filter also admits target `flui_log::ceiling` at WARN, and `setup` sends that
subscriber alone (through its own `Dispatch`, whatever scoped default the thread has), once per
installation, the level the filter asks for and the ceiling, through the platform's sink (the
terminal, logcat, the unified log, the browser console). So a `RUST_LOG=flui_view=debug` that
would otherwise silence every other warning is still told, on every host and from every caller of
`setup`. A subscriber the host already installed filters by its own directives and gets nothing.
Events bridged from `log` (wgpu's, for one) are not compiled out by the ceiling, and the report
says it concerns `tracing` call sites. A build capped below WARN gets the report as an ERROR, and
one capped at `off` holds no call site to say it with.

### Consequences

- Good, because a release build holds no trace or debug call site, the framework's or the
  application's: about 0.7 MB of a generated counter on Linux x86_64 and 0.45 MB on wasm32.
- Good, because `profiling`, debug and agent builds keep every span, so a timeline is taken as
  before.
- Good, because the author writes nothing: `--release` is the request.
- Bad, because `cargo build --release` and `flui build --release` give different binaries, and a
  reader of the manifest does not see why. The book and the facade's feature table say so.
- Bad, because an application that wants debug events in a shipped build cannot have them from
  `flui build --release`; it builds through cargo.
- Bad, because a facade older than the feature fails cargo's resolution of a `--release` build,
  naming the feature; the CLI and the facade are updated together.
- Bad, because an application that links `flui-devtools` gets an empty capture from its release
  build, with no warning from the CLI; the crate's documentation names the builds to take one from.
- Neutral, because CI's `size` job measures the shipping build, so it builds with the feature on.

### Confirmation

- `flui-log`'s `release-max-level-info` is the only `tracing` level feature in a framework
  manifest; a reader of a manifest change checks it.
- `flui-cli`'s `a_release_build_without_an_agent_is_the_one_with_the_ceiling` holds the features
  of each mode, `a_build_mode_is_its_profile_flag_and_its_features` the arguments `mode_args`
  gives a builder, `a_run_is_the_release_mode_whichever_flag_names_the_release_profile` the mode
  of a run, `a_desktop_run_checks_what_it_builds` the desktop run's cargo arguments for a debug,
  a release and an agent run, and `the_framework_key_is_the_normal_dependency_the_manifest_names`
  and `the_framework_key_is_named_by_the_package_the_run_builds` the key of a renamed, optional or
  example's dev-dependency facade.
- `flui-cli`'s `a_desktop_release_build_compiles_the_facade_with_its_ceiling` builds against a
  stand-in facade that does not compile without the feature, and
  `web_release_build_names_the_facade_ceiling` reads the web builder's cargo arguments. The
  Android and iOS builders take the same `mode_args` and run only where their SDKs are.
- The facade's `the_release_ceiling_reaches_tracing_only_through_its_feature` reads `tracing`'s
  features in the graph of a consumer outside the workspace with and without the feature, and
  `a_filter_above_the_release_ceiling_is_reported_where_the_log_goes` builds a consumer of
  `flui-log` without debug assertions and counts the reports for a filter above the ceiling with
  and without a bare level, a rejected override, a second setup, a filter at the ceiling and a
  build without the feature.
- `flui-log`'s `a_filter_is_beyond_the_ceiling_by_its_most_verbose_directive` and
  `a_filter_above_the_ceiling_admits_its_report` hold the level a filter selects above a ceiling
  and the filter the report passes.
- `counter_release_bytes`, `counter_wasm_bytes` and `hot_reload_host_release_bytes` in
  `perf/size.toml`, measured with the feature, as the `size` job builds them.

## Pros and cons of the options

### 1. Keep every call site in every build

- Good, because any `RUST_LOG` works on any binary.
- Bad, because every shipped application carries 0.72 MB of events it never delivers.

### 2. The application's manifest turns on a facade feature

- Good, because the application's choice is written where it can be read.
- Bad, because a feature in the manifest is on for `profiling` too, which then has no phase spans
  and no timeline: the profile a timeline is taken from stops giving one (ADR-0242 §7).
- Bad, because an agent build of the same manifest loses the spans the protocol's `perf` methods
  are to read.
- A variant sets `debug-assertions = true` in `[profile.profiling]`, which `release_max_level_*`
  does not cap. Bad, because every `debug_assert!` and `cfg(debug_assertions)` path then runs in
  the profiled binary, so its timeline measures code the shipped build does not run, and the
  template's profile stops being the release build with symbols.

### 3. The CLI's release build mode turns the feature on

- Good, because the mode that ships is the one that drops the events, and no other build does.
- Good, because the arguments come from one seam every platform's build goes through.
- Bad, because a plain cargo release differs from the CLI's.

### 4. A `cfg` of the framework's own call sites

- Good, because it could be set from the profile with `cfg(debug_assertions)`.
- Bad, because `debug_assertions` is off in `profiling` too, so the timeline goes the same way as
  in option 2.
- Bad, because every call site in the framework would carry a second gate, and the application's
  own events would stay.

## More information

Revisit when Cargo lets a feature or a `cfg` follow the profile, or when the timeline reads spans
at INFO.
