# ADR-0096: Dynamic linking for development builds

- **Status:** Proposed (2026-09-25)
- **Date:** 2026-09-25
- **Related:** [ADR-0041](ADR-0041-workspace-topology-contract.md) and
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (where a `flui-dylib` crate sits),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the facade's feature set),
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (hot reload),
  [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (the reload
  whose images link this dylib),
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected Subsecond record),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (process-global state)
- **Revised:** 2026-11-23 — §3 puts every crate the facade re-exports, the official packages
  included, inside the dylib, because an application image (ADR-0394) links whatever is beside the
  dylib once per image; §2 and §5 follow it and no longer wait for a milestone or a code patcher;
  §4's count is a test of the crate, since `tools/xtask` is gone (ADR-0371).
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

## Decision

### 1. No dynamic linking for framework or test builds

FLUI does not build its own crates, examples, tests or CI through a framework dylib. The
CHANGELOG's "next lever" is withdrawn: the measurement shows framework edits get slower, test
binaries cannot use it, and CI builds without incremental compilation
(`Cargo.toml:788-791`), where the dylib is only extra output.

### 2. App-side dynamic linking waits for its preconditions

No `dynamic-linking` feature exists yet. It is added as a desktop-only, dev-only convenience for
application authors, in the shape of §3, when all of these hold:

- the export count of §4 is measured by its test, and the nightly Windows run executes it;
- the profile `flui create` writes keeps the dylib's crates at opt-level ≥ 2, and the export
  count with every feature of `flui-dylib` at that opt-level, the official packages included, is
  measured under the budget;
- the shape of §3 (the core crates and every crate the facade re-exports in the dylib; the facade
  linked statically beside it) has been built once and runs;
- the rebuild of an app edit is re-measured on Windows and Linux on an otherwise idle host and,
  as the relink of an executable or as an image of ADR-0394, is at least 30% shorter than the
  static build's;
- where it is built for the reload of ADR-0394, two images of one application have been loaded
  against it in one process.

### 3. The shape, if adopted

- A crate `flui-dylib` with `crate-type = ["dylib"]`. It depends on every crate the facade
  re-exports, never on the facade, so no cycle forms: the core crates always, and each optional
  one under a feature that has the facade's name for it: the official packages (`material`,
  `cupertino`, `devtools`) and the capability crates (`system`, `images`). An official package
  builds on `flui-sdk` alone (ADR-0088), so the dylib can hold it. The dylib's source is one
  `use <crate> as _;` per direct dependency. It is published in lockstep with the facade and
  declares `wasm = false`.
- Whatever is linked beside the dylib is linked once into every artifact that uses it: the
  executable, and each image of ADR-0394. A package left there would be compiled into every image,
  and its statics and `TypeId`-keyed tables would start over with each. So the crates outside the
  dylib are the facade, the reload package (in the executable alone) and the application's own
  dependencies (ADR-0394 §5).
- A facade feature `dynamic-linking = ["dep:flui-dylib"]`, with the dependency only under
  `cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))`, and
  `#[cfg(feature = "dynamic-linking")] use flui_dylib as _;` in the facade. A facade feature that
  re-exports an optional crate turns on the dylib's feature of the same name
  (`material = ["dep:flui-material", "flui-dylib?/material"]`), so a crate is in the dylib exactly
  when the application can name it. No `-C prefer-dynamic`.
- §4's count decides whether the optional crates share the core crates' dylib or take a second
  one ("Alternatives considered").
- `flui create` writes `[profile.dev.package."*"] opt-level = 3`; the feature stays off by
  default. `flui run` adds the feature on desktop targets on request and whenever it reloads by
  image, never with `--release`, a device or the browser; it launches through `cargo run`, which
  puts the std DLL on the search path. `flui build` never adds it, so a built application links
  the framework and its packages statically.

### 4. The guard ships with the feature

- A test of `flui-dylib` builds it on Windows with every feature in the template's profile, counts
  the export table and fails above 55,000 exports. The nightly Windows run executes it
  (`nightly-platforms.yml`).
