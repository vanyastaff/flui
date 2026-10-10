# ADR-0182: Explicit numeric text sizing

- **Status:** Accepted
- **Date:** 2026-10-09
- **Supersedes:** ADR-0092 §10, raw query and text-acquisition contract only.
- **Related:** [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md),
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md),
  [ADR-0172](ADR-0172-host-owned-system-preferences.md),
  [ADR-0181](ADR-0181-fallible-text-and-layout-measurement.md)

One accessibility multiplier cannot express size-dependent growth or different
growth profiles for two runs with the same authored size. Putting this authority
on shared shaping resources would also couple otherwise independent presentations
and prevent reusable standalone measurement.

## Decision

Foundation owns `TextSize`, `TextSizeRequest`, `TextScaleProfile` and
`TextSizingIntent`. A size is positive and finite both as authored `f64` and in
the shaper's `f32` representation. The authored value is admitted before growth;
a huge authored value is not rescued by a tiny multiplier. Intermediate products
and final shaped geometry remain checked under ADR-0181.

Profiles select numeric growth, not a font family, semantic accessibility role
or theme preset. The vocabulary includes LargeTitle, Title1/2/3, Headline, Body,
Callout, Subheadline, Footnote and Caption1/2. Absence on a `TextStyle` inherits
independently of font size; Body is the fallback after authored style merging.
An explicit profile replaces that inheritance. Fixed preserves authored logical
size without suppressing independent DPR or weight preferences.

Painting owns `TextSizing`: Fixed, validated Linear, or exact size/profile
answers in one retained store. A sealed exact policy mints its source internally.
A captured policy also returns the sole owner-local admission capability;
cloning its opaque source grants no write authority. Batch admission rejects
conflicting retained answers before publishing any part of the batch. Exact
answers are never interpolated, rounded to a size bucket or replaced with a
scalar estimate.

The policy, source, answer leases and attempt cohorts are owner-local. Their
shared ownership is `Rc`, and short `RefCell` borrows protect numeric updates
within that owner; there is no text-path lock or cross-thread publication.
Admission first collects extensible iterators, then commits closed numeric
values. Measurement releases numeric borrows before entering shaping. The
raster thread receives neutral shaped runs, never sizing authority or debt.

`TextMeasurement` combines a borrowed `TextContext` with an explicit authority.
Direct `TextPainter` measurements enter the same implementation, using fixed
sizing unless the caller supplies an override. An explicit linear factor of
one replaces inherited authority. Removing an override restores inheritance.
`TextContext` retains reusable font and shaping resources, with no ambient
presentation policy; low-level `shape` consumes already selected numeric sizes.
This decision does not relocate ownership of mutable shaping scratch.

Preparation merges authored styles and discovers the complete paragraph frontier,
including default and empty text, before shaping. Missing answers return owned
`TextPreparationPending`, distinct from `TextLayoutError`. A failed committed
layout withdraws its previous cache; a failed transient query returns an error
while preserving previously committed geometry. Paint, caret, selection and
hit-testing use committed shaped geometry rather than resolving size again.
Letter spacing follows the resolved/authored size ratio; word spacing remains a
logical length and line height remains a multiplier.

Cache authority includes font collection/generation and the effective sizing
capture. Appending answers reaches retained policy clones without invalidating
already successful paragraphs. Cached paragraphs and committed icon allocations
retain their resolved answers. An explicit finite attempt view retains its own
observed requests across all selected numeric sources, including cache hits and
missing requests subsequently answered; unrelated attempts do not extend that
working set. Numeric authority and weak attempt membership are independent: a
Fixed or Linear root attempt can retain Exact answers selected by nested
providers, without changing those providers' numeric cache identities or
granting admission authority. Historical answers
use a bounded LRU independently of live geometry and active attempts. The active
working set may exceed the historical capacity. Cancellation releases attempt
pins. A native capture must remain deterministic after all historical and live
leases for a request have retired; bounded storage does not retain an unbounded
record of every retired answer.

`MediaQueryData::text_sizing` is the sole inherited numeric authority, replacing
the writable scalar field. A nearest provider replaces the outer policy even
when fixed; copying parent data preserves its policy when overriding another
field. No provider inherits the render pipeline's policy. Runtime publication
updates the pipeline and root media snapshot before waking mounted consumers.
Raw `SystemPreferences` observations remain separate from this projection.

Icon sizing is resolved during measurement, not widget build. Its allocation
and glyph use one admitted size; an absent glyph still reserves that allocation.
Text editing uses the same paragraph measurement for glyphs, automatic caret
height and collapsed-range/IME geometry. Explicit caret-height overrides remain
logical lengths.

