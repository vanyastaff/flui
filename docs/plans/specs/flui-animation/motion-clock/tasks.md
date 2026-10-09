# motion-clock — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования / design:** [requirements.md](requirements.md), [design.md](design.md).
- **Порядок тем:** listener-delivery и controller-robustness → physics и curves → **motion-clock**
  → остальные (reduce-motion строится на шве `FrameTick` отсюда). База ветки — `main` после
  слияния controller-robustness (builder, контроллер без `Ticker`, Q0-раскладка `controller/` и
  `tests/main.rs`). Один PR на тему; задачи — коммиты/ветки внутри неё по решению оркестратора.
- **Файлы против [../tasks.md](../tasks.md) (C):** платформы, `flui-app`, `media_query.rs` ушли в
  reduce-motion; добавлены `motion.rs`, `vsync.rs`, `controller/mod.rs`, `flui-testing`, агентский
  путь и SnackBar. Правки `flui-scheduler` — только здесь (T6).

Общий DoD: тест, названный в требовании, красный при откате production-ханка (вывод красного
прогона — в PR); `cargo xtask check-changed` зелёный; каждый новый `pub` достижим из production
(runtime, `flui-testing`, devtools, material); нет новых `static` и `Mutex`; ID R/T/D — только в
этом каталоге. `[P]` — параллельно после своих зависимостей.

## T1. Контракт: типы с инертными телами, сигнатуры, красные тесты

- Файлы: `crates/flui-animation/src/{motion.rs (новый), lib.rs (свои строки), vsync.rs,
  controller/{mod,tick}.rs}`, `tests/main.rs` (строка `mod motion_clock`), `tests/motion_clock.rs`,
  `tests/support/` (помощник `tick`), `benches/{vsync_registry,animation_bench}.rs`, все вызовы
  `tick_all(`/`tick_at(` (design «Миграция»; пересчитать `rg` на базе), `flui-testing/src/{lib.rs,
  widgets/host.rs}`, `flui-runtime/src/{presentation.rs,ui_realm/frame.rs}` (поле `MotionClock`,
  чеканка тика).
- Что: `AnimationTime`, `PlaybackRate`, `InvalidPlaybackRate`, `FrameTick`, `MotionClock`
  (инертно: `frame` = сырое время, `set_rate`/`step` хранят и не влияют), `tick_all(&FrameTick)`,
  `tick_at(Duration)`, `set_playback_rate`/`playback_rate` (инертно). Поведение кадра не
  меняется (dilation ещё делится в `tick.rs`).
- Тесты красные по assertion: R2 (обе), R4, R5, R6, R7, R11-R14, R19-R21, R24.
- DoD: красный вывод в PR; `cargo nextest run -p flui-animation -p flui-testing -p flui-runtime`
  компилируется; все прочие тесты зелёные (миграция чисто механическая).

## T2. [P] `MotionClock` (зависит от T1)

- Файлы: `motion.rs`.
- Ребейз на `last_raw`, `step`, монотонность, насыщение (I1, I2).
- DoD: зелёные R2 (вкл. PB), R4 (часы), R5, R7; откат ребейза краснит строку 1→5.

## T3. [P] Реестр и контроллер: типизированное время, скорость на анимацию (зависит от T1)

- Файлы: `vsync.rs` (якоря `AnimationTime`, последний тик, `has_running` без пауз),
  `controller/{tick,mod}.rs` (удалить деление на `time_dilation`, `cycle_elapsed_secs`; поля
  скорости по форме frame-path-state; pending-модель I4; `velocity` I8; `walk_probe` I5).
- DoD: зелёные R4 (контроллер), R6, R11-R14, R19-R21, R24; откат pending-модели краснит
  `paused_run_holds_through_any_gap`; откат I5 краснит `paused_run_requests_no_frames`.

## T4. Runtime: часы презентации, скрытость, пауза (зависит от T2, T3)

- Файлы: `flui-runtime/src/{presentation.rs, ui_realm/{frame.rs, frame_clock.rs, presentations.rs}}`,
  тесты `ui_realm/tests/frame_pipeline_and_vsync.rs` (+ правки тестов на `set_now_for_test`).
