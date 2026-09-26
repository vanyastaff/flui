# Decision panel: q1_packages_location

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Nothing is published yet: `curl https://crates.io/api/v1/crates/{flui-geometry,flui-widgets,flui-material,flui}` all return `crate ... does not exist`. So D7's 'CI builds packages/ against the last published train' has no train to build against until the first release. Until then the only possible mode is a `[patch.crates-io]` path override, which means in-repo packages behave like path deps in practice.",
    "Workspace version is `0.2.0-dev` (Cargo.toml:106) and internal deps pin `version = \"=0.2.0-dev\"` (e.g. crates/flui-material/Cargo.toml [dependencies]). Experiment (below) shows a plain `\"0.2\"` requirement does NOT match a `0.2.0-dev` prerelease even under [patch], so packages must pin the exact prerelease. Every dev bump then touches every package manifest.",
    "flui-material depends directly on 9 core crates (widgets, view, types, objects, rendering, foundation, animation, interaction, scheduler) per crates/flui-material/Cargo.toml [dependencies]. It does not depend on a flui-sdk, which does not exist yet. Moving it out of the workspace needs flui-sdk first, or a dependency on 9 separately versioned crates.",
    "Coupling measured: `git log --since=2026-03-25 -- crates/flui-material crates/flui-cupertino` returns 123 commits. 68 of them (55%) also touch other crates/ directories. Most catalog changes today are cross-cutting core+catalog changes, which a separate repo would split into multi-PR, multi-release sequences.",
    "Catalog size: `find crates/flui-material crates/flui-cupertino -name '*.rs' | xargs cat | wc -l` gives 45024 lines.",
    "Only the root workspace and flui-widgets reference flui-material/cupertino in manifests (`grep -rln flui-material --include=Cargo.toml`). The root facade has them as optional deps (Cargo.toml:525-526) with features `material`/`cupertino` (Cargo.toml:605-606). flui-widgets mentions them only as dev/test-harness consumers (crates/flui-widgets/Cargo.toml:85,182).",
    "xtask change scoping reads a single workspace via `cargo metadata --no-deps --offline` run in `root` (tools/xtask/src/change_scope/classify.rs:325-350). A nested `packages/` workspace would be invisible to `cargo xtask check-changed` and the CI fast lane until classify.rs learns a second root. `cargo xtask facade-combos` (tools/xtask/src/tasks/facade.rs) also assumes material/cupertino are facade features.",
    "ADR-0028 (Accepted) places flui-material/flui-cupertino at layer L7 'Design systems' inside the workspace layering (docs/adr/ADR-0028-design-system-decoupling-contract.md:62,70). Moving them to packages/ or other repos needs ADR-0028/0041 superseded, as the architecture doc already notes (flui-global-architecture.md:599).",
    "CI is one large workflow (.github/workflows/ci.yml has about 22 jobs: checks, plan, fast-lane, clippy, test, test-features, live-smoke, gpu-test, platform-windows, doc, deps, miri, feature-matrix, wasm-check, cross-typecheck, ci aggregator, ...). release.yml:17 states 'Publishing to crates.io is not done here', so there is no release train machinery yet.",
    "`.flutter/` and `.gpui/` reference clones are absent on this host (`ls .flutter .gpui` gives No such file or directory). The Flutter facts below come from the web.",
    "The architecture doc's own D7 proposal (flui-global-architecture.md:177): packages/ is a separate cargo workspace, deps on flui-sdk by version, [patch] to path only for local dev, CI against the published train, split into its own repo when cadence diverges. It is filed as an owner question (§14 item 1, line 723)."
  ],
  "market_precedents": [
    {
      "who": "Flutter (flutter/flutter + flutter/packages)",
      "what": "Framework and first-party packages live in separate repos. flutter/packages pins the framework commits it tests against in .ci/flutter_master.version and .ci/flutter_stable.version, and tests both channels. In the other direction, flutter/flutter pins a packages commit in bin/internal/flutter_packages.version so framework CI catches package breakage.",
      "outcome_or_lesson": "A split repo needs a two-way pin plus an auto-roller bot, run by a large team. Breaking framework changes land as multi-step migrations: land the new API, roll, migrate packages, remove the old one. Too heavy for one owner plus agents, but it is the model for testing 'packages against both published and head'.",
      "source": "https://github.com/flutter/packages/tree/main/.ci ; https://github.com/flutter/flutter/blob/master/bin/internal/flutter_packages.version"
    },
    {
      "who": "Flutter (consolidation trend)",
      "what": "flutter/plugins was merged into flutter/packages and archived Feb 22, 2023. flutter/engine was merged into the flutter/flutter monorepo on Dec 18, 2024.",
      "outcome_or_lesson": "Flutter's own direction is fewer repos. Cross-repo rolls between tightly coupled layers (engine to framework) cost enough to merge them. Only loosely coupled packages that depend on the stable public API stayed separate.",
      "source": "https://github.com/flutter/flutter/wiki/Migrating-Plugins-repository-PRs-to-Packages ; https://github.com/flutter/flutter/issues/160628"
    },
    {
      "who": "Dioxus",
      "what": "One cargo workspace with all crates under packages/: core, router, signals, desktop/web/native, fullstack, cli, devtools, manganis, subsecond. Platform utilities (storage, geolocation, notifications, window) live in a separate DioxusLabs/sdk repo that tracks versions ('crate 0.7 supports Dioxus 0.7') and is marked 'expect breaking changes'.",
      "outcome_or_lesson": "Official first-party packages stay in the main workspace with path deps. Only lower-priority OS-integration plugins moved to a separate repo that versions in lockstep with the framework. The directory name packages/ does not imply a separate workspace.",
      "source": "https://github.com/DioxusLabs/dioxus/blob/main/Cargo.toml ; https://github.com/DioxusLabs/sdk"
    },
    {
      "who": "Bevy",
      "what": "All official crates sit under crates/ in one workspace ('All of Bevy's official crates are within the crates folder'). tests-integration is excluded as a separate build. Third-party ecosystem crates (bevy_egui etc.) are separate repos.",
      "outcome_or_lesson": "Third-party crates lag each roughly quarterly breaking release by about 2-8 weeks (secondary source), and users are pinned to the old version meanwhile. This is the cost that separate-repo official packages would impose on FLUI users each train. Bevy mitigates it with release candidates so ecosystem crates publish rc-compatible versions.",
      "source": "https://github.com/bevyengine/bevy/blob/main/Cargo.toml ; https://engineranked.com/article/bevy-engine-review-2026/"
    },
    {
      "who": "Linebender (Xilem/Masonry vs Vello/Parley)",
      "what": "xilem, xilem_core, xilem_web, masonry, masonry_core, masonry_testing, masonry_winit sit in ONE workspace with path deps. The lower layers vello and parley are separate repos consumed by version (vello 0.8.0, parley 0.8.0 in [workspace.dependencies]).",
      "outcome_or_lesson": "They split repos along a stable, independently useful layer boundary (renderer, text) with its own cadence, not along app-level packages. The UI framework and its widget library stay together because they change together. This matches the '55% of catalog commits touch core' measurement here.",
      "source": "https://github.com/linebender/xilem/blob/main/Cargo.toml"
    },
    {
      "who": "Cargo (tooling constraint)",
      "what": "[patch] is honored only in the workspace-root manifest, and patches in dependencies are ignored.",
      "outcome_or_lesson": "A nested packages/ workspace must carry its own [patch.crates-io] block listing every core crate. External authors won't have it, so 'patch for local dev only' needs a CI mode that strips it. That mode is only meaningful after the first publish.",
      "source": "https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html"
    }
  ],
  "constraints": [
    "No published train exists (crates.io 404 for every flui crate). 'Build against the last published train' cannot run before the first release, so a packages/ workspace starts life identical to path deps plus extra lockfile and target overhead.",
    "Prerelease versions (0.2.0-dev) do not satisfy caret requirements such as \"0.2\". Packages must pin =0.2.0-dev exactly, so every train bump edits every package manifest. A release script or cargo xtask should own that edit.",
    "A nested workspace doubles compilation of shared core crates in CI even with a shared CARGO_TARGET_DIR (probe: two fingerprints for the same path crate). With 27 crates plus wgpu that is roughly a second full core build per packages/ job. Hypothesis: on the real workspace the cost is a large fraction of the core build, not measured.",
    "cargo xtask check-changed / change_scope reads one workspace root (classify.rs:325). packages/ needs tooling work (second root, dependents across roots) before it reaches the merge path; AGENTS.md requires a gate to be an xtask command plus a ci-aggregated job.",
    "Material cannot move off core crates until flui-sdk exists (W3). Today it depends on 9 core crates directly, so D7 is sequenced after flui-sdk, not before.",
    "AGENTS.md: .github/workflows/ changes are 'leave alone unless the task is about them'. Adding a packages CI job is exactly such a task and needs its own PR and owner sign-off.",
    "ADR-0028 (L7 design systems inside the layer stack) and ADR-0041 (workspace topology) must be superseded by a new ADR whichever option is chosen, except for the path-deps single-workspace option."
  ],
  "experiments_run": [
    "Probe at C:\\Users\\vanya\\AppData\\Local\\Temp\\claude\\D--flui\\bbb28042-f972-4a10-8e94-731819e26161\\scratchpad\\probe-packages: an outer workspace (crates/sdk, version 0.2.0-dev) and a nested packages/ workspace (mat) with [patch.crates-io] pointing flui-sdk-probe to ../crates/sdk. `cargo metadata --no-deps` at the root lists only flui-sdk-probe, so the outer workspace does not see packages/ and no `exclude` is needed. `cd packages && cargo check --offline` succeeds and compiles both crates. Result: a nested workspace plus patch works.",
    "Same probe with the requirement changed from \"=0.2.0-dev\" to \"0.2\": resolution fails with 'if you are looking for the prerelease package it needs to be specified explicitly flui-sdk-probe = { version = \"0.2.0-dev\" }'. Result: packages must pin the exact prerelease; the patch does not rescue a caret requirement.",
    "Shared CARGO_TARGET_DIR experiment: `cargo check` at the root, then in packages/. flui-sdk-probe was checked again in the second workspace, and tgt/debug/.fingerprint holds two entries (flui-sdk-probe-375cfd..., flui-sdk-probe-a4d0cd...). Rerunning both gives no thrash (0.01s each) but two artifacts. Result: a nested workspace compiles shared core crates twice.",
    "git coupling measurement: loop over `git log --since=2026-03-25 --format=%H -- crates/flui-material crates/flui-cupertino`, counting commits whose `git show --name-only` also touches another crates/ dir. Output: '123 commits, 68 also touching other crates' (55%).",
    "Synthesis (recommendation, marked as analysis): (1) Now through W3: keep official packages in the SINGLE workspace with path deps, moved physically to packages/ as members. This follows Dioxus and Xilem, keeps atomic cross-cutting PRs (55% coupling), keeps check-changed and agent discoverability, and costs no extra CI. Enforce external-author parity with a type/gate, not a separate workspace: an xtask workspace rule that packages/* may depend only on flui-sdk, flui-platform-api and flui-protocol (the 'forbid-reach' rule already in the doc). (2) After the first publish: add one nightly/weekly job that builds packages/ from a copy with path deps stripped against the last published train (a Flutter-style stable-channel check), not on every PR. (3) Separate repos only for a package whose cadence or ownership diverges, such as OS plugins with external maintainers or a2ui. Precedent: Linebender splits along stable layers, Dioxus sdk is a separate lockstep repo, and Flutter keeps packages separate but with a two-way pin plus roller that a one-owner project cannot staff. This amends D7: a separate workspace is deferred and a single workspace plus dependency-rule gate is adopted now. It also amends plan.md's 'separate repos'."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "Single workspace, path deps, packages/ as a directory of members plus a dependency-rule gate",
      "description": "Move the official packages (material, cupertino, devtools/mcp, hot-reload, a2ui, i18n, OS plugins) physically into packages/ as ordinary members of the ONE root workspace, with path deps and the train version. Tag each one `tier-kind = official` in [package.metadata.flui]. `cargo xtask workspace` enforces parity with external authors: official packages may depend only on flui-sdk, flui-platform-api and flui-protocol, plus official→official edges that are declared; no core crate may name an official crate, even optionally. This is the Dioxus and Xilem/Masonry layout.",
      "pros": [
        "Keeps atomic cross-cutting PRs. 55% of catalog commits since 2026-03-25 touch core crates too (68 of 123, git log measurement), and a core+Material change stays one PR.",
        "No extra CI cost: shared core crates compile once. The nested-workspace probe fingerprinted the same crate twice.",
        "check-changed, the fast lane, facade-combos and nextest keep working unchanged. classify.rs:325-350 reads one workspace root.",
        "Best for agent discoverability: one Cargo.lock, one `cargo metadata`, one grep scope.",
        "External-author parity comes from a type/gate rule (AGENTS.md: 'make rules types, not reviews') instead of a repository boundary.",
        "The only option that needs no ADR-0041 topology supersession. ADR-0028 is amended only to move L7 into the official tier."
      ],
      "cons": [
        "A path dep hides version skew: nothing proves Material builds against the *published* sdk until the published-train check below exists.",
        "A contributor can get a core change merged together with the package fix it forces, which hides breaking sdk changes from external authors. Mitigation: semver-checks on flui-sdk (advisory while Evolving).",
        "Every package rides the train cadence even if it would prefer its own."
      ],
      "cost_now": "Low. Directory moves (git mv crates/flui-material → packages/flui-material, etc.) and root `members` edits, plus a new tier-kind metadata key, the forbid-reach/allowed-deps rule in xtask workspace, and an ADR superseding the L7 part of ADR-0028. Sequencing: the allowed-deps rule can only be strict after flui-sdk exists (W3). Until then it runs in warn/allowlist mode, because Material names 9 core crates directly today.",
      "cost_later": "After the first publish: one scheduled job (nightly or weekly, not per PR) that copies packages/, strips path/[patch] overrides and builds against the last published train. Its tooling lives in xtask. Only the scheduled cadence touches .github/workflows, in its own owner-approved PR.",
      "reversibility": "High. Any package can later be turned into a nested workspace or its own repo (git filter-repo on packages/<name>) once sdk is published. Nothing about this layout closes off B or C.",
      "fits_plan": "Fits W3 sequencing: flui-sdk comes first, then Material is re-pointed onto it. It amends D7 (defers the separate workspace) and plan.md's 'separate repos' in one line. Matches §3.4's own gate idea (forbid-reach, tier-kind = official)."
    },
    {
      "id": "B",
      "name": "packages/ as a nested cargo workspace in this repo (D7 as written)",
      "description": "packages/ has its own Cargo.toml [workspace], its own lockfile and a [patch.crates-io] block pointing every core crate at ../crates for local dev. Packages depend on flui-sdk by version. CI builds packages/ against the last published train.",
      "pros": [
        "Mirrors the external-author experience most faithfully: a version requirement, a separate lockfile, and nothing reachable except by version.",
        "Physically prevents an official package from depending on core internals by path.",
        "A package can move out to its own repo later without changing its manifests."
      ],
      "cons": [
        "There is no train yet: crates.io returns 'does not exist' for every flui crate. Until the first publish the only mode is [patch] to path, i.e. path deps with extra overhead.",
        "Prerelease pinning: '0.2' does not match 0.2.0-dev even under [patch] (probe). Every package must pin =0.2.0-dev exactly, and every dev bump edits every package manifest.",
        "Core crates compile twice in CI even with a shared CARGO_TARGET_DIR (probe: two fingerprints). With about 27 crates plus wgpu that is roughly a second core build per packages job. The size is a hypothesis; it has not been measured on the real workspace.",
        "check-changed/change_scope can't see packages/ until classify.rs learns a second root and cross-root dependents. Until then a core PR can break Material without the fast lane noticing.",
        "Needs a new CI job, which touches .github/workflows (owner sign-off), and supersession of both ADR-0028 and ADR-0041.",
        "Agents must know about two workspaces, two lockfiles and the patch semantics ([patch] is honored only at the workspace root)."
      ],
      "cost_now": "Medium-high. Nested workspace, patch block for about 25 crates, xtask second-root support in change_scope, facade-combos rework, a new CI job and two ADR supersessions. All of this before it delivers anything a path dep doesn't, since no train exists.",
      "cost_later": "Ongoing: a doubled core build per PR that touches packages, manifest churn on every version bump, and cross-root classification to maintain. Its value arrives only after the first publish.",
      "reversibility": "Medium. Folding it back into the root workspace is easy mechanically, but the CI/xtask investment is lost.",
      "fits_plan": "Matches the architecture doc's D7 proposal literally, but conflicts with W3 sequencing: it cannot deliver 'against the published train' before sdk and the first release exist."
    },
    {
      "id": "C",
      "name": "Separate repos per package, one release train (plan.md as written)",
      "description": "flui-material, flui-cupertino, devtools/mcp and the others each live in their own GitHub repo and depend on published flui-sdk. A coordinated release train bumps them all. This is the Flutter flutter/packages model.",
      "pros": [
        "True external-author parity, and each repo has its own issues and cadence.",
        "The core repo's CI never builds packages.",
        "Ownership can be handed off per package (useful for OS plugins with outside maintainers)."
      ],
      "cons": [
        "Makes 55% of today's catalog work multi-repo and multi-PR: land the core API, publish or roll, migrate the package, remove the old API.",
        "Flutter makes this work only with a two-way pin (.ci/flutter_master.version + bin/internal/flutter_packages.version) and an auto-roller bot run by a large team. Flutter itself consolidated plugins into packages (2023) and engine into flutter (2024).",
        "Cannot function before the first publish, or it needs git deps, which crates.io forbids for published crates.",
        "Bevy's ecosystem lags each breaking release by 2-8 weeks (secondary source). Official Material lagging a train would be a user-visible regression.",
        "Worst for agent discoverability: the context is spread across repos, and an agent's worktree can't see the consumer."
      ],
      "cost_now": "High and blocked: needs sdk, the first publish, repo creation, CI per repo and release-train automation.",
      "cost_later": "High permanently for a one-owner project: roller/pin maintenance and multi-repo migrations for every breaking sdk change.",
      "reversibility": "Low. Merging repos back loses issue and PR linkage, and users will have pinned package versions decoupled from the train.",
      "fits_plan": "Literal plan.md wording, but it contradicts the plan's single-owner-plus-agents staffing and the 'fix a bad shape now, pre-1.0' stance."
    },
    {
      "id": "D",
      "name": "Hybrid: A now, a published-train check after the first release, per-package split only on divergence",
      "description": "Adopt A through W3 and the first release. After the first publish, add a scheduled 'published-train' check: xtask copies packages/ into a temp dir, strips path deps/[patch] and builds against the last published crates.io train (the Flutter stable-channel idea, run on a schedule instead of on every PR). A package moves out (to a nested workspace, or to its own repo in lockstep versioning like DioxusLabs/sdk) only when a stated trigger fires: its cadence diverges from the train, an external maintainer owns it, or it has no core-coupled commits for N months. OS plugins and a2ui are the likely first candidates, Material and Cupertino the last.",
      "pros": [
        "All of A's benefits now (atomic PRs, single classification, no double build, agent discoverability).",
        "Adds A's missing proof, 'Material builds on the published sdk as an outsider would', at the only point it can exist (after the first publish) and at scheduled-job cost.",
        "Follows the actual precedents: Dioxus keeps first-party packages in-workspace and moved only OS utilities to a lockstep sdk repo. Linebender splits along stable, independently useful layers (vello, parley), not app-level packages.",
        "Every split gets an objective trigger instead of an up-front bet."
      ],
      "cons": [
        "There is a window (until the first publish) where sdk breakage for external authors is caught only by semver-checks and review.",
        "The strip-and-build xtask command is new tooling (small, but a real gate that must reach CI per AGENTS.md).",
        "Requires writing down the split triggers so they don't become folklore."
      ],
      "cost_now": "Same as A: directory moves, tier-kind metadata, the allowed-deps/forbid-reach rule (warn until sdk exists, then strict), and one ADR superseding ADR-0028's L7 placement and recording D7-amended.",
      "cost_later": "Small: one xtask command (`cargo xtask packages-published` or similar) plus one scheduled CI job after the first release. A per-package split costs a filter-repo run plus a lockstep-version rule, paid only when a trigger fires.",
      "reversibility": "High. Every step is additive and each split is per package.",
      "fits_plan": "Best fit. It sequences after W3 (sdk) and the first publish, keeps the one-owner plus agents model workable, satisfies D7's intent (proof of external-author parity) without D7's premature mechanism, and replaces plan.md's 'separate repos' with 'separate repos on trigger, lockstep versions'."
    }
  ],
  "recommended": "D",
  "rationale": "I recommend D: A now, then a scheduled published-train check after the first publish, and a per-package split only when a stated trigger fires.\n\n**Why not a separate workspace or separate repos now (B and C).** They exist to prove that packages build against the published train, and there is no train yet: crates.io returns 'does not exist' for every flui crate. Before the first publish a nested workspace behaves like path deps and still costs this much:\n- a second compile of the core crates (the probe produced two fingerprints);\n- every package pinning the exact prerelease, =0.2.0-dev, because '0.2' does not match it (probe);\n- change_scope/check-changed can't see packages/ (classify.rs:325-350);\n- a new CI job in .github/workflows;\n- superseding both ADR-0028 and ADR-0041.\n\n**Why one workspace.** Material and the core change together. 68 of 123 catalog commits since 2026-03-25 (55%) also touch core crates. Splitting repos would turn more than half of catalog work into multi-step roll migrations. Flutter makes that work only with two-way version pins and an auto-roller, and has itself merged repos (plugins into packages in 2023, engine into flutter in 2024). Dioxus (packages/ inside one workspace) and Xilem/Masonry (framework and widget crates in one workspace, lower layers vello and parley split out) keep the tightly coupled parts together too.\n\n**Parity for external authors without a separate workspace.** D gets it through a gate rule in `cargo xtask workspace`:\n- a package with `tier-kind = official` may depend only on flui-sdk, flui-platform-api and flui-protocol, plus official→official edges it declares;\n- no core crate may name an official crate.\n\nThis is AGENTS.md's 'make rules types, not reviews'. The rule stays in warn mode until flui-sdk exists (W3), because Material names 9 core crates directly today.\n\n**What changes after the first publish.** A scheduled strip-and-build job checks Material against the published train, like Flutter's stable-channel check. It replaces D7's per-PR mechanism at a fraction of the cost. A package moves to its own repo, versioned in lockstep with the train like DioxusLabs/sdk, only when a stated trigger fires: cadence diverges, an external maintainer takes it over, or core-coupled commits dry up. OS plugins and a2ui are the likely first candidates.\n\n**What the owner has to sign off.**\n- A one-line amendment to D7: separate workspace deferred, single workspace plus the dependency-rule gate adopted now.\n- A one-line amendment to plan.md: 'separate repos on trigger, lockstep versions'.\n- An ADR superseding ADR-0028's L7 placement.\n\n**Unverified.**\n- The doubled-compile cost is measured only on a two-crate probe, not on the real workspace (hypothesis).\n- The Bevy ecosystem-lag figure comes from a secondary source.\n- The .flutter reference clone is absent on this host, so the Flutter facts come from the web (github.com/flutter/packages/.ci, flutter/flutter bin/internal/flutter_packages.version, flutter issue 160628)."
}
```

## judge_q1_packages_location_engineer

```json
{
  "choice": "D",
  "confidence": 0.82,
  "reasons": [
    "No train exists yet: crates.io returns 'does not exist' for every flui crate. So D7's defining mechanism for B, building against the last published train, cannot run. A nested workspace would only add cost: prerelease pinning (the probe showed \"0.2\" does not match 0.2.0-dev), a second compile of core, and change_scope blindness.",
    "Verified that change scoping is single-root. tools/xtask/src/change_scope/classify.rs:325-350 runs `cargo metadata --no-deps --offline` in one `root`. Under B, a core PR could break Material without the fast lane noticing until xtask learns cross-root dependents.",
    "The coupling data favours one workspace: 55% of catalog commits since 2026-03-25 also touch core (68 of 123). Separate repos (C) would turn most catalog work into land/roll/migrate sequences, which Flutter can only sustain with two-way pins plus a roller bot. Flutter itself has consolidated repos since (plugins merged into packages in 2023, engine into flutter in 2024).",
    "The facade already wires the catalogs as optional path deps with =0.2.0-dev (Cargo.toml:525-526, features at 605-606). A and D keep facade-combos and the `required-features` example convention working, with no workflow change now.",
    "D over plain A: A has no proof that a package builds as an outsider's would. D adds that proof at the earliest point it can exist, after the first publish, and at scheduled-job cost. It also turns 'separate repos' from an up-front bet into a split made per package when a stated trigger fires. This matches Dioxus (first-party packages in the workspace, OS sdk in a lockstep repo) and Linebender (split only along stable, independently useful layers).",
    "External-author parity enforced as a gate on `[package.metadata.flui]`, as an allowed-deps rule, follows AGENTS.md's 'make rules types, not reviews'. It reuses the existing `cargo xtask workspace` layer checker, so the enforcement point is not a new one."
  ],
  "conditions": [
    "The allowed-deps / forbid-reach rule must land as part of `cargo xtask workspace` in the same PR that introduces tier-kind = official (a rule counts only if it is in `checks`). Until flui-sdk exists (W3), a per-package allowlist names the current direct core deps explicitly, 9 for Material. The allowlist may only shrink, and it goes strict and empty when W3 lands. A warn mode with no ratchet is not acceptable.",
    "The reverse direction is strict from day one: no core crate or flui-sdk may name an official package, including as an optional dep or dev-dep. Exempt: the facade and the flui-widgets test-harness dev-deps (crates/flui-widgets/Cargo.toml:85,182), each with a stated reason, or those harness tests move into the package.",
    "Only then move directories: git mv to packages/. Keep the members in the root workspace, update the docs/crates.md and docs-links references, and check that change_scope path-prefix ownership still classifies packages/* (classify.rs derives it from metadata, but verify with a check-changed dry run on a packages-only diff).",
    "Write one new ADR that supersedes the L7 placement in ADR-0028 and records D7 as amended: separate workspace deferred, the split triggers written down (cadence divergence, an external maintainer, or core-coupled commits under a threshold over N months), and lockstep versioning for any split repo. ADR-0041 is amended only if the layer model changes. Also amend plan.md's 'separate repos' line.",
    "cargo-semver-checks on flui-sdk runs as a gate (advisory while it is Evolving) from the moment flui-sdk exists. It is the only external-author breakage signal until the first publish.",
    "After the first publish, the strip-path-deps build against the published train is an xtask command plus a scheduled CI job, added in a separate owner-approved workflow PR. A red run opens an issue the same way red main does.",
    "Hypotheses still to confirm before anyone revisits B: the cost of the doubled core compile on the real workspace (measured only on a two-crate probe) and the Bevy ecosystem-lag figure (secondary source)."
  ]
}
```

## judge_q1_packages_location_ecosystem_author

```json
{
  "choice": "D",
  "confidence": 0.8,
  "reasons": [
    "Before the first publish, only D protects app and package authors without making them wait. Nothing is on crates.io yet (all crates return 'does not exist'), so B and C are just path deps with extra cost: a second compile of the core crates (probe found two fingerprints), exact =0.2.0-dev pins in every package, packages/ invisible to check-changed (tools/xtask/src/change_scope/classify.rs:325-350), plus a new workflow and two ADR supersessions.",
    "App authors need Material and Cupertino on the same train as core, with no lag. 68 of 123 catalog commits since 2026-03-25 (55%) also touch core. With separate repos (C), every breaking core change becomes a land/roll/migrate sequence. That is the ecosystem lag Bevy users see (secondary source: 2-8 weeks) and the reason Flutter merged plugins into packages (2023) and engine into flutter (2024).",
    "What a third-party author needs is proof that official packages use only the public surface they can reach too. D provides that with a type/gate rule in `cargo xtask workspace`: tier-kind = official may depend only on flui-sdk, flui-platform-api and flui-protocol. This follows AGENTS.md 'make rules types, not reviews' and runs on every PR. A scheduled job that checks against the published train (B's per-PR idea) is added later, once a published train exists.",
    "D matches practice in comparable projects. Dioxus keeps first-party crates under packages/ in one workspace and moved only OS utilities to a lockstep repo (DioxusLabs/sdk). Linebender keeps xilem and masonry together and splits only stable lower layers (vello, parley). D's per-package split triggers let OS plugins and a2ui go the DioxusLabs/sdk route later without betting on that now.",
    "D is reversible (git filter-repo per package) and keeps a single lockfile and a single `cargo metadata` view. Agents working on a core change then see its consumers, which matters for a project with one owner plus agents."
  ],
  "conditions": [
    "Parity must be proven before the first publish, not only after it. Once flui-sdk exists (W3), add an xtask check (e.g. `cargo publish --dry-run --workspace` or `cargo package` into a local registry) that builds packages/ from packaged sdk tarballs rather than path sources. This catches missing `include` files, path-only features and version-pin mistakes, which the published-train job would otherwise first catch after the release.",
    "Keep a small out-of-tree fixture: a third-party example package, outside `members`, built with only the version requirements an external author would write, e.g. `flui-sdk = \"=0.2.0-dev\"` patched to path. It should use the public extension points (a custom widget, a theme extension, a platform plugin). This is the DX canary for H0-H4 authors, and it catches the case where Material uses something external authors cannot use.",
    "The dependency-rule gate must state its scope explicitly. It covers normal and build deps. It either forbids core→official dev-deps or lists the current exceptions, with reasons: flui-widgets currently dev-depends on flui-material and flui-cupertino (crates/flui-widgets/Cargo.toml:85,182). The facade and flui-app, as composition roots, are the only non-official crates allowed to name official packages, and only through optional features.",
    "Official packages may not rely on `#[doc(hidden)]`, `__private`, or unstable-feature items of flui-sdk. Enforce this with a lint/xtask check or with semver-checks on flui-sdk, which stays advisory while the API is Evolving. Otherwise the parity claim is only as strong as the manifest check.",
    "The dependency rule runs in warn/allowlist mode until flui-sdk lands. Each allowlisted core dependency of Material (9 today) gets an expiry tied to W3, so warn mode does not become permanent.",
    "The split triggers (cadence divergence, an external maintainer, no core-coupled commits for N months, measured with the same git-log method) go into the superseding ADR, together with the lockstep-version rule for any split repo. The owner signs off the one-line amendments to D7 and plan.md, plus the ADR superseding ADR-0028's L7 placement.",
    "The post-publish published-train job is an xtask command plus a scheduled workflow, in its own owner-approved PR. A red run opens an issue, the same way red main does, so a break for external authors cannot pass silently.",
    "Unverified: the size of the doubled-compile cost on the real workspace (measured only on the two-crate probe) and the Bevy ecosystem-lag figure. Neither changes the choice, but neither should be cited as fact in the ADR."
  ]
}
```

## judge_q1_packages_location_owner

```json
{
  "choice": "D",
  "confidence": 0.85,
  "reasons": [
    "Maintenance load. One owner with agents can't run Flutter's two-way pin plus auto-roller (flutter/packages .ci/*.version and flutter/flutter bin/internal/flutter_packages.version). Option C needs that machinery permanently. Option B adds a second workspace root, a second lockfile, a [patch] block and cross-root change_scope (classify.rs:325-350). D keeps one workspace, and its only later addition is one xtask command plus one scheduled job.",
    "Coupling. 68 of the 123 material/cupertino commits since 2026-03-25 (55%) also touch core crates. Separate repos or a nested workspace would split more than half of catalog work into multi-step migrations. D keeps those changes as atomic PRs, and one owner can't staff the alternative.",
    "B and C have nothing to check against yet. crates.io reports 'does not exist' for every flui crate. Their whole reason is building against the published train, and before the first publish that can't run. The probe showed a nested workspace before that point is just path deps with a doubled core compile and exact-prerelease pinning (=0.2.0-dev), for no gain.",
    "Bus factor and agent discoverability. One Cargo.lock, one cargo metadata and one grep scope mean any agent in a worktree sees both the producer and the consumer. Multi-repo context is the worst case for a project run by agents.",
    "Enforced parity for external authors. D enforces parity for external authors with a gate: the allowed-deps / forbid-reach rule in `cargo xtask workspace`. That matches AGENTS.md's 'make rules types, not reviews', and it runs on every PR, whereas a repo boundary is only checked when something is released.",
    "Reversibility. Every step in D is additive, and a split is per package (git filter-repo on packages/<name>), so B or C stay possible later. C is the hardest to undo: issue and PR history is lost, and users' pinned versions drift away from the train.",
    "The precedents support D. Dioxus keeps its first-party crates in one workspace under packages/ and runs its OS-utility sdk as a separate repo versioned in lockstep. Xilem and Masonry share one workspace; only the stable lower layers, Vello and Parley, are separate repos. Flutter itself moved to fewer repos (plugins merged into packages in 2023, engine into flutter in 2024). D over plain A: D names the one proof A lacks (a published-train build) and adds objective split triggers, so the plan.md 'separate repos' intent becomes a rule instead of being dropped.",
    "Limit on work in progress. D adds no new work now beyond what W3 already needs. The directory move and the gate come together with flui-sdk, not as a separate project running in parallel."
  ],
  "conditions": [
    "Sequencing: don't do the physical move (git mv crates/flui-material to packages/...) as a standalone churn PR before flui-sdk exists (W3). Tie it to repointing Material onto flui-sdk so each crate moves once. Until then, only add the tier-kind = official metadata and the rule in warn/allowlist mode.",
    "The allowed-deps / forbid-reach rule becomes strict (hard error) as soon as flui-sdk ships. The warn allowlist has to shrink monotonically: xtask fails if a new core crate is added to it. That stops warn mode from becoming permanent.",
    "The rule forbids any core-tier crate (layers below official) from naming an official package in [dependencies], optional or not. The only exceptions are the facade features and dev-dependencies of test harnesses, each listed with a stated reason. The existing dev-dep in crates/flui-widgets/Cargo.toml:85,182 must be listed or removed.",
    "Write a new ADR superseding the L7 placement in ADR-0028. It records D7 as amended (separate workspace deferred, single workspace plus gate adopted), and a Superseded-by line is added to ADR-0028. Amend plan.md's 'separate repos' line to 'separate repo only on a stated trigger, lockstep versions'. Both need owner sign-off.",
    "Write the split triggers into the ADR as checkable criteria, so they don't become folklore. Examples: cadence diverges from the train for two or more consecutive trains; an external maintainer owns the package; a package's core-coupled commit share stays below a set threshold over N months (measured with the same git log method). Material and Cupertino are explicitly the last candidates.",
    "After the first publish, the published-train check has to reach the merge-adjacent path as AGENTS.md requires. That means a `cargo xtask` command that copies packages/, strips path deps and [patch], and builds against crates.io, plus a scheduled CI job added in its own owner-approved .github/workflows PR. A red run opens the same kind of 'CI is red' issue as main. It is not optional tooling.",
    "Until the first publish, from the moment flui-sdk exists, run cargo-semver-checks on flui-sdk in advisory mode, so an sdk break that a same-PR Material fix would hide is at least surfaced.",
    "The train version bump (currently an exact =0.2.0-dev pin in every internal manifest) is done by one xtask/release command, never by hand-editing manifests.",
    "Unverified inputs that must not become ADR claims without re-checking: the size of the doubled compile cost was measured only on a two-crate probe, and the 2-8 week Bevy ecosystem lag comes from a secondary source."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "D's post-publish check ('copy packages/, strip path deps, build against the last published train') is broken by construction under lockstep exact pins. On main, packages pin the next dev version (=0.3.0-dev style). Once the path is stripped, that requirement matches nothing on crates.io. To make it resolve, the job would also have to rewrite the pins down to the last published version. Then it builds main's Material against an older sdk, and that breaks whenever Material uses sdk API added on main, which the 55% co-change rate says is routine. So the job is either always red or needs version rewriting that tests a combination no user gets. Lockstep users get Material N together with sdk N, and `cargo publish` already verifies exactly that pair at release time.",
      "evidence": "Probe at C:\\Users\\vanya\\AppData\\Local\\Temp\\claude\\D--flui\\bbb28042-f972-4a10-8e94-731819e26161\\scratchpad\\probe-train (cargo 1.98.1): the member has `sdk = { package=..., version = \"=0.3.0-dev\" }` with the path stripped, and `cargo check` gives 'error: no matching package named ... found / location searched: crates.io index'. Today every internal edge is an exact `=0.2.0-dev` pin: Cargo.toml:519-527 and crates/flui-material/Cargo.toml:35-49.",
      "severity": "major",
      "fix": "Drop the 'strip path, build against published train' job as the parity proof. Replace it with the per-PR pre-publish check below, plus semver-checks on flui-sdk against its last published version, which does model an external author pinned to the old sdk."
    },
    {
      "problem": "D's premise that proof of building 'as an outsider would' can only exist after the first publish is false. `cargo package --workspace` builds each member from its packaged tarball against the other members' packaged tarballs in a temporary local registry, with no crates.io train. That catches missing `include` files, path-only features and pin mistakes now, per PR, inside the single workspace. It removes the pre-publish window that D lists as a con. It also makes the scheduled post-publish job unnecessary.",
      "evidence": "The same probe ran `cargo package --workspace --allow-dirty --offline` and printed: 'Verifying flui-probe-mat-zz v0.3.0-dev ... Unpacking flui-probe-sdk-zz v0.3.0-dev (registry `...\\target\\package\\tmp-registry`) ... Compiling flui-probe-mat-zz v0.3.0-dev (...\\target\\package\\flui-probe-mat-zz-0.3.0-dev) Finished'.",
      "severity": "major",
      "fix": "Adopt the ecosystem_author judge's condition as the primary mechanism: an xtask command that runs `cargo package -p <official packages> -p flui-sdk ...`, wired into `cargo xtask checks` or a heavy CI job, available from W3. Keep a scheduled job only if one is still wanted after this check exists."
    },
    {
      "problem": "The engineer, ecosystem_author and owner judges each have a condition that cites crates/flui-widgets/Cargo.toml:85,182 as flui-widgets dev-depending on flui-material/flui-cupertino and needing an exemption. Both lines are comments. flui-widgets has no such dependency. The existing gate would reject one anyway, because Material and Cupertino already declare allowed-dev-dependents. So the proposed exemption is based on misread evidence. D also presents the 'no core crate names official' rule as new, but its reverse direction already exists in `cargo xtask workspace`.",
      "evidence": "crates/flui-widgets/Cargo.toml:85 '# `testing::LaidOut` harness (shared with flui-material/flui-cupertino test' and :182 '# (flui-material, flui-cupertino) flip this from their dev-dependencies'. The actual edge is the reverse: crates/flui-material/Cargo.toml dev-dep `flui-widgets = {..., features = [\"testing\"]}`. The existing rule: crates/flui-material/Cargo.toml:87-88 `allowed-dependents`/`allowed-dev-dependents = [\"flui-localizations\", \"flui-app\", \"flui\"]`, enforced at tools/xtask/src/workspace.rs:9-11,146-147,243-249.",
      "severity": "minor",
      "fix": "In the ADR, remove the flui-widgets exemption. Implement 'core names no official' by generalizing the existing allowed-dependents check keyed on tier-kind, not by adding a parallel rule."
    },
    {
      "problem": "The exemption list in D's conditions misses real core→official edges that exist today. flui-testing, core spine layer 6, dev-depends on flui-devtools, which D makes an official package. flui-devtools in turn dev-depends on flui-testing, a dev-only cycle. flui-cli dev-depends on flui-hot-reload, and flui-app has an optional normal dep on it. hot-reload also enables `flui-view/runtime-internals`, which is not in the flui-sdk/platform-api/protocol allowlist. A strict rule would therefore fail on day one on edges no condition names. A nested workspace or separate repo would also break the devtools↔testing dev-cycle.",
      "evidence": "crates/flui-testing/Cargo.toml:89 [dev-dependencies] … :106 `flui-devtools = { path = \"../flui-devtools\", version = \"=0.2.0-dev\", features = [\"inspector\"] }`; crates/flui-devtools/Cargo.toml dev-dep `flui-testing = { path = \"../flui-testing\" }`; crates/flui-cli/Cargo.toml:106 [dev-dependencies] … :114 `flui-hot-reload = { path = \"../flui-hot-reload\" }`; crates/flui-app/Cargo.toml:65,108; crates/flui-hot-reload/Cargo.toml:27 `\"flui-view/runtime-internals\"`.",
      "severity": "minor",
      "fix": "Before making the rule strict, list these edges with reasons, or remove them. The observation-seam test would move into flui-devtools, and the app→hot-reload edge is already scheduled for removal in W3 per the architecture doc's D15. Include hot-reload's runtime-internals use in the per-package allowlist with a W3 expiry."
    }
  ]
}
```
