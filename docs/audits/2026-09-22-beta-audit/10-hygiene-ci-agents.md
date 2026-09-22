# FLUI Hygiene Audit — CI, Dev Tooling, Agent Rules
Date: 2026-09-22. Read-only audit, no files modified.

---

## 1. CI

### 1.1 Workflow files present
`.github/workflows/`: `ci.yml` (1518 lines, 82.7K — PR/merge gate), `weekly.yml` (advisory maintenance), `release.yml` (tag-triggered binary builds), `coderabbit-trigger.yml` (nudges CodeRabbit review since flui has <10 stars and lost the auto-review gate).

### 1.2 ci.yml job table

| Job | Runner | Toolchain | Cache | Timeout | Command(s) | `-D warnings`? | Notes |
|---|---|---|---|---|---|---|---|
| checks | ubuntu-latest | stable+rustfmt | none (no compile) | 10m | fmt, taplo, typos, check-workspace-inventory.sh, check-runtime-conformance.sh, check-panic-policy.sh, port-check.sh, check-wgsl-uniformity.py, actionlint, zizmor | n/a | Fast source gate; gates every other job |
| clippy | ubuntu-latest | stable+clippy | Swatinem, per-job key, save-if main | 25m | `cargo clippy --workspace --all-targets --locked` + `-p flui-engine --features testing` | yes | |
| test (matrix: ubuntu only) | ubuntu-latest | stable | Swatinem, save-if main | 40m | build --workspace, nextest (exclude flui-platform), then flui-platform under Xvfb+headless | yes (env) | Windows dropped from matrix ("day-to-day dev happens on Windows, paying wgpu/PDB cost in CI is redundant") — comment says re-add once workspace stabilizes |
| test-features | ubuntu-latest | stable | Swatinem | 30m | nextest for flui-assets/full, flui-widgets images/asset-images/network-images, flui facade cupertino+localizations | yes | Split out of `test` purely for parallelism (was serialized 203s) |
| live-smoke | ubuntu-latest | stable | Swatinem | 30m | Xvfb + weston real-window smoke (X11 + Wayland) | yes | |
| gpu-test | **windows-latest** | stable | Swatinem | 60m | nextest flui-engine `testing` GPU readback suite on WARP + composited-layer readback | yes | Merge-blocking; only Windows-runner GPU test; `--test-threads 1` |
| bench-compile (ubuntu only) | ubuntu-latest | stable | Swatinem | 25m | `cargo bench -p flui-rendering --no-run` | yes | Compile-only |
| doc | ubuntu-latest | stable | Swatinem | 25m | `scripts/doc-strict.sh` (RUSTDOCFLAGS=-D warnings) | yes | |
| deny | ubuntu-latest | n/a (cargo-deny) | none | 10m | `cargo deny check` + wgpu-bump ADR-0045 reopen probe | n/a | Has extra `pull-requests: read` permission for the probe |
| doc-test | ubuntu-latest | stable | Swatinem | 30m | `cargo test --workspace --locked --doc` | n/a | nextest doesn't run doctests, hence separate job |
| msrv | ubuntu-latest | 1.97 pinned | Swatinem | 25m | `cargo check --workspace --all-targets --locked` | n/a | |
| miri | ubuntu-latest | nightly+miri | Swatinem | 30m | 4 narrow `cargo +nightly miri test` invocations | n/a (warn) | `continue-on-error: true` — advisory |
| feature-matrix (4-way matrix: group-1/2/3/combinations) | ubuntu-latest | stable+clippy | Swatinem, per-slice prefix-key | 30m | cargo-hack per-feature clippy, 2 passes each group; flui-engine backend powerset; `just facade-combos` | yes | Split from 1 job into 4 for parallelism |
| wasm-check | ubuntu-latest | stable+wasm32 target | Swatinem | 30m | check/clippy wasm32, cdylib link, import-allowlist check, execute wasm32 tests via wasm-bindgen | yes | |
| cross-typecheck | ubuntu-latest | stable + win/mac/android/ios targets | Swatinem | 30m | clippy (no link) of flui-platform win32/macos/android/ios backends + app runners + CLI-on-Windows | yes | Never links/executes those backends |
| cli-macos | **macos-latest** | stable+iOS target | Swatinem (`cache-bin: false`) | 30m | nextest -p flui-cli, clippy iOS runner | yes | Only per-OS CLI coverage before release |
| ci (aggregator) | ubuntu-latest | n/a | n/a | 5m | Re-parses ci.yml with PyYAML to verify every job is in `needs` and none skipped | n/a | Single required branch-protection check (tokio pattern) |

