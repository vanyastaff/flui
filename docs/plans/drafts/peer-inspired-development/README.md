# FLUI peer-inspired development draft

Status: temporary review material, not an approved feature specification or accepted ADR.

The owner requested that the research, proposed decisions and development plan be preserved in a draft PR. This directory is a temporary exception to the repository policy excluding research and roadmaps. Keep the PR in draft; it is not intended for merge as-is.

- [Research](research.ru.md): source-based comparison of Raven, Blinc, Rosin, WaterUI, Ply, Knots and Tessera.
- [Development plan](development-plan.ru.md): existing FLUI capabilities, proposed priorities, dependencies and observable acceptance criteria.
- [Decision status](decisions.ru.md): preserved constraints, recommendations and unresolved choices.

## Evidence boundary

The research describes FLUI at `bb618a7f7f1510ff11093109094ba02c937d1970`, inspected on 2026-11-02. FLUI source links are pinned to that commit so line references remain meaningful. External sources retain the references captured during research; the report distinguishes code inspection from execution.

This draft branch starts at `9a5df98b49b25d267a1cf1d8db3c400bdd6492b7`. Three changes landed after the research baseline: headless storage isolation (#2157), validated backdrop magnification (#2164), and WindowFrame (#2140). The historical comparison has not been silently updated to claim validation of those changes. Reconcile affected proposals with current code before implementation.

No framework behavior changed. Runtime tests, native runs and performance measurements are future acceptance work, not results of this PR.

## How this draft becomes implementation

Review priorities and individual experiments here. Move approved implementation scope into the existing owner-approved feature specs, or a new feature spec when needed. Record durable architecture decisions as ADRs following repository policy. Existing feature task files remain authoritative; this draft does not create a parallel completion tracker.

Remove or relocate these temporary materials before any proposal to merge this branch. Creating this draft does not approve new architecture, platforms, release dates or benchmark claims.
