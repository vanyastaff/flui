# Intent: transactional, presentation-local frame failure containment (the remainder of #561)

- **Status:** Draft
- **Slug:** `561-transactional-frame-failure`   ·   **Date:** `2026-09-11`   ·   **Stated by:** issue #561 (body) and the maintainer's two checklist-audit comments of 2026-09-07; in-session instruction "Стартуй" after the orchestrator proposed #561 as the next unit

## Asked for

> ## Problem
>
> Some caller errors and internal layout panics are caught after partial mutation, then recover through zero/default results. This can leave layout, hit testing, paint, and semantics observing different partial states.
>
> ## Design
>
> - Classify validation, recoverable subtree, backend, and internal invariant failures.
> - Reject caller-controlled invalid values before frame entry.
> - Build recoverable output as a candidate and publish only after validation.
> - Retain the last known-good presentation snapshot on failure.
> - Render detailed local ErrorView in development.
> - Use a neutral local fallback plus privacy-safe structured diagnostics in production.
> - Keep internal invariant violations loud under PANIC-POLICY.
>
> ## Acceptance criteria
>
> - [ ] A failed subtree cannot partially commit frame state.
> - [ ] Layout, hit test, paint, and semantics observe one committed version.
> - [ ] Another presentation and background services continue normally.
> - [ ] Development diagnostics include causal detail and ownership identity.
> - [ ] Production output avoids sensitive data by default.
> - [ ] Tests distinguish last-good retention from zero-value fake recovery.