**Concurrency**: `group: ${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress` only on `pull_request` events (not push/merge_group) — correct.

**Permissions**: workflow-level `contents: read` (least privilege); `deny` job additionally gets `pull-requests: read` for its wgpu-bump probe; `release.yml`'s `release` job gets `contents: write` scoped to just that job. This is good least-privilege discipline — no job elevates beyond what it needs.

**Path filters**: none. Every job runs on every PR regardless of what changed (e.g. a docs-only PR still runs gpu-test, miri, wasm-check, feature-matrix). This is the single biggest opportunity to cut CI cost/time; `checks` job comments imply this was considered ("Ubuntu only — pure source analysis") but no `paths:`/`paths-ignore:` exists anywhere in ci.yml.

**Action pinning**: all `actions/*` and `Swatinem/rust-cache` are pinned to full commit SHA with a version comment (e.g. `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`) — correctly hardened against tag mutation. `dtolnay/rust-toolchain@stable`/`@nightly`/`@master` are pinned to SHA too. `taiki-e/install-action` is SHA-pinned once per job (repeated ~15 times) with per-tool version pins inside (e.g. `zizmor@1`, `cargo-nextest@0.9`). Consistent and correctly hardened workflow — zizmor itself runs in `checks` to keep this honest.

**Tools installed from source every run (no binstall/cache)**:
- `actionlint` — downloaded via `curl`+`tar` from GitHub releases every `checks` run (not through `taiki-e/install-action` because it "has no recipe for it" — comment explains why). No caching of the binary across runs.
- `wasm-bindgen-cli` — `cargo install wasm-bindgen-cli --version "$version" --locked` in `wasm-check` and in `weekly.yml`'s `cli-live-build` — this is a **from-source cargo install**, not a prebuilt binary, and is not cached; every run recompiles wasm-bindgen-cli from scratch, which is expensive for a leaf tool.

**macOS runner usage**: only `cli-macos` (30m budget) in ci.yml — reasonably minimal, well-justified. `release.yml` uses `macos-15-intel` and `macos-latest` (aarch64) for release builds only (tag-triggered, not per-PR). No unnecessary macOS runners in the PR path.

**Windows runner usage**: only `gpu-test` (60m budget, WARP) in the PR path. Reasonably scoped and load-bearing (only merge-blocking pixel-oracle GPU coverage).

