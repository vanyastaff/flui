# ADR-0181: Fallible text and layout measurement

- **Status:** Accepted
- **Date:** 2026-10-09
- **Related:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md),
  [ADR-0054](ADR-0054-the-viewport-commits-one-result.md)

Text requests can be invalid before shaping or overflow during backend arithmetic.
Positive finite `f64` input alone does not guarantee a positive finite `f32` font
size or finite resulting line geometry. A panic or a stand-in size cannot express
an ordinary rejected request to the layout consumer.

`TextContext::shape` and `TextPainter` measurement return `TextLayoutError`.
Admission checks the backend representation, including narrowing underflow, and
shaping validates its resulting geometry. Logical ink arithmetic widens before
multiplication and addition. No arbitrary font-size cap replaces these checks.
`TextPainter` publishes its cache only after paragraph and intrinsic measurements
all succeed; rejection invalidates current-success access.

Box and sliver layout, child measurement, intrinsic, dry-layout and baseline
queries return `RenderResult`. The typed and erased interfaces propagate the same
failure. A valid absent child or absent baseline retains its successful zero or
`None` result; failure is not represented by either. Concrete objects and layout
delegates propagate child errors before accepting their own dependent result.

`RenderError::TextLayout` does not enter third-party panic poison recovery. The
pipeline retains the failing dirty root and its unprocessed batch. The runtime
withholds the scene and automatic retry for unchanged rejected input; a live
layout invalidation or pending build can resume the retained work. Failure
diagnostics remain outside the failed layout attempt. Genuine third-party panics
retain their existing containment policy.

This does not promise rollback of arbitrary render objects or a whole retained
tree. Independent successful relayouts remain accepted. A viewport's own rejected
pass keeps its previously accepted layout metrics. Constraint-derived viewport
dimensions remain accepted, together with their fractional-page mapping; failed
child measurement cannot publish new content dimensions or undo that mapping.
The stand-in/degraded-pass policy of ADR-0054 remains separate. Presented output
is withheld on the ordinary error, rather than mixing a fabricated text result
into a successful frame.

`ViewportOffset` creates a detached `ViewportLayout` proposal. Correction passes
update its pixel position, content bounds and, for shrink wrapping, the measured
viewport dimension. The proposal contains no observers or scheduling state and
is accepted only after child measurement succeeds. A shared `ScrollPosition`
compares the originally accepted state before publication. Reentrant scroll input
refuses the stale proposal with `RenderError::ViewportOffsetChanged`; the pending
batch remains runnable against that input rather than waiting for another change.
No rollback writes over newer input. Direct constraint-derived page mapping in
a regular viewport remains independent of this content proposal.
A fractional-page dimension change that moves the proposed pixel position
requires another child pass. Accepting the mapped offset with geometry measured
at the previous offset would publish two different positions in one result.
The page-resize row asserts the committed child offset as well as scroll metrics.

Public contracts are pinned by `text_contract`, `text_context_contract`,
`rejected_text_layout_is_fallible_through_queries_and_frames`, and
`invalid_authored_text_waits_for_changed_input_and_then_presents`. The page rows
distinguish an independently successful viewport resize from a rejected viewport
pass. Regression evidence includes inverse checks of producer validation, ink
arithmetic, runtime retry parking and fractional-page restoration.
The correction rows reject real text on the second pass and assert published
scroll bounds; reentrant-input rows require recovery without another
invalidation. The independent-boundary row changes a later viewport's content,
corrects only the failed earlier boundary and observes the retained content
change. Removing batch-tail retention makes that row fail on the later viewport's
unchanged scroll extent.

Rejected alternatives were a painter-only clamp (direct shaping callers and
derived overflow bypass it), a geometry-plus-error side channel (parents can
consume failure as success), and using panic payloads for ordinary text failure
(conflates authored rejection with third-party containment).

Revision-bound native sizing is a distinct integration: this error contract
provides its fallible measurement boundary but does not implement Android
nonlinear SP sizing or UIKit trait-dependent font resolution. Native query
failure must not be silently classified as deterministic authored rejection.
