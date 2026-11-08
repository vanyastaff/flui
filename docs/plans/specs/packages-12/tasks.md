# Tasks: producer-owned frame telemetry

- **Status:** Complete on the Windows host; `api-closure` has the environment limitation below
- **Date:** 2026-11-08
- **Requirements:** [requirements.md](requirements.md)
- **Design:** [design.md](design.md)

| Task | State | Validation evidence |
|---|---|---|
| T1. Add the typed diagnostics vocabulary and migrate every producer | done | `r901`: `cargo heli check` for foundation, scheduler, animation, rendering, view, runtime, SDK and devtools passed. `r918`: restored foundation diagnostics tests passed (3 passed, 53 filtered out). |
| T2. Make devtools consume typed span and field identities | done | `r914`: the real-`UiRuntime` profiler end-to-end test passed. `r920`: `cargo heli test -p flui-devtools` passed all 50 tests, including doctests. |
| T3. Remove the dead SDK snapshot bridge and rewrite examples | done | `r931`: `cargo heli test -p flui-sdk --test surface` passed 9 tests. `r922`: the full SDK suite passed compiler guards (1), packaged-SDK parity (1), surface tests (9) and its doctest (1). |
| T4. Strengthen contract and end-to-end regressions; update records | done | `r933`: `cargo heli test -p flui-scheduler frame_span_carries_the_budget_period` passed. `r934`: `cargo heli test -p flui-animation --features diagnostics animation_observation_contract` passed. Mutation check: changing the `Frame` wire name made `r917` fail at compile time with the expected const-classification `E0080`; after restoring the source, `r918` passed. |
| T5. Run affected suites, doctests and repository gates | done with limitation | `cargo fmt --all` and `git diff --check` passed. `r937`: strict eight-crate Clippy with `--all-targets --features flui-animation/diagnostics -- -D warnings` passed. `r939`: `xtask workspace` passed; `r940`: `xtask reach` passed; `r941`: `xtask checks` passed, including formatting, typos, docs and changelog checks. `r942`: `xtask api-closure` could not build `psm` for `x86_64-unknown-linux-gnu` because `x86_64-linux-gnu-gcc` is not installed; no toolchain or system package was installed. Linux, native-platform and GPU validation were not run. |
