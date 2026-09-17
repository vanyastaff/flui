# Review brief — #1180: external schedules landing mid-drain are absorbed at the top of each heap pop

Worktree: /mnt/data/dev/flui-worktrees/view-mid-drain — READ-ONLY for every reviewer (an outside lens runs read-only
`cargo test` there; probes/mutations go in your own detached checkout under /mnt/data/dev/flui-worktrees-scratch/ with its
own CARGO_TARGET_DIR, removed when done; never `just ci`). Base: origin/main at 864b5798 (main has since advanced by
#1184, #1193 — unrelated crates; do not rebase). Two commits: `b849c6f3` (the change) and `e7493121` (the hero gesture test
restored to its original expectation). `git diff 864b5798...HEAD`.
Plan (v2 + v3 deltas, the contract): /mnt/data/dev/flui/.rust-studio/specs/1180-mid-drain-external-builds/plan.md
Builder brief: /mnt/data/dev/flui/.rust-studio/specs/1180-mid-drain-external-builds/brief.md
Issue: `gh issue view 1180`. Flutter at 3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`): `framework.dart`
`BuildOwner.buildScope` (per-iteration re-sort, `index` walk-back), `scheduleBuildFor`, `Element.markNeedsBuild`
(`if (dirty) return`; `_debugCurrentBuildTarget` descendant assert).
House rules: /mnt/data/dev/flui/AGENTS.md; `crates/flui-view/AGENTS.md`; the inbox `parking_lot::Mutex` is never held
across a build; no process markers; panic policy.

Builder-reported facts to judge: (1) the absorb step takes the inbox in ONE statement (`lock; drain().collect::<Vec<_>>()`),
then routes each id: Occupied in `dirty_reasons` → merge; Vacant + not `built_this_frame` → free push at authoritative depth
through the new `route_dirty_element` (Global/LayoutBuilder(scope)/All); Vacant + `built_this_frame` → a re-entry, charged
to `mid_drain_absorbs_left` (16, reset at `build_scope` entry; capped ids are put BACK into the inbox merged against a
concurrent `schedule`, warned once per streak, streak re-armed at the next `build_scope` entry when the previous frame ended
with budget left). (2) No wake suppression (Decision 3 of the plan). (3) `TestView` in build_owner.rs's tests now overrides
`should_skip_rebuild → true` (a shared zero-field test view was inflating wake counts). (4) `test_build_owner_memory_size`
threshold 632 → 688 (+56 measured: `HashSet` 48 + `usize` 8, `bool` in padding). (5) New `crates/flui-view/ARCHITECTURE.md`
(partial: `## Mapping decisions` only, the pattern 4 sibling crates use) and `CHANGELOG.md`; `docs/PORT.md` index row.
(6) `e7493121`: `hero_gesture_tests::a_to_route_that_does_not_maintain_state_falls_back_to_the_deferred_path_without_panicking`
flipped back to "the deferred path measures nothing — the covered `maintain_state(false)` destination is unmounted" with a
two-episode doc (the 2026-08-05 reading rested on the inbox's one-frame deferral); probe quoted: `route_subtree(to)` is
`Some` on main, `None` under the fix. The fixture's "No controller yet" premise corrected (the Navigator auto-installs a hero
observer; its flight over the shared tag survives `install()` and never settles — documented, not fixed, no production
change). (7) Pre-existing, untouched: a broken intra-doc link in `crates/flui-view/src/key/registry.rs:257` fails
`cargo doc --document-private-items -D warnings` on main too.

## 5a — spec compliance (plan Decisions 1–5 + v3 deltas 1–7 + Tests list): MET / NOT MET / EXTRA with file:line.
In particular: absorb at the top of EVERY pop (not once, not after the heap empties); merge vs push vs re-entry classification
exactly as specified; the budget charges RE-ENTRIES ONLY (`built_this_frame`), first-time absorbs free; budget/streak/
`built_this_frame` reset at `build_scope` entry and shared by every drain the frame runs (the layout-builder fixpoint's
drains included); capped ids stay in the inbox for the next frame; warn once per streak; NO `draining` flag, `request_frame`
unchanged; scoped-target routing of an absorbed id (Global id during a `LayoutBuilder(scope)` drain → root bucket; scope-A id
during a Global drain → `isolated[A]`); the `absorbed_mid_drain` span field; every doc in v3 §5 updated; mapping entry records
both Flutter divergences (no descendant-only assert; a self-rescheduler rebuilds once per re-entry where Flutter drops it)
and the stale-id hazard; CHANGELOG; the widgets test flipped with `pending == 0`, `!has_dirty_elements()`, `AnimatedBuilder`
built once; every test the plan lists present (name any missing) and the pins named honestly.

## 5b — code quality (attack)
1. Mutation probes in your scratch checkout: (a) absorb only after the heap empties → which tests redden (the shallow-before-
   deep and the one-build tests must); (b) never charge the budget → the cap test; (c) charge first-time absorbs too → the
   ">16 independent notifications, no warn" test; (d) drop the merge branch (always push) → the one-build test;
   (e) route absorbed ids straight onto the heap ignoring scope → the two scoped-target tests; (f) reset the budget per DRAIN
   instead of per frame → the shared-budget test.
2. Lock discipline: the inbox lock is never held across a build or across `tree.mark_needs_build`/heap pushes? A build calling
   `RebuildHandle::schedule` synchronously (same thread) — deadlock-free (there is a test; confirm the test actually holds
   the shape). Any user code under the inbox lock?
3. Termination: with the budget, can a frame still loop unboundedly? A self-rescheduling element whose schedule lands as a
   MERGE (Occupied) rather than a Vacant re-entry — is that possible (the entry is removed after its build, so a schedule
   DURING its own build is Vacant on the next absorb… but a schedule during its own build that is absorbed BEFORE `dirty_reasons.remove`
   — i.e. the element's own build schedules a sibling that is still on the heap)? Construct the worst case and check it is
   bounded by `MAX_MID_DRAIN_ABSORBS` or by the heap's own finiteness.
4. Ordering: an absorbed id keyed at authoritative depth vs heap entries re-keyed once before the loop — any inversion where
   a child builds before its parent (the parent scheduled mid-drain at a depth shallower than an already-popped-and-built
   child?) — is that Flutter-consistent (Flutter re-sorts and walks back)?
5. The stale-id test (`stale_rebuild_handle_after_a_same_slot_child_swap_lands_on_the_new_occupant`) — does it pin the observed
   behavior honestly without asserting a policy, and is the hazard note at the absorb site accurate?
6. `e7493121`: is the two-episode doc accurate (read `modal_covered_route_without_maintain_state_is_unmounted_and_loses_its_state`
   and the fixture); is the auto-observer flight leak a real production hazard worth an issue (a replaced hero observer's
   in-flight flights never finish) — say so with the evidence you can read.
7. Docs: process markers ("Decision N", "v3 delta", "plan"), false claims, the new partial ARCHITECTURE.md's fit with the
   sibling pattern; the `test_build_owner_memory_size` bump justified.
8. Anything the change makes worse: per-pop inbox lock (uncontended `parking_lot` lock per build — negligible; say so),
   `built_this_frame` growth on a frame that builds thousands of elements (a `HashSet` insert per build — measure or bound).

Output: `5a: PASS/FAIL`, `5b: APPROVE / NEEDS WORK / BLOCK`, numbered findings (file:line, severity, failing scenario, fix),
no praise, under 140 lines, commands at the end.
