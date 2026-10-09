# motion-clock — часы анимации презентации, скорость воспроизведения, гигиена времени кадра

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Закрывает:** D-25, D-33 ([../tasks.md](../tasks.md)); строки [../market.md](../market.md)
  M-TIME-2, M-TIME-3, M-TIME-5, M-TIME-7, M-TIME-9 (время на границе контроллера и реестра),
  M-TIME-12 (скорость на анимацию), M-INTG-9 (slow-motion из devtools).
- **Не здесь:** reduce motion (D-05, M-A11Y-1…5) — отдельная спека
  `docs/plans/specs/flui-animation/reduce-motion/`; эта спека даёт ей шов: `FrameTick` (design.md,
  «Шов для reduce-motion»).
- **Контракты:** [ADR-0097](../../../../adr/ADR-0097-no-process-global-state-gate.md) (выход
  `TIME_DILATION`), [ADR-0027](../../../../adr/ADR-0027-owner-affine-ui-realms.md) §8,
  [ADR-0125](../../../../adr/ADR-0125-vsync-registration-authority.md),
  [ADR-0095](../../../../adr/ADR-0095-agent-protocol-schema-crate.md); новый ADR — design.md.
- **Порядок:** после listener-delivery, controller-robustness (builder, без `Ticker`), physics и
  curves. Состояние контроллера — по frame-path-state
  (`docs/plans/specs/flui-animation/frame-path-state/`): новых `Mutex` эта спека не вводит.

## Сценарии

1. Разработчик в devtools ставит окну скорость 0.1 посреди перехода маршрута: переход идёт дальше
   с текущего кадра в 10 раз медленнее, без отката; скорость 0 — пауза, «шаг» — ровно +16 ms.
2. Указатель над SnackBar: таймер показа стоит (скорость контроллера 0); указатель ушёл — таймер
   продолжает с того же остатка (`flui-material`, production-потребитель скорости на анимацию).
3. Окно свёрнуто на 10 минут посреди анимации и развёрнуто: первый кадр показывает состояние,
   как если бы время шло (догон); статусы и futures доставлены ровно один раз в этом кадре.
4. Тест гоняет анимацию на виртуальных часах со скоростью 2 и `step(16 ms)` — детерминированно.

## Требования

Тесты — через публичный API: `MotionClock`/`Vsync`/`AnimationController` — модуль `motion_clock`
в `crates/flui-animation/tests/main.rs` (раннер крейта после Q0); `HeadlessBinding` —
`crates/flui-testing/tests/main.rs`; внутренности realm (у него нет внешнего API кадра) —
`crates/flui-runtime/src/ui_realm/tests/frame_pipeline_and_vsync.rs`. Время только виртуальное:
`MotionClock::frame(raw)` с явными `Duration`, `ManualClock`. Эталоны — аналитика: время =
Σ Δraw_i · rate_i в целых наносекундах, линейная интерполяция tween без кривой, фаза repeat по
модулю; не production-формула. **PB** — property-based (proptest из Q0). Факты `[U]` из
market.md контрактными тестами не закрепляются.

### Часы презентации

- **R1.** КОГДА две презентации одного realm имеют разную скорость, СИСТЕМА ДОЛЖНА анимировать
  их с разной скоростью от одного сырого времени кадра (верификация ADR-0097).
  Тест: `two_presentations_with_different_rates_animate_at_different_rates`
  (flui-testing, модуль `multi_presentation_clock`).
- **R2.** КОГДА скорость презентации меняется, СИСТЕМА ДОЛЖНА ребейзить эпоху на последнем сыром
  времени: время анимации непрерывно, интервал между последним кадром и сменой не теряется.
  Тесты: `rate_change_rebases_without_a_jump` (строки 1→5, 1→0.1, 1→0, 0→1, смена до первого
  кадра; со старым кодом строка 1→5 даёт откат 0.5 → 0.1); **PB**
  `timeline_is_the_integral_of_rate_over_any_frame_partition`.
