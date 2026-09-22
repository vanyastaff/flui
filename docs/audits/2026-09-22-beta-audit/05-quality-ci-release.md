# FLUI Quality Infrastructure, CI & Release Readiness Audit
Date: 2026-09-22 · Workspace: 28 crates (`/Users/vanyastafford/Develop/flui`)

## 1. Test inventory

- `#[test]` functions: **10,200** across the workspace (rg count).
- `#[tokio::test]`: **45**.
- `#[ignore]`/`#[ignore = "..."]`: **27**, all with stated reasons or clear intent. Buckets:
  - Platform-hardware-gated (needs real AppKit run loop / real window / real WGPU device / Xcode SDK): macOS window routing tests (3, `flui-platform/src/platforms/macos/window.rs`), Windows clipboard/window creation (2), `flui-engine/src/offscreen/mod.rs` wgpu Device/Queue tests (6), iOS artifact tests needing Xcode + installed targets (3, `flui-cli/src/build/tests/ios_artifacts.rs`).
  - Explicit run-manually markers: `flui-platform/tests/performance.rs` (3, run via `--ignored`), `flui-assets/src/loaders/network.rs` (1, needs internet).
  - Stale scaffolding: `flui-platform/tests/integration_template.rs` has 4 `#[ignore] // Remove #[ignore] when implementing` placeholders — dead test skeletons, not real coverage.
  - `flui-platform/tests/display_enumeration.rs`: 1 test needs manual window dragging.
  - No un-explained/undocumented `#[ignore]`s found — every one names why.

