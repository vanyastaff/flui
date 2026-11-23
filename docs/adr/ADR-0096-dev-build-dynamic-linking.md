# ADR-0096: Dynamic linking for development builds

- **Status:** Proposed (2026-09-25)
- **Date:** 2026-09-25
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (core never names an
  official package, which is why no workspace crate can be the dylib),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (§6: an application names its official
  packages itself),
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (hot reload),
  [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (the reload
  whose images link this dylib),
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected Subsecond record),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (process-global state)
- **Revised:** 2026-11-23 — §2 to §5 describe a dylib that `flui run` generates for one
  application, holding every dependency of its reloadable crates, in place of a `flui-dylib`
  workspace crate under a facade feature: that crate could not hold an official package
  (ADR-0081) or one the application names itself (ADR-0088 §6), and an application image
  (ADR-0394) links whatever is beside the dylib once per image.
- **Refs:** decision L in the [decision index](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/decisions.md); the study, with every
  number, command and error, in [`design/dynamic-linking.md`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/dynamic-linking.md)

Nothing in `crates/` has changed as part of this ADR.

The record stays Proposed with §2's preconditions. Dynamic linking does not address framework
test and build time, disk use or memory growth (§1); those go to a separate build-footprint study
(see "Not decided here"). The application side is built when the reload of ADR-0394 needs it.

## Context

Bevy's `dynamic_linking` feature puts the engine in a Rust `dylib` (`bevy_dylib`) that the
facade pulls in with an otherwise unused import. rustc never links a crate twice into one
artifact, so every crate the dylib contains is then taken from it, and an edit to the
application relinks a small executable instead of the whole engine. Bevy calls it its biggest
iteration lever.

