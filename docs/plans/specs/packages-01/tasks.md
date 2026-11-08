# Tasks: bounded facade application surface

- **Status:** Complete
- **Date:** 2026-11-08
- **Requirements:** [requirements.md](requirements.md)
- **Design:** [design.md](design.md)

Implementation and the available focused host validation are complete. The cross-target and suite limitations below are explicit: none of those checks is claimed as passing.

| Task | State | Validation evidence |
|---|---|---|
| T1. Replace the whole application alias and migrate callers | complete | `r907` Clippy passed for `flui`, `flui-app` and `flui-cli`, all targets and features; `r932` passed both external facade-extension consumers, including root window paths; `r935` passed retained `AppHandle` and executor paths; `r943` passed the generated hot-reload workspace. |
| T2. Move Android interop behind the native feature | complete | Android interop is only under `flui::native::android`; `r911` (basic), `r915` (empty) and `r938` (counter) passed generated host builds and the Android dependency/import assertions; `r907` and `r916` passed host Clippy and every facade feature combination. Android target validation remains limited below. |
| T3. Add target-aware facade API closure and path regressions | complete | The root manifest names all six targets with all features and only the AccessKit exception. In `r944`, `pipeline_owner_is_not_facade_surface` passed; the full trybuild target remains red on five unrelated existing snapshot mismatches. The six-target `api-closure` run did not complete; see below. |
| T4. Rewrite affected records and add the changelog | complete | Added `changelog.d/packages-01-bounded-facade.md`, rewrote the affected ADRs, architecture and live specs, and completed the old-path audit. `r919` passed `cargo heli xtask checks`, including docs and changelog validation. |
| T5. Run affected suites and repository gates | complete | `cargo fmt --all` passed; `r907` Clippy and `r916` facade-combos passed. Integrated `r945` xtask checks passed after both facade and telemetry changes. Focused consumers passed as `r911`, `r915`, `r932`, `r935`, `r938` and `r943`. Unavailable and incomplete checks are listed below rather than treated as green. |

## Validation limitations

- `r902`: `cargo heli nextest run -p flui -p flui-app` could not start because `cargo-nextest` is not installed. The ordinary `cargo test` fallback `r903` was cancelled after the 300-second client limit, so the complete `flui`/`flui-app` suites are not claimed as passing; the focused consumers above completed successfully.
- `r944`: the new app-surface compile-fail fixture passed, but the aggregate `thread_boundary_ui` target failed on five existing fixtures outside this task (`driven_controller_has_no_bare_dispose`, `owner_state_stays_on_its_thread`, `owner_values_stay_on_their_thread`, `status_removal_requires_owning_subscription`, `text_editing_controller_stays_on_its_thread`). Their snapshots were not blessed or edited. The integrated rerun `r946` exceeded the 300-second limit and produced no newer aggregate result.
- `r923`: `cargo heli xtask api-closure -p flui` stopped in its first Linux rustdoc build because `psm` could not find `x86_64-linux-gnu-gcc`. Consequently the all-feature Linux, Windows, macOS, iOS, Android and wasm32 closure matrix has no passing run in this environment. `r924` independently confirmed that the Android all-feature check is unavailable without the NDK (`aarch64-linux-android-clang` / `clang.exe` missing).
- `r921`: `cargo heli xtask send-positions --seed` stopped in nightly rustdoc with upstream `wesl` error E0391 (async-drop query cycle). The send-position inventory was therefore not regenerated or edited, and no shrinking-list claim is made.

