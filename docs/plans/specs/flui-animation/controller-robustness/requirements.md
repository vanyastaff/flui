# controller-robustness — требования

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Закрывает:** D-04, D-11, D-26, D-31, D-32, D-34, D-35, D-47 ([../tasks.md](../tasks.md)).
- **Решения владельца (2026-10-06):** `flui_scheduler::Ticker` удаляется; `AnimationControllerBuilder`
  становится основным способом создать контроллер; `CompoundAnimation`, `prelude`, сквозные
  реэкспорты `flui-scheduler` и статики `ALWAYS_*` удаляются; новое per-controller состояние под
  `Mutex` не добавляется (совместимость с [../frame-path-state/](../frame-path-state/)).
- **Соседи:** раздача статуса и идентичность слушателей — [../listener-delivery/](../listener-delivery/);
  NaN/обратное время в `Vsync::tick_all`, time dilation, типизированный `tick_at` —
  [../motion-clock/](../motion-clock/); владеющий handle регистрации — `ownership` (W2).

Все тесты — через публичный API из `tests/` (бинарь `tests/main.rs` после Q0), таблицы — раннер
строк Q0 (`run_cases`); время — явный `tick_at`/`Vsync::tick_all` с виртуальными секундами, без
часов стены. Эталоны: аналитика (концы lerp, clamp), не производственная формула. `[prop]` —
property-тест на `proptest` (добавляет Q0).

## R1. Единственные часы

- **R1.1** КОГДА контроллер создан любым путём, СИСТЕМА ДОЛЖНА продвигать его только через
  `tick_at` (вызывает `Vsync::tick_all` или тест); в контроллере нет `Ticker`, `UpdateScheduler` и
  кода планировщика. Тест: компиляция миграции + `controller_robustness_contract::run_advances_only_through_tick_at`
  (без `tick_at` значение не меняется ни после `forward()`, ни после 10 пустых кадров `Vsync`).
- **R1.2** КОГДА стартует прогон, СИСТЕМА НЕ ДОЛЖНА вызывать чужой код (hook `on_frame_scheduled`,
  `tracing`-подписчик) под guard контроллера. Тест: строка `run_start_reenters_vsync_without_deadlock`
  — value- и status-слушатель, вызванный из `forward()`, зовёт `vsync.has_running()` и
  `vsync.tick_all(t)`; в дочернем процессе раннера `child_process::run_rows` (зависание = падение).
- **R1.3** КОГДА прогон стартует, СИСТЕМА ДОЛЖНА возвращать future завершения (`RunFuture`, бывш.
  `TickerFuture`) с прежним контрактом ADR-0064 §2–5 и ADR-0106: ровно одно разрешение, `Ok` при
  естественном конце, `Err(RunCanceled)` при вытеснении, `stop`, `dispose`, сбросе последнего
  владельца. Тест: перенос `ticker_future_recovery.rs` из flui-scheduler в таблицу
  `run_future_contract` без изменения строк.
- **R1.4** СИСТЕМА НЕ ДОЛЖНА иметь `AnimationController::tick()` (отматывал Vsync-прогон к `t = 0`, D-34); доказательство — удаление.
- **R1.5** КОГДА прогон стартует, СИСТЕМА НЕ ДОЛЖНА писать предупреждение «has no ticker»
  (сейчас пишется на каждый старт каждого production-контроллера, `controller.rs:2346`).

## R2. Конструирование

- **R2.1** КОГДА код создаёт контроллер, СИСТЕМА ДОЛЖНА давать один путь:
  `AnimationController::builder(duration)` с `reverse_duration`, `bounds(ValueRange)`,
  `unbounded()`, `initial_value`, и `build()` (`build_on(Option<&Vsync>) -> DrivenController` — ownership). Восемь конструкторов удаляются.
  Тест: `controller_builder_contract` (таблица: границы по умолчанию `[0, 1]`; `ValueRange::new`
  отказывает NaN, ±inf, `lower >= upper`, переполнение span — эталон: IEEE-754 `f64::MAX - (-f64::MAX) = inf`;
  `unbounded` → `(-inf, inf)`, старт `0.0`; `initial_value(NaN)` на ограниченном → нижняя граница
  с предупреждением, как `set_value`).

## R3. `is_animating`

