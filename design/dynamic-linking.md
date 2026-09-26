# Dynamic linking for development builds

A study of Bevy-style dynamic linking (`bevy_dylib`, the `dynamic_linking` feature) and what it
would buy FLUI. It covers how Bevy does it, what two prototypes measured on this workspace,
the limits per platform, how it meets hot reload, what it means for CI and test binaries, and
the shape FLUI would ship if it adopts it. The decision is recorded in
[ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md).

## Verdict

- **It does not speed up framework or test builds, and FLUI will not use it for that.** An edit
  inside a crate that the dylib contains recompiles that crate and then rebuilds and relinks the
  whole dylib. Measured on Windows, a one-line edit in `flui-widgets` went from 3.5-3.9 s to
  5.6-7.0 s.
- **It speeds up edits to the application crate, by a small absolute amount.** A one-line app
  edit went from about 1.4-2.9 s to about 1.0-1.3 s. Static linking is already fast on this
  host, so the saving is roughly 0.3-1.5 s per rebuild.
- **On Windows it sits at the PE export limit.** With the default facade features the dylib
  exports 64,336 symbols against a hard limit of 65,535. With every facade feature it does not
  link (`LNK1189`). Building the FLUI crates at opt-level 2 brings the count down to 25,616 on
  stable; nothing else on stable does.
- **Hot reload is a separate tool.** Dynamic linking neither replaces Subsecond
  ([ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md)) nor needs it, and the
  engine dylib is not a reload boundary.
