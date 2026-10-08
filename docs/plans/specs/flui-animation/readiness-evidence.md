# Animation: свидетельства аудита 2026-10-08

## Среда и границы

- Source baseline: `91bb1fe1d2a349f62915846303f6f25780ae5c6d` (`origin/main`, получен `git fetch origin main`).
- Изолированный worktree, ветка `codex/animation-readiness-audit`.
- Windows x86_64 MSVC; toolchain из `rust-toolchain.toml`; nextest 0.9.146.
- `CARGO_BUILD_JOBS=6`, `NEXTEST_TEST_THREADS=4`. Изменения аудита только в документации.
- Тесты выполнялись последовательно; production-код и условия тестов не изменялись.
- Под «75 тестов» понимаются top-level tests nextest; табличные строки внутри них в это число
  отдельно не включены. Proptest запускался с настройками исходников, не со 100000 cases.

## Выполнено

```text
cargo nextest run --locked -p flui-animation --all-features --no-fail-fast
Summary: 75 tests run: 75 passed, 32 skipped
Nextest run ID: 1ecc030c-e1f3-4c9f-85ad-4d2c4f4f5e67

cargo nextest run --locked -p flui-animation --all-features --run-ignored only --no-fail-fast
Summary [10.160s]: 32 tests run: 0 passed, 32 failed, 75 skipped
Nextest run ID: db572a9d-3de6-4892-9ac9-c1d6c0fd40be

cargo test --locked -p flui-animation --all-features --doc
136 passed + 1 compile-fail doctest passed; 0 failed; 0 ignored

cargo xtask checks
Exit code 0: formatting, typos, docs, architecture/workspace and repository gates passed.
```

Матрица проверена отдельно: 110 уникальных M-ID совпадают с историческим market;
47 уникальных D-ID имеют маршрут повторной проверки. `git diff --check` чистый.
Репозиторные docs gates исключают архивные корни; их успех не является проверкой
всех исторических ссылок или внешних источников восстановленных спецификаций.

Обычный прогон включает `tick_allocation::a_steady_state_frame_allocates_nothing`.
Он не заменяет бенчи до/после и не даёт верхнюю границу времени кадра.

## Все 32 красных контракта

Каждый элемент списка — реально запущенный тест `flui-animation::animation_it`.
Четыре строки отмечены `(10s)`: ограниченный child-process harness завершил их по таймауту.
Это воспроизведение stalls на этом хосте; чтение кода дополнительно подтверждает guard/reentry
цикл. На нагруженном другом хосте таймаут сам по себе не является доказательством deadlock.

| Семейство | Тесты | Количество |
|---|---|---:|
| controller_robustness | curved_run_clamps_overshoot; curved_run_holds_the_last_finite_value; disposed_drops_late_value_listener; disposed_drops_late_status_listener; disposed_set_value_is_refused; is_animating_false_after_set_value; is_animating_tracks_an_installed_run; max_duration_full_range_starts; nan_frame_time_is_skipped | 9 |
| ownership | curved_status_listener_dies_with_the_wrapper; dispose_releases_value_listeners; last_handle_drop_releases_a_running_controller; proxy_parented_to_a_capturing_switch_is_freed; reverse_status_listener_dies_with_the_wrapper; switch_dispose_releases_callbacks; tween_status_listener_dies_with_the_wrapper; frame_scheduled_hook_may_read_the_controller (10s); frame_scheduled_hook_may_query_vsync (10s) | 9 |
| status_delivery | disposed_mid_fan_out_is_silent; proxy_reentrant_set_parent; reentrant_status_keeps_commit_order; removed_status_listener_is_skipped; status_listener_panic_finishes_the_round; status_listener_panics_compete; switch_hop_announces_status; value_listener_panic_finishes_the_round; value_listener_panics_compete; vsync_walk_contains_a_child_registry_panic; vsync_walk_contains_a_sibling_panic; switch_parent_status_reentry (10s); switch_parent_value_reentry (10s) | 13 |
| spring | fling_completes_on_the_frame_that_reaches_the_bound | 1 |

Примеры actual → expected из вывода:

```text
curved_run_clamps_overshoot: 1.5 -> 1.0
curved_run_holds_the_last_finite_value: NaN -> 0.2
disposed_set_value_is_refused: 0.8 -> 0.3
disposed_drops_late_status_listener: retained strong count 2 -> 1
reentrant_status_keeps_commit_order:
  [Forward, Reverse, Completed] -> [Forward, Completed, Reverse]
status_listener_panic_finishes_the_round: [] -> [Forward]
vsync_walk_contains_a_child_registry_panic: parent value 0.0 -> 0.5
frame_scheduled_hook_may_read_the_controller: killed: still running after 10s
```

## Статические запросы для повторения

Из корня checkout, с `rg`; для Windows перечислять пути явно, без Bash brace expansion:

```text
rg -n '^#\[ignore' crates/flui-animation/tests
rg -n 'MotionSpec|AnimatedValue' crates/flui-widgets/src packages
rg -n 'AnimationBehavior|reduce_motion|disable_animations|SystemPreferences' crates/flui-animation/src crates/flui-runtime/src crates/flui-widgets/src packages
rg -n 'tick_all|motion_clock' crates/flui-runtime/src crates/flui-testing/src
rg -n 'TIME_DILATION|time_dilation' crates/flui-scheduler/src crates/flui-animation/src
rg -n 'Keyframes|Stagger|JumpAt|\.cubic\(' crates/flui-widgets/src packages/flui-material/src packages/flui-cupertino/src
rg -n 'CompoundAnimation|ALWAYS_|pub mod prelude|pub use flui_scheduler' crates/flui-animation/src/lib.rs
git ls-tree -r --name-only 801543a3f docs/plans/specs/flui-animation
```

Совпадение в rustdoc не считается production call site; отрицательный поиск дополнять чтением
алиасов, реэкспортов, generic вызовов и макросов перед удалением public API.

## Не выполнено в этом аудите

Полный workspace gate, consumer suites widgets/material/cupertino/runtime/testing,
GPU readback, native live smoke, wasm/cross-target, сравнимые before/after benches,
исчерпывающий adversarial review всех численных алгоритмов. Их результаты из старых PR
не переносились на текущий SHA. Новый план предусматривает отдельные критерии закрытия.

Состояние PR читалось через GitHub CLI. Основная историческая волна merged; PR #1515 на момент
чтения OPEN. Статус может измениться: перед реализацией перечитать актуальный remote main.