- **R3.** КОГДА скорость презентации 0, СИСТЕМА ДОЛЖНА заморозить время, не заявлять спрос кадра
  `Animation` и не держать цикл кадров; `step(dt)` ДОЛЖЕН сдвинуть время ровно на `dt` при любой
  скорости и запросить один кадр. Тесты: строка `paused_clock_steps_exactly`;
  `a_paused_presentation_requests_no_animation_frames` (runtime).
- **R4.** КОГДА скорость (презентации или анимации) отрицательна, NaN или ±inf, СИСТЕМА ДОЛЖНА
  отвергнуть её (`InvalidPlaybackRate::{Negative, NonFinite}`) без изменения состояния.
  Тест: `invalid_rates_are_refused_and_leave_state_unchanged` (таблица: часы, контроллер).
- **R5.** КОГДА сырое время кадра меньше предыдущего, СИСТЕМА ДОЛЖНА выдать предыдущее время
  анимации (dt = 0): значения и статусы контроллеров не меняются. Повтор того же сырого времени
  — тот же тик. Тест: `backwards_raw_time_holds_the_timeline` (назад на 1 ns, на 10 s, к нулю,
  повтор).
- **R6.** КОГДА реестр получает тик раньше последнего принятого (два источника часов на одном
  `Vsync`, как `adopt_vsync` в тестах), СИСТЕМА ДОЛЖНА трактовать его как последний.
  Тест: строка `a_registry_ticked_by_two_clocks_never_regresses`.
- **R7.** КОГДА Δraw · rate или сумма переполняет `Duration`, СИСТЕМА ДОЛЖНА насыщаться на
  `Duration::MAX`, не паниковать и завершать конечные запуски одним переходом статуса.
  Тест: `huge_frame_gaps_saturate_and_complete_runs_once` (Δ = 10⁶ s, `Duration::MAX`, rate 1e300).
- **R8.** Время на входе контроллера и реестра ДОЛЖНО быть типизированным (`Duration`,
  `FrameTick`); единственная `f64`-граница — тестовый override realm — ДОЛЖНА отвергать NaN, ±inf и
  отрицательное (`Duration::try_from_secs_f64`) и не тикать (D-33). Тест:
  `a_non_finite_test_time_is_refused_and_the_run_keeps_running` (runtime; со старым кодом NaN
  якорил запуск, `has_running()` оставался истинным навсегда).
- **R9.** КОГДА realm рисует кадр, СИСТЕМА ДОЛЖНА взять одно сырое время на все презентации и
  выдать всем контроллерам одной презентации один `FrameTick`. Тест:
  `every_controller_of_a_frame_samples_one_tick` (runtime).
- **R10.** КОГДА презентация скрыта или кадры приложения выключены, СИСТЕМА ДОЛЖНА не тикать её
  `Vsync` и не учитывать её запуски в спросе кадров; первый видимый кадр ДОЛЖЕН сэмплировать
  время, включающее скрытый интервал (догон), и доставить статусы и futures ровно один раз.
  Тест: `a_hidden_presentation_catches_up_on_its_first_visible_frame` (runtime; строки: tween
  завершился, repeat в фазе по модулю, симуляция; хук «кадров нет» — `frames_enabled = false`).

### Скорость на анимацию (M-TIME-12)

- **R11.** КОГДА скорость контроллера меняется посреди запуска, СИСТЕМА ДОЛЖНА применить её с
  ближайшего тика, сохранив локальное время запуска на этом тике: время до тика идёт по старой
  скорости, после — по новой (WAAPI «pending playback rate, current time preserved»). Для всех
  видов запуска: tween, кривая, repeat, симуляция. Тест: **PB**
  `controller_local_time_is_the_integral_of_its_rate` (эталон — сумма по отрезкам);
  `rate_change_mid_run_keeps_value_continuous` (таблица по видам запуска).
- **R12.** КОГДА скорость контроллера 0, СИСТЕМА ДОЛЖНА держать значение, статус «running», не
  учитывать запуск в `Vsync::has_running()` и не тикать его; КОГДА скорость снова > 0, запуск
  ДОЛЖЕН продолжиться с того же локального времени, сколько бы времени презентации ни прошло.
  Тест: строки `paused_run_holds_through_any_gap`, `paused_run_requests_no_frames`.
