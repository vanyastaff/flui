# FLUI peer-inspired development draft

Status: temporary review material. The owner authorized the first shared FLUI benchmark implementation; the broader roadmap remains proposed, not an accepted ADR.

The owner requested that the research, proposed decisions and development plan be preserved in a draft PR. This directory is a temporary exception to the repository policy excluding research and roadmaps. Keep the PR in draft; it is not intended for merge as-is.

- [Research](research.ru.md): source-based comparison of Raven, Blinc, Rosin, WaterUI, Ply, Knots and Tessera.
- [Development plan](development-plan.ru.md): existing FLUI capabilities, proposed priorities, dependencies and observable acceptance criteria.
- [Decision status](decisions.ru.md): preserved constraints, recommendations and unresolved choices.
- [Published benchmarks](benchmark-review.ru.md): upstream numbers, measurement conditions, source code and limits of the claims.
- [FLUI comparison plan](benchmark-plan.ru.md): scenarios, peer cohorts, metrics, correctness, statistical policy and implementation order.
- [First FLUI baseline](benchmark-baseline.ru.md): shared scene, measured samples, correctness evidence, A/A variation and host limitations.

## Evidence boundary

The research describes FLUI at `bb618a7f7f1510ff11093109094ba02c937d1970`, inspected on 2026-11-02. FLUI source links are pinned to that commit so line references remain meaningful. External sources retain the references captured during research; the report distinguishes code inspection from execution.

This draft branch starts at `9a5df98b49b25d267a1cf1d8db3c400bdd6492b7`. Three changes landed after the research baseline: headless storage isolation (#2157), validated backdrop magnification (#2164), and WindowFrame (#2140). The historical comparison has not been silently updated to claim validation of those changes. Reconcile affected proposals with current code before implementation.

No framework library behavior changed. The PR now adds a benchmark example and integration consumer, authorized separately by the owner. Its implementation contract and execution evidence live in [the benchmark-grid spec](../../specs/benchmark-grid/tasks.md). Synchronous widget-pipeline timings and offscreen GPU correctness have been exercised; native presentation and peer comparisons remain future work.

The benchmark supplement was researched on 2026-11-02. Its FLUI tooling inventory uses the branch base `9a5df98b49b25d267a1cf1d8db3c400bdd6492b7`; it does not retroactively update the earlier architecture review. Published peer results are attributed to their authors. The first FLUI-only baseline pins implementation commit `e3413f98726e9034f31aa4b47febb6f1999137af`. New FLUI/peer comparisons are NOT_RUN; experimental noise thresholds are not accepted release gates.

## How this draft becomes implementation

Review priorities and individual experiments here. Move approved implementation scope into the existing owner-approved feature specs, or a new feature spec when needed. Record durable architecture decisions as ADRs following repository policy. Existing feature task files remain authoritative; this draft does not create a parallel completion tracker.

Remove or relocate these temporary materials before any proposal to merge this branch. Creating this draft does not approve new architecture, platforms, release dates or benchmark claims.
