# integration — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); **требования:** [requirements.md](requirements.md) (R1–R22)
- **Порядок:** волна W3 — после ownership (W2) и listener-delivery (A): семантика снятия
  слушателя во время раздачи (D-07) и изоляция panic (D-01) — их. Ветка и worktree на задачу
  (`cargo xtask worktree new animation/<slug>`); ID R/T — только в этом каталоге.

## Граф

```text
T1 контракт ─► T2 RenderAnimatedTransform ─► T3 переходы ─┬─► T4 [P] drawer
                                                          ├─► T5 [P] dismissible
T1 ─► T6 [P] RefreshIndicator (D-16, не зависит от T2) ───┤
                                                          └─► T7 документы, perf, changelog
I3 (flui-interaction, другая сессия) ─► T8 fling-потребитель
```

## Задачи

| ID | Работа | Требования | Зависит | [P] |
|----|--------|------------|---------|-----|
| T1 | контракт: `TransformMotion`, `RenderAnimatedTransform` с инертными телами (матрица всегда тождественная, слушатель не ставится); строка в `RENDER_OBJECT_TYPES` и заготовки `harness_animated_transform_*`; все тесты R1–R21 — красные по assertion (`elements_built > 0`, неверная матрица); вывод красного прогона в PR | все | A, W2 | — |
| T2 | render-объект: кэш сэмпла, классификация, пометки с коммитом после отправки, слой для переноса, hit-test, `apply_paint_transform`, semantics, `attach`/`detach` | R3, R5, R11, R12, R15 | T1 | — |
| T3 | `SlideTransition`/`ScaleTransition`/`RotationTransition` на `ProxyAnimation` + приватный `RenderView`; миграция тестов `slide_transition.rs`, cupertino `route.rs`; удалить `LaidOut::transform_scale`/`transform_rotation`; строка readback-теста | R1, R2, R4, R6–R10, R13, R14 | T2 | — |
| T4 | drawer: `SlideTransition` + `FadeTransition`, удалить value-слушатель | R16, R17 | T3 | ‖ T5, T6 |
| T5 | dismissible: `SlideTransition`, rebuild только при смене `(value ≠ 0, sign)` | R18 | T3 | ‖ T4, T6 |
| T6 | RefreshIndicator: `position(...)`, rebuild только на смене фазы, переподписка при замене `RefreshController` | R19–R21 | T1 | ‖ T2–T5 |
| T7 | `crates/flui-rendering/ARCHITECTURE.md:381-386`, Mapping decision в `crates/flui-objects/ARCHITECTURE.md`, perf-сценарий «slide transition tick» в `crates/flui-widgets/tests/perf.rs` + `perf/baseline.toml` (до/после), `changelog.d/` | — | T3–T6 | — |
| T8 | после I3: снять литералы `±8000`, общий `start_ballistic` в `scroll/` | R22 | I3, T6 | — |

## Разрешённые файлы

| ID | Файлы |
|----|-------|
| T1, T2 | `crates/flui-objects/src/proxy/{animated_transform.rs,mod.rs}`, `crates/flui-objects/tests/render_object_harness.rs`, `crates/flui-widgets/tests/{contracts.rs,transitions.rs,main.rs}`, `crates/flui-widgets/tests/{dismissible.rs,scroll.rs}`, `packages/flui-material/tests/drawer.rs` (красные тесты) |
| T3 | `crates/flui-widgets/src/transitions/{slide,scale,rotation}_transition.rs`, `crates/flui-widgets/tests/{slide_transition.rs,transitions.rs}`, `packages/flui-cupertino/tests/route.rs`, `crates/flui-testing/src/widgets.rs`, `tests/composited_layer_update_readback.rs` |
| T4 | `packages/flui-material/src/drawer.rs`, `packages/flui-material/tests/drawer.rs` |
| T5 | `crates/flui-widgets/src/interaction/dismissible.rs`, `crates/flui-widgets/tests/dismissible.rs` |
| T6 | `crates/flui-widgets/src/scroll/refresh_indicator.rs`, `crates/flui-widgets/tests/scroll.rs` |
| T7 | `crates/flui-rendering/ARCHITECTURE.md`, `crates/flui-objects/ARCHITECTURE.md`, `crates/flui-widgets/tests/perf.rs`, `crates/flui-widgets/perf/baseline.toml`, `changelog.d/<branch-slug>.md` |
| T8 | `crates/flui-widgets/src/scroll/{scrollable.rs,refresh_indicator.rs,mod.rs}`, `crates/flui-widgets/tests/scroll.rs` |

`dismissible.rs` пересекается с темой retarget (D-30, масштаб скорости 1/300): T5 ребейзится на
неё или идёт одной веткой через владельца миграции `flui-widgets` (W2).

## Definition of Done (каждая задача)

- Первый коммит — контракт и тесты, красные по assertion (вывод в PR); второй — поведение.
- Откат production-ханка в отдельном worktree роняет строку по названной причине: R1/R16/R18/R19
  — `elements_built > 0` с `RebuildReason::AnimationTick`; R2 — пиксели различаются в точках,
  выбранных так, чтобы сдвинутый и несдвинутый кадр давали разный цвет; R4 — касание в старой
  позиции; R11 — потерянная пометка после `SendError`.
- Счётчики rebuild читаются из `HeadlessBinding::last_frame_report()`; тест не помечает
  корень грязным сам и не использует хелперы, которые это делают.
- Каждый новый render-объект: строка `RENDER_OBJECT_TYPES`, `harness_animated_transform_*`
  (layout, paint, hit-test, intrinsics через `forward_single_child_box_queries!`, тик), строка
  таблицы в шапке `render_object_harness.rs`.
- GPU readback — один поток, локальный прогон на Windows; вывод в PR (в CI GPU-задач нет).
- `cargo nextest run -p flui-objects -p flui-widgets -p flui-material -p flui-cupertino` по
  затронутым; `cargo test -p flui-objects --test render_object_harness`;
  `cargo xtask check-changed` зелёный один раз перед PR; perf до/после в PR (T7).