## Native integration boundary

Raw render-object queries receive the complete mutable query context, rather
than independent text and child-query channels. A text loan borrows that context,
so retaining it across recursive child measurement or erased child layout is a
compile error. `text_loan_across_query` and `text_loan_across_erased_layout` pin
these raw-hook boundaries. Independently held aliases of the shared scratch
resources return `TextContextBusy` instead of panicking. The pipeline retains
layout debt without poisoning; the runtime waits for explicit input invalidation
rather than continuously retrying the competing loan. The public
`competing_text_measurement_waits_for_changed_input` pins this recovery path.

Post-frame callbacks belong to the presentation whose lifecycle issued their
handle. An incomplete segment withholds its callbacks while healthy scopes and
ordinary host-frame completion remain deliverable. Between segments, scheduling
through a retained handle creates completion demand without dirtying a clean
widget tree or submitting another scene. Presentation withdrawal closes callback
admission before capture retirement. The runtime's
`native_text_preparation_does_not_complete_the_presentation` and
`clean_presentation_completion_keeps_the_tree_clean` pin these consumer paths.

A registered producer's incomplete segment retains its original animation tick,
host instant, constraints, accepted DPR, numeric policy and multi-source cohort.
Pipeline layout debt and completion callbacks remain with their existing owners.
Resuming runs the retained layout/fixpoint tail, without entering a new animation
sample or widget build. Other presentations can progress independently; this does
not change the runtime's single aggregate scene-sink contract.

Compatibility uses opaque layout and build-admission witnesses tied to their
existing owners. Accepting new work advances the witness before deduplication
and wake delivery, including an already-pending element. Moving retained work
between queues does not. Dirty-channel requests are validated and applied before
compatibility is checked. Source, constraints, DPR or premise replacement
withdraws old geometry and cancels the obsolete segment. Exhausted witnesses
permanently refuse compatibility rather than wrap.

The composition root receives an owned numeric frontier after Runtime checkout
returns. Its native capsule and sole admission writer remain App-owned;
settlement rejoins the exact presentation's owner FIFO. Segment and receipt
identities fence replies after cancellation, reentry or closure. Dropping a
receipt preserves delivery debt for a bounded retry. A held expired receipt parks
unavailable geometry and permits document repair; a matching late reply may heal
the segment. Native acquisition owns separate bounded ticket observation and a
readiness/park cue. Waiting for that producer cue retains the document barrier.
Numeric admission alone never publishes geometry or completion callbacks.

`registered_text_preparation_preserves_the_actual_presentation_attempt` pins
fractional continuation and rejection of an obsolete already-pending rebuild.
`owner_delivery_contract` pins released-checkout native service and refusal of
a reply after addressed close.

The portable contract does not establish native sizing support by itself.
Captured platform evaluation must run after pipeline and text loans are released,
retain exact request keys, and revalidate presentation/capture authority before
and after native calls. A pending request must retain layout debt without panic
poisoning, publish no partial scene, and resume through bounded service work.
Android nonlinear SP and UIKit trait-dependent numeric evaluation have installed
composition-root consumers. Native acquisition, conversion and OS-generated
change sequencing still require execution evidence on those platforms.

An incomplete presentation cannot expose its partially updated editable geometry.
`PipelineCell` retains layout publication readiness independently of owner
checkout; the runtime withdraws readiness before failure diagnostics and deferred
document grants. Caret, range, bounds and point queries use the same guarded
layout reader and return `NoLayout` while unavailable. Document reads and edits
remain possible. Only completing the replacement segment republishes geometry;
admitting numeric answers alone does not. The public
`parked_text_preparation_does_not_publish_partial_ime_geometry` pins refusal,
document repair and restoration through the real focused text store.

Native capture acquisition belongs at the existing host-only `HostWindow` /
`OwnerPlatform` seam. Platform-native capsules stay in the backend and app
composition root; Foundation request/result values cross that boundary. Runtime
and Painting receive numeric answers, without a native evaluator on shared text
resources. Retaining native resources is distinct from permission to publish into
a presentation that has closed or replaced its capture. App owns one current
producer per installed presentation and services its deadline through the native
owner, including background opportunities. Portable installed-record tests pin
deadline pacing and document-grant recovery; they do not execute SDK conversion.

Rejected alternatives are two independently writable scalar/policy fields,
inferring override intent from scalar equality with one, ambient policy on shared
`TextContext`, pre-scaling authored styles in build, and representing missing
native answers as deterministic authored rejection. Each loses either independent
ownership, exact growth, restoration or safe retry semantics.
