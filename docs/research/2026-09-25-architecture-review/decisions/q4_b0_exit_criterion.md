# Decision panel: q4_b0_exit_criterion

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Current B0 exit (roadmap.md:617): 'just ci green on a clean Mac; cargo build --workspace per README without lld; 26 crates; no file > 3000 lines in flui-app and flui-widgets'. The architecture doc (flui-global-architecture.md:172, :726 §14 q4) already calls the 26-crate count a coincidence and asks to replace it with 'tier gate and forbid-reach green'.",
    "'just ci' is stale: `ls justfile Justfile` gives 'No such file' at the repo root. The current gate is `cargo xtask ci` (AGENTS.md Commands table). The workspace version is still `0.2.0-dev` (Cargo.toml:106), so the 'version 0.2.0' item in B0 is also open.",
    "Crate count today: `ls crates | wc -l` = 27 crates plus the facade. The target in the architecture doc (§3.3, line 172) is 26 plus the facade.",
    "Layer gate EXISTS but is weaker than the target tier gate. tools/xtask/src/workspace.rs:191-300 `check_layers` reads a numeric `[package.metadata.flui] layer` (0..10, names at Cargo.toml:91-98) and checks only DIRECT normal/build edges: `to > from` is an error, and the same layer is allowed. There is no tier letter (V/C/S/R/K/H/packages), no declared order inside a tier, and no transitive check. The `allowed-dependents` key (workspace.rs:9-13, 231-257) covers ADR-0028 for material/cupertino (crates/flui-material/Cargo.toml `allowed-dependents = [\"flui-localizations\",\"flui-app\",\"flui\"]`). It runs in `cargo xtask checks` (tasks/checks.rs:107-111), so it is on the merge path.",
    "A forbid-reach gate EXISTS only as a special case: `TREE_FACTS` in tools/xtask/src/tasks/facade.rs:53-92 holds 3 `cargo tree` substring facts, all about flui-hot-reload. It runs only through `cargo xtask facade-combos` or `feature-matrix` (tasks.rs:600-603, 831), meaning the heavy `feature-matrix` CI job (ci.yml:1379, HEAVY_JOBS ci.yml:1692-1693). It is not in the fast `checks`. It also uses a host-target `cargo tree` with no `--target all`.",
    "Experiment 1 (forbid-reach today). I ran `cargo tree -p <crate> --locked -e normal --prefix none` for each crate and counted matches. flui-foundation, tree, scheduler, painting, layer, semantics and animation reach none of wgpu/winit/windows/objc2/flui-platform/tokio. flui-interaction, rendering, objects, view, widgets, testing and material reach winit=1, windows=1, flui-platform=1, tokio=1, wgpu=0 and flui-engine=0. The source edge is `cargo tree -p flui-interaction -e normal -i flui-platform --depth 1` -> 'flui-platform <- flui-interaction'.",
    "Experiment 2 (the target matters). With `--target all`, flui-widgets, flui-view and flui-testing reach objc2 (4 versions) and jni=1. On the Windows host they show objc2=0. A reach gate must therefore use `--target all`, or it passes on one OS and fails on another.",
    "Experiment 3 (vacuous gate risk). `cargo tree -p flui-widgets|flui-view -e normal --all-features --target all` shows wgpu=0. So 'forbid-reach wgpu' for tier K (arch doc line 111) is ALREADY green today. The criterion from §14 q4, if it names only wgpu, would pass at B0 without any D1 work. The binding forbid set for K has to include flui-platform and the OS crates (winit, windows, objc2*, jni, tokio?), and that set is RED today until D1 (flui-platform-api) lands.",
    "A globals gate does NOT exist. There is no `globals` command in the xtask enum (tools/xtask/src/main.rs:35-105) and no grep hit in tools/xtask/src. My rough regex scan of crates/*/src and src (this includes #[cfg(test)] modules in src): 24 `thread_local!` (platform 5, app 4, widgets 4, view 2, others 1 each) and 89 non-const `static NAME:` lines (platform 21, widgets 12, view 9, hot-reload 8, app 6, ...). Commands: `grep -rn 'thread_local!' crates/*/src src | wc -l` -> 24, and `grep -rnE '^\\s*(pub(\\([a-z]+\\))?\\s+)?static\\s+(mut\\s+)?[A-Z_0-9]+\\s*:' ... | wc -l` -> 89. These are a seed estimate, not a syn scan.",
    "A module-DAG gate does NOT exist. `grep -rln 'module.DAG|module-dag'` finds nothing in tools/xtask. The arch doc (line 151) notes it was promised but never implemented.",
    "A process-marker gate does NOT exist. `grep marker tools/xtask/src` matches only device-probe PASS markers (device/plan.rs, device.rs). The AGENTS.md 'no Cycle N / Phase B markers' rule is enforced only by review.",
    "Perf/frame-counter ratchet does NOT exist. There is no `perf` command. `bench-collect` and `bench-compile` exist (main.rs:71,105), and CI runs `cargo xtask bench-collect` twice.",
    "One frame transaction is NOT met today. The frame sequence is driven from two production crates: `grep -rnE '\\.run_frame_with_layout_builders\\('` gives flui-app 1, flui-testing 2 (plus 4 in flui-view src, rough, likely tests/defs), and `HeadlessBinding::pump_frame` sits at crates/flui-testing/src/lib.rs:955 with its own begin/draw-frame orchestration (lib.rs:992-1105). flui-app calls `.handle_begin_frame(` once. The arch doc schedules W1 (gates) and W2 (flui-runtime, one transaction) both for B0 (arch doc lines 629-630).",
    "Old file-size criterion: `find crates/flui-app crates/flui-widgets -name '*.rs' -exec wc -l {} +` finds 5 files over 3000 lines. They are realm_dispatch.rs 7149, editable_text.rs 4725, tests/navigator_public.rs 3798, tests/scroll.rs 3107 and text/controller.rs 3020. There are 17 such files across all crates. Nothing enforces a limit today.",
    "Other ratchets named in arch §8 and their current state: deny.toml:103 `multiple-versions = \"allow\"`; Cargo.toml:397 `undocumented_unsafe_blocks = \"allow\"`.",
    "Anti-vacuity pattern already in the repo: `checks` runs `wgsl --self-test` before `wgsl` (tasks/checks.rs:108-109). A unit test pins the exact list of in-process checks (tasks/checks.rs:138-160, `the_in_process_checks_include_the_link_check_under_strict`). So 'gate X exists on the merge path' is itself checkable: X is present in that pinned list.",
    "Proposed objective B0 exit (each item is one command, run on main in CI):\n(1) `cargo xtask workspace`: the tier gate. Manifests declare `tier = V|C|S|R|K|H|pkg` plus an intra-tier order. Direct edges must go down, or sideways only to an earlier crate in the declared order. 0 findings.\n(2) `cargo xtask reach`: a new command in `checks`. It reads `forbid-reach` from each manifest and checks it transitively with `cargo metadata`/`cargo tree -e normal --target all --locked`, with every feature combination the facade supports. K must not reach wgpu, flui-engine, flui-platform, winit, windows, objc2*, jni or flui-app. Packages must not reach wgpu, engine, app or OS crates. The 3 hot-reload TREE_FACTS fold into it. It must ship a self-test that fails on a seeded violation.\n(3) `cargo xtask globals`: a syn scan of every `static` and `thread_local!`, compared with an allowlist seeded from the scan. Exit when it is green and there is no entry in tiers V/C/K outside the named trampoline cell and the ID counters.\n(4) `cargo xtask module-dag -p flui-widgets`: green against the declared module order.\n(5) One transaction: a call-site allowlist in `globals`/`reach` style. `run_frame_with_layout_builders` and `SchedulerBinding::handle_begin_frame` are called in production only from flui-runtime. The check is `rg 'pub fn pump_frame' crates/flui-testing` returning empty, plus the allowlist check.\n(6) `cargo xtask perf --check`: the frame-counter baseline is recorded and the check fails on any increase.\n(7) Markers plus 'core does not name official packages': part of `checks`.\n(8) A file-length ratchet instead of the fixed 3000 limit: an allowlist of current oversize files (5 in app/widgets) that may only shrink, with realm_dispatch.rs split during the B0 ui_realm work.\n(9) `cargo xtask ci` green on the macos-latest runner (this replaces the dead 'just ci'), plus README `cargo build` with no lld config.\n(10) Workspace version is 0.2.0.\nThe crate count becomes a derived fact of the tier table and is no longer a target."
  ],
  "market_precedents": [
    {
      "who": "rust-lang/rust (tidy)",
      "what": "`x test tidy` style check with a fixed LINES = 3000 file-length limit and a per-file `ignore-tidy-filelength` opt-out; the same tool also bans TODO/XXX, dbg!, and undocumented unsafe in core/alloc.",
      "outcome_or_lesson": "The '3000 lines' number in the B0 exit matches rustc's tidy limit. rustc runs it as a permanent per-file gate with explicit exemptions, not as a milestone target. That supports turning FLUI's '>3000' into an allowlist ratchet inside `checks`.",
      "source": "https://raw.githubusercontent.com/rust-lang/rust/master/src/tools/tidy/src/style.rs"
    },
    {
      "who": "EmbarkStudios cargo-deny",
      "what": "`[bans] deny = [{ name, wrappers = [...] }]` lets only the listed crates depend on a banned crate DIRECTLY and denies every transitive use elsewhere. Also `multiple-versions` with a per-crate `skip`.",
      "outcome_or_lesson": "This is a ready-made transitive reach mechanism that FLUI already runs (`cargo xtask deps`). It only works for one global wrapper set per crate, not per-tier forbids, so FLUI still needs its own per-tier `reach`. cargo-deny could back the 'wgpu only via flui-engine' and 'OS crates only via platform backends' rules for free.",
      "source": "https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html"
    },
    {
      "who": "Flutter (dev/bots/analyze.dart)",
      "what": "The repo-wide analysis script runs verify* steps. Among them: framework bad-imports/layer checks (e.g. no circular imports, package import rules), transitive package dependency allowlist, test/example cross-import checks.",
      "outcome_or_lesson": "Flutter enforces its layering (foundation < ... < widgets) as a CI script over imports and a transitive dependency allowlist, not by counting packages. It is a module-DAG and reach gate, the same shape as the proposed B0 gates.",
      "source": "https://raw.githubusercontent.com/flutter/flutter/master/dev/bots/analyze.dart"
    },
    {
      "who": "Nx (monorepo tool)",
      "what": "`enforce-module-boundaries`: projects carry tags, and `depConstraints` say which tags may depend on which. A conformance check runs over the whole project graph. Projects without tags may depend on nothing.",
      "outcome_or_lesson": "Tag-based tier constraints map directly to FLUI's tier letters. 'Untagged means no dependencies allowed' corresponds to 'a crate without a tier fails the gate', which workspace.rs:211 already does for layers.",
      "source": "https://nx.dev/docs/features/enforce-module-boundaries"
    },
    {
      "who": "matklad / rust-analyzer ARCHITECTURE.md",
      "what": "Recommends stating architectural invariants explicitly and notes that important invariants are often an absence of something.",
      "outcome_or_lesson": "Forbid-reach is exactly an absence invariant. The post only asks for documentation; FLUI's 'make rules types, not reviews' stance goes further and gates it.",
      "source": "https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html"
    },
    {
      "who": "GN / Chromium (from memory, not fetched: gn.googlesource.com returned 503)",
      "what": "`assert_no_deps` on a target fails the build if any listed target is anywhere in its transitive dependency tree. `visibility` restricts who may depend on a target.",
      "outcome_or_lesson": "Hypothesis, since the page was not verified. This is the closest build-system analogue to a per-crate `forbid-reach` list checked on every build, and it separates 'who may depend on me' (visibility, like FLUI's allowed-dependents) from 'what I must never reach' (assert_no_deps, like forbid-reach).",
      "source": "https://gn.googlesource.com/gn/+/main/docs/reference.md (503 at fetch time)"
    }
  ],
  "constraints": [
    "If the B0 criterion names only 'forbid-reach wgpu' for tier K, it passes vacuously. K crates already reach wgpu=0 with --all-features and --target all. The exit must list the OS set (flui-platform, winit, windows, objc2*, jni), which is red today because of flui-interaction -> flui-platform, and so ties B0 to D1.",
    "A reach gate must use `cargo tree --target all` (or cargo metadata with no target filter). The Windows host showed objc2=0 for flui-widgets, while --target all showed 4 versions.",
    "The existing reach facts run only in the heavy feature-matrix job. To be on every PR's merge path the new gate belongs in `cargo xtask checks`. `cargo tree` resolves without compiling, and my runs over 14 crates finished within the tool timeout, so fitting the 'checks compile nothing' contract is plausible (timing not measured).",
    "Each new gate should have a self-test that fails on a seeded violation (wgsl --self-test precedent), or 'green' proves nothing. The pinned list test in checks.rs makes 'the gate is wired' checkable.",
    "Ratchet allowlists (globals, file length, unsafe, multiple-versions) must be seeded by scan in the same PR as the gate (arch §8). My static/thread_local counts are regex estimates that include in-src test modules and are not a valid seed.",
    "'One frame transaction' depends on W2 (flui-runtime extraction). It is an exit criterion only if the owner keeps W2 inside B0, as the arch doc's §11 table places it. Otherwise it moves to B1.",
    "'just ci' in the current exit refers to a tool that does not exist in the repo and must be rewritten as `cargo xtask ci` on macos-latest.",
    "AGENTS.md: a new gate must be both a `cargo xtask` command and a CI step under the `ci` aggregator. Changing .github/workflows is outside a normal task, so the B0 gate PRs that touch CI need explicit owner sign-off."
  ],
  "experiments_run": [
    "`ls crates | wc -l` -> 27; `ls justfile Justfile` -> not found; `grep ^version Cargo.toml` -> 0.2.0-dev",
    "Read tools/xtask/src/workspace.rs (layer check: direct edges, numeric layer, same-layer allowed), tasks/checks.rs (7 in-process checks, pinned by unit test), tasks/facade.rs (TREE_FACTS, hot-reload only), main.rs (no globals/reach/module-dag/perf/markers command)",
    "Per-crate `cargo tree -p <c> --locked -e normal --prefix none | sort -u` counting wgpu/winit/windows/objc2/flui-platform/flui-engine/flui-app/tokio for 14 crates: K and render crates reach flui-platform, winit, windows and tokio, and none reach wgpu or flui-engine",
    "`cargo tree -p flui-interaction -e normal -i flui-platform --depth 1` -> flui-platform <- flui-interaction (the single edge that makes K reach OS crates)",
    "Same with `--target all`: widgets, view and testing reach objc2 (4) and jni (1); `--all-features --target all` for widgets and view: wgpu=0",
    "Regex scans: 24 thread_local!, 89 non-const statics across crates/*/src and src (rough)",
    "`find ... -exec wc -l` -> 5 files over 3000 lines in app/widgets (realm_dispatch.rs 7149), 17 workspace-wide",
    "Call-site grep for `.run_frame_with_layout_builders(`: flui-app 1, flui-testing 2, flui-view 4; `.handle_begin_frame(`: flui-app 1, scheduler 13; `pub fn pump_frame` at flui-testing/src/lib.rs:955",
    "grep .github/workflows for xtask commands: `checks`/`checks --strict` and `feature-matrix --slice` are present (TREE_FACTS reach CI only via feature-matrix, a HEAVY job per ci.yml:1692)"
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "Minimal swap: replace '26 crates' with 'tier gate + forbid-reach green'",
      "description": "Take §14 q4 literally. Remove the crate count. Add (1) `cargo xtask workspace` with tier letters and (2) a new `cargo xtask reach`. Keep the rest of the current exit, with 'just ci' rewritten to `cargo xtask ci` on macos-latest and the README lld item kept. Keep the fixed '>3000 lines' limit.",
      "pros": [
        "Smallest change to the roadmap, and exactly what the arch doc asked (flui-global-architecture.md:726 q4)",
        "Both gates build on code that already exists: workspace.rs:191-300 check_layers and facade.rs:53-92 TREE_FACTS",
        "Cheap to land in W1"
      ],
      "cons": [
        "Vacuous if the forbid set for tier K names only wgpu. `cargo tree -p flui-widgets|flui-view -e normal --all-features --target all` already gives wgpu=0, so it is green today with no D1 work",
        "Leaves out globals, module-DAG and one-transaction. These are exactly the architecture invariants B0 (W1/W2) exists to establish",
        "Keeps the fixed 3000-line target. Five files in app/widgets exceed it (realm_dispatch.rs is 7149 lines), and a fixed number turns into a split-for-the-metric exercise, not a permanent rule",
        "Nothing proves the gate is on the merge path. The reach facts today run only in the heavy feature-matrix job (ci.yml:1379, 1692)"
      ],
      "cost_now": "Low: tier metadata in 28 manifests plus a reach command, about 1-2 PRs",
      "cost_later": "High. Globals, module-DAG and one-transaction slip into B1 without a gate, and B1 work (the !Send flip, Subsecond, the reactive graph) builds on an unverified shape",
      "reversibility": "Easy to add items later, but B0 would already be declared closed",
      "fits_plan": "Partly. It matches §14 q4 wording but undercuts W1's own list (tier metadata, forbid-reach, globals, module-DAG, markers, perf; arch doc W1 row)"
    },
    {
      "id": "B",
      "name": "Full 10-item gate list, everything green inside B0",
      "description": "The B0 exit is these 10 commands, all green on main in CI: workspace (tiers), reach, globals (with no V/C/K entries beyond the trampoline and ID counters), module-dag -p flui-widgets, a one-transaction call-site allowlist, perf --check, markers plus 'core does not name official', a file-length ratchet, `cargo xtask ci` on macos-latest plus README build without lld, and version 0.2.0.",
      "pros": [
        "Every architecture invariant is checked by a command before B1 builds on it",
        "The crate count falls out of the tier table and stops being a target",
        "Matches the arch §11 table, which puts W1 and W2 both in B0"
      ],
      "cons": [
        "'Globals: no V/C/K entries' is a content goal, not a gate. Today there are about 24 thread_local! and 89 non-const statics (rough regex count). Shrinking them is W5 work, the precondition for Subsecond, so B0 would absorb B1 scope",
        "perf --check has no baseline and no phase counters yet (no perf command in main.rs:35-105). Making it B0-blocking puts a measurement project on the critical path",
        "One transaction needs all of W2 (flui-runtime extraction plus moving flui-testing up; pump_frame at flui-testing/src/lib.rs:955). This is large and couples B0 to the riskiest refactor",
        "Several of the items need changes under .github/workflows, which requires owner sign-off for each one"
      ],
      "cost_now": "Very high: most of W1 and W2 plus part of W5 before B0 closes",
      "cost_later": "Low: B1 starts on a fully gated shape",
      "reversibility": "Hard to scope down once announced. Unfinished ratchet targets tend to become exemptions",
      "fits_plan": "Formally fits the §11 wave table but contradicts the roadmap principle 'close a milestone by a command' by bundling long work into one gate. It risks keeping B0 open for too long"
    },
    {
      "id": "C",
      "name": "Hybrid: 'gates exist, are wired and are anti-vacuous' plus binding structural greens; content ratchets frozen, not driven to zero",
      "description": "The B0 exit has three kinds of items. [G] Wiring, checkable by one unit test: the pinned list in tools/xtask/src/tasks/checks.rs:138-160 contains workspace-tiers, reach, globals, module-dag, markers, core-names-no-official and file-length. Each has a `--self-test` that fails on a seeded violation (the wgsl --self-test precedent at checks.rs:108-109), and `cargo xtask checks` runs all of them on every PR. [S] Structural greens (binding): (1) tier gate: every manifest declares tier V/C/S/R/K/H/pkg plus an order within the tier; edges go down, or sideways only to an earlier crate in that order; 0 findings. (2) reach, with `--target all --locked -e normal` over the facade's feature combos. The K forbid set is {flui-platform, winit, windows, objc2*, jni, wgpu, flui-engine, flui-app}; the pkg set is {wgpu, flui-engine, flui-app, OS crates}. The 3 hot-reload TREE_FACTS fold into it. It is red today through flui-interaction -> flui-platform, so it closes only with D1a. (3) module-dag -p flui-widgets green. (4) One transaction: a call-site allowlist in which `run_frame_with_layout_builders` and `handle_begin_frame` have exactly one production caller, and `rg 'pub fn pump_frame' crates/flui-testing` is empty. This item applies only while W2 stays in B0; otherwise it moves verbatim to the B1 exit. [R] Ratchets frozen, not targets: the globals, file-length (seeded with the current oversize files; realm_dispatch.rs must drop off during the ui_realm.rs work), unsafe-undocumented and multiple-versions allowlists are seeded by syn scan in the same PR as the gate and may only shrink. Perf: the counters and a recorded baseline exist, and `cargo xtask perf --check` runs as non-blocking in B0 and becomes blocking at B1 exit. [P] Hygiene: `cargo xtask ci` green on macos-latest (replaces the dead 'just ci'; there is no justfile), README `cargo build --workspace` without lld config, and workspace version 0.2.0 (Cargo.toml:106 is still 0.2.0-dev).",
      "pros": [
        "Every item is one command, and the wiring is itself pinned by a test, so 'the gate exists' cannot quietly regress",
        "Anti-vacuity is explicit: the binding K forbid set includes flui-platform and the OS crates, so B0 really requires D1a, and self-tests prove every gate can fail",
        "--target all avoids passing on one OS and failing on another (objc2=0 on the Windows host, 4 versions with --target all)",
        "Ratchets freeze debt at B0 without pulling W5 content work (reducing globals) into B0. This follows arch §8's 'seed by scan in the same PR'",
        "It moves the gates from the heavy feature-matrix job onto every PR's merge path (checks). cargo tree compiles nothing, so this fits the checks contract (plausible, timing not measured)",
        "The file-length ratchet matches rustc tidy practice (LINES=3000 with per-file exemptions), not a one-shot target",
        "The one-transaction item follows whatever the owner decides about W2, with no rewrite of the criterion"
      ],
      "cons": [
        "More xtask work than option A: about 6 new check modules, each with a self-test",
        "The reach check over the facade's feature combos inside checks may be slow (hypothesis, not measured). It may need to be limited to the default feature set plus --all-features",
        "Freezing globals at their current count means B0 does not prove the realm-ownership invariant, only that it does not get worse",
        "The CI step changes (macos-latest ci run, perf non-blocking step) touch .github/workflows and need owner sign-off"
      ],
      "cost_now": "Medium. W1 track A as planned (gates with seeded allowlists), plus D1a to turn reach green. W2 only if it stays in B0",
      "cost_later": "Low to medium. B1 tightens the perf gate and the one-transaction check if deferred, and later waves shrink the ratchets under an existing gate",
      "reversibility": "High. Allowlists and forbid sets are data in manifests and xtask; items can move between B0 and B1 without rewriting any gate",
      "fits_plan": "Best fit. It implements the W1 row literally, ties the reach green to the D1a entry that already sits in W1 track B, keeps the roadmap rule 'a milestone closes when a command proves it', and answers §14 q4: the crate count becomes a derived fact of the tier table"
    },
    {
      "id": "D",
      "name": "Keep the counts, add gates alongside",
      "description": "Keep '26 crates' and 'no file > 3000 lines in app/widgets' as they are, and add the tier and reach gates as extra items.",
      "pros": [
        "The owner's original numbers stay readable",
        "Zero rework of the roadmap wording"
      ],
      "cons": [
        "The arch doc (line 172) calls the 26-crate count a coincidence. It would pressure the team to merge or split crates to hit a number",
        "The fixed 3000 limit on two crates is ad hoc. 17 files workspace-wide exceed it, and nothing enforces it after B0",
        "'just ci' stays dead text unless it is rewritten anyway"
      ],
      "cost_now": "Low",
      "cost_later": "Medium: the metric-driven crate and file surgery has to be undone when the tier table changes",
      "reversibility": "Easy",
      "fits_plan": "Poor. It contradicts the target architecture's premise that tiers, not counts, define the shape"
    }
  ],
  "recommended": "C",
  "rationale": "A B0 exit has to be both objective and non-vacuous. Option A is not non-vacuous: a 'forbid-reach wgpu' rule for tier K is already green today (`cargo tree -p flui-widgets -e normal --all-features --target all` gives wgpu=0), so it proves nothing. The binding forbid set must include flui-platform and the OS crates. That set is red today through the one edge flui-interaction -> flui-platform, which correctly ties B0 to D1a, already scheduled in W1. Option B turns content work (reducing globals toward zero, a perf baseline, parts of W5) into B0 blockers and keeps the milestone open too long. Option C separates three kinds of item:\n- Gates that exist and are wired: pinned in the in-process list test at tools/xtask/src/tasks/checks.rs:138-160, each with a self-test on a seeded violation, following the wgsl --self-test precedent.\n- Structural invariants that must be green: tiers, reach with `--target all`, module-DAG, and one frame transaction, conditional on W2 staying in B0.\n- Debt that is only frozen: allowlists seeded by syn scan that may only shrink, which is the rustc tidy file-length model.\n\nWhat exists today in tools/xtask:\n- The numeric layer gate covers direct edges only (workspace.rs:191-300, in checks).\n- allowed-dependents covers ADR-0028.\n- The hot-reload-only TREE_FACTS run only in the heavy feature-matrix job (facade.rs:53-92).\n- There is no reach, globals, module-dag, markers, file-length or perf command (main.rs:35-105).\n\nThe same exit also fixes two stale items: 'just ci' (there is no justfile, so it becomes `cargo xtask ci` on macos-latest) and the still-open version 0.2.0 (Cargo.toml:106 is 0.2.0-dev). The crate count becomes a derived fact of the tier table.\n\nOwner decisions needed:\n- Whether W2 (one transaction) stays in B0.\n- Sign-off for the workflow changes: the macos-latest ci run and the perf step.\n\nHypotheses not verified:\n- How long the reach check over the facade's feature combos takes inside checks.\n- Exact globals counts, which need a syn scan; the current numbers come from a regex over src that includes test modules."
}
```

## judge_q4_b0_exit_criterion_engineer

```json
{
  "choice": "C",
  "confidence": 0.82,
  "reasons": [
    "The key facts check out. roadmap.md B0 row: the exit really says 'just ci ... 26 крейтов ... > 3000 строк'. flui-interaction/Cargo.toml:31 depends on flui-platform directly, so a reach gate whose K set includes flui-platform and the OS crates is red today and requires D1a. A wgpu-only set would pass vacuously (research experiment 3).",
    "The pinned list test at tools/xtask/src/tasks/checks.rs:~150 asserts the exact in-process list (docs-links, workspace, toolchain, wgsl --self-test, wgsl, paths-filter, font-assets --package-list). That makes 'the gate is wired on the merge path' checkable by one unit test, and the wgsl --self-test pattern already exists to prove a gate can fail.",
    "Option A is vacuous for K on wgpu and drops the invariants W1 exists to build. Option B makes content reduction (globals toward zero, a perf baseline) B0 blockers and absorbs W5 and measurement work into B0. Option D keeps a count that the arch doc itself calls a coincidence. C is the only option that is objective, non-vacuous and bounded.",
    "Treating debt as 'frozen ratchet, not target' follows rustc tidy (file length 3000 with per-file exemptions) and cargo-deny skip lists. An allowlist that can only shrink is permanent, while a milestone number stops being enforced once B0 closes.",
    "Keeping the forbid sets and allowlists as data lets items move between B0 and B1 without rewriting the gates, so the unresolved W2 scope decision does not block writing the criterion."
  ],
  "conditions": [
    "Wiring is one unit-test assertion: extend the in_process() list pinned in tools/xtask/src/tasks/checks.rs to name workspace (tiers), reach, globals, module-dag, markers and file-length. Each one runs `<gate> --self-test` against a seeded violation before its real run.",
    "The K forbid set must name flui-platform, winit, windows, objc2*, jni, wgpu, flui-engine and flui-app. Write it into the roadmap text so the reach item cannot be satisfied by wgpu alone. B0 reach-green depends explicitly on D1a.",
    "Reach resolves with `--target all -e normal --locked`, or cargo metadata with no target filter. In `checks` it runs default features plus --all-features only, and the full facade combos stay in feature-matrix. Measure the time before merging. If it is above about 30s on CI, move it to a separate fast job, not a heavy one.",
    "Fold the 3 hot-reload TREE_FACTS (tools/xtask/src/tasks/facade.rs:53-92) into reach in the same PR, so there are not two reach mechanisms.",
    "Seed every ratchet allowlist (globals via syn with #[cfg(test)] modules excluded, file length, undocumented unsafe, multiple-versions) by a scan in the same PR as its gate. The regex counts (24 thread_local!, 89 statics) are not a valid seed.",
    "The one-transaction item is conditional. It stays in the B0 exit only if the owner keeps W2 in B0. Otherwise it moves word for word to the B1 exit, along with making perf --check blocking. Record that decision in the roadmap, not implicitly.",
    "Rewrite the dead 'just ci' as `cargo xtask ci` on macos-latest. Keep the README no-lld build and version 0.2.0 (Cargo.toml still 0.2.0-dev) as hygiene items.",
    "Any change to .github/workflows (the macos ci run, a non-blocking perf step, a new job in the ci aggregator) needs explicit owner sign-off per AGENTS.md. Gates folded into `cargo xtask checks` avoid a workflow edit.",
    "Drop the crate count from the exit completely. The tier table in the manifests is the source of truth, and docs/crates.md is derived from it.",
    "Tier-order semantics must be decided before the first manifest edit: same-tier edges are allowed only to an earlier crate in the declared order, and `cargo xtask workspace` stays a direct-edge check while transitive absence belongs to reach. That keeps the two gates from overlapping."
  ]
}
```

## judge_q4_b0_exit_criterion_ecosystem_author

```json
{
  "choice": "C",
  "confidence": 0.8,
  "reasons": [
    "From the app and package author's side (H0-H4), what matters at B0 is that the public tiers are stable and no K or package path reaches OS crates or the engine. Only option C makes that binding. Its K forbid set covers flui-platform, winit, windows, objc2*, jni, wgpu, flui-engine and flui-app. A wgpu-only set is vacuous: `cargo tree -p flui-widgets -e normal --all-features --target all` already gives wgpu=0 today. The flui-interaction -> flui-platform edge keeps the full set red until D1a lands, which is the reason for a B0 gate.",
    "Reach has to be checked with `--target all`. On the Windows host objc2 shows 0 for flui-widgets, but with --target all it shows 4 versions. A third-party author building on macOS or Android would otherwise get a gate that passed on the maintainer's machine and fails on theirs.",
    "Option C converts debt (globals, file length, undocumented unsafe, multiple-versions) into allowlists that can only shrink. It does not turn it into zero targets. That follows rustc tidy (LINES=3000 plus per-file ignores) and keeps W5 content work out of B0, so B0 can actually close and B1 can start. Option B would stall the milestone that external authors are waiting on.",
    "It removes the crate count (the arch doc calls 26 a coincidence) and the dead 'just ci' item: there is no justfile, and the current gate is `cargo xtask ci`. It also puts the unmet version 0.2.0 on the exit list. Every item is one command, so a milestone closes by proof.",
    "Pinning the wiring in the in-process list test (tools/xtask/src/tasks/checks.rs) and requiring a --self-test per gate (the precedent is wgsl --self-test) makes 'gate exists and can fail' itself regression-proof. Today the only reach facts, TREE_FACTS in facade.rs:53-92, run only in the heavy feature-matrix job."
  ],
  "conditions": [
    "The K forbid set in the exit text lists flui-platform and the OS crates explicitly: winit, windows, objc2*, jni, wgpu, flui-engine and flui-app. A criterion that names only wgpu does not count.",
    "reach uses `cargo tree`/`cargo metadata` with `--target all --locked -e normal`. It covers at least the default features plus --all-features of the facade, and the full combo set only if timing in `checks` is measured and acceptable (this timing is unverified).",
    "Every new gate ships a --self-test that fails on a seeded violation, and it appears in the pinned in-process checks list test.",
    "Ratchet allowlists are seeded by a syn scan in the same PR as the gate, not by the regex counts (24 thread_local!, 89 statics are estimates that include test modules). Entries may only be removed.",
    "The tier gate adds an explicit intra-tier order, so a same-tier edge must point to an earlier crate. A crate with no tier fails the gate. It also covers package-tier crates, so external package templates can declare `tier = pkg` and be checked the same way.",
    "One-transaction (a single production caller of run_frame_with_layout_builders/handle_begin_frame, and no pub pump_frame in flui-testing) stays a B0 item only if the owner keeps W2 in B0. Otherwise it moves verbatim to the B1 exit, and B0 notes this explicitly.",
    "Perf: counters and a recorded baseline exist at B0 as a non-blocking step, and the step becomes blocking at the B1 exit.",
    "Changes under .github/workflows (the macos-latest `cargo xtask ci` run, the perf step, moving reach into checks) get explicit owner sign-off, per AGENTS.md.",
    "realm_dispatch.rs (7149 lines) drops off the file-length allowlist as part of the B0 ui_realm work, so the ratchet shows real movement rather than only a freeze."
  ]
}
```

## judge_q4_b0_exit_criterion_owner

```json
{
  "choice": "C",
  "confidence": 0.8,
  "reasons": [
    "The current B0 exit (roadmap.md:617) has two items that cannot be used as they stand. 'just ci' refers to a tool that does not exist: there is no justfile, and the gate today is `cargo xtask ci`. '26 crates' is a count the architecture doc itself calls a coincidence (flui-global-architecture.md:172). Both must go, which rules out option D.",
    "Option A can pass without proving anything. If tier K forbids only wgpu, the gate is green today: `cargo tree -p flui-widgets -e normal --all-features --target all` gives wgpu=0. Only a forbid set that includes flui-platform, winit, windows, objc2* and jni is red today, through the single edge flui-interaction -> flui-platform. That is the set that makes B0 actually depend on D1a.",
    "Option B pulls content work into B0: shrinking about 24 thread_local! and 89 non-const statics (regex estimate), plus a perf baseline that has no command yet. For a sole owner with agents, that means B0 stays open too long and the WIP limit is broken. The roadmap rule is that a milestone closes when a command proves it, not after a whole wave is finished.",
    "Option C splits the exit into three kinds of item. Gates must be wired and able to fail: the pinned list test in tasks/checks.rs plus a --self-test for each gate, following the wgsl --self-test precedent. Structural invariants must be green: tiers, reach, module-DAG, and one frame transaction if W2 stays in B0. Debt is only frozen: allowlist ratchets that may only shrink, the rustc tidy LINES=3000 model. Every item is one command, and all of it is data in manifests or xtask, so items can move between B0 and B1 without rewriting a gate.",
    "I confirmed that none of these gates exist today. Grepping tools/xtask/src/main.rs for Reach, Globals, ModuleDag, Perf, Markers or FileLen finds nothing. What does exist: the numeric layer gate in workspace.rs, which checks direct edges only and runs in checks; allowed-dependents for ADR-0028; and the hot-reload-only TREE_FACTS, which run only in the heavy feature-matrix job.",
    "Moving reach into checks and running it with --target all removes the pass-on-Windows, fail-on-Mac gap: objc2=0 on the host versus 4 versions with --target all.",
    "I lowered confidence from about 0.9 because C still adds about 6 xtask modules, each with a self-test, which is a real maintenance and bus-factor cost for one owner. Two things are unmeasured: how long reach takes inside checks, and the exact seed counts, which need a syn scan."
  ],
  "conditions": [
    "The K forbid set in the exit text must name flui-platform, winit, windows, objc2*, jni, wgpu, flui-engine and flui-app. Wgpu alone does not count, because it is already green today.",
    "Reach must use `cargo tree`/`cargo metadata` with `--target all --locked -e normal`. If a timing run shows the full facade feature matrix is too slow for checks, limit it to the default features plus --all-features. The run must be measured before the gate is committed to checks.",
    "Every new gate ships in the same PR as (a) a --self-test that fails on a seeded violation and (b) an entry in the pinned in-process list test in tools/xtask/src/tasks/checks.rs. A gate without both does not count toward B0.",
    "The ratchet allowlists (globals, file-length, undocumented-unsafe, multiple-versions) are seeded by a syn/cargo scan in the gate's own PR, never from the regex estimates, and may only shrink. At B0 they are frozen, not driven to zero.",
    "The one-frame-transaction item is binding for B0 only if the owner explicitly keeps W2 in B0. Otherwise it moves verbatim into the B1 exit. The owner must make that decision up front.",
    "perf --check is non-blocking in B0 (counters plus a recorded baseline) and becomes blocking at the B1 exit.",
    "Replace 'just ci' with `cargo xtask ci` green on macos-latest. The workflow changes that requires (the macOS run, the perf step) need explicit owner sign-off under the AGENTS.md rule that .github/workflows is left alone unless the task is about it.",
    "Keep the hygiene items as they are: README `cargo build --workspace` with no lld config, and workspace version 0.2.0 (Cargo.toml:106 is still 0.2.0-dev). The crate count is only reported as a derived fact of the tier table and is never a target.",
    "WIP limit: land the gates in order. The tier gate and reach go first, since they are the most valuable and build on existing code. Then markers and file-length, which are cheap. Then globals and module-dag. Don't start new gate PRs while one is still in flight."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "The binding K forbid set named in C (and in every judge's conditions) is flui-platform, winit, windows, objc2*, jni, wgpu, flui-engine, flui-app. Checked with --all-features, it stays red after D1a, so the claim that the item goes red today and closes only with D1a is false. flui-widgets reaches jni through the network-images feature and HTTPS certificate verification, not through the platform layer. The set also mixes platform-abstraction crates with generic FFI crates that ordinary ecosystem dependencies pull in.",
      "evidence": "`cargo tree -p flui-widgets -e normal --all-features --target all --locked --prune flui-platform -i jni` gives jni v0.22.4 <- rustls-platform-verifier v0.7.0 <- reqwest v0.13.5 <- flui-assets <- flui-widgets. The chain is crates/flui-widgets/Cargo.toml:177 `network-images = [\"asset-images\", \"flui-assets/network\"]` and crates/flui-assets/Cargo.toml:69 `network = [\"dep:reqwest\"]`. With flui-platform pruned, the K graph still contains core-foundation 0.10.1, windows-sys 0.61.2 and web-sys (file scratchpad/k_pruned.txt, 247 packages).",
      "severity": "major",
      "fix": "Forbid the platform-abstraction and windowing crates: flui-platform, winit, android-activity, ndk, the `windows` crate (not windows-sys), objc2-app-kit and objc2-ui-kit, wgpu, flui-engine, flui-app. Generic FFI crates (jni, windows-sys, core-foundation, bare objc2) should be allowed, or go on a per-edge allowlist entry with a stated reason, such as reqwest -> rustls-platform-verifier. Then re-verify that the set is red today only through flui-interaction -> flui-platform, which it currently is: `--prune flui-platform` leaves none of winit, windows, objc2-app-kit or wgpu."
    },
    {
      "problem": "The one-transaction item targets the wrong functions and passes vacuously. The requirement is exactly one production caller of `run_frame_with_layout_builders` and `handle_begin_frame`, plus an empty grep for `pub fn pump_frame`. The second transaction in flui-testing calls neither of those functions. handle_begin_frame already has two production callers. The grep passes after a simple rename.",
      "evidence": "The method calls inside crates/flui-testing/src/lib.rs:955-1079 include `drive_frame_with_lane`, `build_scope`, `tick_all` and `hit_test`. They do not include handle_begin_frame or run_frame_with_layout_builders. The production callers of handle_begin_frame are crates/flui-scheduler/src/scheduler.rs:1945 (inside the scheduler's own frame driver) and crates/flui-app/src/app/runtime.rs:2060. drive_frame_with_lane has production callers in runner/{android.rs:454, desktop.rs:504, ios.rs:426, mod.rs:624, mod.rs:681, frame_pacing.rs:1115, frame_pacing.rs:1195}.",
      "severity": "major",
      "fix": "Define the item by who may drive frame phases, not by function names. Either (a) make the scheduler's frame-driving entry points (drive_frame_with_lane, handle_begin_frame/handle_draw_frame) and the view/binding frame entry reachable only from flui-runtime, via pub(crate), a sealed token or a capability type (the AGENTS 'types, not reviews' rule), or (b) run a syn scan that allows calls to them only inside flui-runtime, with a self-test. Do not use `pub fn pump_frame` absence as the criterion."
    },
    {
      "problem": "The reach gate cannot be built naively on `cargo tree -i <name>`. For an absent crate the command errors out, so a gate that only checks the exit code fails when the graph is actually clean. For a crate present in several versions it reports an ambiguous specification. The existing TREE_FACTS pattern (a needle search over `cargo tree` output) works only because it matches a single name.",
      "evidence": "`cargo tree -p flui-widgets ... -i wgpu` -> `error: package ID specification 'wgpu' did not match any packages`. `-i objc2` -> `error: specification 'objc2' is ambiguous` (objc2@0.5.2 and 0.6.4). The same happens for windows-sys (0.52, 0.59, 0.61) and core-foundation (0.9.4, 0.10.1). The existing pattern is tools/xtask/src/tasks/facade.rs:53-92.",
      "severity": "minor",
      "fix": "Build reach on the resolved graph from `cargo metadata --locked` (no target filter), or on a single `cargo tree --prefix none` output, and match against package names or globs. The self-test should seed both an absent crate and a crate present in several versions."
    },
    {
      "problem": "The panel's cost concern about reach timing is unfounded, and the citation for the layer gate is wrong. The workspace layer check lives in tools/xtask/src/workspace.rs, not under tasks/, and it already treats dev-dependency edges differently. That matters for the rule 'sideways only to an earlier crate' because flui-view has a dev-dependency cycle with flui-testing.",
      "evidence": "`time cargo tree -p flui -e normal --all-features --target all --locked` gives real 0.386s, and the same run for flui-widgets gives 0.377s, so the default-plus-all-features restriction the judges asked for is unnecessary. `grep -rln 'fn check_layers' tools/xtask/src` finds tools/xtask/src/workspace.rs, where line ~239 branches on `DependencyKind::Development` (allowed-dev-dependents). crates/flui-view/Cargo.toml:63-67 declares the dev-only cycle with flui-testing.",
      "severity": "minor",
      "fix": "Run reach over the full facade feature combos inside `checks`, since it takes under 1s per graph. In the tier-gate definition, say that the tier order applies only to normal and build edges, and that dev edges keep going through allowed-dev-dependents. Correct the path to tools/xtask/src/workspace.rs."
    }
  ]
}
```
