# interpolation — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md) (сценарии I1–I15, черновик ADR)
- **Порядок PR:** тема идёт в группе «остальное» (после motion-clock). Ветка и worktree на
  задачу (`cargo xtask worktree new animation/<slug>`); ID I/T — только в этом каталоге.
- **Владение:** единственный писатель `crates/flui-foundation/src/geometry/{matrix4,lerp}.rs` и
  `crates/flui-painting/src/styling/color.rs` (`lerp`, `lerp_oklab`) в этом проходе.

## Граф

```text
T1 контракт + ADR ─┬─► T2 [P] Matrix4 ─► T5 AnimatedContainer::transform ─┐
                   ├─► T3 [P] Color Oklab                                  ├─► T7 документация
                   ├─► T4 [P] Angle ─► T6 AnimatedRotation                 │
                   └─► T8 [P] домен выброса (AnimatedContainer, hero, int) ┘
```

## Задачи

| ID | Работа | Сценарии | Зависит | [P] |
|----|--------|----------|---------|-----|
| T1 | контракт: `Angle` с инертным `nearest_equivalent` (возвращает self), `RotationPath`, `AnimatedRotation`/`AnimatedContainer::transform` как заглушки без анимации; все таблицы тестов, красные по assertion; ADR (статус Proposed) и правка `lerp.rs` модульного doc | все | — | — |
| T2 | декомпозиция CSS Transforms 2 §13.1 в f64 (приватный модуль рядом с `matrix4.rs`), заимствование у вырожденной оси, дискретный переход, точные концы | I1–I6 | T1 | ‖ T3, T4, T8 |
| T3 | `Color::lerp` → premultiplied Oklab; `Lerp for Color`; удалить `lerp_oklab`, `OklabColorTween`; пример `oklab_gradient`, `animated_box_app`; бенчи до/после | I7–I9 | T1 | ‖ T2, T4, T8 |
| T4 | `Angle`, `Lerp for Angle`, `nearest_equivalent` | I10 | T1 | ‖ T2, T3, T8 |
| T5 | `AnimatedContainer::transform` через `OptTween<Matrix4>` | I14 | T2 | — |
| T6 | `AnimatedRotation` (implicit, `RotationTransition` внутри; `Shorter` берёт текущий угол как `reference`) | I11 | T4 | ‖ T5 |
| T8 | клампы домена: `AnimatedContainer` padding/margin/width/height, hero `current_rect`; NaN → `begin` у `IntTween`/`StepTween` | I12, I13, I15 | T1 | ‖ T2, T3, T4 |
| T7 | ADR → Accepted, `Superseded-by` в ADR-0098; `docs/` крейтов flui-animation и flui-painting (`ARCHITECTURE.md` mapping decision 13 о `Color::lerp`); `changelog.d/` | — | T2–T6, T8 | — |

## Разрешённые файлы

| ID | Файлы |
|----|-------|
| T1, T7 | `docs/adr/ADR-NNNN-interpolation-contracts.md`, `docs/adr/ADR-0098-owned-f64-geometry-values.md` (только строка `Superseded-by`), `crates/flui-foundation/src/geometry/{lerp.rs,angle.rs,mod.rs}`, `crates/flui-widgets/src/animated/{animated_rotation.rs,mod.rs,animated_container.rs}`, `crates/flui-widgets/src/lib.rs` (экспорт), тестовые модули ниже, `crates/flui-painting/ARCHITECTURE.md`, `crates/flui-animation/{README.md,docs/*.md}`, `changelog.d/<branch-slug>.md` |
| T2 | `crates/flui-foundation/src/geometry/{matrix4.rs,matrix4_decompose.rs,lerp.rs}`, `crates/flui-foundation/tests/main.rs` + новый `tests/geometry/matrix4_lerp.rs` |
| T3 | `crates/flui-painting/src/styling/color.rs`, `src/lerp_impls.rs`, `tests/color_property.rs`, `tests/main.rs`, `benches/color_bench.rs`; `crates/flui-animation/src/{tween_types.rs,lib.rs}`, `examples/oklab_gradient.rs`, `benches/animation_bench.rs`; `examples/animated_box_app.rs` |
| T4 | `crates/flui-foundation/src/geometry/{angle.rs,mod.rs,lerp.rs}`, `crates/flui-foundation/tests/geometry/` |
| T5, T6 | `crates/flui-widgets/src/animated/{animated_container.rs,animated_rotation.rs,mod.rs}`, `crates/flui-widgets/src/lib.rs`, `crates/flui-widgets/tests/implicit_animations.rs` |
| T8 | `crates/flui-widgets/src/animated/animated_container.rs`, `crates/flui-widgets/src/navigator/hero_flight.rs`, `crates/flui-widgets/tests/{implicit_animations.rs,hero_flight.rs}`, `crates/flui-animation/src/tween_types.rs`, `crates/flui-animation/tests/contracts/tween.rs` |

Пересечения: `animated_container.rs` — T5 и T8 (последовательно или одна ветка); `tween_types.rs`
— T3 и T8; `hero_flight.rs` — владелец миграции `flui-widgets` (W2) согласует порядок.

## Definition of Done (каждая задача)

- Первый коммит — контракт и тесты, красные по assertion (вывод в PR); второй — поведение.
- Строка-фикс падает при откате production-ханка в отдельном worktree по названной причине:
  I1 — NaN в элементах, I4 — потерянный shear, I7 — 128 вместо 99, I8 — потемнение (Oklab с
  прямой alpha), I12 — debug panic `RenderContainer padding must be non-negative`, I13 —
  отрицательная ширина.
- Эталоны — аналитика и определение Oklab; [U]-факты (правило ничьих округления CSS, значения
  Oklab хроматических цветов) контрактом не закрепляются.
- Удаления сверены с `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4); rg по удалённым именам
  пуст во всём workspace; `packages/` мигрированы в том же PR.
- Изменившийся `.snap` объяснён в PR и перепрогнан без обновления snapshot'ов.
- `cargo nextest run -p <crate>` по затронутым крейтам; `cargo test --doc` для flui-painting и
  flui-animation; `cargo xtask check-changed` зелёный один раз перед PR; бенчи T3 до/после.