- Per-crate `#[test]` counts (function-body count, not assertions): flui-widgets 1446, flui-objects 996, flui-rendering 956, flui-material 729, flui-view 789, flui-types 656, flui-engine 631, flui-scheduler 528, flui-interaction 521, flui-app 529, flui-platform 428, flui-animation 307, flui-geometry 250, flui-foundation 243, flui-semantics 210, flui-painting 174, flui-tree 126, flui-layer 114, flui-cupertino 94, flui-log 87, flui-cli 76, flui-testing 74, flui-assets 69, flui-build 53, flui-devtools 37, flui-hot-reload 16, flui-localizations 8, flui-macros 7.
  - **Thin on tests**: `flui-macros` (7 — it's a near-skeleton proc-macro crate per workspace comments), `flui-localizations` (8, small catalog crate but is on the publish list), `flui-hot-reload` (16, despite being architecturally load-bearing — dlopen/hot-reload lifecycle — and the subject of multiple recent "resolved" memory notes about lifecycle bugs). `flui-devtools` (37) is also comparatively light for a crate with `[[bench]]` targets and a growing profiler/timeline surface.
  - Top-level `tests/` directory (workspace-level integration/demo tests): `composited_layer_update_readback.rs`, `cupertino_demo.rs`, `demo_layer_snapshots.rs`, `facade_smoke.rs`, `material_demo.rs`, `vertical_slice_demo.rs`, plus a `snapshots/` golden-file dir.

### docs/testing.md taxonomy

Documents a layered test-tier model (not one harness): Diagnostics → Painting → Layer → Render object → **Frame** (whole headless frame via `flui_testing::HeadlessBinding`) → **Widget** (`flui_widgets::testing`) → Accessibility → Gesture replay → Log capture → GPU readback → Demo composition (`tests/demo_layer_snapshots.rs`) → Live E2E (`tools/live-smoke`, real X11/Wayland). Explicit rule: test-only APIs live in `flui-testing`, not behind a `testing` feature on a shipped crate (with a documented, narrow exception for widget-mounting helpers). wasm32 is called out as "compiled everywhere, executed in one place" — most wasm-touching commands only prove `cargo check`/clippy/link, not execution; `just wasm-test` is the one recipe that actually runs assertions on wasm32 (via `wasm-bindgen-test-runner` + node), and it self-guards against silently-inert "0 passed" (a documented past real bug, issue #985 — `web_time::Instant` swapped for `std::time::Instant` and every compile-only gate stayed green while the code panicked at runtime).

### `just ci` / justfile test/gate recipes

`just ci` (per docs/testing.md, mirrored in justfile's `ci:` recipe) runs, in order:
1. `cargo fmt --all -- --check` (fmt-check)
2. `scripts/check-workspace-inventory.sh` (crate inventory + layer-policy drift guard)
3. `scripts/check-runtime-conformance.sh` (docs/runtime-contract.toml vs source tree)
4. `scripts/port-check.sh` (architecture refusal triggers)
5. `cargo clippy --workspace --all-targets -- -D warnings`
6. `cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast`
7. `FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast` (Linux only; skipped elsewhere with a message)
8. `cargo test --workspace --locked --doc`
9. `scripts/doc-strict.sh` (`cargo doc --no-deps --document-private-items` with every `testing` feature on)

Other named justfile test/build recipes of note: `test-ci` (CI-mirroring subset), `test-crate`, `test-name`, `test-debug`, `test-all`, `wasm-link-check` (really links the two wasm cdylibs + import-surface check via `scripts/check-wasm-imports.sh`), `cross-typecheck` (clippy-only, no link, of flui-platform's Windows/macOS/Android/iOS backends with `-D warnings` — the only gate that ever compiles those backends from a non-native host), plus **7 macOS-only, host-gated executable probes** that build a staged `.app` bundle and assert on a printed `_RESULT=PASS` marker: `macos-close-path`, `macos-frame-pump`, `macos-resize-jitter`, `macos-ime`, `macos-a11y` (implied by memory notes), `ios-sim`, plus `live-smoke` / `live-smoke-wayland` (X11/Wayland real-window E2E via `xvfb-run`/weston). These document real, hard-won platform bugs they were built to catch (e.g. the AppKit frame-pump defect, the iOS "frozen but logging frames" white-screen bug) — but **none of the macOS/iOS ones run in the hosted CI matrix** (see §2); they are `just`-only, dependent on a developer's real Mac.

## 2. CI

`.github/workflows/ci.yml` (1418 lines) — job graph per its own header comment, gated on a fast `checks` job:
- `checks` — fmt + taplo + typos + inventory + runtime-conformance + panic-policy + port-check + actionlint + zizmor (seconds, no compile)
- `clippy` — workspace lint, `-D warnings`
- `feature-matrix` — cargo-hack per-feature clippy (`--each-feature --optional-deps`), 4-slice matrix (3 package groups + combination checks) + flui-engine backend-feature-pair powerset + every `flui` facade feature combo, xilem-style
- `wasm-check` — `cargo check` for wasm-capable crates + real cdylib link of the two web demos + import-surface check vs `scripts/wasm-import-allowlist.txt`
- `cross-typecheck` — clippy-only (no link/test) of flui-platform's Windows/macOS/Android backends **(no iOS in the hosted CI job, unlike the justfile's `cross-typecheck` recipe which adds `aarch64-apple-ios`)**
- `test` — build + nextest (lib+integration) on ubuntu, plus a dedicated flui-platform (Linux-runnable subset) step
- `gpu-test` — full GPU readback suite on **windows/WARP, merge-blocking** (timeout 60 min — the longest single job)
- `bench-compile` — criterion benches compile+link only (no perf assertions)
- `doc` — rustdoc, `RUSTDOCFLAGS=-D warnings`
- `deny` — cargo-deny advisories/bans/licenses/sources
- `doc-test` — rustdoc examples executed as tests
- `msrv` — `cargo check` on declared 1.97 MSRV
- `miri` — UB check on flui-rendering's PipelineOwner and flui-view's GlobalKey plane, explicitly **advisory** (non-blocking)
- `ci` — aggregator: the single required check for branch protection

Runners observed: `ubuntu-latest` (majority — checks/clippy/feature-matrix/wasm/test/deny/doc/msrv/miri), `windows-latest` (gpu-test only, 60 min timeout), and matrix `${{ matrix.os }}` for two jobs (feature-matrix and one other, timeouts 25–40 min) which likely spans macOS too via matrix. **No dedicated macOS runner job for actual test execution appears in ci.yml** — the macOS-specific executable probes (frame-pump, IME, close-path, a11y, resize-jitter, ios-sim) are `just`-only and are NOT invoked from `ci.yml` at all; they require a real Mac with GUI session, which GitHub-hosted macOS runners (headless) generally cannot provide. This is a real CI blind spot given the memory notes document real macOS-only regressions that only these probes caught.

Timeouts range 5–120 minutes per job (most jobs 25–40 min; `gpu-test` 60 min; weekly's `bench` job 120 min).

Cache: Swatinem-style rust cache is referenced in the header comment ("Patterns borrowed from linebender/xilem... Swatinem cache + taiki-e/install-action for pinned tool versions").

PR vs. push vs. nightly: `ci.yml` triggers on `pull_request` and `push` to `main`. `.github/workflows/weekly.yml` (251 lines) runs on `schedule: "17 5 * * 1"` (Mondays) + `workflow_dispatch`, with jobs `advisories` (10 min), `latest-deps` (45 min), `bench` (120 min — perf regression baseline), `nightly-canary` (30 min), `release-lints` (45 min). `.github/workflows/coderabbit-trigger.yml` runs CodeRabbit AI review on PRs.

Gates confirmed present: clippy `-D warnings` (yes), doc build with `-D warnings` (yes), cargo-deny (yes, weekly `advisories` + presumably also part of `ci.yml`'s `deny` job), MSRV check (yes, 1.97), miri (yes, advisory/non-blocking, narrow scope — only 2 crates' specific modules), coverage tooling (**not found** — no llvm-cov/tarpaulin job in either workflow), semver-checks (**not found** — no `cargo-semver-checks` job), cross-target checks for Android/iOS/wasm (yes for Android+wasm via `cross-typecheck`/`wasm-check`; iOS present only in the local justfile recipe, absent from hosted `cross-typecheck` job).

### CI wall-time / recent runs (`gh run list --limit 15`)

Sampled runs from 2026-09-22 (today): "CI" completions ranged from ~11 min to ~28 min wall time (createdAt→updatedAt), consistent with the per-job timeout budget. One "CI" run at 08:09:14 had `conclusion: failure`; the very next run at 08:16:43 succeeded — suggests some flakiness/retry-on-push rather than a systemic red build. A `Release` workflow run at 05:42 completed successfully in ~3 minutes (tag-triggered smoke, see §3). Nothing looked stuck or perpetually red at sample time; one "CI" run was `in_progress` at the time of the snapshot.

## 3. Release readiness

- **Actual workspace version is `0.1.0`**, not 0.2.0. `Cargo.toml` `[workspace.package].version = "0.1.0"` (line 118), confirmed by `cargo metadata` (`flui-geometry 0.1.0`) and by `cargo package -p flui-geometry --no-verify --allow-dirty` succeeding and emitting `flui-geometry-0.1.0.crate`. **`CHANGELOG.md`'s own header claims "Workspace version: `0.2.0`"** — this is stale/wrong and should be fixed before any release messaging goes out; a git tag `v0.1.0` already exists (`git tag`: `backup/hot-reload-parity-macos-ios-12156070`, `v0.1.0`), consistent with the real 0.1.0, not the changelog's 0.2.0 claim.
- `publish` settings: every one of the 27 crates checked declares `publish = ["crates-io"]` explicitly (no `publish = false` outliers found) — deliberate, uniform.
- Internal dependency graph is **publish-clean**: every real (non-dev) `flui-*` path dependency across the workspace carries a matching `version = "=0.1.0"` pin (verified across all `[dependencies]` blocks, ~90 internal edges checked) — this is exactly the shape `cargo publish` needs (a path dep without a version fails publish). The only unversioned path deps found are dev-dependencies (dropped from the published manifest by design, e.g. `flui-cli`'s `flui-hot-reload` dev-dep, `flui-foundation`'s `flui-macros`/`flui-types` dev-deps) — explicitly commented as intentional in the manifests.
- **`cargo package -p flui-geometry --no-verify --allow-dirty` succeeded** (36 files, 566.8 KiB / 109.7 KiB compressed) — basic manifest mechanics for a leaf crate work. This was not run with `--verify` (full build) and not attempted for crates deeper in the dependency graph (e.g. `flui-app`, `flui-widgets`) which would take much longer and pull in the full internal graph at pinned `=0.1.0` versions — none of which are actually published yet, so a real `cargo publish --dry-run` for any crate beyond the leaves (flui-geometry, flui-types, flui-tree, flui-macros, flui-log) will fail today with "dependency ... not found on crates.io" until the lower crates are published first, in dependency order.
- `CHANGELOG.md` follows Keep a Changelog format correctly, is large (53KB) and detailed, with an `[Unreleased]` section actively accumulating (2026-09 entries present) — good hygiene, aside from the version-number drift noted above. It explicitly states "FLUI is pre-release and not published to crates.io."
- `.github/workflows/release.yml` exists and is tag-triggered (`push: tags: ["v*"]` + `workflow_dispatch`): builds `flui-cli` release binaries for 5 targets (linux x64/arm64, macOS x64/arm64, windows x64), smoke-tests each binary, packages `.tar.gz`/`.zip` archives plus `SHA256SUMS`, and creates a **draft** GitHub release (publishing the draft is a manual human step; tags containing `-` are marked pre-release). This is a binary-distribution pipeline for the `flui` CLI tool, **not** a `cargo publish` pipeline — no crates.io publish step exists anywhere in the workflows.
- `cargo-binstall` support: `[package.metadata.binstall]` is referenced in `flui-cli/Cargo.toml` per the release workflow's own comments (archive naming must match); not independently verified line-by-line here.
- Licensing: dual MIT/Apache-2.0 (`LICENSE`, `LICENSE-APACHE` present), `NOTICE` file present and unusually careful — explicitly documents FLUI's relationship to Flutter (behavior-reference only vs. ported-test-scenario files, which are enumerated by crate) and disclaims Google/Flutter affiliation. This is well above average NOTICE hygiene for a pre-1.0 project.

**Bottom line on crates.io readiness: manifests are mechanically ready (versions, `publish` flags, and internal version pins are all consistent), but nothing has ever actually been published** — the dependency-order bootstrapping (leaves first) has not been done, there is no CI job that runs `cargo publish --dry-run`, and the changelog's version claim doesn't even match the real manifest version. Treat this as "publish-ready plumbing, unexercised."

## 4. Quality gates present

All of the following exist at the repo root and appear to be **CI-enforced, not just `just`-only**, based on `ci.yml`'s `checks` job listing (fmt, taplo, typos, inventory, runtime-conformance, panic-policy, port-check, actionlint, zizmor) plus dedicated `clippy`/`deny`/`doc`/`msrv` jobs:
- `deny.toml` (7.2K) — cargo-deny 0.19 schema, `[graph] all-features = true` (deliberately covers feature-gated deps like `reqwest` behind `flui-assets/network`), advisories (`yanked = "warn"`, `unmaintained = "all"`, `unsound = "all"`), vulnerabilities always hard-error under 0.19.
- `clippy.toml` — MSRV-aware (`msrv = "1.97"`), documents `unwrap`/`expect` exemption for `#[test]`/`#[cfg(test)]` per `docs/PANIC-POLICY.md`.
- `rustfmt.toml`, `typos.toml` (excludes generated/vendored/preserved-verbatim doc dirs), `.coderabbit.yaml` (32K — AI review config pointing at AGENTS.md/docs/PORT.md/docs/FOUNDATIONS.md as its knowledge base, with a "no praise, one finding per comment" tone instruction).
- `docs/PANIC-POLICY.md` exists and has a dedicated CI check (`scripts/check-panic-policy.sh`, 40.9K — substantial).
- `scripts/` holds ~30+ purpose-built conformance/probe scripts beyond the obvious ones: `check-workspace-inventory.sh` (36K), `check-runtime-conformance.sh` (36.9K), `port-check.sh`, `check-wasm-imports.sh`, `check-adr-citations.py`, `check-hot-reload-loop.py`, `check-ios-*` (5 scripts), `check-macos-*` (5 scripts), `check-wgsl-uniformity.py`, `check-window-show.py`, `doc-strict.sh`. These are unusually thorough for gating architectural drift, not just style.

## 5. Benchmarks

10 crates declare `[[bench]]` targets and depend on `criterion`: flui-animation, flui-engine, flui-painting, flui-interaction, flui-scheduler, flui-semantics, flui-rendering, flui-types, flui-view, flui-testing. CI's `bench-compile` job only compiles+links benches (no perf assertions/regression gate in `ci.yml`). Actual perf regression tracking lives in the **weekly** workflow's `bench` job (120 min timeout) — this is the real perf baseline/tracking mechanism, run weekly rather than per-PR, so a perf regression can land and ride for up to a week before weekly catches it.

## 6. Open issues triage (`gh issue list --state open --limit 300`)

**106 open issues.** Label coverage is partial (56 of 106 have no `area:` label), so bucketing below is a hybrid of labels + keyword search over titles:

| Bucket | Count |
|---|---|
| text / input / IME | 19 |
| layout / render / engine / perf | 19 |
| accessibility (a11y/semantics) | 17 |
| other (no clear keyword match) | 23 |
| CI / lint / tooling | 8 |
| android | 5 |
| windows | 5 |
| material | 5 |
| navigation / routing | 3 |
| web/wasm | 1 |
| docs | 1 |
| ios / macos / cupertino | 0 explicit keyword hits (folded into platform/other) |

Label-based counts: `area: architecture` 31, `bug` 21, `priority: medium` 15 (+3 differently-formatted `priority:medium`), `testing` 14, `priority: high` 14, `enhancement` 13, `performance` 8, `area: pipeline` 6, `priority: critical` 5, `documentation` 4, `architecture` 3 (separate/duplicate of `area: architecture`?), `tech-debt` 1, `priority: low` 1.

**Critical (5):**
- #1043 engine: preserve native window lifetime in safe Renderer construction
- #566 Track Runtime.1 dependency order and exit evidence
- #563 Build native and headless multi-window runtime conformance infrastructure
- #551 Make event-loop authority explicit with OwnerPlatform and typed proxy commands
- #546 Complete the Flutter 3.44.0 widget fidelity corpus and close Business.1

**High (14, selected):** #1092 (winit key mapping to W3C codes), #1091 (text-layout geometry without full relayout), #1041/#1039/#1037 (layout/rendering perf-shape work), #1040 (stale focus notifications on reentrant changes), #565/#564/#562/#560/#559/#558 (Runtime.1 program — execution/lifecycle/GPU-services architecture), #542 (routing/named routes/PopScope), #540 (EditableText selection/multiline/obscured).

Notable: a large fraction of critical/high issues cluster around a named internal initiative ("Runtime.1") concerning event-loop authority, execution services, and multi-window conformance — this looks like the project's current major architectural front, not scattered debt.

## 7. GitHub health

- **PR velocity**: sampled the 30 most recently merged PRs — spans roughly 2026-09-16 through 2026-09-22 (today), i.e. **~30 PRs merged in ~6 days**, an unusually high merge cadence (5+/day), consistent with heavy automated/agent-assisted contribution.
- **Contributors** (`git shortlog -sne`): dominated by one human identity across multiple email aliases — `vanyastaff <ivan.kondrashkin@gmail.com>` (2839 commits), `vanyastaff <24690216+vanyastaff@users.noreply.github.com>` (243), `vanyastaff <noreply@anthropic.com>` (81) — clearly the same person authoring through different tools/setups. Additional identities: `Claude <noreply@anthropic.com>` (257 commits — AI pair-programmer attribution), `dependabot[bot]` (14), `el Gentleman <elgentleman@flui.dev>` (6, the only apparent second human).
- **Bus factor: effectively 1.** All substantive commits trace to a single maintainer (across aliases) plus AI-assisted commits attributed to Claude under that maintainer's direction. One other contributor (`el Gentleman`) has only 6 commits. This is a significant risk factor for a project heading toward a public beta/release — no redundancy in domain knowledge, review, or maintenance capacity.

---

## Executive summary (for the top-level report)

**Publishable to crates.io as-is: NO.** Manifest mechanics are actually in good shape — every crate declares `publish = ["crates-io"]`, and all ~90 internal path dependencies carry the exact matching `version = "=0.1.0"` pin that `cargo publish` requires, verified by a live `cargo package -p flui-geometry --no-verify --allow-dirty` (succeeded, 0.1.0). But three things block an actual release today: (1) **nothing has ever been published to crates.io**, so any crate beyond the graph's leaves (flui-geometry, flui-types, flui-tree, flui-macros, flui-log) will fail `cargo publish --dry-run` because its pinned `=0.1.0` internal deps don't exist on the registry yet — leaf-first bootstrapping has to happen in dependency order and no CI job automates or even dry-run-verifies this; (2) **CHANGELOG.md's own header claims workspace version `0.2.0`**, contradicting the real `0.1.0` in `Cargo.toml`/`cargo metadata`/the `v0.1.0` git tag — release messaging is already inconsistent with itself; (3) the only existing `release.yml` builds and drafts prebuilt `flui` CLI binaries (5 targets, tag-triggered) — there is no crates.io publish workflow at all.

**CI blind spots:** no coverage tooling (llvm-cov/tarpaulin) in any workflow; no `cargo-semver-checks` job despite pre-1.0 API churn; miri is narrow and explicitly advisory (2 crates, 2 modules); perf regression detection lives only in the **weekly** cron (120 min bench job), so a regression can ride a week before being caught; and most importantly, **no hosted-CI execution of the macOS/iOS-specific probes** (frame-pump, IME, close-path, resize-jitter, a11y, ios-sim) that the project's own memory notes credit with catching several real, subtle platform bugs — these are `just`-only and depend on a developer's real Mac with an active GUI session, which GitHub's macOS runners can't provide headlessly. iOS is also missing from the hosted `cross-typecheck` job (present only in the local justfile variant).

**Top 8 quality-infra tasks before beta:**
1. Fix the CHANGELOG/Cargo.toml version-number contradiction (0.1.0 vs. claimed 0.2.0) before any release announcement.
2. Add a CI job (even weekly/manual) that actually runs `cargo publish --dry-run` in dependency order across all 27 publishable crates, to catch registry-readiness regressions before a real release attempt.
3. Add a crates.io publish workflow (leaf-first ordering script), separate from the existing CLI-binary `release.yml`.
4. Close the macOS/iOS hosted-CI gap — either a self-hosted macOS runner or a documented, enforced "release cannot ship without a fresh local run of the macOS/iOS `just` probes" gate, since these are the only coverage of real regressions already found once.
5. Add `cargo-semver-checks` (or equivalent) now, before the API surface stabilizes toward 1.0 — retrofitting semver discipline after the fact is much harder.
6. Add coverage measurement (llvm-cov) with a tracked baseline, especially given `flui-macros`, `flui-hot-reload`, and `flui-localizations` are visibly thin on tests relative to their architectural importance.
7. Move perf-regression detection from weekly-only to per-PR (or at least per-merge-to-main) for the hot paths already benchmarked (rendering/scheduler/interaction), given #1037/#1039/#1041/#1043 show active perf-sensitive work in flight.
8. Address the bus-factor risk directly — bring in/formalize a second reviewer or maintainer before beta, since 21 open bugs + 5 critical architecture issues (the "Runtime.1" initiative) currently depend on one person's continuity.