FLUI has two pointers to it. `CHANGELOG.md:1098-1104` notes "Bevy-style `dynamic_linking`
(`flui-dylib`) noted as the next lever if needed" after the move to one integration-test binary
per heavy crate. The architecture review proposes a dev-only `dynamic-linking` feature on the
facade ([`docs/research/2026-09-25-architecture-review/report-architecture.ru.md:189`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-09-25-architecture-review/report-architecture.ru.md#L189)). Neither
was measured, and the review's own verification showed that a facade feature over a crate that
depends on the facade is a Cargo cycle
([`docs/research/2026-09-25-architecture-review/judges-and-verification.md:238`](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-09-25-architecture-review/judges-and-verification.md#L238)).

Two prototypes measured it on the Windows dev host (MSVC `link.exe`, rustc 1.98.1, the
workspace dev profile, which builds workspace crates at opt-level 1 and dependencies at
opt-level 3, `Cargo.toml:779-822`):

| Measurement | Static | Dynamic |
|---|---|---|
| One-line edit in the app crate | 1.4-2.9 s | 1.0-1.3 s |
| One-line edit in `flui-widgets` | 3.5-3.9 s | 5.6-7.0 s (17 s on the first rebuild) |
| Cold build, `link.exe` | 145.8 s | 148.0 s |
| Executable | 19.7 MB | 0.4 MB, plus a 43.9 MB DLL, a 66.4 MB import library and a 95.1 MB PDB |
| Exports of the dylib, default facade features | — | 64,336 of 65,535 |
| Exports, every facade feature | — | 67,387: `LNK1189`, and rust-lld "too many exported symbols" |
| Exports, FLUI crates at opt-level 2 (stable) | — | 25,616 |
| Exports, nightly `-Zshare-generics=n` | — | 27,249 |

Three facts follow from the table.

1. **The gain is one link, on app edits only.** The dynamic build saves the final link of a
   19.7 MB executable: 0.3-1.5 s per app edit. Any edit inside a crate the dylib contains
   rebuilds and relinks the dylib (3.76 s of the widgets rebuild in `--timings`), so framework
   contributors lose 2-3 s per edit. Integration-test binaries link the crate under test
   statically and cannot use a dylib that contains it; the 188-executable cost the CHANGELOG
   worried about was already removed by consolidation.
2. **Windows sits at the export ceiling.** PE export ordinals are 16-bit. About 40,000 of the
   64,336 exports are shared generic instances from FLUI's own crates: rustc shares and exports
   generics across crates only at opt-level 0 and 1, and the dependencies are already at 3
   (`Cargo.toml:807-813`). The fix Bevy applied, `-Zshare-generics=n`, is nightly-only, and the
   toolchain is pinned to stable (`rust-toolchain.toml`). The only stable fix is opt-level ≥ 2
   for the dylib's crates, whose cost on framework rebuilds is unmeasured.
3. **std becomes a DLL.** The executable needs `std-<hash>.dll` on `PATH`: `cargo run` and
   nextest add it, `cargo test --doc`, a debugger launch or a double-click do not (measured:
   `STATUS_DLL_NOT_FOUND` with a minimal `PATH`).

A third prototype, on Linux (rustc 1.100.0, run of 2026-11-23), built the dylib outside the
application's workspace:

| Step | Result |
|---|---|
| A generated crate with `crate-type = ["dylib"]` depends on the application's dependency with the same features; a generated launcher depends on it | cargo compiles the dylib with `-C prefer-dynamic`, as it does every dylib that is not the package being built, so std is the shared one |
| The application's library, whose manifest names only its dependency, is built with `cargo rustc --lib --crate-type cdylib -- --cfg … --extern <the dylib> -C prefer-dynamic`, and names the dylib under that `cfg` | it links; the dependency's functions are undefined symbols of the 8.5 KB image, resolved from the dylib |
| Both builds share one target directory | cargo gives the dependency the same unit in both workspaces: one rlib file serves the dylib and the application's build |
| The launcher loads two builds of the image and each calls a function that increments a `static` of the dependency | 1, then 2: one copy of the dependency's static serves both images |

## Decision

### 1. No dynamic linking for framework or test builds

FLUI does not build its own crates, examples, tests or CI through a framework dylib. The
CHANGELOG's "next lever" is withdrawn: the measurement shows framework edits get slower, test
binaries cannot use it, and CI builds without incremental compilation
(`Cargo.toml:788-791`), where the dylib is only extra output.

### 2. The application side waits for its preconditions

Nothing generates a dylib yet. `flui run` does, in the shape of §3, when all of these hold:

- the shape of §3 has been built once for a generated FLUI project with Material: a launcher,
  the dylib and two images of the application in one process, with the unit check of §4 passing;
- the export count of §4 is measured on Windows for that project at the profile `flui create`
  writes, and is under the budget in one dylib or in the two of §3;
- the rebuild of an app edit is re-measured on Windows and Linux on an otherwise idle host and, as
  an image of ADR-0394, is at least 30% shorter than the static build's.

### 3. The shape: one dylib per application, generated by `flui run`

- **What it holds.** Every normal dependency of the application's reloadable crates (ADR-0393 §2)
  that is not itself reloadable: the facade, the official packages the application names, and the
  application's other dependencies, each with the features cargo resolved for the application.
  Build dependencies and procedural macros are not in it.
- **Who builds it.** `flui run` writes, under the project's target directory, a workspace of two
  crates: the dylib (`crate-type = ["dylib"]`, one `use <crate> as _;` per dependency) and a
  launcher that depends on it and on the reload package. It copies the application's lock file,
  its profiles and its `[patch]` tables. The dylib is built again only when the application's
  dependencies change, which is a process restart (ADR-0393 §2).
- **One switch.** The tool builds the dylib, the launcher and the image into a target directory
  of its own and passes every crate of those builds `--cfg flui_dev_reload`. What exists only for
  reload is written under that `cfg`: the adopt of ADR-0394 §2 in the framework's crates, and what
  the derives and `#[flui::main]` write into an application's. A plain `cargo build`, which does
  not pass it, shares no artifact with the tool's build and contains none of it.
- **How the application links it.** The application's manifest does not change. The tool builds
  the reloadable crate as ADR-0394 §1 says, passing the dylib to rustc, and the code
  `#[flui::main]` writes under the `cfg` names it. rustc takes every crate the dylib contains from it, so
  the image holds the reloadable crates' own code and nothing else.
- **Why every dependency and not the framework alone.** Whatever is linked beside the dylib is
  linked once into every image. A dependency left there would be compiled into every image, and
  its statics and thread-locals would start over with each: a runtime handle an earlier image
  made would be used by code that asks another copy of the runtime's context.
- **Two dylibs where one cannot hold the exports.** When §4's count is over the budget, the tool
  writes two: the framework's crates with the official packages, and the remaining dependencies in
  a second dylib that depends on the first. Cargo builds both with `-C prefer-dynamic`, so they
  share std.
- **No workspace crate, no feature, nothing published.** The dylib is the tool's output for one
  project. The facade gains no `dynamic-linking` feature and no optional dependency, and no tier
  or kind is assigned (ADR-0081): the generated workspace is not part of FLUI's.
- **Never in a built application.** `flui build` generates nothing, and a build without the
  `cfg` contains no reference to a dylib. `flui run --release`, a device and the browser
  run the static build.
- `flui create` writes `[profile.dev.package."*"] opt-level = 3`, which keeps the dylib's export
  count where the table measured it.

### 4. The guards are the tool's

- **One unit per crate.** For every crate the dylib holds, the unit cargo compiled for the dylib
  and the unit it resolved for the application's build are the same file. The tool reads both
  from cargo's JSON messages and refuses to start with a mismatch, naming the crate: a mismatch
  would link a second copy into every image without an error.
- **The export count.** On Windows the tool counts the dylib's export table after it links,
  writes two dylibs above 55,000 exports, and runs the static build with a process restart for
  every edit when the two still do not fit, saying so.
- **The launcher finds std.** The tool starts the launcher with the toolchain's library directory
  on the loader's search path, as `cargo run` does.
- The nightly Windows run's `flui-cli` scenarios gain one that generates a project with every
  official package, builds its dylib and reads the count, added by the change that builds §3.

### 5. The dylib is not a reload boundary

It is loaded once and never unloaded. Which code is loaded again, and how, is ADR-0311's hook and
the mechanism behind it: an image of ADR-0394 links this dylib, which is why one copy of the
framework, of each official package and of each dependency serves the launcher and every image.

## Consequences

- Nothing changes in the code now.
- Contributors keep the static build, which already relinks in 1.4-3.9 s on the dev host.
  Faster linking remains a per-machine choice (`.cargo/config.toml:11-24`).
- An application's manifest, its `main` and its release build are untouched by the dylib.
- The first `flui run` of a project links one large dylib, and a change of dependencies links it
  again.
- The dylib of an application with many dependencies exports more than the framework alone, so
  the Windows budget is the application's and not only FLUI's.
- rustc links each crate once per artifact, so the dylib holds exactly one copy of every
  process-global static. Dynamic linking does not split UI runtime or registry identity and needs
  no entry in ADR-0097's allowlist.
- The reading of the dlopen worker in the study found two unreported hazards (the old image
  unmapped before the reassemble drops its views; the worker's own `REQUEST_REBUILD`,
  `packages/flui-hot-reload/src/dispatch.rs:24`). Both are unrun hypotheses and belong to the
  hot-reload work, not to this decision.

## Alternatives considered

- **Adopt Bevy's shape as the review proposed: a `flui-dylib` workspace crate under a facade
  feature.** Rejected. At the default feature set it uses 98.2% of the Windows export table.
  Framework contributors would get slower rebuilds. It cannot depend on an official package
  (ADR-0081), and once the facade names none (ADR-0088 §6) nothing would turn on a feature that
  puts one in it, so Material would be linked beside it and into every image. It is also one more
  crate published in lockstep (Bevy's `bevy_dylib` has lagged its releases, bevy#17723 and
  bevy#22654).
- **A `dynamic-linking` facade feature over a dylib that depends on the facade.** Rejected: a
  Cargo cycle, the shape the review reproduced.
- **The framework in the dylib and the application's other dependencies beside it.** Rejected for
  the reason §3 gives: a dependency with a static is then per image. It remains the fallback §4
  names when the exports do not fit.
- **`-Zshare-generics=n` or Cranelift.** Rejected as project settings: both need nightly, and
  Cranelift has no unwinding on Windows or macOS.
- **A third-party-only dylib for test targets** (wgpu, naga, `windows`, the font crates). Not
  measured. Worth a `cargo build --timings` run before it is considered; it is outside this
  decision.
- **Reject outright.** Not chosen: an image that links the framework statically is the 27 MB
  worker of today (ADR-0394, Context).

## Not decided here

- The workspace's own build footprint. Dynamic linking changes only the final link of an
  application; it does nothing for the size of `target/`, the number of test binaries, duplicate
  builds of the upper stack, or peak memory. Those belong to the build-footprint study
  ([open questions, "Still open"](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/open-questions.md#still-open)): `target/` size by
  artifact kind, test binary count, duplicate builds from per-crate `testing` features, peak
  memory, per-worktree targets, and the levers (test consolidation, sccache, split debuginfo,
  nextest archives, cargo-sweep, a shared target).
- Whether a project can turn the generated dylib off in `flui.toml`.
- How the generated workspace follows an application that inherits its dependencies from a
  workspace root, beyond copying the lock file, the profiles and the patches.
