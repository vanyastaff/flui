# H7 — Speculative-scaffolding feature-gating audit

**Date:** 2026-06-30
**Tracker row:** Cross.H `H7` ("Speculative-scaffolding feature-gating")
**Exit gate:** *"feature flags audited; no leak of speculative code into stable builds."*

## Method

Surveyed all workspace crates for `#[cfg(feature = …)]` / `cfg!(feature = …)` usage and
cross-checked each referenced feature against the declaring crate's `[features]` table.
Exercised the high-feature crates with `cargo hack --each-feature` and `--no-default-features`,
and confirmed the baseline gates (`cargo check --workspace --all-targets`, clippy
`-D warnings`, `scripts/port-check.sh`). The audit was run via the `rust-studio:tooling-lead`
agent; findings were independently re-verified before any edit.

`target_feature` references (`sse2`, `neon`, `atomics`) are CPU-feature gates, not Cargo
features, and are out of scope.

## The one real leak (fixed)

**`crates/flui-devtools/src/lib.rs` — `pub mod profiler;` was ungated.** The `profiling`
feature only gated the crate-root re-export and the prelude; the 614-LOC `profiler` module
itself compiled into *every* build, including `--no-default-features`, while `default = []`.
So `flui_devtools::profiler::*` was reachable with no feature enabled — a speculative-surface
leak and an inconsistency with the already-gated `timeline`/`hot-reload` modules.

Fix: gate the module with `#[cfg(feature = "profiling")]` (matching `timeline`/`hot-reload`),
correct the two doc lines that wrongly claimed profiling was a default feature, and add a
`[[example]]` entry so `examples/profiler_demo.rs` (which imports `flui_devtools::profiler`
unconditionally) carries `required-features = ["profiling"]` and is skipped in the default
build instead of failing `--all-targets`.

Consequence (intended): the profiler's unit tests now run only with `profiling` enabled,
consistent with every other gated feature in the workspace. Verified: `cargo nextest run -p
flui-devtools --features profiling,timeline,hot-reload` → **23 passed / 0 failed** (incl. the
5 `profiler::tests::*`).

## Implicit-dependency-feature leaks (fixed)

Three features enabled an optional dependency by its bare name, which under resolver 2 creates
an implicit same-named feature that downstream crates can toggle — leaking an internal
dependency choice as public API. Converted to the explicit `dep:` form (behavior-preserving;
no downstream crate referenced the implicit features):

- `crates/flui-platform/Cargo.toml`: `desktop = ["dep:winit"]`, `winit-backend = ["dep:winit", "dep:arboard"]`
- `crates/flui-foundation/Cargo.toml`: `pretty = ["dep:tracing-forest"]`

## Manifest hygiene (fixed)

- `crates/flui-scheduler/Cargo.toml`: `serde` now uses `{ workspace = true, optional = true }`
  instead of a per-crate `version = "1.0"` pin, matching every other member and removing
  version-drift risk.

## Dead feature flags (documented; not a leak; left for the maintainer)

`crates/flui-app/Cargo.toml` declares five features that gate no source code
(`android`, `ios`, `web`, `debug-overlay`, `performance-overlay`); `cargo hack --each-feature`
confirms every variant compiles identically. `crates/flui-platform/Cargo.toml` similarly has
`web`/`wayland`/`x11` placeholders.

These do **not** breach the H7 gate — a flag that gates nothing ships no speculative *code*.
The platform flags are forward-planning aligned with the Cross.P roadmap rows (P1–P6), and
removing roadmap-committed feature names is a maintainer scope decision, not a mechanical
hygiene fix. They are recorded here so they are not mistaken for a defect; wire them to real
`cfg()` guards when the corresponding backends/overlays land, or drop them then.

## Verification

```
cargo check --workspace --all-targets                         exit 0  (example skipped by default)
cargo check -p flui-devtools --features profiling --all-targets exit 0  (module + example build)
cargo check -p flui-foundation --features pretty               exit 0
cargo check -p flui-scheduler  --features serde                exit 0
cargo clippy --workspace --all-targets -- -D warnings          exit 0
scripts/port-check.sh -v                                       exit 0  (21 triggers + FR-033 + N-geom.U16 + Cross.H2/H3/H7)
cargo fmt --all -- --check                                     exit 0
cargo nextest run -p flui-devtools --features profiling,timeline,hot-reload   23 passed / 0 failed
```

## Outcome

The single speculative-code leak is closed and the feature surface is audited end-to-end.
Remaining items (dead flags) are non-leaking and recorded for a maintainer decision. **H7: done.**
