# flui-animation / physics — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **Порядок:** ветка и worktree на задачу от `Q0` (`cargo xtask worktree new animation/<slug>`), один
  PR на задачу, `cargo xtask check-changed` зелёный. ID R/T/D — только в этом каталоге, не в коде,
  именах тестов и коммитах. Тесты — через публичный API в `tests/contracts/*.rs`, смонтированных из
  `tests/main.rs` (Q0), таблицами `run_table`; property — `proptest` (dev-зависимость добавляет Q0).

## Зависимости

```text
Q0 ─▶ T1 контракт ─┬─▶ T2 пружина [P] ─┬─▶ T4 fling ─────────────┐
                   ├─▶ T3 трение/границы [P] ─▶ T5 bouncing ──────┼─▶ T7 миграция docs (Z)
                   └─▶ T6 dpr-допуск scroll (после T2, T3) ──────┘
T8 удаление smoothing (решено владельцем; AnimatedValue — за retarget T4)
```

## Задачи

| ID | Работа | Требования | Разрешённые файлы | Зависит | [P] |
|---|---|---|---|---|---|
| T1 | Контракт: `SimulationError`, `SimulationParameter`, `SimulationBounds`, `Tolerance` (приватные поля, конструкторы), сигнатуры `Result` у конструкторов пружины/трения/`SpringSimulation`, `rest_time`, `BouncingScrollSimulation` и `ScrollMetrics::device_pixel_ratio` с инертными телами; миграция всех вызовов на новые сигнатуры; удаление `ScrollSpringSimulation`, `GravitySimulation`, `ClampedSimulation`, `through`, `with_snap_to_end`, `end_position`, `Simulation::tolerance`, `Tolerance::time`; все тесты T2–T6, красные по assertion | R1–R17 (тесты) | `simulation.rs`, `spring.rs`, `controller.rs`/`controller/run.rs` (сигнатуры), `controller_tests.rs`, `tests/contracts/{simulation,spring,controller_sources}.rs`, `benches/animation_bench.rs`, `lib.rs` (свои строки), `flui-widgets/src/scroll/{scroll_physics,page_view,scrollable,refresh_indicator}.rs`, `flui-widgets/tests/{scroll,contracts}.rs` | Q0 | — |
| T2 | Пружина: `(ω, ζ)`, единая C/S-форма с рядом и устойчивыми корнями, проверки области, `rest_time` по огибающей, `x(t ≥ rest_time) = end`, домен времени, `Duration` у перцептивных конструкторов | R1–R8, R17 | `simulation.rs` (модуль пружины можно вынести в `simulation/spring.rs`), `tests/contracts/spring.rs` | T1 | ‖ T3 |
| T3 | Трение и границы: `drag` → `Result`, покой по остатку пути, `time_at_x` позади старта → `+inf`, `SimulationBounds` в `BoundedFrictionSimulation` | R9, R13, R14 | `simulation.rs` (раздел трения), `tests/contracts/simulation.rs` | T1 | ‖ T2 |
| T4 | Fling: `until_crossing`, константа пружины по умолчанию, удаление `FLING_TOLERANCE` | R11 | `controller/run.rs` (одна функция; согласовать с B), `simulation.rs`, `tests/contracts/controller_sources.rs` | T2 | ‖ T5 |
| T5 | `BouncingScrollSimulation` и `BouncingScrollPhysics` на ней | R12 | `simulation.rs`, `flui-widgets/src/scroll/scroll_physics.rs`, `tests/contracts/simulation.rs`, `flui-widgets/tests/scroll.rs` | T2, T3 | ‖ T4 |
| T6 | dpr-допуск: `WeakPipelineCell` в `ScrollableState`/`RefreshIndicatorState`, `with_device_pixel_ratio` во всех баллистических вызовах, `Tolerance::for_device_pixel_ratio` в `Clamping`/`Bouncing`/`PageScrollPhysics`, `ScrollPhysics` с невалидными экстентами → `None` | R9, R10, R13 | `flui-widgets/src/scroll/{scroll_physics,page_view,scrollable,refresh_indicator}.rs`, `flui-widgets/tests/{scroll,contracts}.rs` | T2, T3 | — |
| T7 | Документация крейта: README (7 примеров), `docs/GUIDE.md`, `docs/PATTERNS.md`, раздел физики в `docs/ARCHITECTURE.md` (инварианты из design), фрагмент `changelog.d/` | — | `crates/flui-animation/{README.md,docs/*}`, `changelog.d/<branch-slug>.md` | T2–T6 | ‖ |
| T8 | Удалить `smoothing` (+ пример, бенч, реэкспорты) — решение владельца. `AnimatedValue` не трогается (R16: переписывает retarget T4) | R15 | `smoothing.rs`, `examples/smoothing_follow.rs`, `benches/animation_bench.rs`, `tests/contracts/simulation.rs`, `lib.rs` | T2; решение | — |

## Тесты по задачам (имя → раннер)

- T2: `spring_constructors_refuse_outside_the_admitted_domain`, `perceptual_springs_match_published_conversions`,
  `perceptual_branches_are_consistent`,
  `spring_matches_analytic_reference` (таблицы `run_table`, эталон из requirements, не из кода);
  `spring_outputs_are_finite_over_the_admitted_domain`, `rest_time_is_conservative_and_tight`,
  `spring_run_is_independent_of_frame_partition` (proptest; последний — `AnimationController::animate_with`
  и `AnimationController::tick_at` с явными временами), `simulation_time_domain_edges`; compile-fail
  `spring_description_literal`.
- T3: `friction_rest_and_time_queries` (новые строки в существующей таблице), `bounds_refuse_unordered_and_nan`.
- T4: `fling_completes_on_the_frame_that_reaches_the_bound` (60 Гц: завершение на первом кадре ≥ 0.29537 с).
- T5: `bouncing_simulation_hands_friction_to_spring_at_the_edge`,
  `bouncing_fling_into_the_edge_overscrolls_and_returns`.
- T6: `scroll_fling_rest_scales_with_device_pixel_ratio`, строка `inverted_extents_do_not_fling`.
- T8: удаление без тестов (rg по именам пуст).

## Definition of Done (каждая задача)

- Каждый тест задачи красный на T1 (вывод красного прогона по assertion — в PR) и красный с откатом
  production-хунка задачи в изолированном checkout; зелёный с ним.
- Эталонные значения взяты из requirements (аналитика market-C), не вычислены production-кодом.
- Ни одного нового `pub` без production-пользователя: `rest_time` — `pub(crate)` (R17), `BouncingScrollSimulation`
  — `BouncingScrollPhysics`, `SimulationBounds`/`Tolerance::for_device_pixel_ratio` — scroll-физика.
- `cargo nextest run -p flui-animation` и `-p flui-widgets` зелёные; `cargo xtask check-changed` зелёный;
  `CARGO_BUILD_JOBS=6`, `NEXTEST_TEST_THREADS=4`.
- Нет `unwrap()`/`assert!` на путях с пользовательским вводом; нет `static`; симуляции без блокировок.
- Удаление/переименование `pub` сверено с `crates/flui-sdk/tests/surface.rs` и `rg` по `packages/`;
  найденные вызовы мигрируют в том же PR. Предел скорости fling (I3) не меняется.
- Ни один тест не закрепляет факт рынка с пометкой [U] (ветка `b < 0`, `response`); такие ветки —
  только тест `perceptual_branches_are_consistent`.
- PR-порядок: после merge listener-delivery и controller-robustness (T4 правит `controller/run.rs`).
- Бенч `spring_simulation`/`animated_value_color_frame` до/после на одном хосте (задача Z собирает).
