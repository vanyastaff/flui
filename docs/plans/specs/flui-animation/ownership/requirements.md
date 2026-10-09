# ownership — владеющий handle контроллера (требования)

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`; `file:line` — на этот коммит
- **Закрывает:** D-21, D-22, D-42; ручные пары `register`/`unregister`/`dispose` у 12 владельцев
  (список — [design.md](design.md) «Миграция»); строки рынка M-OWN-2, M-OWN-4
  ([../market.md](../market.md))
- **Зависит от:** controller-robustness (builder — основной способ создать контроллер, `Ticker`
  удалён, Vsync — единственные часы), frame-path-state (состояние анимации realm-local, без
  per-animation `Arc<Mutex>`), listener-delivery (раздача статуса и сдерживание panic),
  reduce-motion (её settle-путь переиспользуется для «нет часов»), motion-clock (допустимый `now`)
- **Связанная спека:** [../retarget/requirements.md](../retarget/requirements.md) (якорь
  `Continue` в Vsync общий)

## Проблема

Контроллер и его место в `Vsync` живут раздельно: владелец хранит пару
`Option<Vsync>, Option<VsyncRegistration>` и вручную пишет `register` в `init_state`,
`unregister` + `dispose` в `dispose` (12 файлов). Ошибки уже случились:
`FloatingHeaderHostState::dispose` не вызывает `dispose()` контроллера
(`crates/flui-widgets/src/scroll/sliver_persistent_header.rs:292-301`);
`ScaffoldMessenger::start_display_timer` перезаписывает регистрацию без снятия старой
(`packages/flui-material/src/scaffold_messenger.rs:549-569`);
`RenderSliverFloating*::set_snap_controller` меняет контроллер, не перенося слушателя
(`crates/flui-objects/src/sliver/sliver_persistent_header.rs:1067-1069`, подписка только в
`attach` `:1289-1295`). `VsyncScope::update_should_notify` всегда `false`
(`crates/flui-widgets/src/animated/vsync_scope.rs:76-80`), потребители читают scope через `get`
без зависимости — смена реестра замораживает все контроллеры (D-22). Без `VsyncScope` implicit-виджет
не анимируется вовсе, а документация обещает «свой scheduler-ticker»
(`vsync_scope.rs:21-23`, `animated/mod.rs:9-11`, `ticker_mode.rs:180-196`) (D-21).

## Термины

- **Handle** — `DrivenController`: контроллер плюс его регистрация в `Vsync`, единственный
  способ поставить контроллер на часы (имя — [design.md](design.md)).
- **Без часов (unbound)** — handle, у которого нет реестра (нет `VsyncScope` выше).
- **Тест через публичный API**, время — явные `Vsync::tick_all(now)`; ряды — в таблицах
  `run_table` (`crates/flui-animation/tests/main.rs` после Q0) и `run_cases`
  (`crates/flui-widgets/tests/contracts.rs`, семейство `animation_ownership`).

## Требования

### Владение и освобождение

- **R1.** КОГДА handle уничтожается (drop) или вызван его `dispose`, СИСТЕМА ДОЛЖНА сначала
  снять регистрацию, затем вызвать `dispose` контроллера; ни один guard реестра или контроллера
  не удерживается во время доставки `RunCanceled` и retire пользовательских замыканий.
  Тест: `dropping_a_driven_controller_unregisters_then_cancels_its_run` (run_table): после drop
  `Vsync::has_running()` = false, future → `Err(RunCanceled)`, continuation вызывает
  `vsync.tick_all` и `builder.build_on(Some(&vsync))` без deadlock.
- **R2.** КОГДА `dispose` вызван повторно или после него следует drop, СИСТЕМА ДОЛЖНА ничего не
  делать (идемпотентно). Тест: `dispose_then_drop_is_one_retirement` (счётчик continuation = 1).
- **R3.** КОГДА владелец заменяет handle в поле (новый поверх старого), СИСТЕМА ДОЛЖНА освободить
  старый как в R1. Тест (D-42b): `a_second_display_timer_retires_the_first`
  (`packages/flui-material/tests`, через публичный `ScaffoldMessenger`): два `Completed`-перехода
  без `cancel_display_timer` оставляют в реестре одну регистрацию и одну rebuild-подписку.
- **R4.** КОГДА `FloatingHeaderHost` размонтирован посреди snap, СИСТЕМА ДОЛЖНА отменить run
  контроллера (D-42a). Тест: `unmounting_a_floating_header_mid_snap_cancels_the_snap`
  (`run_cases`): future snap → `Err(RunCanceled)` в кадре размонтирования.
- **R5.** КОГДА render-object плавающей шапки получает новый snap-контроллер в состоянии
  attached, СИСТЕМА ДОЛЖНА перенести слушатель `mark_needs_layout` на новый, а `detach` снимает
  его с текущего (D-42c). Тест: `swapping_the_snap_controller_moves_the_layout_listener`
  (`render_object_harness`, ряд `harness_*`): тик старого не помечает layout, тик нового помечает.
- **R6.** КОГДА пользовательский код из status-, value-слушателя или continuation этого же
  контроллера роняет последнего владельца handle (state, захваченный через `Rc<RefCell<_>>`),
  СИСТЕМА ДОЛЖНА завершить текущий `tick_all` без panic и deadlock; остальные контроллеры кадра
  тикают. Тест: `dropping_the_last_owner_from_its_own_listener_mid_frame` (run_table, два handle
  на одном Vsync, первый роняет себя и второй; второй в этом кадре пропущен, в следующем не тикает).
- **R7.** КОГДА continuation, выполняемая из `dispose`/drop, паникует, СИСТЕМА ДОЛЖНА к моменту
  выхода panic уже снять регистрацию и пометить контроллер disposed; при drop во время unwind
  внешний panic остаётся первым, внутренний payload удерживается без вызова его `Drop`, процесс
  не abort. Тест: `a_panicking_continuation_in_drop_keeps_the_first_failure_first` (дочерний
  процесс `support/child_process.rs`, проверка exit status и stderr).

### Привязка к часам

- **R8.** КОГДА вызывается `rebind(Some(v))` с тем же реестром (`Vsync::is_same`), СИСТЕМА ДОЛЖНА
  ничего не менять (регистрация, якорь, статус). Тест: `rebinding_to_the_same_registry_is_a_no_op`.
- **R9.** КОГДА `rebind` переносит handle с идущим run в другой реестр, СИСТЕМА ДОЛЖНА сохранить
  прошедшее время run: первый тик нового реестра продолжает с elapsed последнего сэмпла (C⁰,
  без отката к началу run). Property-тест (proptest, run_table):
  `rebinding_mid_run_keeps_the_value_continuous` — случайные duration, момент переноса, разные
  базы `now` у реестров; эталон — линейный run `start + (target − start)·t/D`, вычисленный в тесте.
- **R10.** КОГДА реестр исчерпал идентичности (ADR-0125), СИСТЕМА ДОЛЖНА вернуть из `rebind`
  `Err(VsyncRegistrationError::Exhausted)` и оставить handle без часов (R12); прежний реестр
  освобождён. Тест: in-`src` ряд `rebind_refused_by_an_exhausted_registry_settles_unbound`
  (приватный сид счётчика, как `vsync_nesting_and_reentrancy`).
- **R11.** КОГДА `VsyncScope` перестраивается с другим реестром, СИСТЕМА ДОЛЖНА уведомить
  зависимых (`update_should_notify` = идентичность изменилась), а каждый мигрированный потребитель
  — перепривязаться в `did_change_dependencies`. Тест: `swapping_the_scope_registry_moves_every_consumer`
  (`run_cases`): дерево с `AnimatedOpacity`, `AnimatedSize`, `AnimatedSwitcher`, `Scrollable`
  (fling), `Dismissible`, плавающей шапкой; после смены реестра старый `len()` = 0, все run
  продолжаются от нового `tick_all`.
- **R12.** КОГДА handle без часов запускает run (или теряет часы посреди run), СИСТЕМА ДОЛЖНА
  завершить run тем же settle-путём, что reduce-motion R2–R4 (time-based — цель, статус и `Ok`
  ровно один раз; конечный repeat — конец последнего плеча, `Ok`; бесконечный repeat — паркуется
  в начале первого плеча, future не разрешается до `rebind` на часы; simulation — по R13), и
  залогировать один `warn` на handle. Тест: `an_unbound_controller_settles_every_run_kind_at_once` (таблица по видам
  run) и `an_implicit_widget_without_a_scope_lands_on_its_target` (`run_cases`).
- **R13.** КОГДА без часов завершается simulation, СИСТЕМА ДОЛЖНА взять первое `t` из ряда
  0.25 s·2ⁿ (до 64 s), где `is_done(t)`, иначе последний конечный сэмпл; NaN не публикуется.
  Тест: строка в таблице R12 с `FrictionSimulation` (эталон `final_x` по формуле Flutter из
  market-B P11) и с симуляцией, которая не завершается никогда.
- **R14.** КОГДА `TickerMode` не имеет `VsyncScope` выше, СИСТЕМА ДОЛЖНА всё равно отдать
  поддереву свой (неподключённый) реестр: потомки становятся «без часов» по R12, а не
  «замороженными». Ложные обещания fallback удалены из `vsync_scope.rs`, `animated/mod.rs`,
  `animated_opacity.rs`, `ticker_mode.rs`. Тест: `ticker_mode_without_a_scope_does_not_freeze_descendants`.
- **R15.** КОГДА поддерево с анимированным виджетом переносится GlobalKey под другой `TickerMode`,
  СИСТЕМА ДОЛЖНА перепривязать контроллер к новому реестру (D-3 зоны 7). Тест:
  `reparenting_across_ticker_mode_moves_the_registration`. Требует, чтобы активация элемента
  после переноса вызывала `did_change_dependencies` при смене провайдера (решение владельца,
  design «Открытые вопросы»).

### Тип как правило

- **R16.** СИСТЕМА ДОЛЖНА делать ручную регистрацию контроллера непредставимой вне
  `flui-animation`: `Vsync::register`/`try_register`/`unregister` — `pub(crate)`; контроллер на
  часах получается только из builder (`build_on(vsync)`). Доказательство: компиляция workspace и
  trybuild-фикстура `registering_a_bare_controller_does_not_compile`
  (`crates/flui-animation/tests/compile_fail/`).
- **R17.** СИСТЕМА ДОЛЖНА получать реестр для handle только в `init_state`/
  `did_change_dependencies`: `VsyncScope::maybe_of(&dyn LifecycleContext)` регистрирует
  зависимость (ADR-0078 §1). Доказательство: trybuild `vsync_scope_maybe_of_in_build_does_not_compile`.

### Граничные и отказные сценарии (сводка)

| Сценарий | Поведение | Тест |
|---|---|---|
| dispose во время тика (из value-слушателя) | run отменён, остаток кадра тикает | `disposing_from_a_value_listener_mid_tick` |
| слушатель перезапускает/останавливает контроллер | handle не участвует; порядок — listener-delivery | ссылка на listener-delivery |
| panic в слушателе | первый отказ первым, следующий кадр тикает | listener-delivery; R6 повторно с panic |
| добавление/снятие слушателя во время уведомления | listener-delivery | — |
| два handle на одном Vsync, один роняет другой | R6 | R6 |
| realm остановлен посреди анимации | teardown дерева роняет handle → run `Err(RunCanceled)`, реестр пуст | `stopping_the_realm_mid_animation_cancels_every_run` (flui-runtime `tests/`) |
| dt = 0, огромный dt, время назад, NaN `now` | неконечный `now` непредставим (`FrameTick` motion-clock R8); время назад держит `MotionClock` (R5); якорь R9 насыщается | `rebinding_mid_run_keeps_the_value_continuous` (строки с dt=0 и шагом назад) |
| переполнение счётчика регистраций | R10 | R10 |
