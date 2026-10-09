# controller-robustness — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования, design:** [requirements.md](requirements.md), [design.md](design.md). **База:** ветка Q0 (`controller/{mod,run,tick,status,dispose}.rs`, `tests/main.rs`, proptest); внутренности контроллера и `vsync.rs` правятся после слияния frame-path-state F2 (через `mutate`/`commit`/`share.rs`).

Общий DoD каждой задачи: тест, названный в требовании, красный при откате production-ханка (вывод
красного прогона — в PR); `cargo xtask check-changed` зелёный; новые `pub` достижимы из production;
нет новых `static`; фрагмент `changelog.d/` дополнен строками задачи; каждое удаление или
переименование сверено с `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4: `flui_sdk::animation` —
весь крейт, `flui-sdk/src/lib.rs:27`), `packages/` мигрированы в том же PR. Тема идёт первой вместе
с listener-delivery; факты `[U]` из ledger'ов не пинуются контрактными тестами до проверки.
`[P]` — параллельно после T1.

## T1. Контракт (первый коммит — инертные тела и красные тесты)

- Файлы: `error.rs`, `builder.rs`, `controller/mod.rs` (`builder()`, сигнатура `set_value -> Result`),
  `vsync.rs` (варианты ошибки, `attach_child -> Result`), `lib.rs` (свои строки),
  `tests/controller/robustness.rs`, `tests/vsync/admission.rs`, `tests/main.rs` (строки `mod`).
- `ValueRange`, `RangeDefect`, `AnimationInput`, структурный `AnimationError` (без
  `TickerNotAvailable`; сайты `controller.rs:590, 823, 930, 1224, 1411, 1431, 1442, 1597, 1619, 1630,
  1732, 2248, 2254`, `builder.rs:141` — поведение то же), builder (`build()` пока через старый
  конструктор). Таблицы `controller_robustness_contract`, `controller_builder_contract`,
  `vsync_admission_contract`, `animation_error_is_structured`, property R4.4/R5.1 — красные по
  assertion (вывод — в PR). Строка R1.2 на `main` строит контроллер на тестовом `UpdateScheduler` с
  `on_frame_scheduled`, зовущим `vsync.has_running()`: зависание в дочернем процессе — красный вывод.

## T2. [P] Curved-прогон: конечность и clamp (D-04)

- `controller/tick.rs` (ветка `Time`, `tick_time_based`; hunk согласовать с motion-clock — владелец
  файла). Hold при не конечном, clamp в границы, warn через латч. Тесты R4.1–R4.4, R9.2, R9.4.
- DoD: откат краснит `curved_run_holds_last_finite_value_on_nan_curve`, `prop_curved_run_value_is_finite_and_bounded`.

## T3. [P] plan/commit и тотальная длительность (D-26)

- `controller/run.rs`, `controller/mod.rs` (`scaled_run_duration`): `plan_*(&Inner) ->
  Result<RunPlan, _>` → `commit` → `finish`. Тесты R5.1, R5.2, R9.3.
- DoD: откат краснит `max_duration_run_starts_without_panic`, `refused_run_start_leaves_installed_run_untouched`.

## T4. [P] Dispose (D-31)

- `controller/dispose.rs`, `controller/status.rs` (только инертная регистрация; согласовать с
  listener-delivery); миграция 30 production-сайтов `set_value` (`flui-widgets` — через W2).
- Тесты R6.1–R6.4, `late_listener_drop_panics_outside_guard`.
- DoD: откат краснит `disposed_controller_refuses_set_value`, `dispose_clears_value_listeners`.

## T5. [P] Реестр Vsync (D-35)

- `vsync.rs`, `flui-widgets/src/animated/ticker_mode.rs:143`. Один родитель, подъём для `WouldCycle`,
  без стека пути (`debug_assert!` на подъёме), дедуп по `Arc::ptr_eq`, `register` паникует с упоминанием
  `try_register`. Тесты R7.1–R7.4.
- DoD: откат краснит `duplicate_registration_is_refused`, `nested_registry_ticks_once`.

## T6. [P] `is_animating` обязателен (D-11)

- `animation.rs`, `curved.rs`, `tween.rs`, `reverse.rs`, `proxy.rs`, `switch.rs`, `constant.rs` — по
  одному методу; прочие ханки этих файлов у listener-delivery (rebase через интегратора).
- DoD: откат краснит `wrappers_forward_is_animating`.

## T7. Перенос future завершения (зависит от T1)

- `flui-animation/src/completion.rs` из `flui-scheduler/src/ticker.rs:1323-1910` (правку
  flui-scheduler упорядочивает владелец motion-clock); `ticker_future_recovery.rs` →
  `flui-animation/tests/completion/` без изменения строк; 24 сайта `TickerFuture`/`TickerCanceled`
  (navigator, cupertino `button.rs`); `flui-sdk/tests/surface.rs:24`. Имя `RunFuture` — по ответу владельца.
- DoD: `run_future_contract` зелёный до и после (чистый перенос; в PR сказано, что красного нет).

## T8. Контроллер без Ticker, builder — единственный путь (D-32, D-34; зависит от T1, T6, T7)

- `controller/{mod,run,tick}.rs`: удалить `ticker`, `restart_ticker` (остаются счётчики),
  `warn_if_no_ticker`, `tick()`, восемь конструкторов; `run_generation` → `pub(crate)`;
  `is_animating` и `walk_probe` — один предикат. `builder.rs`: `build()` напрямую.
- Миграция 147 вхождений в 52 файлах (production — 19 сайтов в 15 файлах, design §6; `flui-widgets`
  — через W2); устаревшие комментарии из design §6. Тесты R1.1, R1.2, R1.5, R2, R3.1, R9.5.
- DoD: откат краснит `is_animating_tracks_installed_run` (строка бывшей `without_ticker`-формы) и
  `run_advances_only_through_tick_at`.

## T9. Удалить `Ticker` из flui-scheduler (зависит от T7, T8; исполняет владелец motion-clock)

- Дизайн — здесь (design §3, §6). `flui-scheduler/src/{ticker.rs,lib.rs}`, `scheduler.rs:82,
  3358-3370`, `tests/integration_tests.rs`, `examples/animation_ticker.rs`, `ARCHITECTURE.md`;
  `flui-foundation/src/id.rs:706-709`; doc-ссылки `flui-runtime` `pump.rs:15`, `ui_realm/pump.rs:77`,
  `flui-app` `frame_pacing.rs:426`.
- DoD: `rg "\bTicker\b"` — только проза о transient callbacks; `cargo xtask checks` и `globals` зелёные.

## T10. [P] Удаления по решению владельца (зависит от T1)

- `CompoundAnimation`/`AnimationOperator` (`compound.rs`, README/GUIDE/PERFORMANCE/ARCHITECTURE),
  `prelude` (`lib.rs:174-200`), реэкспорты `flui_scheduler` (`lib.rs:167-171`), `ALWAYS_*`
  (`constant.rs`, `globals` в `Cargo.toml:67-68`) с миграцией `transition_route.rs:59,72,78` →
  `ConstantAnimation::{completed,dismissed}`; сверка `flui-sdk/tests/surface.rs`.
- DoD: `cargo xtask facade-combos`, `cargo test -p flui-animation --doc` зелёные; changelog `### Removed`.

## T11. ADR и changelog (зависит от T8)

- `docs/adr/ADR-NNNN-vsync-sole-animation-clock.md` из design §7 (номер выдаёт интегратор);
  `Superseded-by` в ADR-0064 (§1, §6) и ADR-0125 (форма `attach_child`); `changelog.d/` из design §8;
  строка `## Mapping decisions` про hold NaN кривой (тест `curved_run_holds_last_finite_value_on_nan_curve`) — через Z.
- DoD: `cargo xtask checks` зелёный.
