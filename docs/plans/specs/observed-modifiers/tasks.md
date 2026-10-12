# Delivery graph

Status: Approved, implementation in progress.
Parent [#1565](https://github.com/vanyastaff/flui/issues/1565).

| Task | Depends on | Requirements | Acceptance | Status |
|---|---|---|---|---|
| T1 Web producer, #1566 | Approved contract | R1/R2/R8 | DOM keyboard/pointer/wheel source-preservation RED/GREEN, controlled getter limits, reentry/retirement and next delivery | In progress |
| T2 Portable consumers, #1567 | Approved contract | R3–R7 | Public runtime/mounted field/actions, default select-all, both shortcut forms, composition and Slider proposals | In progress |
| T3 Integrate and migrate docs | T1/T2 | R2/R8/R9 | Durable ADR, architecture/public docs, changelog, no obsolete masks or misleading completeness claims | Pending |
| T4 Prove and review | T3 | R1–R9 | Production-hunk counterfactuals, scoped gates, independent Standards/Spec review, exact-head CI | Pending |

T1 and T2 are independent; builds are serialized per host. Tickets close through
the integration PR merge, not through a partial helper or local-green claim.
Approval authorizes implementation/publication, not merging the PR.
