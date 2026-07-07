# Changelog

All notable changes to the FLUI workspace are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
FLUI is pre-release and not published to crates.io; entries are grouped under
`[Unreleased]` until a first tagged release cuts them over. Workspace version:
`0.2.0` (all crates share `[workspace.package].version`). Fine-grained phase
history lives in [`docs/ROADMAP-TRACKER.md`](docs/ROADMAP-TRACKER.md); this
file records the repo-consumer-visible summary.

## [Unreleased]

### Added

- **`cargo-deny` CI job** (merge-blocking): the in-repo `deny.toml` was never
  executed in CI; the first wired run surfaced four real advisories —
  `anyhow` 1.0.102 unsound `downcast_mut` (RUSTSEC-2026-0190),
  `crossbeam-epoch` 0.9.18 invalid deref (RUSTSEC-2026-0204), and a
  `quick-xml` DoS pair (RUSTSEC-2026-0194/0195). Three fixed by lockfile
  bumps; the transitive quick-xml pair (build-time Wayland scanner) and the
  unmaintained `ttf-parser` notice carry documented ignores. `just deny`
  added.

- **CI gates**: integration tests now run in CI (previously `--lib` only —
  the Core.0/Core.2 exit-gate suites in `crates/*/tests/` were never
  executed); new `doc-test` job runs every rustdoc example; new `msrv` job
  verifies the declared 1.96 floor; new advisory `miri` job checks the
  `flui-rendering` subtree arena (the workspace's densest `unsafe` hot spot);
  the `gpu-test` WARP readback suite is promoted from advisory to
  merge-blocking after 3 consecutive green full-suite runs.
- **Panic policy** ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)):
  `Result` for caller-triggerable failures, `expect("BUG: <invariant>")` for
  internal invariants, enforced by `clippy::unwrap_used` at workspace level
  (tracked crate-level opt-outs burned down per quality wave).
- This changelog.

### Changed

- **Toolchain pin 1.96.0 → 1.96.1** (MIR miscompilation fix, cargo HTTP
  retries, three libssh2 CVEs in cargo). MSRV unchanged at 1.96.
- Lockfile: `wgpu` 29.0.3 → 29.0.4, `anyhow` → 1.0.103, `crossbeam-epoch`
  → 0.9.20, `swash` → 0.2.9 (off a yanked version). `clippy.toml` gains
  `msrv = "1.96"` so MSRV-aware lints track the declared floor.
- **Dev profile `debug = 1` → `"line-tables-only"`**: panic backtraces keep
  file:line, the variable/scope DWARF that ballooned `target/debug/deps`
  toward ~20 GB is gone, and the local default now matches what CI has built
  with since PRs #236/#242. `--profile dbg` remains the full-debuginfo
  opt-in. New `just sweep` recipe (cargo-sweep) prunes stale artifacts.

- **Lint normalization**: every workspace crate now inherits
  `[workspace.lints]` via `[lints] workspace = true` (12 crates previously
  bypassed workspace lints entirely; 3 carried stale local copies), enforced
  by a new drift guard in `scripts/check-workspace-inventory.sh`.
- `flui-assets` restored to `[workspace] members` — it is built and tested by
  CI again.

### Pre-changelog milestones

Recorded retroactively from `docs/ROADMAP-TRACKER.md`; evidence links live
there.

- **2026-07-01 — Core.2 exit**: full render-object catalog (37 concrete
  RenderBox/RenderSliver objects extracted to `flui-objects`), 250/250
  per-object harness tests, catalog CI guard.
- **2026-06-30 — Core.0 exit / Core.1 substantially delivered**: view/element
  core contracts locked (`specs/004-view-element-core`, keyed reconciliation,
  `IntoView` authoring surface, element storage); `flui-widgets` slice with 14
  widget families; `flui-animation` re-enabled; production vsync + lazy
  slivers end-to-end; C1.11 contract-validation report (4,847 tests passing).
  Core.1 formal exit still awaits a windowed run (C1.10/C1.12 — see the OPEN
  ITEM in the tracker).
- **2026-06 — GPU engine hardening**: WGSL readback/oracle suite (~440 tests)
  runs on CI via WARP; image-filter pipeline (blur/ColorFilter) sized to
  content bounds; deterministic-replay IR purity witness.
- **Business.1 (in flight)**: Flutter widget-catalog port continues
  (`RichText`/`Icon` landed); tracked in `docs/ROADMAP.md`.