- **R13.** Скорость контроллера ДОЛЖНА переживать перезапуски (новый запуск стартует с текущей
  скоростью) и компоноваться со скоростью презентации умножением. Тест: строки
  `rate_survives_restart`, `controller_and_presentation_rates_compose`.
- **R14.** `velocity()` ДОЛЖЕН сообщать скорость в единицах значения за секунду времени
  презентации (производная по локальному времени × скорость контроллера; 0 при паузе).
  Тест: строка `velocity_scales_with_rate` (эталон — аналитическая производная tween без кривой).
- **R15.** КОГДА указатель над SnackBar, СИСТЕМА ДОЛЖНА остановить таймер показа; когда ушёл —
  продолжить с остатка. Тест: `snack_bar_display_timer_pauses_while_hovered`
  (`packages/flui-material/tests/scaffold.rs`; со старым кодом SnackBar закрывается под указателем).

### Devtools, тесты, удаление глобального состояния

- **R16.** КОГДА агент devtools шлёт операцию `motion` (скорость, шаг), СИСТЕМА ДОЛЖНА применить её
  к часам названного окна через owner-inbox и ответить состоянием; невалидная скорость —
  `invalid_request`, закрытое окно — `gone`. Тесты: `agent_motion_sets_rate_and_steps_a_paused_window`
  (runtime `agent_admission`), `motion_op_round_trips_over_the_endpoint` (flui-devtools tests).
- **R17.** `HeadlessBinding` ДОЛЖЕН давать часы своей и каждой установленной презентации (скорость,
  шаг). Тест: R1 и `headless_step_advances_only_the_named_presentation`.
- **R18.** СИСТЕМА НЕ ДОЛЖНА содержать процесс-глобального коэффициента времени: `TIME_DILATION`, его
  API в `flui-scheduler` и запись `globals` удалены. Доказательство: `cargo xtask globals` зелёный
  без записи; R1.

### Отказы и реентерабельность

- **R19.** КОГДА слушатель во время тика делает dispose контроллера, меняет его скорость,
  перезапускает его, делает `animate_to` на последнем кадре, снимает регистрацию или отпускает
  последнего владельца, СИСТЕМА ДОЛЖНА: dispose/unregister — не тикать его дальше в этом кадре;
  скорость и retarget — применить с **следующего** тика; drop владельца — без use-after-free и
  panic (обход держит клон на шаг). Тест: `listener_reentry_during_tick` (таблица по действиям).
- **R20.** КОГДА пользовательский код паникует внутри `tick_all`, СИСТЕМА ДОЛЖНА оставить часы уже
  продвинутыми (тик выдан и borrow часов отпущен до вызова), первый panic остаётся первым,
  следующий кадр — с монотонным временем. Сдерживание обхода — listener-delivery (D-01).
  Тест: строка `panic_in_listener_keeps_the_clock_monotone`.
- **R21.** КОГДА два контроллера одного `Vsync` с разными скоростями, СИСТЕМА ДОЛЖНА тикать оба
  одним `FrameTick`, каждый — со своей скоростью, в порядке регистрации. Тест: строка
  `two_controllers_with_different_rates_share_one_tick`.
- **R22.** КОГДА realm останавливается посреди анимации, СИСТЕМА ДОЛЖНА не тикать после
  `Stopping`; `MotionClock` не держит обязательств доставки, его drop инертен. Тест:
  `a_stopping_realm_ticks_no_presentation` (runtime).
- **R23.** КОГДА `step` или смена скорости презентации приходят во время обхода (агент через
  inbox, тест из слушателя через `HeadlessBinding`), СИСТЕМА ДОЛЖНА применить их к следующему
  кадру. Тест: строка `step_during_a_tick_applies_next_frame`.
- **R24.** КОГДА dt = 0 (повторное время), СИСТЕМА ДОЛЖНА пересэмплировать то же значение без
  смены статуса. Тест: строка `zero_dt_resamples_the_same_value`.