- **R3.1** КОГДА у контроллера установлен прогон (`forward`, `reverse`, `animate_*`, `repeat*`,
  `fling*`, `animate_with*` вернули `Ok` и прогон не завершён), СИСТЕМА ДОЛЖНА отвечать
  `is_animating() == true`; после естественного конца, `stop`, `reset`, `set_value`, `dispose`,
  settle нулевой длительности/дистанции — `false`; заглушённый `Vsync` прогон не снимает
  (`true`). Тест: `is_animating_tracks_installed_run` (строка на каждый переход).
- **R3.2** КОГДА анимацию оборачивают `CurvedAnimation`, `TweenAnimation`, `ReverseAnimation`,
  `ProxyAnimation`, `AnimationSwitch`, СИСТЕМА ДОЛЖНА отвечать так же, как источник; `ConstantAnimation`
  — `false`. Метод становится обязательным в трейте (без default). Тест:
  `wrappers_forward_is_animating` (контроллер остановлен `set_value(0.5)` — сейчас обёртки
  отвечают `true`, D-11).

## R4. Curved-прогон

- **R4.1** КОГДА кривая возвращает конечное значение, выводящее `value` за `[lower, upper]`,
  СИСТЕМА ДОЛЖНА публиковать значение, зажатое в границы. Тест: `curved_run_clamps_overshoot_to_bounds`
  (кривая-заглушка `t ↦ 1.5`, `[0, 1]`, `tick_at(0.5)` → `1.0`; `t ↦ -0.5` → `0.0`).
- **R4.2** КОГДА выход кривой или итоговое `start + span·eased` не конечно, СИСТЕМА ДОЛЖНА
  оставить последнее опубликованное значение (без записи и без уведомления value-слушателей на этом
  кадре), предупредить один раз на контроллер, и продолжить прогон; на `t >= 1` — точная цель,
  статус `Completed`, future `Ok`. Тест: `curved_run_holds_last_finite_value_on_nan_curve`
  (кривая `NaN` в `(0.3, 0.7)`, кадры 0.2/0.5/1.0: значения `0.2`, `0.2`, `1.0`).
- **R4.3** КОГДА контроллер неограничен, СИСТЕМА ДОЛЖНА пропускать конечный overshoot без clamp.
  Тест: `curved_run_on_unbounded_controller_keeps_overshoot`.
- **R4.4** `[prop]` ДЛЯ любой кривой с произвольными `f64` (включая NaN/±inf/субнормали) и любого
  разбиения времени СИСТЕМА ДОЛЖНА держать `value` конечным и в границах после каждого кадра, а на
  последнем кадре — равным цели. Тест: `prop_curved_run_value_is_finite_and_bounded`.

## R5. Арифметика длительности и отказ до мутации

- **R5.1** КОГДА длительность (базовая, reverse, per-run, период repeat) равна `Duration::MAX` или
  любой другой, СИСТЕМА НЕ ДОЛЖНА паниковать в `forward/reverse/animate_*/repeat_with`; масштабированная
  длительность ≤ базовой. Тест: `max_duration_run_starts_without_panic` (строка на метод);
  `[prop]` `prop_scaled_run_duration_never_exceeds_base` (эталон: `fraction·base ≤ base` при
  `fraction ∈ [0,1]`).
- **R5.2** КОГДА старт прогона отказан (`Disposed`, не конечный вход, плохой диапазон, пружина),
  СИСТЕМА ДОЛЖНА оставить контроллер ровно в прежнем состоянии: значение, статус, установленный
  прогон, его future (не разрешён), следующий кадр продолжает старый прогон. Тест:
  `refused_run_start_leaves_installed_run_untouched` (строка на каждую причину отказа, середина
  прогона `[0,1]` за 1 с).

## R6. Dispose

- **R6.1** КОГДА контроллер освобождён, СИСТЕМА ДОЛЖНА отказывать `set_value` с
  `AnimationError::Disposed`, не меняя значение и не уведомляя. Тест: `disposed_controller_refuses_set_value`.
- **R6.2** КОГДА после `dispose` добавляют value-слушателя, СИСТЕМА ДОЛЖНА уронить
  колбэк сразу, вне lock, вернуть id, не совпадающий ни с одним живым, и никогда его не вызвать
  (без паники ни в debug, ни в release). Status-слушатель после `dispose` — инертная
  `StatusSubscription` по listener-delivery R5 (канал статуса принадлежит той теме). Тест: `disposed_controller_drops_late_listeners`
  (колбэк держит `Arc`-сторож; `strong_count` возвращается к 1 до выхода из `add_*`).
- **R6.3** КОГДА контроллер освобождён, СИСТЕМА ДОЛЖНА очистить оба канала слушателей, разорвав
  циклы «слушатель держит контроллер». Тест: `dispose_clears_value_listeners` (через `Weak`
  контроллера: после `dispose` и drop внешнего handle `Weak::upgrade` = `None`).
