# ownership — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); [requirements.md](requirements.md)
- **Порядок:** группа «остальное» — после listener-delivery, controller-robustness, physics,
  curves, motion-clock и frame-path-state. **Один PR на спеку:** задачи — коммиты ветки
  `animation/ownership`; [P]-задачи можно вести в отдельных worktree и вливать в ветку спеки.
  Первый коммит — контракт T1, красный вывод в PR. Каждое удаление или переименование `pub`
  сверяется с `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4); `packages/` мигрируют в том же
  PR. ID R/T — только в этом каталоге, не в коде, именах тестов, коммитах.
- **Готово для задачи:** каждый названный тест падает с откатом production-ханка (прогон в
  отдельном worktree, вывод в PR) и проходит с ним; `check-changed` зелёный; rustdoc нового `pub`
  описывает контракт, ошибки, panics и пример.

## Граф

```text
T1 контракт ─┬─▶ T2 handle+Vsync ─┬─▶ T3 widgets: scope, implicit, TickerMode ─┐
             │                    ├─▶ T4 [P] scroll/navigator/interaction ─────┼─▶ T6 видимость register → pub(crate), docs, ADR
             │                    └─▶ T5 [P] packages + flui-objects ──────────┘
             └─▶ T7 (решение владельца) flui-view: activate пересматривает зависимости ─▶ R15
```

## Задачи

| ID | Работа | Требования | Файлы | Зависит | [P] |
|---|---|---|---|---|---|
| T1 | Контракт: `DrivenController` (инертные тела: `rebind` → `Ok(())`, Drop пуст), `builder.build_on(Option<&Vsync>) -> DrivenController`, `build()` (ручной), `AsRef` без `Deref`, `VsyncScope::maybe_of`, `VsyncScope::detached`; все тесты R1–R14, R16–R17 красные по assertion / stderr trybuild | все, кроме R15 | `flui-animation/src/controller/driven.rs`, `builder.rs`, `lib.rs` (строки реэкспорта), `tests/main.rs` + `tests/contracts/driven_controller.rs`, `tests/compile_fail/`; `flui-widgets/src/animated/vsync_scope.rs`; `flui-widgets/tests/{contracts.rs,ownership.rs}` | B, frame-path-state | — |
| T2 | Поведение handle: порядок освобождения, Drop под unwind (`Terminal`-custody payload), `rebind` с `resume_elapsed`, `ClockBinding::{Manual,Bound,Missing}`, settle без часов (R12–R13, через settle-путь reduce-motion) | R1–R3, R6–R10, R12, R13 | `controller/{driven,run,mod}.rs`, `vsync.rs` (якорь resume) | T1 | — |
| T3 | `VsyncScope::update_should_notify` по идентичности; `maybe_of`; `ImplicitController` и 4 implicit-виджета, `AnimatedSize`, `AnimatedSwitcher`, `TickerMode` (`detached`, удалить pass-through); удалить ложные docs (`vsync_scope.rs:21-23`, `animated/mod.rs:9-11`, `animated_opacity.rs:40-42`, `ticker_mode.rs:27-33,180-188`) | R11, R12 (widget), R14, R17 | `flui-widgets/src/animated/*` | T2 | ‖ T4, T5 |
| T4 | `Dismissible` (два handle, удалить танец unregister `:954-1012`), `TransitionRoute` (удалить `will_dispose_controller`), `Scrollable`, `RefreshIndicator`, `FloatingHeaderHost` (D-42a) | R4, R11 | `flui-widgets/src/{interaction/dismissible.rs,navigator/transition_route.rs,navigator/binding.rs,scroll/{scrollable,refresh_indicator,sliver_persistent_header}.rs}` | T2 | ‖ T3, T5 |
| T5 | Пакеты: `CupertinoButton`, `DrawerController`, `InkWell`, `ScaffoldMessenger` (D-42b); `RenderSliverFloating*::set_snap_controller` переносит слушателя (D-42c) + `harness_*` ряд | R3, R5 | `packages/flui-{cupertino,material}/src/*`, их `tests/`; `flui-objects/src/sliver/sliver_persistent_header.rs`, `tests/render_object_harness.rs` | T2 | ‖ T3, T4 |
| T6 | `Vsync::register/try_register/unregister` и `AnimationController::dispose` → `pub(crate)`; trybuild `driven_controller_has_no_bare_dispose`; миграция `flui-testing`, примеров, бенчей; trybuild R16; ADR; `changelog.d/<branch>.md`; `flui-sdk/tests/surface.rs`; раздел владения в `flui-animation/docs/ARCHITECTURE.md` передаётся задаче Z | R16 | `vsync.rs`, `flui-testing/src/*`, `examples/*`, `benches/*`, `docs/adr/ADR-NNNN-*.md`, `docs/adr/ADR-0125-*.md` (Superseded-by) | T3, T4, T5 | — |
| T7 | (если владелец утвердит) `on_activate` вызывает `did_change_dependencies`, когда у элемента есть inherited-зависимости; тест R15 | R15 | `flui-view/src/element/behavior.rs`, `flui-view/tests/` | T1 | ‖ T2–T5 |

### T1 — детали контракта
- Красный прогон: `cargo nextest run -p flui-animation -E 'test(/driven|rebind|unbound/)'` и
  `-p flui-widgets -E 'test(animation_ownership)'`; вывод — в описание PR.
- Тела инертны без `todo!`/`unimplemented!` (clippy). `Drop` в T1 пуст — R1 красный по
  assertion `has_running() == false`.
- Proptest-ряд R9 использует существующий `proptest` (добавляет Q0) с фиксированным seed в
  `.proptest-regressions`.

### T2 — детали
- Drop: `if std::thread::panicking()` — `catch_unwind` вокруг `controller.dispose()`, payload в
  custody без `Drop` (как `Terminal`, `animation.rs:167`); иначе panic распространяется после
  `Retired`.
- `resume_elapsed: Option<Duration>`: crate-private чтение снимка контроллера (`mutate`/`Published`
  frame-path-state) в `try_register`; `None`, если run нет.

### T4 — проверка отката
- `unmounting_a_floating_header_mid_snap_cancels_the_snap` с откатом (handle → голый контроллер
  без `dispose`) должен падать: future остаётся pending.
- После удаления танца Dismissible прогнать `crates/flui-widgets/tests/dismissible.rs` целиком
  (`cancelling_a_fully_slid_card_restores_it_without_dismissal` и соседи).

## Порядок слияния и общие файлы

`lib.rs`, `Cargo.toml` крейта — только свои строки; `flui-widgets/tests/contracts.rs` —
новое семейство `animation_ownership` добавляет T1, ряды — T3–T5. Миграция `flui-widgets`
идёт в этом же PR (общие файлы — через оркестратора).
