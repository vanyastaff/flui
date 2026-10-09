# retarget — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); [requirements.md](requirements.md)
- **Порядок:** группа «остальное», после ownership (нужны `DrivenController` и якорь Vsync),
  physics (пружина, допуски покоя), curves (контракт `Curve`), listener-delivery. **Один PR на
  спеку:** задачи — коммиты ветки `animation/retarget`; [P]-задачи можно вести в отдельных
  worktree и вливать в ветку спеки. Каждое удаление или переименование `pub` (`AnimatedValue::advance`)
  сверяется с `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4); `packages/` — в том же PR. ID R/T —
  только в этом каталоге.
- **Готово для задачи:** каждый названный тест падает с откатом production-ханка (отдельный
  worktree, вывод в PR) по названной причине и проходит с ним; property-тесты — с зафиксированным
  seed в `.proptest-regressions`; эталоны — из `../market.md` или независимого метода в тесте;
  `[U]`-факты market не закрепляются; `cargo xtask check-changed` зелёный.

## Граф

```text
T1 контракт ─▶ T2 slope + velocity ─▶ T3 сегмент, retarget, якорь ─┬─▶ T4 AnimatedValue ─▶ T5 implicit-виджеты ─┐
                                                                    └─▶ T6 [P] fling_across: Dismissible, Drawer ─┼─▶ T8 ADR, docs, changelog
                                       T7 (решение владельца) back gesture ◀── T6 ────────────────────────────────┘
```

## Задачи

| ID | Работа | Требования | Файлы | Зависит | [P] |
|---|---|---|---|---|---|
| T1 | Контракт: `MotionSpec`, `retarget`, `fling_across`, новая форма `AnimatedValue` (тела инертны: `animate_to` → snap), `.spring()` у implicit-виджетов; все тесты R1–R21 красные по assertion | все | `flui-animation/src/{controller/run.rs,spring.rs,lib.rs}`, `tests/main.rs` + `tests/contracts/retarget.rs`; `flui-widgets/src/animated/*` (сигнатуры), `flui-widgets/tests/{contracts.rs,implicit_animations.rs,dismissible.rs}`; `packages/flui-material/tests/drawer.rs` | ownership T2, physics | — |
| T2 | `velocity()` curved-run через `Curve::slope` (D-36); сам `slope` вводит curves T9 первым (X13) | R1, R2 | `controller/run.rs` | T1, curves T9 | — |
| T3 | `Segment` (Spring \| Curve-Hermite) как run-simulation; захват шва вне lock с проверкой поколения; `RunAnchor::{Fresh,Continue}` в `walk_probe`; Vsync: номер кадра и последний тик регистрации; отказ неконечного до мутации | R3–R5, R7, R10–R16 | `controller/{run,tick,mod}.rs`, `vsync.rs` | T2 | — |
| T4 | `AnimatedValue<T>` на `DrivenController`: run-«часы», `SegmentCell` (realm-local по frame-path-state), §3.1 сокращение, `set_motion`, `snap_to`, `rebind`, `AnimatedValueView` (`Animation<T>`); удалить `advance`; `TwoWayConverter` для `EdgeInsets`, `Alignment`; бенч `animated_value_color_frame` через `tick_all` | R6, R8, R9, R17, R21 | `spring.rs`, `benches/animation_bench.rs` | T3 | — |
| T5 | Implicit: `ImplicitAnimation` → `AnimatedValue` для `TwoWayConverter`, прогресс + §3.1 для остальных; `ContainerGeometry` с derive; `AnimatedOpacity` → `AnimatedValueView<f64>` в `ProxyAnimation`; ужесточить `animated_opacity_retargets_from_the_current_value_midflight`; `ScrollController::animate_to` → `retarget` | R20, R21 | `flui-widgets/src/animated/{implicitly_animated,animated_opacity,animated_padding,animated_align,animated_container}.rs`, `scroll/scroll_controller.rs`, `tests/implicit_animations.rs` | T4 | ‖ T6 |
| T6 | `fling_across` в `Dismissible` (удалить `FLING_VELOCITY_SCALE`) и `DrawerController`; `InvalidExtent` через controller-robustness (`error.rs`) | R18 | `flui-widgets/src/interaction/dismissible.rs`, `packages/flui-material/src/drawer.rs`, тесты | T3 | ‖ T4, T5 |
| T7 | (если владелец утвердит) back gesture: `PopPacing` со скоростью, settle `fling_across` при `|v| ≥` порога | R19 | `flui-widgets/src/navigator/{back_gesture,binding,transition_route}.rs`, `tests/back_gesture.rs` | T6 | — |
| T8 | ADR «retarget сохраняет значение и скорость»; `changelog.d/<branch>.md`; строки `../market.md` M-INT-1/2/3/5/6, M-TIME-10 → «есть» со ссылкой на тесты; `flui-sdk/tests/surface.rs`; раздел для `docs/ARCHITECTURE.md` крейта — задаче Z | — | `docs/adr/ADR-NNNN-*.md`, `changelog.d/`, `crates/flui-sdk/tests/surface.rs` | T5, T6 | — |

### T1 — красный прогон
- `cargo nextest run -p flui-animation -E 'test(/retarget|slope|velocity|interruption|fling_across/)'`
  и `-p flui-widgets -E 'test(animation_retarget)'`; вывод — в описание PR.
- Ключевой красный ряд условия владельца:
  `repeated_interruption_keeps_position_and_velocity_continuous` падает на C¹ при инертном
  `animate_to` (snap) — первая точка шва в выводе.

### T3 — проверка «проходит в обе стороны»
- `the_frame_after_a_retarget_has_no_hold`: с откатом якоря на `Fresh` разность первого кадра
  равна 0 вместо `v(T)·dt` — падает.
- `retargeting_to_the_same_target_is_invisible`: с откатом захвата скорости (v = 0) значение в
  0.25 s отличается от −0.0233595799066923 на > 1e-3 — падает.

### T6 — проверка
- `a_dismissible_release_keeps_finger_speed_on_any_width`: с `1/300` на ширине 1200 px первая
  пиксельная скорость ≈ 6000 px/s вместо 1500 — падает.

## Риски порядка

- `curve.rs` правят curves и retarget: `slope` вносится после слияния curves, конфликт решает
  интегратор.
- `spring.rs` правит physics (D-39): T4 стартует от слитой physics.
- `AnimatedValue::advance` удаляется: бенч и `README.md` крейта (`:571`) обновляются в T4/T8.