- **R6.4** КОГДА `dispose` вызван из value- или status-слушателя посреди `tick_all`, СИСТЕМА ДОЛЖНА
  завершить кадр без deadlock и panic, отменить future, а следующий `tick_all` — не тикать этот
  контроллер и тикать остальные. Тест: `dispose_from_listener_during_tick` (строки: value, status).
- **R6.5** КОГДА последний владелец контроллера отпускается из его колбэка, СИСТЕМА ДОЛЖНА
  разрушить состояние после возврата из `tick_at`, без удержания lock. Тест: существующая строка
  terminal-owner матрицы `controller_sources_allow_reentry_and_preserve_run_ownership` (переносится Q0).

## R7. Реестр Vsync

- **R7.1** КОГДА `attach_child` замкнул бы цикл или ребёнок уже имеет родителя, СИСТЕМА ДОЛЖНА
  отказать типизированной ошибкой (`WouldCycle`, `AlreadyAttached`). Тест: `vsync_admission_contract`
  (строки: сам в себя, потомок, второй родитель, переподвешивание после `detach_child`).
- **R7.2** КОГДА реестр прикреплён к ребёнку, СИСТЕМА ДОЛЖНА тикнуть каждый реестр не более раза
  за вызов `tick_all`. Цикл после `WouldCycle` непредставим (один родитель, один поток; после
  send-flip T6c реестр `!Send`), поэтому стека пути нет; подъём несёт `debug_assert!`.
  Тест: строка `nested_registry_ticks_once` в `vsync_admission_contract`.
- **R7.3** КОГДА тот же контроллер регистрируют в том же реестре повторно или того же ребёнка
  прикрепляют повторно, СИСТЕМА ДОЛЖНА отказать `AlreadyRegistered` / `AlreadyAttached`, оставив
  первую регистрацию; `register` паникует с сообщением, называющим `try_register`. Тест: строки
  `duplicate_registration_is_refused`, `duplicate_child_is_refused` (контроллер тикается один раз
  за кадр: value-слушатель вызван ровно 1 раз).
- **R7.4** КОГДА два контроллера в одном `Vsync` и слушатель A перезапускает, останавливает или
  освобождает B, СИСТЕМА ДОЛЖНА: B, идущий позже, — переякорить и тикнуть в этом кадре от `t = 0`;
  B, уже пройденный, — тикнуть со следующего кадра. Тест: `two_controllers_one_vsync` (3 строки).

## R8. Ошибки

- **R8.1** КОГДА операция отказывает, СИСТЕМА ДОЛЖНА возвращать `AnimationError` со структурными
  полями, без `String`; `TickerNotAvailable` удаляется. Тест: `animation_error_is_structured`
  (строка на каждую точку отказа, `matches!` по полям; `Display` содержит значения).

## R9. Отказы и время (сценарии adversarial-матрицы)

- **R9.1** Panic в status-слушателе посреди тика: future прогона `Ok`, следующий кадр тикает
  остальные контроллеры — владелец [../listener-delivery/](../listener-delivery/); здесь только
  проверка, что R4/R6 не ломают строку `a_panicking_status_listener_leaves_the_finished_run_ok`.
- **R9.2** Panic в `Curve::transform`: поднимается без lock, прогон остаётся, следующий кадр
  вызывает кривую снова. Тест: `curve_panic_leaves_controller_usable` (после panic `stop()` и
  `forward()` работают).
- **R9.3** Retarget на последнем кадре: `animate_to(x)` из status-слушателя `Completed` ставит новый
  прогон; старый future `Ok`, новый — pending; следующий кадр якорит новый прогон. Тест:
  `retarget_from_completed_listener`.
- **R9.4** dt = 0: два `tick_at(t)` подряд дают одно значение. Огромный dt: `tick_at(f64::MAX)`
  завершает прогон в цель. Время назад: `tick_at(0.6)` после `0.8` даёт значение кадра `0.6` (чистая
  функция). Тест: `time_edges_on_curved_run` (3 строки; NaN/inf в `tick_all` — motion-clock).
- **R9.5** Realm остановлен посреди анимации (дроп `Vsync`): контроллер, которого держит виджет,
  сохраняет прогон (`is_animating() == true`), future pending до `dispose()`, затем `Err(RunCanceled)`.
  Тест: `dropped_vsync_leaves_run_until_dispose` — пинует честное поведение (см. риски design).
