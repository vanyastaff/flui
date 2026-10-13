# Delivery graph

Status: Implemented; final acceptance is tracked by the integration PR.
Parent [#1565](https://github.com/vanyastaff/flui/issues/1565).

| Task | Depends on | Requirements | Acceptance | Status |
|---|---|---|---|---|
| T1 Web producer, #1566 | Approved contract | R1/R2/R8 | DOM keyboard/pointer/wheel source-preservation RED/GREEN, controlled getter limits, reentry/retirement and next delivery | Implemented and locally verified |
| T2 Portable consumers, #1567 | Approved contract | R3–R7 | Public runtime/mounted field/actions, default select-all, both shortcut forms, composition and Slider proposals | Implemented and locally verified |
| T3 Integrate and migrate docs | T1/T2 | R2/R8/R9 | Durable ADR, architecture/public docs, changelog, no obsolete masks or misleading completeness claims | Complete |
| T4 Prove and review | T3 | R1–R9 | Production-hunk counterfactuals, scoped gates, independent Standards/Spec review, exact-head CI | Counterfactuals and review complete; final gate and CI results tracked by the integration PR |

T1 and T2 are independent; builds are serialized per host. Tickets close through
the integration PR merge, not through a partial helper or local-green claim.
Approval authorizes implementation/publication, not merging the PR.

## Acceptance evidence

Chrome 154 executed all 138 modifier-matrix rows, keyboard/pointer/wheel getter
reentry and retirement controls, and independent source-versus-delivery witnesses.
All passed; the previous producer made all four controls fail. The existing twenty
browser controls also passed without page errors.

The mounted `text_editing`, `focus_actions_and_shortcuts` and
`controlled_slider_input_and_geometry` families passed after implementation.
Separately reverting command projection, activator composition refusal, field
composition refusal and Slider composition refusal made the relevant family fail
for the intended behavior. Production bytes were restored after each experiment.
Independent Standards and Spec reviews found no merge-blocking defects.

The pre-integration scoped gate passed with 702 tests, the platform compiler
guard, strict rustdoc/doctests, Windows/wasm lint and both 61-case feature passes.
Updating to newer main preserved all five production files byte-for-byte. The
integration PR records the final gate base/head and exact-head CI result; local
skips for Linux native execution and Apple/Android checks remain CI obligations.

This evidence uses synthetic DOM events and mounted consumers. Physical layouts,
native modifier observation parity and Web IME text transport are outside the
approved acceptance boundary, as stated in the design.
