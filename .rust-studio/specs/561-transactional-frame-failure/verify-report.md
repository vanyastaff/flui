# Verify Report: 561 transactional frame failure

- **Spec:** [`spec.md`](spec.md)
- **Status:** Pass, accounting-only close
- **Verified on:** 2026-09-13, `main` at `f21148db`
- **Merged PRs:** #1024, #1025, #1026, #1027, #1028, #1029, #1030
- **Open follow-ups:** #1031 semantics candidate/commit; #1032 paint poison budget

## Command Evidence

- `cargo nextest run -p flui-rendering --locked --test rendering_it -E 'test(a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero) | test(a_leaf_that_never_committed_stands_in_with_zero)' --no-fail-fast` — 2 run, 2 passed, 342 skipped.
- `cargo nextest run -p flui-view --locked --features test-utils -E 'test(/lifecycle_panic_containment/) | test(/dense_reconcile_containment/) | test(/recovered_panics/) | test(/inherited_dependency/)' --no-fail-fast` — 36 run, 36 passed, 732 skipped.
- `cargo nextest run -p flui-app --locked -E 'test(/frame_failure_containment/) | test(/frame_commit_state_tests/) | test(/held_input/) | test(/frame_failure_privacy/) | test(/frame_failure_report/)' --no-fail-fast` — 65 run, 65 passed, 431 skipped.
- `just gate` — passed: fmt, text-check, inventory, runtime-conformance, panic-policy, port-check, workspace clippy, flui-engine wgpu-test clippy, and doc-strict.
- Prior PR evidence retained: PR-4 `just ci` and `just live-smoke` green; #1030 CI green.

## Criteria

- **AC1:** Pass. PR #1027 covers child `init_state` containment through flui-view seam (a); PR #1030 final repair retained the green selected flui-view containment coverage.
- **AC2:** Pass. PR #1027 covers dense insert `create_render_object` containment and exact-slot `ErrorView` topology.
- **AC3:** Pass. PR #1027 covers repeated dense mount failures without render ghosts or slab growth.
- **AC4:** Pass. PR #1027 covers dense inactive GlobalKey retake update failure, finalization, key cleanup, and no duplicate cleanup.
- **AC5:** Pass. PR #1027 covers dense update containment for `did_update_view` and `update_render_object`.
- **AC6:** Pass. PR #1026 and PR #1028 cover removal-path hooks, drain forwarding, and contained reports for lifecycle/render unmount cases; selected flui-view and flui-app filters passed.
- **AC7:** Pass. PR #1027 and PR #1028 cover `BUG:` payloads as contained-but-classified with `internal_invariant: true`.
- **AC8:** Pass. PR #1028 covers tail retry repaint and re-presentation after a post-pipeline error.
- **AC9:** Pass. PR #1028 covers `TreeRevision` / `FrameCommitState`: `Idle` does not commit an uncommitted frame, `Presented` and `NoPresent` do, deferred first frame stays uncommitted, and secondary failure does not freeze primary reprobe.
- **AC10:** Pass. PR #1029 and final PR #1030 repair cover held pointer input while uncommitted, replay on commit, move collapse, hover leave behavior, and teardown drop without `Cancel`; selected flui-app held-input filters passed 65/65 with the related frame/report/privacy selectors.
- **AC11:** Pass. PR #1024 covers last-good layout poison retention versus zero stand-in; targeted layout-poison run passed 2/2.
- **AC12:** Pass. PR #1028 covers profile-aware `PanicText`, explicit verbatim opt-in, recovered non-string redaction, and typed `Pipeline { error }` retention.
- **AC13:** Pass. PR #1028 covers diagnostic payloads: `RecoveredAt`, `view_type_id`, `hook`, segment `phase`, and presentation `address`.
- **AC14:** Pass. ADR-0048, PANIC-POLICY, `docs/runtime-contract.toml`, CHANGELOG, and this spec status are updated across PRs #1024-#1030; this bookkeeping closes the remaining accounting gap.

## Gate Verdicts

- **QA:** code/test retained scope complete; accounting incomplete before this report, now cleared by the 7.1 execution entry and Done status.
- **Reviewer:** no code/test blocker; accounting incomplete before this report, now cleared by the same bookkeeping.
- **Follow-ups:** #1031 and #1032 are deliberately outside the #561 close criteria.

**Verdict:** COMPLETE.
