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
| The frame's phase spans are DEBUG and per-node spans are TRACE (ADR-0242 §7). `FrameTimingLayer`, the Chrome and Tracy export and the driver's perf read them, so a build without them has no frame timeline | ADR-0242 §7 and §8 |
| The `profiling` profile of a generated project inherits `release` and keeps the symbols a profiler needs; it is the build a timeline is taken from | `crates/flui-cli/src/templates/mod.rs`, `[profile.profiling]` |
| A Cargo feature cannot differ by profile: a feature written into the manifest is on for `release` and for `profiling` alike | Cargo's feature resolver |
| An application has one `tracing` and one static maximum level: any crate in the graph that turns the feature on sets it for every crate | `tracing`'s `level_filters` |
| `flui build` and `flui run` choose the build: `--release` (every platform), a named `--profile`, or a debug build, and `flui run --agent` adds the facade's `driver` feature | `crates/flui-cli/src/build/`, `crates/flui-cli/src/commands/run.rs` |
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
(`flui`, or its rename). The arguments come from the one place that selects the built package's
cargo target, so every platform's build gets them. They are left out of a debug build, of a
named `--profile` (`profiling` keeps its spans) and of an agent build (`flui run --agent`), whose
driver reads the phase spans. A package that does not depend on the facade builds without the
ceiling.

A plain `cargo build --release` is a cargo build, not FLUI's release mode, and keeps its events;
an application that ships through cargo alone names the feature itself.

### §3. A filter above the ceiling is reported outside the filter

When the subscriber FLUI installs selects a level its build compiled out, the application runner
writes one line to standard error naming the level the filter asks for and the ceiling, once per
process, so a `RUST_LOG=flui_view=debug` that the filter would otherwise silence is still told.
Events bridged from `log` (wgpu's, for one) are not compiled out by the ceiling, and the line says
it concerns `tracing` call sites.

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
- Neutral, because CI's `size` job measures the shipping build, so it builds with the feature on.

### Confirmation

- `flui-log`'s `release-max-level-info` is the only `tracing` level feature in a framework
  manifest; a reader of a manifest change checks it.
- A test of `flui-cli` reads the arguments the release, debug, profile and agent builds pass, and
  one reads the facade key from a manifest that renames it.
- A test of the facade reads `tracing`'s features in the graph of a consumer outside the
  workspace with and without the facade feature.
- A test of `flui-log` reports the level a filter selects above a ceiling, for a subscriber it
  installed and for one it did not.
- `counter_release_bytes` and `counter_wasm_bytes` in `perf/size.toml`, built with the feature.

## Pros and cons of the options

### 1. Keep every call site in every build

- Good, because any `RUST_LOG` works on any binary.
- Bad, because every shipped application carries 0.72 MB of events it never delivers.

### 2. The application's manifest turns on a facade feature

- Good, because the application's choice is written where it can be read.
- Bad, because a feature in the manifest is on for `profiling` too, which then has no phase spans
  and no timeline: the profile a timeline is taken from stops giving one (ADR-0242 §7).
- Bad, because an agent build of the same manifest loses the driver's perf.

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
