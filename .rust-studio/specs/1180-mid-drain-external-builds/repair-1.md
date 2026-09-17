# Repair round 1 — #1180 (worktree /mnt/data/dev/flui-worktrees/view-mid-drain, branch fix/1180-mid-drain-external-builds-join-the-scope)

Two reviewers at e7493121: rust-reviewer (5a PASS, 5b NEEDS WORK; probes a–g run) and a deepseek outside lens (5a PASS,
5b NEEDS WORK). Code is correct; the work is false claims, one fictional hazard, a duplicated routing path and test gaps.
Fold ALL of it into ONE commit; rerun the crate gates + `just ci`; report tails and disagreements in writing.

## A. False invariant — the "stale-id hazard" does not exist (must-fix, integrity)
`ElementId` is generation-carrying (`crates/flui-foundation/src/id.rs`, `ElementTree::resolve_index` rejects stale ids —
the ABA guard since #167). The reviewer's probe: `old=ElementId(index=1, generation=1) new=ElementId(index=1, generation=2)`,
so `stale_rebuild_handle_after_a_same_slot_child_swap_lands_on_the_new_occupant`'s only discriminating assert sits inside
an `if new_child_id == old_child_id` that is never true, and its name asserts a behavior that cannot happen. Delete the
hazard paragraphs everywhere (`ARCHITECTURE.md`, `CHANGELOG.md`, the absorb-site comment, the test doc — including "a
generation-carrying `ElementId` is out of scope": it exists). Rewrite the test with UNCONDITIONAL asserts pinning the truth
(same `index()`, different id; a stale schedule is an inert miss; the new occupant builds exactly once and is not dirtied by
the stale handle); rename it accordingly.

## B. False claim — the budget is per `build_scope` CALL, not per frame (must-fix)
`BuildOwner::service_child_requests_impl` re-enters `build_scope` mid-frame (the lazy-sliver service pass, reached every
frame from `ui_realm.rs` → `binding.rs`), resetting `mid_drain_absorbs_left`/`built_this_frame`/the streak. Real bound today:
16 × (1 + lazy-service passes ≤ `lazy_band_pass_budget` + 1) — finite, no termination bug, but the field doc, ARCHITECTURE.md
and CHANGELOG say "shared by every drain the frame runs — none of them re-enters `build_scope`". Pick ONE and make doc, code
and test agree: (a) route the lazy service pass through the non-resetting internal path (`build_scope_target` with the
prologue `build_scope` shares — the early return + target selection — factored so `build_scope` = reset + that path), so the
bound is genuinely per frame; or (b) state the real bound and pin it (extend `mid_drain_absorb_budget_is_shared_across_the_frames_separate_drains`
or add a test that drives a lazy service pass and asserts the total). Prefer (a) if the factoring is small; say which you did.

## C. Wording — "re-entry" ≠ "self-rescheduler" (should-fix)
Any element re-notified after it already built this scope is charged: a child notifying its already-built parent, an A↔B
ping-pong. The constant doc, the absorb comment, ARCHITECTURE.md and the warn text ("self-rescheduled id(s)") equate the two;
the warn will mislead the first user who hits it. Fix the definition ("a Vacant landing for an id that already completed a
build in this `build_scope`") and the warn ("re-entered id(s) …").

## D. Shape — `route_dirty_element` duplicates the pop-time classification (REDO)
The pop already runs `nearest_layout_builder_scope` + accept + `defer_dirty_element` on every popped entry, so absorbed ids
pushed straight onto the heap are routed identically; mutation (e) (ignore scope at absorb time) survives all 11 tests; the
helper re-`match`es `target` with an `unreachable!("matched above")` arm in a lib path. Delete the helper and push onto
`self.dirty_elements` with a one-line comment that the pop defers non-accepted ids (or extract ONE `classify(target, tree,
id, live_scopes)` shared by both sites — only if it removes code net). Keep the two scoped-target tests as pins of the
end state; their docs must say the pop-time classification is what routes.

## E. Tests (gaps the probes exposed)
1. Streak warn gating is unpinned — mutation (g) (warn on every capped absorb) survives. Add: two consecutive capped frames →
   one warn total; a capped id followed by further pops in the same drain → one warn. (Counting the warn: a test-only
   accessor on the streak flag is enough if a subscriber is heavy.)
2. `mid_drain_schedule_for_a_descendant_builds_in_the_same_drain` mounts `parent` and `descendant` as SIBLINGS under root
   and survives mutation (a) (absorb after the heap empties) — it pins only two-frames→one. Make it a real child so it pins
   per-pop absorption too (and keep a sibling variant if it is one line).
3. `crates/flui-widgets/tests/implicit_animations.rs` ~L679-682 credits the "built exactly once" assertion to #1180 — that
   already held on base (the build comes from the reconcile's heap schedule); the discriminators are `pending == 0` (~L684)
   and the post-tick count (~L703). Reword.
4. `TestView::should_skip_rebuild → true` is a module-wide fixture change (~50 pre-existing tests); scope the override to a
   dedicated leaf type for the #1180 fixtures (or record the coverage change where `TestView` is defined — the reviewer found
   only the new wake pin depends on it; scoping is still the cleaner shape).

## F. Docs
1. Process markers: `build_owner.rs` ~L1198 ("v3 §3"), ~L1735 ("Decision 1"), ~L5067 ("Decision 4") → plain English; keep
   "#1180".
2. Third Flutter divergence, unrecorded: Flutter's `BuildScope._scheduleBuildFor` skips `scheduleRebuild` while `_building`;
   FLUI fires `on_build_scheduled` mid-drain (pinned by `mid_drain_schedule_still_requests_a_frame_like_an_out_of_frame_schedule`).
   Add it to the mapping entry (this was the plan's Decision 3 "recorded as the alternative", which never reached a shipped doc).
3. Citation precision (ARCHITECTURE.md ~L43-44): at 3.44.0 it is `BuildScope._dirtyElementIndexAfter`, re-sorting only when a
   mid-flush `_scheduleBuildFor` set `_dirtyElementsNeedsResorting` — not "on every iteration".
4. The hero fixture doc (`hero_gesture_tests.rs` ~L120-133): cite the filed issue #1195 for the auto-observer flight leak.
5. `absorbed_mid_drain` span field: it also counts the between-frames inbox absorbed on the FIRST iteration (every vsync-tick
   schedule), so a profiler cannot separate re-entries from ordinary animation rebuilds — count after the first pop (or rename
   to what it counts), and declare the field on the `during_layout` span too (`layout_builder.rs` ~L202) so fixpoint absorbs
   are not dropped.
6. `built_this_frame`: note in `test_build_owner_memory_size`'s comment that the set retains peak capacity (`clear()` keeps
   it) — O(peak builds per scope) per owner, ~9 B/slot; one SipHash insert per completed build. Bounded; no change required.

## G. Perf nit (do if trivial)
A capped id is re-drained and put back at EVERY later pop of the same drain (lock/collect/re-lock/insert + `mark_needs_build`
each time). Bounded; a local capped-id set that skips re-absorbing them for the rest of the drain removes the churn.

## Gates (paste tails)
`cargo nextest run -p flui-view -p flui-widgets -p flui-material -p flui-app -p flui -p flui-testing`,
`cargo clippy -p flui-view -p flui-widgets --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`bash scripts/doc-strict.sh` (the gate; the crate-scoped `--document-private-items` doc build fails on a pre-existing
`key/registry.rs` link — leave it), `typos`, `bash scripts/port-check.sh`, then `just ci` once.
Do NOT rebase (main has #1184/#1193; the PR merges via squash).

## Report back
Diff summary; which option you took for B and why; red evidence for E1/E2 (the mutations reddening); gate tails;
disagreements in writing.