- Одно сырое время на кадр; `frame()` и отпускание borrow до `tick_all` (I6); скрытая — не
  тикается и не заявляет спрос; на паузе — нет спроса `Animation`; `step` заявляет кадр;
  `set_now_for_test(Duration)` + граница `try_from_secs_f64` у оставшихся `f64`-входов.
- DoD: зелёные R3 (runtime), R8, R9, R10, R22, R23; откат пропуска скрытых краснит R10 (статус
  доставлен до показа); `cargo nextest run -p flui-runtime` целиком.

## T5. [P] `flui-testing`: часы презентаций (зависит от T2)

- Файлы: `flui-testing/src/lib.rs`, `tests/multi_presentation_clock.rs`.
- `with_motion_clock`, `with_presentation_motion_clock`; `pump_frame`/`pump_presentation` чеканят тик
  через свои часы.
- DoD: зелёные R1, R17; откат (тик сырым временем) краснит R1.

## T6. `flui-scheduler` одной последовательностью (зависит от T3; controller-robustness T7, T8 слиты)

- Файлы: `flui-scheduler/src/{config.rs, scheduler.rs, lib.rs}`, `Cargo.toml` (запись `globals`
  `config::TIME_DILATION`), затем ханки `flui-scheduler` из controller-robustness T9 (`ticker.rs`,
  `lib.rs`, `scheduler.rs:82, 3358-3370`, `tests/integration_tests.rs`, `examples/animation_ticker.rs`,
  `ARCHITECTURE.md`) — по её design, без изменения решения.
- DoD: `rg "time_dilation|TIME_DILATION|reset_epoch|adjust_for_epoch|current_frame_time_stamp"
  crates packages src examples tools` пуст (кроме ADR/plans); `cargo xtask globals` зелёный без
  записи (R18); DoD controller-robustness T9 (`rg "\bTicker\b"`) выполнен.

## T7. [P] Devtools: операция `motion` (зависит от T4)

- Файлы: `flui-protocol/src/` (новые `MotionRequest`, `MotionState`, wire-тесты),
  `flui-view/src/{dev_agent.rs, __runtime.rs}` (`AgentWindow::motion`, `AgentPort::motion`),
  `flui-runtime/src/ui_realm/agent.rs` (`DevAgentPort::motion` → inbox → часы презентации),
  `packages/flui-devtools/src/agent/session.rs` (op `motion`), `packages/flui-devtools/tests/`,
  `flui-runtime/src/ui_realm/tests/agent_admission.rs`, `flui-sdk/tests/surface.rs` (если
  `AgentWindow::motion` идёт через SDK-реэкспорт `dev_agent`).
- DoD: зелёные R16; невалидная скорость → `invalid_request`; закрытое окно → `gone`; Windows
  named-pipe — локальный прогон в PR (в CI только clippy).

## T8. [P] SnackBar: пауза таймера под указателем (зависит от T3)

- Файлы: `packages/flui-material/src/{scaffold_messenger.rs, snack_bar.rs}`,
  `packages/flui-material/tests/scaffold.rs`.
- `MouseRegion` вокруг SnackBar → `set_playback_rate(PAUSED | NORMAL)` на контроллере показа +
  `rebuild.schedule`.
- DoD: зелёный R15; откат краснит его (закрытие под указателем).

## T9. ADR, docs, changelog (зависит от T2-T8)

- `docs/adr/ADR-NNNN-presentation-motion-clock.md` из design «Черновик ADR» (номер — у
  интегратора); статус ADR-0097 — «`TIME_DILATION` удалён по ADR-NNNN»;
  `changelog.d/<branch-slug>.md` из design; `flui-runtime/ARCHITECTURE.md` — строки «Unasserted»
  про порядок Vsync заменить ссылками на R9/R10; `crates/flui-animation/docs/*` — через задачу Z.
- DoD: `cargo xtask checks` зелёный (markers, docs-links, changelog).