- **Recommendation: defer.** No `dynamic-linking` feature before H1. If it is added later, it is
  an app-side, desktop-only, dev-only convenience in the shape described under
  [If adopted](#if-adopted-the-design), and it ships only together with a Windows export-count
  gate and the opt-level fix.

## Where the idea came from

- `CHANGELOG.md:1098-1104` records the move to one integration-test binary per heavy crate. The
  old layout was "~5.9 GB across 188 executables", and the entry ends with "Bevy-style
  `dynamic_linking` (`flui-dylib`) noted as the next lever if needed." No `flui-dylib` code
  exists.
- The architecture review lists a dev-only `dynamic-linking` facade feature
  (`docs/research/2026-09-25-architecture-review/report-architecture.ru.md:189`,
  `synthesis.md:168`), calling it Bevy's biggest iteration lever
  (`market/workspace_ecosystem_structure.md:11,47`; `designs/dx_first.md:355`;
  `designs/performance_first.md:146`). None of those sources measured it. The research
  catalogue listed it among the claims never verified. This document is that verification.

## How Bevy does it

- **The crate.** `bevy_dylib` has `crate-type = ["dylib"]`, depends on `bevy_internal`, and its
  whole source is one otherwise unused `use bevy_internal as _;` with a lint suppression
  ([Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_dylib/Cargo.toml),
  [lib.rs](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_dylib/src/lib.rs)).
- **The feature.** The `bevy` facade declares
  `dynamic_linking = ["dep:bevy_dylib", "bevy_internal/dynamic_linking"]`, and the dependency
  exists only on non-wasm targets
  ([bevy Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/Cargo.toml)).
- **Why an unused import is enough.** For an executable, rustc prefers rlibs and links a dylib
  only for a dependency that is not available as an rlib. It also guarantees that "a library
  never appears more than once in any artifact"
  ([Rust reference, linkage](https://doc.rust-lang.org/reference/linkage.html)). `bevy_dylib`
  exists only as a dylib, so naming it forces dynamic linking, and every crate the dylib already
  contains is then taken from it instead of being linked again. std is linked dynamically too.
  No `-C prefer-dynamic` is needed; Fyrox sets it only because it loads two separate dylibs that
  would otherwise each carry a copy of std
  ([Fyrox book](https://fyrox-book.github.io/beginning/hot_reloading.html)).
- **Why the facade and the dylib are separate crates.** `bevy` depends on `bevy_dylib`, which
  depends on `bevy_internal`, never back on the facade. A facade feature that pulls in a crate
  depending on the facade is a Cargo cycle even when the edge is optional; the review reproduced
  that shape on a minimal workspace ("cyclic package dependency", exit 101,
  `docs/research/2026-09-25-architecture-review/judges-and-verification.md:238`).
- **Feature unification.** Cargo resolves features once per build graph, so the dylib contains
  whatever feature set that build unified. Crates the dylib does not contain are linked
  statically into the executable next to the dylib; mixing the two is allowed as long as no
  crate appears twice.
- **What Bevy says it buys and costs.** The `bevy_dylib` docs promise "much faster" incremental
  builds and warn against shipping it ([docs.rs](https://docs.rs/bevy_dylib)). The
  [cheat book](https://bevy-cheatbook.github.io/setup/bevy-config.html) says it works best on
  Linux and that Windows and macOS "have had issues". Robert Krahn measured about 2.9 s static
  against 0.49 s dynamic for a polars app on an M1
  ([post](https://robert.kra.hn/posts/2022-09-09-speeding-up-incremental-rust-compilation-with-dylibs/));
  Embark's motivation was 30+ s links on Windows
  ([rust-ecosystem#13](https://github.com/EmbarkStudios/rust-ecosystem/issues/13)).

## What was measured

Two independent prototypes ran on the Windows dev host (32 cores, MSVC 14.44 `link.exe`, rustc
1.98.1 x86_64-pc-windows-msvc, the pinned channel in `rust-toolchain.toml`). Neither is in the
repository. Both built against the workspace at `cab06137d`.

- **Standalone prototype.** A separate workspace depending on FLUI by path, with the lockfile
  and toolchain copied over. It mirrors the dev profile, with the FLUI crates overridden back
  to opt-level 1. `flui_dylib` is `crate-type = ["dylib"]` with `use flui as _;`, and `app` is
  the body of `examples/counter.rs` plus an optional `use flui_dylib as _;`. No rustflags.
- **In-workspace prototype.** Three temporary members: `crates/flui-dylib` (`crate-type =
  ["dylib"]`, depends on `flui` with `material`, `src/lib.rs` is `pub use flui::*;`,
  `[package.metadata.flui] layer = 6, wasm = false`), `examples/counter_dylib` (the counter
  example plus `use flui_dylib as _;`), and `examples/counter_static` (the same package without
  `flui-dylib`, the fair static comparison, since the root `--example counter` also pulls in the
  facade's dev-dependencies). These builds set `-C prefer-dynamic`, which the Bevy shape does
  not need.

The dev profile is the workspace's own: `opt-level = 1`, `incremental = true`,
`debug = "line-tables-only"` for workspace crates (`Cargo.toml:779-805`), and `opt-level = 3`,
`debug = false` for every dependency (`Cargo.toml:807-822`).

### Commands

The in-workspace variants, each in its own target directory:

```powershell
# A. baseline
cargo build --example counter
# B. static package, same target dir as A
cargo build -p counter-static
# C. static package, rust-lld
$env:RUSTFLAGS='-C linker=rust-lld'; cargo build -p counter-static
# D. dylib, link.exe
$env:RUSTFLAGS='-C prefer-dynamic'; cargo build -p counter-dylib
# E. dylib, rust-lld
$env:RUSTFLAGS='-C prefer-dynamic -C linker=rust-lld'; cargo build -p counter-dylib
# F. E on nightly with shared generics off, every facade feature
$env:RUSTUP_TOOLCHAIN='nightly'
$env:RUSTFLAGS='-C prefer-dynamic -C linker=rust-lld -Zshare-generics=n'
cargo build -p counter-dylib
```

Every figure is the wall time of `cargo build`, three runs each. "Touch" changes only a file's
mtime. "Edit" appends `#[allow(dead_code)] fn __dylib_probe() -> u32 { N }` with a new `N` per
run, so the crate hash really changes, and restores the file afterwards. The example file is the
example's `main.rs`; the widgets file is `crates/flui-widgets/src/container.rs`. The per-unit
split comes from `cargo build --timings`. Exports and imports were read with `dumpbin /exports`
and `dumpbin /dependents`.

### Rebuild times (in-workspace prototype, seconds)

| Variant | Cold | Touch example | Touch widgets | Edit example | Edit widgets | Outputs |
|---|---|---|---|---|---|---|
| A. `--example counter`, link.exe | 145.8 | 1.66 / 1.48 / 1.46 | 7.29 / 3.28 / 3.45 | 1.51 / 1.94 / 1.51 | 3.73 / 3.57 / 3.55 | exe 19.7 MB, pdb 36.7 MB |
| B. `counter-static`, link.exe | 67.8\* | 2.66 / 2.71 / 1.91 | 8.44 / 4.39 / 4.25 | 2.32 / 2.85 / 2.66 | 3.87 / 3.88 / 4.18 | exe 19.7 MB, pdb 36.3 MB |
| C. `counter-static`, rust-lld | 103.6 | 1.65 / 1.64 / 1.82 | 7.13 / 3.22 / 3.25 | 1.38 / 1.38 / 1.39 | 3.57 / 3.45 / 3.46 | exe 19.6 MB |
| D. `counter-dylib`, link.exe | 148.0 | 1.55 / 1.58 / 1.68 | 10.02 / 8.24 / 9.09 | 1.16 / 1.27 / 1.12 | 7.33 / 6.92 / 6.98 (first: 17.08) | exe 0.40 MB; dll 43.9 MB; import lib 66.4 MB; `.exp` 39.9 MB; dll pdb 95.1 MB |
| E. `counter-dylib`, rust-lld | 100.9 | 1.30 / 1.01 / 1.03 | 9.22 / 5.12 / 5.12 | 1.05 / 1.04 / 1.03 | 5.61 / 5.60 / 5.59 | exe 0.40 MB; dll 43.8 MB; dll pdb 85.6 MB |
| F. E on nightly, `-Zshare-generics=n`, all features | 94.2 | not run | not run | 1.84 / 1.12 / 1.10 | 8.44 / 5.14 / 5.58 | exe 0.73 MB; dll 36.2 MB |

\* B reused A's target directory and rebuilt only the crates whose feature set changed, so it is
not a cold build. The other cold figures used fresh target directories with a warm OS file
cache and were run once.

Where an edit-widgets rebuild spends its time (`--timings`, seconds):

| Unit | Static (B) | Dylib (D) |
|---|---|---|
| `flui-widgets` | 1.46 | 1.45 |
| `flui-material` / `flui-app` | 0.9 / 1.1 | 0.9 / 1.1 |
| `flui` facade | 0.43 | 0.46 |
| `flui-dylib` (codegen and link) | — | 3.76 |
| final binary | 2.14-3.02 | 0.68 |

### Rebuild times (standalone prototype, one-line app edit, seconds)

| Profile for the FLUI crates | Static | Dynamic |
|---|---|---|
| opt-level 1 (the dev profile) | 1.79-2.11 | 0.95-1.04 (1.47 on the first run) |
| opt-level 2 | 1.92-2.42 | 1.04-1.10 |

### Exported symbols

| Build | Exports of the dylib |
|---|---|
| Default facade features (`material`), FLUI crates at opt-level 1 | **64,336** (both prototypes) |
| Plus `cupertino` and `localizations` | 64,467 |
| `material`, `cupertino`, `localizations`, `signals`, `a11y` | **67,387, does not link** |
| Default features, FLUI crates at opt-level 2 (stable) | **25,616** |
| All features, nightly `-Zshare-generics=n` | 27,249 |

About 40,000 of the 64,336 are shared generic instances, recognisable by the instantiating-crate
suffix in the symbol name; the named ones are almost all FLUI crates, led by `flui_widgets`
with 5,707. The other 24,000 are ordinary items, the largest groups being `flui_view`
(about 17,900), `windows` (5,070) and `read_fonts` (4,595).

### Errors, verbatim

With every facade feature, `link.exe`:

```text
error: linking with `link.exe` failed: exit code: 1189
  = note: LINK : fatal error LNK1189: library limit of 65535 objects exceeded
error: could not compile `flui-dylib` (lib) due to 1 previous error
```

The same build with `rust-lld`:

```text
error: linking with `rust-lld` failed: exit code: 1
  = note: rust-lld: error: too many exported symbols (got 67387, max 65535)
error: could not compile `flui-dylib` (lib) due to 1 previous error
```

With `material` alone both linkers succeed.

### Does the result run

- The dynamic executable imports only `flui_dylib.dll` and `std-44a584f44bc3dd65.dll`; the
  dylib imports std plus d3d12, dxgi and the other system DLLs. The whole stack really is
  linked dynamically.
- Started with a minimal `PATH`, the standalone prototype's executable exits with `0xC0000135`
  (`STATUS_DLL_NOT_FOUND`). The in-workspace one stayed alive with no window, consistent with
  the loader's missing-DLL dialog, which was not seen.
- With the toolchain's `bin` directory on `PATH`, and under `cargo run`, which adds it itself,
  every variant opens "FLUI App", selects the discrete GPU through DX12 and paints frames with
  no panic or error lines. The nightly toolchain ships `std-*.dll` only under
  `lib\rustlib\x86_64-pc-windows-msvc\lib`, not in `bin`.

### How far to trust the numbers

- The two prototypes ran on the same host at overlapping times, so timings are indicative. The
  per-variant spread in the tables shows the noise.
- The standalone prototype did not measure a framework edit, and neither measured the cost of
  opt-level 2 on framework rebuilds, which is the reason the dev profile keeps workspace crates
  at opt-level 1 (`Cargo.toml:807-813`).
- Only Windows was measured. Linux and macOS have no 16-bit ordinal limit and may trade
  differently.
- The shape recommended below (a dylib over the core crates, with the facade itself and any
  official package linked statically beside it) was not built. Both prototypes put the facade
  inside the dylib.

## Reading the numbers

1. **The win is the final link of one executable.** A 19.7 MB executable becomes 0.4 MB; the
   app-edit rebuild drops by 0.3-0.4 s with rust-lld and by 1.2-1.5 s with `link.exe`
   (package against package). The root `--example counter` path already rebuilds in about
   1.5 s.
2. **Framework edits get slower.** Any edit below the facade now also rebuilds `flui-dylib`:
   3.76 s of codegen and link for a 44 MB DLL with 64k exports, a 66 MB import library and a
   40 MB `.exp`. The first dylib rebuild after a cold build took 17 s. This is the loop FLUI's
   own contributors live in, and the loop the CHANGELOG hoped to shorten.
3. **Cold builds do not change.** 148 s dynamic against 146 s static with `link.exe`. rust-lld
   cut cold builds by about 30% for both shapes (many build scripts and proc-macros are linked),
   measured once; the incremental gain from rust-lld is small, which matches the measurement
   already recorded in `.cargo/config.toml:22-24`.
4. **The export ceiling is structural.** Shared generics are what overflow the table: rustc
   shares and exports monomorphised generics across crates only at opt-level 0 and 1
   ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)), and on Windows
   each shared instance becomes an export. Dependencies are already at opt-level 3
   (`Cargo.toml:813`), so the shared instances come from FLUI's own crates. The default feature
   set uses 98.2% of the table today; every new widget, render object or feature eats the rest.
5. **Only one stable fix exists.** `-Zshare-generics=n` (the fix Bevy applied in
   [PR #2016](https://github.com/bevyengine/bevy/pull/2016)) is nightly-only and the toolchain is
   pinned to stable. Raising the FLUI crates to opt-level 2 cuts exports by 60% and keeps the
   app-edit gain (about 1.05 s against 2.0 s), at an unmeasured cost to framework rebuilds.
   Bevy's current Windows guidance is the same trade: dynamic linking there "must also enable
   the performance optimizations" ([Bevy setup](https://bevy.org/learn/quick-start/getting-started/setup/)).

## Limits per platform

| Target | Status | Why |
|---|---|---|
| Windows (MSVC) | Possible, fragile | The 65,535-export limit ([bevy#1110](https://github.com/bevyengine/bevy/issues/1110), [bevy#14930](https://github.com/bevyengine/bevy/issues/14930), open since 0.14.1). Needs opt-level ≥ 2 for the dylib's constituent crates. The executable needs `std-<hash>.dll` on `PATH`. |
| Linux | Possible, unmeasured | No ordinal limit; the default linker is already `rust-lld` (`.cargo/config.toml:11-13`); Bevy reports it works best here. |
| macOS | Possible, unmeasured | Listed as having "had issues" by the cheat book; no FLUI run. |
| wasm32 | Not supported | No dynamic linking; Bevy excludes it in its manifest. |
| Android | Not supported | The app is already a `cdylib` built with `cargo ndk` (`CHANGELOG.md:70-78`); a Rust `dylib` and a shared libstd would have to be packaged into the APK and loaded. This is reasoning, not a sourced statement. |
| iOS | Not supported | Same packaging problem inside an app bundle; same caveat. |

Further limits on every platform:

- **std is dynamic.** `cargo run` adds the toolchain library directory to the search path, and
  so does nextest since 0.9.72 ([nextest env vars](https://nexte.st/docs/configuration/env-vars/)).
  `cargo test --doc`, a debugger launch, or double-clicking the executable need
  `rustc --print target-libdir` on `PATH` by hand.
- **Not for release.** Shipping would mean shipping libstd and the framework dylib, and
  cross-crate LTO stops at the dylib boundary ([bevy#13510](https://github.com/bevyengine/bevy/discussions/13510)).
- **Generics still compile in the app.** Generic code the app instantiates is monomorphised in
  the app crate; at opt-level 0 and 1 instances already exported by the dylib are reused, and
  that reuse is what inflates the export table.
- **One more lockstep artifact.** `bevy_dylib` has repeatedly lagged Bevy releases on crates.io
  ([bevy#17723](https://github.com/bevyengine/bevy/issues/17723),
  [bevy#22654](https://github.com/bevyengine/bevy/issues/22654)).

## Hot reload

### Subsecond

- Subsecond routes calls through a jump table and links only the changed code as a patch
  against a first "fat" build, patching only the crate that contains `main`
  ([docs](https://docs.rs/subsecond/latest/subsecond/)). Its author describes it as similar in
  spirit to marking a crate as a dylib, without changing any crate
  ([HN](https://news.ycombinator.com/item?id=44369642)). It does not need dynamic linking.
- The two were briefly incompatible: `dx serve --hot-patch` could not find `libbevy_dylib`
  ([dioxus#4154](https://github.com/DioxusLabs/dioxus/issues/4154)), closed by PR #4174. No report
  either way was found for Windows.
- On Windows Subsecond reads symbols from PDBs. FLUI keeps MSVC's PDB output on purpose
  (`.cargo/config.toml:19-21`); `/DEBUG:NONE` must not come back.
- Bevy 0.17 ships Subsecond-based system hot patching behind `hotpatching`, main crate only
  ([Bevy 0.17](https://bevy.org/news/bevy-0-17/)).
- Consequence for FLUI: [ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md) stands
  on its own. The Subsecond spike it requires should also run once with dynamic linking on, if
  the feature exists by then, because Subsecond patches the main crate and the framework would
  sit in a separate image.

### The current dlopen worker

The crate documents one residual risk: a returned `Scene` can hold `Arc<dyn …>` values whose
vtables live in the plugin image (`crates/flui-hot-reload/src/lib.rs:24-30`), plus the TLS
destructor handling in the plugin macro (`crates/flui-hot-reload/src/plugin.rs:226-248`).
Reading the worker path turned up two more. Both are **hypotheses that were not run**:

1. **The old worker image may be unmapped while the host still holds its code.**
   `WorkerReloadDriver::poll` unloads the old image (`crates/flui-hot-reload/src/worker.rs:458`)
   and loads the new one (`worker.rs:460`); the realm reassembles only afterwards
   (`crates/flui-app/src/app/hot_reload.rs:226-228`, reaching `perform_reassemble` at
   `crates/flui-app/src/app/presentation.rs:1300`). The host's element tree still holds views
   and closures built by the old image: `build_counter_ui` returns a `BoxedView`
   (`examples/hot_reload_counter/logic/src/lib.rs:29`) with an `on_tap` closure (`lib.rs:36`).
   On Windows, `FreeLibrary` (`crates/flui-hot-reload/src/dynlib.rs:211`) unmaps an image whose
   reference count reaches zero, so the reassemble would drop or compare those views through
   vtables in unmapped memory. macOS's deferred unmap (`docs/hot-reload.md:42`) would hide it.
   To check: run `hot_reload_counter` on Windows, edit the logic crate, save.
2. **The worker has its own copy of the framework's globals.** The worker is a `cdylib`
   (`examples/hot_reload_counter/logic/Cargo.toml`) that statically links its own
   `flui-hot-reload` and, through `flui-widgets`, its own `flui-painting`. Its `on_tap` calls
   `request_rebuild()` (`lib.rs:38`), which reads the worker's own `REQUEST_REBUILD`
   (`crates/flui-hot-reload/src/dispatch.rs:24,78-85`); the host registers its hook only in the
   host's copy, so the tap would log "called before host registered a hook" and nothing would
   rebuild. It is the same class of problem as the worker's separate `FONT_SYSTEM`
   (`docs/hot-reload.md:213`, `crates/flui-painting/src/text_layout/layout.rs:124`).

A shared framework dylib with a dynamic std, as Fyrox builds it, would remove the duplicated
globals of issue 2 but not issue 1, and Fyrox itself calls code reloading "based on wildly
unsafe functionality" ([Fyrox book](https://fyrox-book.github.io/beginning/hot_reloading.html)).
It is not worth building for a path that ADR-0094 deletes. The two issues belong in
`flui-hot-reload`'s crate docs, or better, in a Windows repro run, until that deletion.

### Globals

rustc never links a crate twice into one artifact, so a framework dylib holds exactly one copy
of every process-global static (`FONT_SYSTEM`, `REQUEST_REBUILD`, `REGISTRY_STACK`,
`TIME_DILATION`, `APP_RUNTIME`). Dynamic linking therefore does not split realm or registry
identity, and it does not interact with the globals gate of
[ADR-0097](../docs/adr/ADR-0097-no-process-global-state-gate.md). For crates linked statically
beside the dylib (the facade, official packages) this follows from the same rule but was not
built.

## CI and test binaries

- **The CHANGELOG lever does not apply.** An integration-test binary for `flui-widgets` links
  `flui-widgets` itself, so a dylib containing `flui-widgets` cannot serve it, and a dylib of
  everything below it is rebuilt by any edit below it. The 188-executable problem was already
  solved by consolidating to one binary per heavy crate (`CHANGELOG.md:1098-1103`).
- **CI would pay, not gain.** CI builds with `CARGO_INCREMENTAL=0` (`Cargo.toml:788-791`), and
  the gain is incremental relinking. A dylib adds a 44 MB image, a 66 MB import library and a
  95 MB PDB to every build that includes it.
- **A third-party-only dylib** (wgpu, naga, `windows`, the font crates) for test targets is
  conceivable, since those crates rarely change, but it is untested; `windows` alone is the
  largest rlib in the graph (78.7 MB). Measure with `cargo build --timings` before considering
  it.
- **Workspace builds.** The workspace has no `default-members` (`Cargo.toml:80-84`), so a
  `flui-dylib` member would be built by every `--workspace` build and every `--all-features`
  facade combination (`tools/xtask/src/tasks/facade.rs:31`). That is acceptable on Linux, but
  it is extra cost with no test behind it; the design below keeps the crate out of those
  graphs except where a gate needs it.

## Alternatives

| Lever | Verdict |
|---|---|
| Faster linker on Linux | Already the default (`rust-lld`); mold stays a local opt-in (`.cargo/config.toml:12-17`). Bevy notes that turning dynamic linking off may help mold. |
| rust-lld on Windows | Measured before at 6.7-7.1 s against 7.3-7.5 s for a widgets test rebuild and not adopted (`.cargo/config.toml:22-24`). The prototypes agree: about 30% on cold builds, little on incremental ones. |
| `-Zshare-generics=n` | Fixes the export ceiling, nightly-only. Not a project setting on a pinned stable toolchain. |
| Cranelift | About 30% faster codegen, nightly-only, no unwinding on Windows or macOS ([cg_clif](https://github.com/rust-lang/rustc_codegen_cranelift/blob/main/Readme.md)); tests that rely on `catch_unwind` break. A personal opt-in at most. |
| Two dylibs (engine stack, framework) | Halves the export pressure; unmeasured; doubles the lockstep artifacts and needs `prefer-dynamic` for std. |
| Profile tuning, as Zed does | Zed uses no dylib; it relies on `split-debuginfo`, `codegen-units` and opt-level 3 for proc-macros ([Zed Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/Cargo.toml)). FLUI already optimises dependencies and trims debuginfo. |
| Interpreted UI description, as Makepad does | Removes recompiles for UI edits entirely ([Makepad](https://github.com/makepad/makepad)); a different product decision, out of scope. |

## Recommendation

1. **Do not add dynamic linking before H1**, and never as a lever for framework or test
   builds. It is not a beta item.
2. **If it is added, add it in Bevy's shape**, never as a facade feature over a crate that
   depends on the facade.
3. **Ship it with its guard or not at all:** opt-level ≥ 2 for the dylib's constituent crates in
   every profile that builds it, and a Windows export-count gate.
4. **Keep expectations small in the docs:** about a second per app edit on Windows; nothing for
   framework edits; unmeasured elsewhere.
5. **Hot reload stays on Subsecond** (decision D15, ADR-0094). Keep PDBs on Windows. Record the
   two worker hazards until the dlopen path is deleted.

## If adopted: the design

- **Crate.** `flui-dylib`, `crate-type = ["dylib"]`, in `crates/flui-dylib`. It depends on the
  core crates the facade re-exports, never on the facade and never on an official package. Its
  source is one `use <crate> as _;` per direct dependency, under
  `#![allow(unused_imports)]`, nothing else. Today it would sit in layer 9 beside `flui-app`
  (`Cargo.toml:91-103`) with `wasm = false`; under
  [ADR-0081](../docs/adr/ADR-0081-workspace-tiers-and-reach-facts.md) it would sit in tier H with
  kind `internal`, ordered before the facade: ADR-0081 has no dev-only kind, and `tool` means
  "never a dependency" (ADR-0096 leaves this to be settled on adoption). It must be published in lockstep with the facade,
  because an application from crates.io resolves the facade's optional dependency.
- **Feature.** `dynamic-linking = ["dep:flui-dylib"]` on the facade, with the dependency under
  `[target.'cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))'.dependencies]`,
  and in `src/lib.rs` `#[cfg(feature = "dynamic-linking")] use flui_dylib as _;`. The facade's
  own 403 lines (`src/*.rs`) stay static, which is harmless.
- **Official packages** (Material, Cupertino and the rest under
  [ADR-0088](../docs/adr/ADR-0088-official-packages-sdk-and-facade.md)) stay outside the dylib
  and link statically against the core crates it exports. Acceptance must build that shape
  once, because neither prototype did.
- **`flui create`.** The templates today carry only `[profile.release]`
  (`crates/flui-cli/src/templates/basic.rs:47-51`). Every template gains
  `[profile.dev.package."*"] opt-level = 3`, which in an application project covers the FLUI
  crates and so keeps the dylib under the export limit. That setting is worth having without
  dynamic linking too; FLUI's own workspace already does it for its dependencies
  (`Cargo.toml:807-813`). The template does not enable `dynamic-linking` by default.
- **`flui run`.** It launches through `cargo run` (`crates/flui-cli/src/commands/run.rs:318-319`),
  which puts the std DLL on the search path. An opt-in in `flui.toml` or a `--dynamic-linking`
  flag adds `--features flui/dynamic-linking` on desktop targets only, and never together with
  `--release`, a device target or the browser.
- **Inside this workspace.** Workspace crates stay at opt-level 1 for contributor rebuilds, so an
  example built here with the feature needs its own profile (a `dylib` profile inheriting `dev`
  with `opt-level = 2`, or per-package overrides); which one is cheaper is unmeasured.
- **Release safety.**
  - `flui build` never passes the feature.
  - The facade refuses a release build with it:
    `#[cfg(all(feature = "dynamic-linking", not(debug_assertions)))] compile_error!(…)`. Any gate
    that builds the facade in release with `--all-features` then has to leave this feature out.
  - A reach fact (ADR-0081 §2, where today's `TREE_FACTS` at `tools/xtask/src/tasks/facade.rs:53`
    move) states that `flui-dylib` is absent from the facade's default normal graph, as
    `TREE_FACTS` already states for `flui-hot-reload`.
  - The feature is not part of any Stable surface; it adds no `pub` item.
- **Gate.** A `cargo xtask` command builds `flui-dylib` on Windows with every facade feature in
  the profile the template uses, reads the export table (the `object` crate is already in the
  lockfile), and fails above a budget of 55,000 exports. It needs a step in a Windows CI job;
  today the Windows jobs are heavy-only (`gpu-test` at `.github/workflows/ci.yml:949` and
  `platform-windows` at `ci.yml:1049`),
  and adding the step is a workflow change for the owner to approve.
- **Docs.** `docs/getting-started.md` gains the `PATH` note for `cargo test --doc` and for
  launching the executable outside `cargo run`, and the expected gain stated as measured.

## Not verified

- Any Linux or macOS number.
- Framework-edit rebuild cost with the FLUI crates at opt-level 2, and the export count at
  opt-level 2 with every facade feature.
- The recommended shape: dylib over the core crates, facade and official packages static.
- Subsecond with a framework dylib on Windows.
- The two dlopen worker hazards.
- A third-party-only dylib for test targets.
- Debugger and PDB behaviour across the dylib boundary.

## Sources

- Bevy: [bevy_dylib docs](https://docs.rs/bevy_dylib),
  [bevy_dylib Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_dylib/Cargo.toml),
  [bevy_dylib lib.rs](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_dylib/src/lib.rs),
  [bevy Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/Cargo.toml),
  [setup](https://bevy.org/learn/quick-start/getting-started/setup/),
  [cheat book](https://bevy-cheatbook.github.io/setup/bevy-config.html),
  [0.17 notes](https://bevy.org/news/bevy-0-17/),
  [#1110](https://github.com/bevyengine/bevy/issues/1110),
  [#14930](https://github.com/bevyengine/bevy/issues/14930),
  [PR #2016](https://github.com/bevyengine/bevy/pull/2016),
  [#17723](https://github.com/bevyengine/bevy/issues/17723),
  [#22654](https://github.com/bevyengine/bevy/issues/22654),
  [#13510](https://github.com/bevyengine/bevy/discussions/13510),
  [bevy_egui#22](https://github.com/vladbat00/bevy_egui/issues/22).
- Rust: [reference, linkage](https://doc.rust-lang.org/reference/linkage.html),
  [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html),
  [rust#59752](https://github.com/rust-lang/rust/pull/59752),
  [nextest env vars](https://nexte.st/docs/configuration/env-vars/),
  [cg_clif](https://github.com/rust-lang/rustc_codegen_cranelift/blob/main/Readme.md).
- Others: [Embark rust-ecosystem#13](https://github.com/EmbarkStudios/rust-ecosystem/issues/13),
  [Robert Krahn on dylibs](https://robert.kra.hn/posts/2022-09-09-speeding-up-incremental-rust-compilation-with-dylibs/),
  [subsecond](https://docs.rs/subsecond/latest/subsecond/),
  [dioxus#4154](https://github.com/DioxusLabs/dioxus/issues/4154),
  [Fyrox hot reloading](https://fyrox-book.github.io/beginning/hot_reloading.html),
  [Zed Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/Cargo.toml),
  [Makepad](https://github.com/makepad/makepad).
- In this repository: [ADR-0096](../docs/adr/ADR-0096-dev-build-dynamic-linking.md),
  [ADR-0094](../docs/adr/ADR-0094-hot-reload-through-subsecond.md),
  [the hot-reload guide](../docs/hot-reload.md), [CHANGELOG](../CHANGELOG.md), and the review's
  [verification record](../docs/research/2026-09-25-architecture-review/judges-and-verification.md).
