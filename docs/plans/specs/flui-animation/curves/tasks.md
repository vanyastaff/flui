# curves — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md) (контракт, API, сценарии C1–C14)
- **Порядок PR:** listener-delivery и controller-robustness → **physics и curves** → motion-clock →
  остальное. Тема curves стартует от `Q0` (раскладка тестов, proptest в `Cargo.toml`).
- **Ветка/worktree на задачу:** `cargo xtask worktree new animation/<slug>`. ID C/T — только в
  этом каталоге, не в коде, тестах и коммитах.

## Граф

```text
Q0 ─► T1 контракт ─┬─► T2 [P] валидация + serde ─┐
                   ├─► T3 [P] солвер             ├─► T6 документация крейта
                   ├─► T4 [P] Elastic, NaN, удаление SawTooth/Threshold ┤
                   ├─► T9 [P] Curve::slope (до retarget T2, composition T3) ┤
                   └─► T5 Steps ─► T7 потребитель: CupertinoActivityIndicator (composition) ┘
composition T3 (cubic-сегменты, M-CRV-9) ─► T8 удаление Catmull-Rom / Curve2D / ParametricCurve
```

`ReverseCurve`/`Curve::reversed` удаляются в T2 (замена существует уже сейчас).

## Задачи

| ID | Работа | Сценарии | Зависит | [P] |
|----|--------|----------|---------|-----|
| T1 | контракт: `CurveError`, `JumpAt`, `Steps` с инертным `transform` (возвращает t), `try_new` всех кривых — заглушки `Ok`; rustdoc политики `Curve`; все таблицы тестов, красные по assertion | все | Q0 | — |
| T2 | приватные поля, `const fn new` с `assert!`, `try_new`, serde `try_from`/`into`, `Split::try_with_curves`; `ThreePointCubic` хранит два сегмента; удалить `ReverseCurve`/`reversed` | C6, C7, C1 (`ThreePointCubic`) | T1 | ‖ T3, T4 |
| T3 | солвер: таблица 11, Newton с вилкой, бисекция до (hi−lo)·Y' < 1e-7; бенч `curved_value` до/после | C3, C4, C5, C14 | T1 | ‖ T2, T4 |
| T4 | нормированная огибающая Elastic; удалить `SawTooth` и `Threshold` (нет потребителя — решение владельца); единая политика NaN/диапазона во всех кривых; исправить rustdoc `SlowOutFastIn`, `EaseInOutCubic`, `EaseIn/OutBack` | C1, C2, C8, C10, C11 | T1 | ‖ T2, T3 |
| T5 | `Steps` по CSS Easing 1 §2.3.1 | C9, C1 | T2 | — |
| T7 | потребитель `Steps`: сегменты дорожки `CupertinoActivityIndicator` — `to(A[k], 125 мс, Steps::new(1, JumpAt::Start))` (вместе с composition T6, одна ветка); каретка не делается — `editable_text.rs` у text-ime (решение владельца) | C12, C13 (переписаны на индикатор) | T5; composition T6 | — |
| T9 | `Curve::slope` (provided: центральная/односторонняя разность второго порядка, неконечное → 0; `Linear` = 1, `Cubic` — аналитика с пределом `y''/x''` при `x'(s)=0`, `Steps` = 0, `FlippedCurve`/`Interval` — цепное правило, стёртая кривая делегирует); первым среди потребителей (X13) | slope-строки C15 | T1 | ‖ T2–T4 |
| T6 | `README.md`, `docs/{GUIDE,PERFORMANCE,ARCHITECTURE,PATTERNS}.md` крейта; `changelog.d/` | — | T2–T5 | — |
| T8 | удалить `ParametricCurve`, `Curve2D`, `Curve2DSample`, `CatmullRomCurve`, `CatmullRomSpline`; документация; строка `renames.md:98`; changelog | — | composition T3 (cubic-сегменты ключевых кадров, M-CRV-9) | — |

## Разрешённые файлы

| ID | Файлы |
|----|-------|
| T1 | `crates/flui-animation/src/curve.rs`, `src/lib.rs` (свои строки экспорта), `tests/contracts/curve.rs` (новый модуль `tests/main.rs`) |
| T2–T5 | `crates/flui-animation/src/curve.rs`, `tests/contracts/curve.rs`; T3 — ещё `benches/animation_bench.rs` (строка `curved_value`) |
| T7 | `packages/flui-cupertino/src/activity_indicator.rs` (ветка composition T6) |
| T9 | `crates/flui-animation/src/curve.rs`, `tests/contracts/curve.rs` |
| T6, T8 | `crates/flui-animation/{README.md,docs/*.md,src/curve.rs,src/lib.rs}`, `docs/plans/specs/naming/renames.md`, `changelog.d/<branch-slug>.md` |

## Definition of Done (каждая задача)

- Первый коммит PR — контракт и тесты, красные по assertion; вывод красного прогона в описании
  PR. Второй — поведение.
- Каждая строка-фикс падает при откате production-ханка в отдельном worktree по названной
  причине (C3/C4 — по величине |Δy|, C10 — по скачку 2⁻¹⁰, C11 — по 0 вместо 1, C12 — по
  `elements_built > 0` или неизменной видимости).
- Эталоны: market.md (бисекция), ручной расчёт CSS-алгоритма, аналитика; не формула production.
  Непроверенные [U]-факты ledger'ов контрактом не закрепляются.
- Удаление проверено против `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4); rg по удалённым
  именам в `crates/`, `packages/`, `src/`, `examples/`, `docs/` пуст; `packages/` мигрированы в
  том же PR.
- `cargo nextest run -p flui-animation` (и `-p flui-widgets -p flui-objects` для T7) зелёный;
  `cargo test --doc -p flui-animation`; `cargo xtask check-changed` зелёный один раз перед PR.
- T3: бенч `curved_value` до/после на одном хосте в PR.
