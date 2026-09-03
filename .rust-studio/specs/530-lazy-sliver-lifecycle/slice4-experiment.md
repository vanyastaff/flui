# Slice 4 experiment — correction model, executed 2026-09-03 on 5cada644 (worktree, patch not committed)

Setup: `virtualized_band.rs` patched so backward suppression is env-gated (`FLUI_EXP_SUPPRESS_BACKWARD`)
and step 11 prints `(scroll, last, anchor, band, measured, pending, emitted, hint, total)` per pass.
Scene: the oracle's ('SliverList can handle inaccurate scroll offset due to changes in children list'),
via a scratch test recording the onstage window at each checkpoint.

| checkpoint | oracle | FLUI (both modes identical) | pixels |
|---|---|---|---|
| mount | [0,12] | [0,12] | 0 |
| drag −750 | [16,28] | [14,28] (14 is on screen in Flutter too, unasserted) | 750 |
| swap (odd items 0→96) | [12,19] | [14,21] | 942 (= 750 + 192: items 11, 13 above anchor 14 grew) |
| drag +250 #1 | [10,16] | [12,18] | 788 (= 692 + 96: item 9 entered the band and grew) |
| #2 | [7,13] | [9,15] | 634 |
| #3 | – | [7,13] | 576 (two items grew: −250 + 192) |
| #4 | – | [4,10] | 422 |
| #5 | [0,6] | [1,8] | 172 = 942 + 480 (items 1..9 grew) − 1250 |

Findings:
1. **Suppression is irrelevant to this scene**: with in-frame servicing every build triggers another
   layout pass at the same offset (`is_backward == false`), which emits the pending correction within
   the frame. It can only delay a resident item's own remeasure by one frame during a backward drag —
   which is itself a one-frame content jump. Remove it (amend ADR-0003's consumer note).
2. **The divergence is entirely the swap-time anchor correction** (+192). Flutter keeps the first
   *retained* child's stale offset and lets the two grown odd items push the visible content down
   192 px with no user input; FLUI keeps the first *visible* item pixel-stationary and corrects the
   offset instead. From there both models accumulate the same growth (+96 per odd item entering the
   band above the anchor) and clamp at 0; FLUI is exactly 192 px further from the top, one more
   250 px drag away from the oracle's `[0,6]`.
3. Both models are self-consistent; there is no `[1,8]` bug to fix, only a model to record.

Decision for slice 4: keep the stationary-anchor model (no visual jump on off-screen remeasure — the
Compose/GPUI anchor behaviour), record it as a mapping decision + ADR amendment, replace the oracle
pin with a FLUI oracle asserting (a) anchor screen position unchanged across the swap, (b) the
windows above, (c) `[0,6]` at `pixels == 0` after the sixth drag; move the manifest entry to
`diverged` with this table as the reason. Remove the backward suppression and retarget
`harness_sliver_list_anchor_correction_forward_emits_backward_suppresses` to "emits regardless of
direction".