- The facade refuses a release build with the feature
  (`compile_error!` under `all(feature = "dynamic-linking", not(debug_assertions))`).
- `flui-dylib` is an optional dependency of the facade that no default feature turns on; the
  facade's manifest states it and its reader confirms it (ADR-0371).

### 5. The dylib is not a reload boundary

It is loaded once and never unloaded. Which code is loaded again, and how, is ADR-0311's hook and
the mechanism behind it: an image of ADR-0394 links this dylib, which is why one copy of the
framework's code and statics serves the executable and every image.

## Consequences

- Nothing changes in the code now. The CHANGELOG entry stands as history; this ADR is where the
  lever's verdict lives.
- Contributors keep the static build, which already relinks in 1.4-3.9 s on the dev host.
  Faster linking remains a per-machine choice (`.cargo/config.toml:11-24`).
- If the feature is adopted: one more crate published in lockstep (Bevy's `bevy_dylib` has
  lagged its releases, bevy#17723 and bevy#22654); a test the nightly Windows run executes; a documented `PATH` step
  for doctests and for launching the executable outside `cargo run`; and every facade
  combination that builds in release with `--all-features` must leave the feature out.
- rustc links each crate once per artifact, so a dylib holds exactly one copy of every
  process-global static. Dynamic linking does not split UI runtime or registry identity and needs no
  entry in ADR-0097's allowlist.
- The reading of the dlopen worker in the study found two unreported hazards (the old image
  unmapped before the reassemble drops its views; the worker's own `REQUEST_REBUILD`,
  `packages/flui-hot-reload/src/dispatch.rs:24`). Both are unrun hypotheses and belong to the
  hot-reload work, not to this decision.

## Alternatives considered

- **Adopt now, as the review proposed.** Rejected: at the default feature set the dylib uses
  98.2% of the Windows export table and every new feature or widget eats the rest; with every
  facade feature it already fails to link. Framework contributors, the people building FLUI
  today, would get slower rebuilds.
- **A `dynamic-linking` facade feature over a dylib that depends on the facade.** Rejected: a
  Cargo cycle, the shape the review reproduced: a crate that depends on the facade while the
  facade names it optionally.
- **`-Zshare-generics=n` or Cranelift.** Rejected as project settings: both need nightly, and
  Cranelift has no unwinding on Windows or macOS.
- **Two dylibs (engine stack and framework).** Not measured; halves the export pressure but
  doubles the lockstep artifacts and needs `prefer-dynamic` to keep one copy of std.
- **The official packages linked statically beside the dylib.** Rejected: it serves an executable
  that is the only artifact, and an application image is a second one. Material would be compiled
  into every image and its statics would start over with each reload.
- **A third-party-only dylib for test targets** (wgpu, naga, `windows`, the font crates). Not
  measured. Worth a `cargo build --timings` run before it is considered; it is outside this
  decision.
- **Reject outright.** Not chosen: on Linux, with no export limit, and for application authors
  whose framework is a dependency at opt-level 3, the app-edit gain may be larger than on this
  host. §2's preconditions decide it with measurements instead.

## Not decided here

- The workspace's own build footprint. Dynamic linking changes only the final link of an
  application; it does nothing for the size of `target/`, the number of test binaries, duplicate
  builds of the upper stack, or peak memory. Those belong to the build-footprint study
  ([open questions, "Still open"](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/design/open-questions.md#still-open)): `target/` size by
  artifact kind, test binary count, duplicate builds from per-crate `testing` features, peak
  memory, per-worktree targets, and the levers (test consolidation, sccache, split debuginfo,
  nextest archives, cargo-sweep, a shared target).
- Whether the opt-in lives in `flui.toml`, on the `flui run` command line, or both.
- Which in-workspace profile builds an example with the feature, if one ever needs to.
- The tier and kind of `flui-dylib`. ADR-0081's kinds do not fit it cleanly: `tool` means "never
  a dependency", and the facade depends on it. The proposal is tier H with kind `internal`,
  ordered before the facade, and `publish` in lockstep with it; ADR-0081 is amended when the
  feature is adopted.
