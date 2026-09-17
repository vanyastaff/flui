# Builder brief — #1180: external schedules landing mid-drain are absorbed at the top of each heap pop

Worktree (yours alone): /mnt/data/dev/flui-worktrees/view-mid-drain, branch fix/1180-mid-drain-external-builds-join-the-scope
(from origin/main 864b5798). Never edit /mnt/data/dev/flui; scratch under /mnt/data/dev/flui-worktrees-scratch/.
The operative spec is the PLAN (v2 + `## v3 deltas`): /mnt/data/dev/flui/.rust-studio/specs/1180-mid-drain-external-builds/plan.md
— read it fully; Decisions 1–5, the Tests list and every v3 delta are the contract. Issue: `gh issue view 1180`.
Flutter at 3.44.0 (`git -C /mnt/data/dev/flui/.flutter describe --tags`): `framework.dart` `BuildOwner.buildScope`,
`scheduleBuildFor`, `Element.markNeedsBuild` (`if (dirty) return`; the `_debugCurrentBuildTarget` descendant assert).
House rules: /mnt/data/dev/flui/AGENTS.md; `crates/flui-view/AGENTS.md`; the lock discipline already documented in
`build_owner.rs` (the inbox `parking_lot::Mutex` is never held across a build — `lock; drain().collect::<Vec<_>>()`
in ONE statement, then release, then act; a build may call `schedule` synchronously); no process markers; panic
policy; the `LockDiscipline/StatementDrop` port-check trigger (flui-view is not in its scope yet, keep clean anyway).

## Where
- `crates/flui-view/src/owner/build_owner.rs`: the absorb step at the top of the heap loop in `drain_build_scope`
  (Decision 1 + v3 §1/§2); `BuildOwner` fields `mid_drain_absorbs_left`, `mid_drain_cap_streak`,
  `built_this_frame: HashSet<ElementId>` (reset at `build_scope` entry per v3 §3); the `build` span's
  `absorbed_mid_drain` field; the false comment in `drain_build_scope`; the docs listed in v3 §5; the mapping-decision
  entry in `crates/flui-view/ARCHITECTURE.md` (or the crate's `## Mapping decisions` home — find it) per Decision 5 +
  v3 §2; `crates/flui-view/CHANGELOG.md` `### Changed`.
- `crates/flui-view/src/owner/rebuild_handle.rs`: docs (v3 §5).
- `crates/flui-widgets/src/testing.rs`: `LaidOut::tick`/`pump_for` docs (v3 §5).
- `crates/flui-widgets/tests/implicit_animations.rs`: flip `zero_duration_retarget_lays_out_the_new_target_on_the_same_pump`
  (`pending == 0`, `has_dirty_elements() == false`, `AnimatedBuilder` built once) and rewrite its doc paragraph
  (the one-frame deferral is gone; it names #1180 as the pin it flips).

## Tests — exactly the plan's list plus the v3 additions; RED first for each non-pin (show the failing run on the
unchanged code; for the "one build" oracle install a `tree_observer` or count in the view — `element_rebuilt`
fires only with an observer).

## Gates (paste tails)
`cargo nextest run -p flui-view -p flui-widgets -p flui-material -p flui-app -p flui -p flui-testing`,
`cargo clippy -p flui-view -p flui-widgets --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`RUSTDOCFLAGS="-D warnings" cargo doc -p flui-view --no-deps --document-private-items`, `typos` on changed files,
`bash scripts/port-check.sh`, then `just ci` once at the end.

## Constraints
No public signature changes (`ExternalBuildScheduler` is `pub(crate)`; `BuildOwner` gains private fields).
Commit on the branch (`fix(view): …`), explicit paths, no push.

## Report back
Diff summary; red→green evidence per test (name pins honestly); gate tails; deviations and why; `NOTE:`; `MEMORY:`.