**`.config/nextest.toml`**: used in CI (all `cargo nextest run` invocations read it implicitly from workspace root). Profile: `slow-timeout = 60s / terminate-after 10` (10 min hard kill), `leak-timeout = 1s`. Overrides: trybuild/compile-fail/CLI-template tests get `300s / terminate-after 4` (20 min budget); GPU readback tests get a `max-threads = 1` test-group to avoid driver contention. No `retries` configured anywhere (a flaky GPU/live-smoke test gets zero automatic retry — relies on `gpu-test`'s serialized `--test-threads 1` to avoid flakiness instead). No `junit` output configured (no machine-readable test report artifact uploaded for external tooling/dashboards).

**iOS in cross-typecheck**: yes — `cross-typecheck` clippies flui-platform + flui-app + flui with `--target aarch64-apple-ios`, and `cli-macos` additionally clippies the iOS runner (needs `xcrun`, hence macOS-hosted). So iOS is compile-checked but never linked/executed in CI (a `.rust-studio/specs/ios-input-verification` effort covers execution via local XCUITest, not CI).

### 1.3 `just ci` vs CI workflow — actual divergence

`just ci` = `gate` + `test-ci` + `test-doc`, where:
- `gate` = fmt-check, text-check (typos+taplo, silently skipped if binaries absent), font-assets-check, inventory-check, runtime-conformance-check, panic-policy-check, port-check, wgsl-uniformity-check, clippy, doc-strict
- `test-ci` mirrors CI's `test`+`test-features`+flui-platform steps reasonably closely (with OS-conditional skip logic for the flui-platform Xvfb step)
- `test-doc` = `cargo test --workspace --locked --doc` (mirrors `doc-test` job)

**Jobs run in CI but NOT run by `just ci` (must go to CI to ever see red)**:
- `feature-matrix` (4 slices) — there IS a separate `just feature-matrix` recipe, but it is not part of `just ci`
- `wasm-check` — separate `just wasm-check` recipe exists, not part of `just ci`
- `cross-typecheck` — separate `just cross-typecheck` recipe exists, not part of `just ci`
- `cli-macos` — no local equivalent at all (macOS-only, no just recipe)
- `live-smoke` — separate `just live-smoke`/`live-smoke-wayland` recipes exist, not part of `just ci`
- `gpu-test` — **no just recipe exists for this at all**; WARP/Windows GPU readback suite cannot be run locally except by a contributor with a DX12 Windows box running the same `cargo nextest -p flui-engine --features testing` command by hand
- `bench-compile` — no direct equivalent (`just bench`/`bench-all` exist but run benches rather than `--no-run` compile-check)
- `msrv` — no `just msrv` recipe
- `miri` — separate `just miri` recipe exists (and is well-documented), not part of `just ci`
- `deny` — separate `just deny` recipe exists, not part of `just ci`
- `checks`'s `actionlint`/`zizmor` — not wired into any `just` recipe at all (workflow-linting tools only run in CI, never locally)

Net effect: **`just ci` (and therefore the pre-push hook, since `gate` is what it runs) exercises well under half of the CI job graph** — it never runs feature-matrix, wasm-check, cross-typecheck, cli-macos, live-smoke, gpu-test, bench-compile, msrv, miri, or deny locally. A contributor who only trusts `just ci` before pushing will still discover feature-flag-interaction bugs, wasm breaks, cross-platform clippy violations, GPU pixel regressions, MSRV breaks, UB, and advisory/license issues only after pushing to CI. This is the most consequential CI/local-parity finding in the audit — worth either documenting explicitly in AGENTS.md/CONTRIBUTING.md (it currently says "run `just ci`" with no caveat) or adding a `just ci-full` recipe that also runs the cheap subset (deny, msrv, wasm-check, cross-typecheck) locally.

### 1.4 Observed PR CI wall time (last 10 runs, `gh run list`)

| run | conclusion | duration |
|---|---|---|
| 35754763763 | success | 12m50s |
| 35751510735 | success | 28m18s |
| 35751457035 | cancelled | 1m9s |
| 35751382312 | success | 28m49s |
| 35703938415 | success | 28m6s |
| 35703262969 | failure | 53s (failed fast, in `checks`) |
| 35698206020 | success | 11m38s |
| 35697168192 | success | 11m15s |
| 35691824512 | success | 19m2s |
| 35691655494 | success | 20m26s |

Typical successful PR CI wall time: **~12–29 minutes**, median around ~20 minutes. Job breakdown for the fastest recent green run (35754763763, 12m50s total): `checks` 43s → (parallel) longest jobs were `test (ubuntu-latest)` 11m41s and `gpu-test` 10m30s; most other jobs (clippy, doc, wasm-check, cli-macos, cross-typecheck, miri, live-smoke, deny, msrv) finished in 1–7 minutes. `test` and `gpu-test` are the critical path.

## 2. Local dev tooling

### 2.1 justfile
932 lines, ~78 recipes across groups (build, test, quality, port, maintenance, ci). `just --list` groups are used (`[group("...")]`) consistently. No dead/orphaned recipes were found by inspection, but coverage gaps exist (see §1.3: no `just msrv`, no `just deny` inside `ci`, no `just gpu-test` at all). `default` runs `just --list --unsorted`. `install-hooks` points git at `scripts/githooks` (custom hook, not lefthook/pre-commit framework).

### 2.2 Git hooks
No `lefthook.yml`/`.pre-commit-config.yaml` — the repo uses a **hand-rolled** `scripts/githooks/pre-push` (3.6K, well-commented) that runs `just gate` (the non-test half of `just ci`) before push, with a text-only fast path for markdown-only pushes and an explicit list of markdown files that ARE contract-gate inputs (`is_contract_gate_input`). Escape hatch: `git push --no-verify`. This is deliberate, documented tooling choice, not an oversight — reasonable given the "why a hook and not a line in a guide" comment. No pre-commit hook (so a slow `cargo fmt --check` failure is only caught at push, not commit).

### 2.3 Misc config files
- `.editorconfig` — **absent**.
- `CODEOWNERS` — **absent** (neither root nor `.github/`).
- `.gitattributes` — present, sensible (LF normalization, Rust/Windows-script exceptions, binary declarations, Cargo.lock marked `-diff linguist-generated`).
- `dependabot.yml` — covers both `cargo` and `github-actions` ecosystems, weekly Monday schedule, 7-day cooldown (documented as a supply-chain-incident mitigation per zizmor's dependabot-cooldown audit), well-reasoned grouping (accesskit cohort, ui-events cohort, patch-and-minor bucket) with inline rationale for each group. This is unusually well-maintained dependabot config.
- `.claude/settings.local.json` — trivial: `{"enabledMcpjsonServers": ["cratesio"]}`. No secrets, no broad permissions.
- `.codex/config.toml` — `[mcp_servers.cratesio]` only, mirrors `.mcp.json`.
- `.mcp.json` — declares only `cratesio` (crates.io lookups) as enabled; three others (`rust-analyzer-mcp`, `rust-docs-mcp`, `rust-mcp-server`) explicitly disabled with a dated comment explaining why (avoid duplicate rust-analyzer). AGENTS.md documents this file as authoritative over `.codex/config.toml`.
- `.lsp.json` — single `rust-analyzer` stdio entry for `.rs` files. Minimal, correct.
- `package.json` at repo root — **does not exist** (the audit prompt's premise that one exists is incorrect for this repo).
- `openspec/`/`specs/` — **neither exists** at the root.
- `.rust-studio/` — actively maintained: `tasks/` (10 files), `specs/` (58 spec directories), `research/` (27 files). Newest specs are dated 2026-09-22 (today), oldest 2026-06-16 — actively used, not abandoned. Not referenced by CI or AGENTS.md's port-check sweep exclusions except as an excluded root for the "no internal process-ID markers" sweep (`docs/{audits,brainstorms,...}`, `.rust-studio/specs`, `specs`, `openspec`).
- `docs/working-preferences.md`, `docs/verdicts.md` — **neither exists**, despite being asked about as if rust-studio references them; no evidence in AGENTS.md either.

### 2.4 `tools/` and `scripts/` inventory
- `tools/`: three subdirectories only — `decoy-face/`, `live-smoke/`, `web-server/` (support crates/binaries for CI's live-smoke and web-demo jobs, not shell scripts).
- `scripts/`: 30 executable scripts (19 `.py`, 11 `.sh`) plus `wasm-import-allowlist.txt`, `fixtures/`, `githooks/`, `tests/`. Every `.sh` script has a `#!/usr/bin/env bash` shebang; **all but two** (`probe-metal-acquire-main-thread.sh`, `probe-upstream-silent-facts.sh`) declare `set -euo pipefail` — those two weekly-only advisory probes lack it, worth a small hardening pass since they curl/parse external data (crates.io) with `|| true`-style tolerance already baked in by design, so the omission may be intentional but is inconsistent with the rest of the fleet.
- Notable large/load-bearing scripts: `port-check.sh` (86.6K, 22 refusal triggers + FR-033), `check-panic-policy.sh` (40.9K), `check-runtime-conformance.sh` (36.9K), `check-workspace-inventory.sh` (36.0K) — these are the CI `checks` job's core gates and are all bash, not just grep one-liners.
- Bash-version dependency: per project memory, `check-runtime-conformance.sh` needs Python 3.12 (`tomllib`) via a `/tmp/py312shim`, and `just`'s port-check step needs bash 4 (`mapfile`) via a `/tmp/bashshim` on macOS (system `/bin/bash` is 3.2). These workarounds are NOT encoded in the scripts themselves — a fresh macOS contributor without the shims will silently hit unrelated-looking failures.

## 3. Agent rules

### 3.1 AGENTS.md structure
134 lines (not 14.7K as the audit prompt assumed — the file is compact by design, consistent with its own stated goal). Sections: Prime Directive (3 rules), Quick Start, Code Navigation, Tech Stack, Build & Development Commands, Architecture Constraints (port methodology table), Documentation (task→doc routing table), AI Context Files, Error Triage, Definition of Done, Agent Rules (marker-ban policy). **Readable in under 5 minutes** — it is denser prose than a typical AGENTS.md but stays on-topic throughout; no padding.

**Explicitly declared as the single source**: "AGENTS.md (this file) is the single agent guide, shared by every agent runtime. There are no per-runtime shims and no separate path-scoped rule files." This directly contradicts the audit prompt's premise of "per-crate CLAUDE.md/AGENTS.md, commit 8822fdf8 says AGENTS.md for all 22 crates" — **no per-crate AGENTS.md or CLAUDE.md files exist today** (`find crates -maxdepth 2 -iname AGENTS.md -o -iname CLAUDE.md` → 0 results). Either commit 8822fdf8 was reverted/consolidated back into the single-file model, or the premise is stale; worth confirming with `git log --oneline -- 'crates/*/AGENTS.md'` if this history matters.

**Mechanically enforced vs prose-only**:
- Mechanically enforced (via `just`/CI scripts): ID offset pattern is spot-checked by port-check triggers; no-RwLock-Box-dyn, no-async-in-hot-paths, no-todo/unimplemented, no-Box-dyn-View-fields, no-From-f32, dyn-boundary allowlist, no-locks-in-public-API, no-flui-log-outside-composition-roots (via `docs/workspace-layers.toml` + inventory-check), no-println-in-foundation (`port-check.sh`'s 22 triggers), lifecycle-capability-scope (`check-frame-capability-scope.sh`), panic-policy `expect("BUG: …")` convention (`check-panic-policy.sh`), marker-ban sweep (part of port-check/inventory), render-object harness catalog (`cargo test -p flui-objects --test render_object_harness`).
- Prose-only (relies on agent discipline, no gate): Prime Directive rules 1–3 (Flutter-inspiration-not-imitation, market survey before design, Definition-of-Done anti-cheating) are unenforceable by tooling by nature — they describe judgment calls, which is reasonable, but means an agent that skips the ADR-writing step for a divergence will not be caught by any script.

**What an AI agent most often needs that AGENTS.md does not cover**:
- No **worktree policy** (nothing about git worktrees, branch naming, or how to structure multi-crate concurrent work).
- No explicit **commit message format** (conventional commits, subject-line length, etc. — CONTRIBUTING.md/PR template don't specify one either).
- No **"what NOT to touch"** list (no mention of generated files, vendored code, or crates under active large refactor that agents should avoid).
- "How to run one crate's tests fast" is only partially covered: `just test-crate <crate>` and `just test-name` are in justfile but not surfaced in AGENTS.md's "Build & Development Commands" section (which only calls out two commands justfile doesn't cover already, on the assumption the reader has already run `just --list`).
- No **"how to add a widget"** walkthrough (there is a routing table entry to `docs/crates.md` "Adding a New Crate" for new crates, but not for adding a widget/render object within an existing crate — Error Triage rule 2 gestures at the render-object harness requirement but not the how-to).

### 3.2 CONTRIBUTING.md vs docs/contributing.md
`CONTRIBUTING.md` (root, 19 lines) is a **thin pointer**: "full contributor guide lives in docs/contributing.md", plus a duplicated "run `just ci`" instruction and a duplicated pointer to AGENTS.md/FOUNDATIONS.md that AGENTS.md itself also states. Not a meaningful duplication problem — this is the standard GitHub-discoverability pattern (root CONTRIBUTING.md is where GitHub looks) delegating to a docs/ file for the real content. Low risk, but the "run just ci" instruction appears in three places (root CONTRIBUTING.md, AGENTS.md Quick Start, PR template checklist) with no single source of truth — a change to the actual gate command would need updating in three files.

### 3.3 PR / issue templates vs AGENTS.md Definition of Done
`.github/PULL_REQUEST_TEMPLATE.md` checklist: `just ci`, Flutter-reference-checked, tests-that-would-fail-without-change, public-API-documented, layering-per-FOUNDATIONS, no-new-banned-patterns, dependencies-declared-through-workspace. This **aligns well** with AGENTS.md's Definition of Done (verified behavior + recorded divergence + honest scope) — the "tests that would fail without this change" checkbox is a direct match. One gap: the PR template does not ask whether an ADR/`## Mapping decisions` entry was added for a Flutter divergence, which is the crux of AGENTS.md's Prime Directive rule 3 and Definition-of-Done item 1 — an agent following the PR template alone could tick every box while skipping the divergence-recording requirement.
Issue templates: `bug_report.yml`, `feature_request.yml`, `architecture_change.yml`, `config.yml` — reasonable structured set; not deeply cross-checked against AGENTS.md in this pass.

## 4. Lint allowances

- **Crate-level `#![allow(...)]`**: only `flui-hot-reload/src/lib.rs` has one (line 111). No crate has a blanket `#![allow(clippy::unwrap_used)]` at the crate level — the workspace lint (`unsafe_code = "warn"` in root `Cargo.toml`, plus presumably `clippy::unwrap_used` at workspace level though not directly grepped at crate level) appears to hold workspace-wide without per-crate opt-outs, which is a good sign of consistent enforcement.
- **`#![forbid(unsafe_code)]`**: `flui-painting`. **`#![cfg_attr(not(test), deny(unsafe_code))]`**: `flui-engine`. `flui-platform` explicitly does NOT blanket-allow unsafe at crate level — comment says "no longer opts the whole tree out of `unsafe_code = "warn"`: each [unsafe use has a] module-level `#![allow(unsafe_code)]`" — i.e. unsafe is scoped narrowly per-module rather than crate-wide, which is the tighter, more auditable pattern.
- **Item-level `#[allow(...)]` counts per crate** (top offenders): flui-engine 7, flui-view 2, flui-rendering 2, flui-platform 2, flui-foundation 1, flui-animation 1. All other crates: 0. Total is small (15 across the whole workspace) — lint-allowance debt is not a significant problem here.
- **`missing_docs`**: appears (as a workspace lint reference, not necessarily "allow") in the crate-level lib.rs of 25 of 28 crates — i.e. `#![warn(missing_docs)]` or similar is broadly declared, consistent with the doc job being a hard CI gate.
- **`#[expect(...)]`**: 243 uses across `crates/*/src` — this is the "ratcheted" style (an `expect` fails loudly if the lint stops firing, unlike `allow` which silently rots), consistent with the project's stated preference for `expect` over `allow` as a ratchet mechanism (seen elsewhere in AGENTS.md's "no test locks" / ratchet language).
- **`todo!()`/`unimplemented!()` in non-test src**: 15 raw hits, but on inspection nearly all are **doc-comment examples** (`/// todo!()`) or **prose mentioning** `unimplemented!()` (flui-platform's Linux stub module, explicitly documented as an intentional stub), not live production `todo!()`/`unimplemented!()` calls. This matches AGENTS.md's stated rule ("No `unimplemented!()`/`todo!()` in production code except platform-init stubs on linux/ios/android") — the Linux platform stub is the documented, sanctioned exception.

## 5. Cargo hygiene

- **Workspace size**: 38 packages total via `cargo metadata --no-deps` (28 "active" library/framework crates per justfile's `active_crates` list, plus demo/fixture/test-harness binaries: `flui-web-demo`, `flui-painting-demo`, `flui-desktop-scene`, `hot-reload-counter-{types,logic,host}`, `flui-hot-reload-lifecycle-fixture`, `flui-web-counter`, `flui-web-server`, `flui-live-smoke`).
- **Missing `description`**: `flui-web-demo`, `flui-painting-demo` — both are demo binaries, low-stakes since they are never published.
- **Missing `keywords`/`categories`**: 10 packages — all demo/fixture/test-support crates (`flui-desktop-scene`, `hot-reload-counter-*`, `flui-hot-reload-lifecycle-fixture`, `flui-web-demo`, `flui-painting-demo`, `flui-web-counter`, `flui-web-server`, `flui-live-smoke`). None of these ship to crates.io, so this is cosmetic, not a publish-blocker.
- **Missing `readme`**: 13 packages, notably including three **real, published-looking** framework crates — `flui-localizations`, `flui-material`, `flui-cupertino` — plus the same 10 demo/fixture crates above. The three framework crates lacking a README is worth fixing before any crates.io publish of those catalogs (release-lead/docs-engineer territory).
- **`[lints] workspace = true`**: present in all 28 crates under `crates/*/Cargo.toml` (verified — no crate is missing it).
- **Duplicate dependency versions**: `cargo tree --workspace --duplicates -e normal` shows 17 duplicated crate names / 33 version-entries, dominated by the macOS/AppKit FFI stack moving in lockstep pairs — `objc2` 0.5.2/0.6.4, `objc2-app-kit` 0.2.2/0.3.2, `objc2-foundation` 0.2.2/0.3.2, `block2` 0.5.1/0.6.2, `core-foundation` 0.9.4/0.10.1, `core-graphics`/`core-graphics-types` two versions each — plus font-stack pairs (`font-types`, `read-fonts`, `skrifa` each two versions), `bitflags` 1.3.2/2.13.2, `hashbrown` 0.14.5/0.17.1, `rustc-hash` 1.1.0/2.1.3, `smol_str` 0.2.2/0.3.6, `codespan-reporting` 0.12.0/0.13.1, and a `syn` 2.0.119/**3.0.6** split (worth double-checking — a syn 2→3 split this early is unusual and likely indicates one dependency has jumped ahead of the rest of the graph). This is tracked as a metric already (weekly.yml's `nightly-canary` job runs `cargo tree --duplicates` as an informational step, and AGENTS.md-adjacent policy says duplicates get fixed only by bumping the upstream that pins the old version, never patched) — so this is known, tracked debt, not an oversight.
- **`cargo hack`**: installed, `0.6.45`.
- **`cargo-machete` / `cargo-udeps`**: **not installed** on this machine — unused-dependency audit was skipped per the audit's own instructions; recommend running `cargo machete` at least once in CI or as a periodic weekly.yml job, since none of the existing gates (deny, clippy, feature-matrix) catch a genuinely-unused normal dependency.

---

## Appendix: files referenced
- `/Users/vanyastafford/Develop/flui/.github/workflows/ci.yml`
- `/Users/vanyastafford/Develop/flui/.github/workflows/weekly.yml`
- `/Users/vanyastafford/Develop/flui/.github/workflows/release.yml`
- `/Users/vanyastafford/Develop/flui/.github/workflows/coderabbit-trigger.yml`
- `/Users/vanyastafford/Develop/flui/.github/dependabot.yml`
- `/Users/vanyastafford/Develop/flui/.github/PULL_REQUEST_TEMPLATE.md`
- `/Users/vanyastafford/Develop/flui/.github/ISSUE_TEMPLATE/*.yml`
- `/Users/vanyastafford/Develop/flui/justfile`
- `/Users/vanyastafford/Develop/flui/AGENTS.md`
- `/Users/vanyastafford/Develop/flui/CONTRIBUTING.md`
- `/Users/vanyastafford/Develop/flui/.config/nextest.toml`
- `/Users/vanyastafford/Develop/flui/.claude/settings.local.json`
- `/Users/vanyastafford/Develop/flui/.codex/config.toml`
- `/Users/vanyastafford/Develop/flui/.mcp.json`
- `/Users/vanyastafford/Develop/flui/.lsp.json`
- `/Users/vanyastafford/Develop/flui/.gitattributes`
- `/Users/vanyastafford/Develop/flui/scripts/githooks/pre-push`
- `/Users/vanyastafford/Develop/flui/scripts/*.sh`, `/Users/vanyastafford/Develop/flui/scripts/*.py`
- `/Users/vanyastafford/Develop/flui/.rust-studio/{tasks,specs,research}`
- `/Users/vanyastafford/Develop/flui/crates/*/Cargo.toml`, `crates/*/src/lib.rs`
