# ADR-0114: Lines align inside the allocated paragraph box

- **Status:** Accepted
- **Date:** 2026-10-04
- **Supersedes:** ADR-0092 §10 step 4b's direction-only line alignment. Text context ownership, shaping and display-list contracts remain unchanged.

## Context

`TextPainter` previously shifted the whole shaped block for center/right
alignment. A short second line kept its direction-only position within that
block; `Justify` did not stretch whitespace. A loose finite wrap cap could
also move text beyond the size reported by its painter. Paint and caret queries
agreed with each other, but did not implement the advertised line alignment.

## Decision

`ParagraphSpec` carries explicit `text_align` and `min_width`. The allocated
alignment width is the larger of the minimum width and measured kept content
width under existing wrap and overflow sizing. A finite `max_width` is the wrap
cap, not an implicit fill width. Tight width 200 aligns within 200; loose
0..200 aligns within the actual longest kept line. Unbounded and no-soft-wrap
breaking retain their original cap independently of alignment allocation.

`Start` and `End` resolve using FLUI's explicit `TextDirection`, then map to
Parley's left/right alignment. Center/right offsets and justification use
Parley's native line alignment and adjusted cluster advances. The painter does
not apply a second block shift or reconstruct direction-only line positions.
Painted glyphs, carets, selection boxes and line metrics read the same aligned
layout. Intrinsic widths are captured before justification and remain independent
of alignment. A changed alignment invalidates the cached layout; an unchanged
alignment preserves it. Ellipsis fitting and kept-line selection precede the
allocation calculation and subsequent placement.

Parley 0.11.1's public `BreakLines::set_prior_line_width` is the confined adapter
for separate breaking and alignment widths. It is doc-hidden and explicitly
warns that the escape hatch has not been carefully evaluated upstream. FLUI
re-breaks the same shaped data at the original cap, supplies the allocated width
for each committed line and runs native alignment once. This adds a line-break
pass; it does not add font shaping to that alignment step and does not claim a
performance improvement. The existing kept count still limits metrics and
placement after re-breaking. Native justification leaves the final line of the original paragraph and
explicit hard-break lines unstretched. A last kept soft line without ellipsis
remains a soft line and is justified even when later lines are hidden. Inferred
bidi base direction remains unchanged.

## Alternatives

- Shift the block and patch shorter lines with local formulas. Rejected: it
  duplicates native alignment and leaves justification and caret positions to
  maintain separately.
- Break at the allocated width. Rejected: it changes the wrap policy and can
  alter a paragraph's lines after measurement.
- Align at the finite wrap cap. Rejected: it paints beyond a loose paragraph's
  reported allocation and confuses a cap with a request to fill space.

## Validation

The public `caret_contract` family checks unequal-line center/right positions
against independently measured left-aligned widths, including actual painted
glyph coordinates, carets, hit queries and selection boxes. It covers tight,
loose and unbounded allocations, explicit RTL Start/End, native Hebrew RTL
center/right positions, trailing whitespace, soft-line justification, the last
kept soft line, original-final/hard-break controls, cached alignment changes and
ellipsis. Three independent production controls replace alignment with
direction-only Start, supply the wrap cap as allocation, or keep the stale
layout after an alignment change. Each fails its distinguishing public rows
while retaining the new public input shape. The wrap-cap control is
distinguished by finite loose allocation and an unbounded paragraph with a
minimum allocation; an unbounded zero-minimum case alone does not distinguish it.
