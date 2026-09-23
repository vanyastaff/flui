# ADR-0059: FLUI stays on cosmic-text and owns its own family resolution

- **Status:** Accepted — to be superseded by ADR-0077
- **Date:** 2026-09-06

## Context

Issue #927 — Cupertino text rendering a ~1.24 em space wherever
`CupertinoSystemText` was not installed — traced to font matching in
cosmic-text 0.19, not to FLUI:

1. `FontMatchKey` derives `Ord` with `not_emoji: bool` first and sorts
   ascending, so emoji faces sort ahead of everything in the unfiltered fallback
   tail — the opposite of the sort's own comment.
2. The candidate filter drops an entire present family when the exact
   requested weight is missing, and the next family in the tail — possibly an
   emoji face — takes the run.

#929 is a third defect of the same shape; #930 reports them upstream. Three
correctness defects in one layer of one dependency is a reason to ask whether
the dependency is right. The market scan and sourcing are on #931.

## Decision

**Stay on cosmic-text for now, and keep FLUI's own family resolution whatever
shaper sits underneath.**

`flui-painting`'s `text_layout::font_resolve` resolves a style's family against
the host database before shaping, so a missing family is answered by FLUI's
policy rather than by whatever the shaper's fallback tail sorts first. The
upstream defects are worked around there, not waited on. That layer carries
over to parley unchanged; it is the part of this decision that is not a bet.

The reason for staying was cost, and the dominant cost is gone: it was
replacing glyphon, for which parley had no wgpu equivalent. ADR-0067 removed
glyphon and gave the engine its own atlas fed by shaper-agnostic glyph images.
ADR-0077 proposes the move to parley; the rasterization bridge it still has to
prove is the last piece of this record's cost argument.

## Consequences

- `font_family_fallback` (#928) and per-glyph family chains are expressible only
  through FLUI's resolution layer, because cosmic-text's `Attrs` carries one
  family.
- The upstream posture — cosmic-text maintained to COSMIC's own needs, with
  external font-matching correctness off its critical path — argues for owning
  the resolution layer, which this record does.

## Alternatives rejected

- **Vendor or fork cosmic-text.** Trades three defects for the whole crate's
  maintenance; the defects sit in one layer FLUI can sit above.
- **Leave the question open.** "We are on cosmic-text" would be history rather
  than a decision, and the next symptom would re-litigate it from scratch.
