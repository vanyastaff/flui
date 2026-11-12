# Shared benchmark grid tasks

Status: implementation, correctness validation and first baseline collection complete. Timings are advisory on a shared Windows host; 12/16 A/A cells are inconclusive for a 10% improvement claim. External comparisons and native presentation remain outside this approved implementation.

- [x] BG-T1 (BG-1–3): Add shared scene and an example runner using existing owners/counters.
- [x] BG-T2 (BG-4): Execute population, geometry, input, local-update and GPU checks; preserve deliberate-fault failures and restored passes.
- [x] BG-T3 (BG-2–3): Validate the timing boundary, nearest-rank statistics and failure/output behavior.
- [x] BG-T4 (BG-5): Pin source and machine; collect all primary cells, controls and A/A variation with raw samples.
- [x] BG-T5 (BG-6): Run scoped gates, document limitations and publish the report/artifacts in draft PR #2172; keep it unmerged.

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

Baseline evidence:

- Source `e3413f98726e9034f31aa4b47febb6f1999137af`, clean checkout at series start. Release build r8793, 31.7 s, exit 0. Run window 2026-11-02 11:17:36–11:25:02 UTC. The [report](../../drafts/peer-inspired-development/benchmark-baseline.ru.md) links raw artifacts and machine/source/binary/lock/font hashes.
- 80 primary processes × 10,000 samples; 10 full-rebuild controls × 1,000; eight GPU captures × 11. Each process has 120 warmups. All 98 processes exit 0. Primary modes/size/kind combinations each have five fresh-process repeats. Every local sample has 4 rebuild / 1 layout / 9 paint / 1 produced frame; every retained sample has zero work and produced frames.
- All eight capture processes pass sampled pixel/glyph/clip/header-delta checks with DX12 requested. The selected adapter is not exposed by the capture API. Native runtime, native presentation, GPU duration, process CPU time and semantics updates are not measured.
- CLI rejects 101 items, zero frames and an existing output directory; SHA-256 of the existing summary remains unchanged.
- `node target/validate-grid-baseline.cjs target/grid-baseline-2026-11-02`: VERIFIED, 810,088 workload samples; independent recomputation of percentiles, totals, calibration, run order, matrix and hashes. The validator also rejected an unfinished matrix before the final capture completed. Repeated successfully from the extracted published archive using its `validate.cjs`.
- Shared-host limitation: compilers seen after five primary and one capture process. Boundary-only checks cannot prove continuous isolation. These runs are preserved and flagged; no outlier was discarded. Four of 16 cells satisfy the descriptive A/A threshold; the remaining 12 are INCONCLUSIVE. A dedicated-runner A/A is required before any 10% improvement claim, not before retaining this observational baseline.
- Archive `raw-results.zip`: 2,522,490 bytes, SHA-256 `d4652e6196d61ba2182f360b2c5aa1d5a99d33bc239e377ca0b42d4a1e7a1ae4`. The archive is temporary draft evidence, to move to immutable artifact storage before any merge proposal.
- Final documentation/artifact gate: `cargo heli xtask checks` r8804, exit 0 (32.6 s); 25 relative links in draft/spec/example documents checked separately, none broken; `git diff --check` passed. The implementation files and Cargo inputs are unchanged from the measured source commit.
- Pre-push review: no library API/dependency/platform changes; the only new callback stores a bounded item index in an owner-local Cell, with no borrowed state held across user code. CLI bounds sizes/samples, output directories cannot be reused, and failed runs cannot replace existing summaries. Native/GPU timings remain unavailable rather than inferred. Reproduction scripts and raw samples live in the temporary evidence archive, not in the runtime.
