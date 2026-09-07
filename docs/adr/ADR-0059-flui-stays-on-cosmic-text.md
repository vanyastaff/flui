# ADR-0059: FLUI stays on cosmic-text, on cost, and owns its own family resolution

- **Status:** Accepted
- **Date:** 2026-09-06
- **Relates to:** [ADR-0016](ADR-0016-unified-font-system-registration.md) (one
  shared `FontSystem`, which this record does not disturb),
  [ADR-0002](ADR-0002-engine-wide-threading-architecture.md) §G3 (`FONT_SYSTEM`
  sharding, still open and unaffected by shaper choice)
- **Supersedes nothing.**

## Context

Issue #927 — every Cupertino text run rendering a ~1.24 em space wherever
`CupertinoSystemText` is not installed — turned out to have its cause upstream,
in `cosmic-text` 0.19, not in FLUI:

1. **`FontMatchKey` sorts emoji first.** It derives `Ord` with `not_emoji: bool`
   as its first field (`cosmic-text` 0.19's `FontMatchKey`, `font/system.rs`)
   and its `font_match_keys.sort()` is ascending, so `false` — the emoji faces — sorts ahead of
   everything else in the unfiltered fallback tail. The comment on that sort
   states the opposite intent.
2. **The candidate filter abandons a family over one missing weight.**
   `font_weight_diff == 0 || variable_weight_match || is_mono`
   (`cosmic-text` 0.19's candidate filter in `font/fallback/mod.rs`) drops an entire present family when the
   exact requested weight is absent, and the next family in the tail — which
   may be an emoji face, per 1 — takes the run.

Issue #929 is a third defect of the same shape, and #930 tracks reporting 1 and
2 upstream. Three correctness defects in one dependency, all in font matching,
is a reason to ask whether the dependency is right — not to keep patching
symptoms.

The full market scan, the LOC accounting and the sourcing live on issue #931 and
are not restated here. What follows is what the decision turns on.

## Decision

**Stay on cosmic-text.** Record the reason as cost, not as ecosystem consensus,
because the evidence does not support the second claim. Keep FLUI's own family
resolution layer (added in #927) regardless of what sits underneath it.

## Why — and what the first version of this argument got wrong

The case was originally built on Zed: the best-resourced project with FLUI's
exact architecture (wgpu plus a glyph atlas) has had "use a HarfBuzz based text
system" open since 2024-07-08 and has not migrated. **That argument is
withdrawn.** Zed invests in GPUI as far as its editor needs and no further;
GPUI is a means there, not a product. Their non-migration is evidence about
their priorities, and an absence-of-action signal is the weakest kind
available.

With Zed removed, the ecosystem direction is fairly clearly *toward* parley:
Bevy migrated to it in 0.19; Blitz/Dioxus rejected cosmic-text outright and
never adopted it; Xilem/Masonry were built on parley from the start. egui
rejected parley, but for an immediate-mode reason — it "wants to be given a full
rectangle that it can arrange text inside" — that does not transfer to FLUI's
retained-mode box-constraint protocol, which hands text a max width and asks for
a size. That is parley's model, and Bevy, Blitz and Slint all fit it.

So the case for staying rests on **cost alone**. That is a weaker position than
the first version claimed, and this record states it as such.

### The cost, verified rather than estimated

≈2,800 LOC rewritten plus ≈1,200–1,800 new, dominated by replacing glyphon
(1,768 LOC including a 128-line WGSL shader). The question that settles the
arithmetic is whether parley has a glyphon-equivalent — a wgpu glyph atlas with
batching.

**It does not.** `glifo` (linebender/vello; first released 2026-05-15, 0.3.0 on
2026-08-07) is the closest thing, and 0.3.0's dependency set is `vello_common`,
`skrifa`, `peniko`, `bytemuck`, `hashbrown`, `smallvec`, `foldhash`, `log`, plus
optional `core_maths` and `png` — **no wgpu at all**, verified against the
registry rather than read off a README. It
is CPU-side glyph caching shaped to vello's rendering model, not a standalone
atlas-and-batching renderer. That leaves two routes, both larger than a text
swap:

1. adopt vello wholesale, which replaces FLUI's entire rendering backend rather
   than its text stack;
2. hand-roll the atlas, the batching and the WGSL over skrifa/swash.

Binary weight is a second, independent cost: parley moved text analysis to
ICU4X at an admitted +100 kB and ~7% perf, and egui's equivalent move grew a
minimal wasm build from 603 kB to 2.64 MB. FLUI ships a wasm target.

### The precedent migration was not clean

Bevy's is the closest precedent. Its public justification was thin
("meaningfully better documentation and somewhat nicer to use"); it shipped an
unexplained `text2d` performance regression; it lost
`PositionedGlyph::byte_index`/`byte_length`, which parley does not expose; and
per a maintainer the code reduction was "largely due to the deletion of tests
(which were not replaced)". A migration that trades tests for line count is the
opposite of what this repository's Definition of Done asks for.

### What the upstream posture actually argues for

The maintainer's stated position (2026-01-29): *"At this time, cosmic-text does
what we need it to do for COSMIC applications and I am extremely busy and
tired."* That is not abandonment; it is a dependency maintained to one desktop's
needs, with external font-matching correctness explicitly off its critical path.

This is the strongest direct signal in the whole scan, and it does **not** argue
for migrating — it argues for FLUI owning the layer where the defects were.
`flui-painting`'s `text_layout::font_resolve` resolves a style's family against
the host database before shaping, so a missing family is answered by FLUI's own
policy rather than by whatever the shaper's unfiltered tail happens to sort
first. That layer is worth keeping under parley too, which makes it the part of
this decision that is not a bet.

## Consequences

- The three upstream defects are reported (#930) and worked around locally, not
  waited on. A declined report is a re-open trigger, not a crisis.
- `font_family_fallback` (#928) and per-glyph family chains remain expressible
  only through FLUI's own resolution layer, because cosmic-text's `Attrs` carries
  one family. If a shape arrives that the layer cannot express, that is a
  re-open trigger rather than a reason to fight the API.
- ADR-0002 §G3 (`FONT_SYSTEM` sharding for parallel layout) is unaffected: it is
  a question about the shared instance, not about which crate provides it.

## Re-open triggers

Explicit, so this is a decision with an expiry condition rather than a default:

- **A parley-compatible wgpu glyph atlas reaches maturity.** This is the dominant
  cost and the one thing that would change the arithmetic outright.
- **FLUI has independent reason to adopt vello.** The text stack then comes along
  and the marginal cost collapses to near zero.
- **cosmic-text stops releasing, or the #930 reports are declined.** The second is
  the sharper signal: a declined correctness report says the posture above has
  hardened.
- **FLUI needs something cosmic-text structurally cannot express** — inline boxes,
  or per-glyph family chains that the resolution layer cannot reach.

## Alternatives rejected

- **Migrate to parley now.** Rejected on the glyphon replacement alone: no
  wgpu-side equivalent exists, so the migration is a text swap plus a
  hand-rolled renderer, and the closest precedent lost tests and gained a
  regression doing less.
- **Adopt vello.** Not rejected on merit — it is out of scope for a text
  decision. It replaces the rendering backend, and this record explicitly makes
  it a re-open trigger rather than pretending the two questions are separable.
- **Vendor or fork cosmic-text.** Trades three upstream defects for the whole
  crate's maintenance, and the defects are all in one layer FLUI can sit above
  instead.
- **Leave the question open.** Rejected because "we are on cosmic-text" would
  then be a fact about history rather than a decision, and the next symptom
  would re-litigate it from scratch.