> (audit comment 1, 2026-09-07) **❌ "A failed subtree cannot partially commit frame state."** ADR-0048: *"Mid-segment tree mutations are not rolled back. A build that panicked after rebuilding half its dirty list leaves the element tree mixed-version."* Contained and reported, not transactional.
>
> **❌ "Layout, hit test, paint, and semantics observe one committed version."** Two named gaps: pointer events arriving before the retry hit-test the live tree (only the pump's ambient re-probe is gated), and semantics has no candidate/validate/publish — a failed frame simply does not publish, so the last published version stands.
>
> **⚠️ "Tests distinguish last-good retention from zero-value fake recovery."** ADR-0048 has a "Retry and last-good retention" section, so the mechanism is designed; what I did not find is a test whose oracle *distinguishes* the two, which is the criterion's actual demand.
>
> **⚠️ "Production output avoids sensitive data by default."** flui-log is private-by-default since #572/#784, which covers the log sink. Whether a `FrameFailureReport` handed to an embedder's handler carries anything sensitive is a separate question this audit did not settle.
>
> [The secondary-realm window handler] is **blocked behind the secondary-window rendering gap** … It should be sequenced after that, not picked up as a quick win.

> (audit comment 2, 2026-09-07) I opened PR #1005 to fill criterion 6 … **It was vacuous and I have closed it.** … What a correct fixture must satisfy simultaneously: layout **re-armed** before the cyclic pass, or nothing runs; assertions on the node the bookkeeping **actually tips**; constraints **loose enough** that the retained size is not recoverable from the constraints alone — otherwise a collapse to `Size::ZERO` is invisible; and the poisoned node must not be the **root**, whose size comes from constraints by construction. … The cheap discriminator for whoever writes it: **delete the poisoning step and re-run**. If the test still passes, it is measuring something else.

## What's wrong today

A frame that fails part-way is contained to its presentation and reported, but what it leaves behind is not one version: a build that panics after rebuilding half its dirty list leaves the element tree mixed-version; a pointer event that arrives between the failed frame and its retry is hit-tested against that live, half-built tree; semantics publishes nothing on failure, so the accessibility tree describes a frame the user no longer sees; and a poisoned layout node "recovers" through its last committed geometry — or `Size::ZERO` if it never had one — with no test able to say which of the two a reader is looking at. The maintainer's own attempt at that test passed with the poisoning step deleted.

## What "fixed" looks like

After a failed frame, everything that reads frame state — layout, hit test, paint, semantics — sees the same committed version (the last good one) until a later frame commits a new one; a failed subtree never leaves a half-applied mutation behind; a test exists that turns red when the poisoning step is removed and tells last-good retention apart from a zero-value stand-in; and a `FrameFailureReport` reaching an embedder in production carries nothing sensitive by default.

## Who feels it

Framework users whose app hits a build/layout panic in one subtree and keeps running (they see a torn frame or a hit target that no longer exists); assistive-technology users (a stale a11y tree after a failed frame); embedders wiring a frame-failure handler (what the report carries); the next maintainer reading ADR-0048's residual-gaps list; the #561 issue, roughly 2-of-6 today.

## Constraints the user owns

- PANIC-POLICY: internal invariant violations stay loud; only caller-controlled and recoverable-subtree failures are contained.
- ADR-0048 (frame transaction boundary) and ADR-0027 (presentation-local realms) are the record to extend, not replace; ADR-0062 (the paint queue is the cross-pass record) and the paint-side frame atomicity landed in #1020 (`PoisonPhase`; a poisoned frame is discarded whole, the dirty queue survives) are the shape the other passes should match.
- Flutter 3.44.0 is the floor: upstream has no transactional build/layout (per-element/per-node `_reportException`, continue) and publishes whatever semantics is dirty; "one committed version" is an improvement over the reference and owes ADR / `## Mapping decisions` accounting and replacement tests.
- The secondary-realm window handler is NOT this unit: blocked behind secondary-window rendering (one frame-pump closure per backend; `attach_root_widget` primary-only).
- A criterion-6 test must satisfy the four fixture requirements in the audit and fail when the poisoning step is deleted.

## Not this

- Not the secondary-window handler wiring (blocked; sequence after secondary-window rendering).
- Not a change to what a successful frame produces.
- Not the log sink's privacy (flui-log is private-by-default since #572/#784) — only what the report handed to an embedder carries.
- Not a new retry policy; the retry/last-good mechanism in ADR-0048 stands.

---

## User amendments

- **2026-09-11, in-session message (after the orchestrator proposed #561 as the next unit):**
  > Стартуй
- **2026-09-11, approach gate (three answers, chosen from options the orchestrator presented):**
  > B′: закрыть в этой спеке — hit-test reads a committed version via a copy-on-write geometry shadow (PR-4), not a recorded residual.
  > Profile-aware — the typed `FrameFailureReport` carries the panic text verbatim under `cfg(debug_assertions)`, redacted in release, with an explicit release opt-in.
  > Вынести G2b, взять tail-retry fix — semantics candidate/commit is filed to the threaded-lane issue; in its place this unit fixes the retry that parks `Idle` with screen N-1 / tree N after a tail failure.

## Corrections

| Date | What changed | Why it surfaced only now |
|------|--------------|--------------------------|
| 2026-09-11 | "What 'fixed' looks like" — *semantics* leaves the "one committed version" set for this unit; the criterion's a11y half is filed with the threaded raster lane (ADR-0045 successor). | Exploration showed `run_frame` orders layout → paint → semantics, so a layout/paint failure never publishes semantics; a11y can lead the screen only after a scene-submit failure (one pump), and the candidate/commit split burns two consumed side effects in `SemanticsOwner::flush`. The user chose to file it out. |
| 2026-09-11 | Added to "What's wrong today": an `Errored` tail after a painted `run_frame` never arms a repaint, so the retry parks `Idle` with the screen at N-1 and the tree at N — a real defect found while exploring, taken into this unit in G2b's place. | It is the same "retry never re-presents" root cause behind the mixed-version symptoms; ADR-0048's "quiescent ending" documented it as accepted without seeing that the tree had advanced. |
| 2026-09-11 | "A failed subtree cannot partially commit" is narrowed to panics OUTSIDE `build()` — user `build()` panics are already contained per element (ErrorView substitution, Flutter parity). | The scout found `behavior_commons::build_or_recover`; the audit's "mixed-version" wording predates that landing. |
