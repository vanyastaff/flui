# Shared benchmark grid tasks

Status: implementation and verification in progress; no measured result accepted yet.

- [x] BG-T1 (BG-1–3): Add shared scene and an example runner using existing owners/counters.
- [ ] BG-T2 (BG-4): Execute population, geometry, input, local-update and GPU checks; preserve deliberate-fault failures and restored passes.
- [ ] BG-T3 (BG-2–3): Validate the timing boundary, nearest-rank statistics and failure/output behavior.
- [ ] BG-T4 (BG-5): Pin source and machine; collect all primary cells, controls and A/A variation with raw samples.
- [ ] BG-T5 (BG-6): Run scoped gates, document limitations and update the draft PR with artifacts.

Source work is confined to the facade example and its integration consumer; no framework API or runtime behavior changes are intended.

Validation before baseline collection (2026-11-02, Windows x86_64):

- `cargo heli test --locked -p flui --test main --no-default-features`: three tests passed (`grid_has_every_item_and_local_change_preserves_body`, `wall_clock_includes_work_and_percentiles_use_nearest_rank`, `pixel_oracle_rejects_blank_missing_and_stale_captures`).
- Deliberately changing the local header update from modulo 2 to modulo 1 failed `grid_has_every_item_and_local_change_preserves_body` at the assertion requiring `Benchmark 1` (heli r8771, exit 101). Restored the update.
- Deliberately clamping elapsed time to zero failed `wall_clock_includes_work_and_percentiles_use_nearest_rank` at the 5,000,000 ns injected-delay lower bound (heli r8774, exit 101). Restored the real elapsed time. This is a sensitivity test, not an estimate of scheduler precision.
- `cargo heli nextest run --locked -p flui --no-default-features`: 15/15 passed, 0 skipped (r8779, 493.481 s test execution). Runner metadata/docs were edited during this run, so heli correctly declined to cache its verdict; the tested scene/measurement source was unchanged.
- `cargo heli clippy --locked -p flui --all-targets --no-default-features -- -D warnings`: passed after runner metadata changes (r8789).
- `cargo heli fmt --all` and `cargo heli xtask checks`: passed (r8786/r8790).
- Pilot DX12 readbacks for 100/1,000/53,000 buttons passed population, geometry, input, fill/glyph/gap/clip and header-only pixel delta checks. Pilots are not accepted baseline measurements; final captures use the pinned release binary.
- Release compilation exposes an existing `unfulfilled_lint_expectations` warning in `flui-engine/src/command_ir.rs:224` about a debug-only overflow indicator. No suppression or unrelated source change was made.
