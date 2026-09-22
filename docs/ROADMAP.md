[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Back to README](../README.md)

# FLUI Roadmap

Active objective: a verified beta (see [BETA.md](BETA.md) for acceptance criteria and per-platform evidence). The living plan with tracks, epics and the worker backlog lives in the roadmap document: https://claude.ai/code/artifact/4e1d6ca0-5ce6-4653-bcd6-22177719f34a. Historical roadmaps are in `docs/archive/` and are not a source of status.

## Milestones

| Milestone | Theme | Exit criterion |
|---|---|---|
| B0 Reset | Environment and hygiene; crate split; per-realm font system; docs archive; version 0.2.0-dev | `just ci` green on a clean Mac with no workarounds; 26 crates; no file over 3000 lines in `flui-app` and `flui-widgets` |
| B1 App loop | Signals and Router ADRs; Unicode-correct multiline text editing; `Form`; keep-alive; async tasks; hot reload behind one flag | `flui create` → a three-screen Notes app with a form, a 10k-row list and async retry builds from `flui` alone and passes widget tests; `flui run --hot` preserves state on macOS |
| B2 Platforms | Windows and Linux live acceptance protocol; IME and clipboard on all three desktops; Web in three browsers; Material 3 controls; partial repaint | BETA.md platform matrix: macOS/Windows/Linux = beta, Web = beta candidate; frame budgets measured on three OSes |
| B3 Agents and docs | Devtools protocol; `flui mcp`; `flui test` with golden tests; Cupertino parity; docs site with tutorial and wasm demo | An agent creates a screen, runs a scenario and asserts the result through `flui mcp` without a human; the docs site is live |
| B4 Beta release | crates.io for every facade crate; semver-checks; coverage; Android/iOS honestly experimental with working examples | A clean consumer outside the repository builds Notes from crates.io; every row of BETA.md "What beta must demonstrate" has dated evidence |

Milestones are ordered by dependency, not by calendar; a milestone closes when its exit criterion is verified by a command.

---

[← Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Back to README](../README.md)
