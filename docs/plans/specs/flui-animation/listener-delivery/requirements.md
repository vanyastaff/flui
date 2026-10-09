# listener-delivery — требования

- **Статус:** черновик · **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`; `file:line` — на этот коммит
- **Закрывает:** D-01, 02, 06, 07, 09, 10, 27, 28, 29, 44 ([../tasks.md](../tasks.md)); части D-28 и
  D-44 про `CompoundAnimation` снимает её удаление (controller-robustness T10).
- **Контракты:** ADR-0064 §4, ADR-0106, ADR-0109 §3, ADR-0125, ADR-0127; новый ADR — design.
- **Зависимость:** frame-path-state F2 (примитивы `share.rs`, `mutate`/`commit`) сливается раньше
  правки внутренностей; канал от вида хранения не зависит.

**Термины.** Канал статуса — очередь событий источника (контроллер, `ProxyAnimation`,
`AnimationSwitch`) и его подписчики; `Reverse`/`Curved`/`Tween` подписывают на канал родителя.
Фиксация — смена статуса под состоянием владельца и постановка в очередь; раздача — вызов
слушателей вне замков. Подписка — `#[must_use] StatusSubscription`, `Drop` снимает слушателя.

**Тесты** — публичный API, `crates/flui-animation/tests/contracts/status_delivery.rs` (модуль
`tests/main.rs` из Q0), строки `run_table`; deadlock/abort-строки — `child_process::run_rows`
(10 с). Время — явные `tick_at(secs)`/`Vsync::tick_all(now_secs)`. Эталон порядка — сценарий теста
(документированный статус каждого действия), не production-код. Property — `proptest` (Q0).

## Порядок доставки
**R1 (D-06).** КОГДА слушатель во время раздачи вызывает операцию, меняющую статус
(`forward`/`reverse`/`stop`/`animate_to*`/`set_value`/`dispose`), СИСТЕМА ДОЛЖНА поставить новый
статус в очередь и доставить его каждому слушателю только после того, как текущий статус доставлен
всем; каждый слушатель видит статусы в порядке фиксации; после затихания последний статус,
увиденный каждым живым слушателем, равен `status()`.
Тест: `status_delivery_order`, строки `restart_on_completed`, `stop_on_forward`,
`retarget_on_last_frame` (на `Completed` в кадре `tick_at(1.0)` слушатель вызывает
`animate_to(0.3)`), `set_value_on_dismissed`. Property: `status_order_property` — случайные
сценарии из ≤8 слушателей и действий {restart, stop, retarget, remove(self|other), add, dispose,
panic}; инварианты: последовательность каждого слушателя — подпоследовательность зафиксированной,
без повторов подряд, последний элемент = `status()`.

**R2 (ADR-0064 §4).** КОГДА новый прогон вытесняет прежний, в том числе из слушателя статуса,
СИСТЕМА ДОЛЖНА доставить статус нового прогона всем слушателям раньше, чем продолжения
`RunFuture` вытесненного прогона; первый payload паники продолжения возобновляется после того,
как очередь канала опустела, остальные удерживаются.
Тест: `status_delivery_order`, строки `nested_displacement_status_before_cancel`,
`two_failing_continuations_first_wins`.

**R3.** КОГДА слушатель подписывается во время раздачи, СИСТЕМА ДОЛЖНА не вызывать его со статусами,
зафиксированными до подписки (включая уже стоящие в очереди), и вызывать с каждым следующим.
Тест: `status_delivery_order`, строка `subscribe_during_delivery`.

**R4 (два источника).** КОГДА слушатель контроллера A из `Vsync::tick_all` перезапускает,
останавливает или освобождает контроллер B того же реестра, СИСТЕМА ДОЛЖНА доставить статусы B в
порядке фиксации B, а оставшиеся слушатели A — получить статус A. Порядок гарантирован внутри
канала; между каналами — вложенность вызовов.
Тест: `status_delivery_order`, строки `listener_restarts_sibling_later_in_walk`,
`listener_disposes_sibling`.

## Снятие и владение
**R5 (D-07).** КОГДА подписка снята (drop guard) или источник освобождён (`dispose`) во время
раздачи, СИСТЕМА ДОЛЖНА не вызывать этого слушателя ни с текущим, ни с последующими статусами;
после `dispose` не вызывается ни один слушатель, а ожидающие продолжения доставляются.
Подписка после `dispose` возвращает инертную `StatusSubscription`, колбэк уничтожается сразу,
вне замка (статусная часть controller-robustness R6.2 — здесь).
Тест: `status_listener_removal`, строки `drop_later_subscription`, `drop_own_subscription`,
`dispose_from_listener`, `dispose_from_listener_still_delivers_cancel`, `subscribe_after_dispose_is_inert`.

**R6 (D-27).** КОГДА подписка уничтожается, СИСТЕМА ДОЛЖНА снять ровно свою регистрацию; подписка
другой анимации, канала или повторно выданного места не затрагивается; drop после уничтожения
источника — no-op; `detach()` оставляет слушателя до `dispose`/уничтожения источника.
Счётчик мест не переполняется: исчерпание — постоянный отказ (panic после снятия замка, как
переполнение ёмкости `Vec`), место не выдаётся повторно.
Тест: `status_subscription_identity`, строки `drop_only_removes_own`, `foreign_animation_untouched`,
`drop_after_source_gone`, `detach_lives_until_dispose`; исчерпание — in-src
`status_channel::tests::exhausted_slots_refuse_permanently` (приватная граница счётчика, как
ADR-0125).

**R7 (D-28).** КОГДА слушатель подписан через `ReverseAnimation`, `CurvedAnimation` или
`TweenAnimation`, СИСТЕМА ДОЛЖНА держать его ровно столько, сколько живёт подписка; drop подписки
освобождает захваты, даже если родитель жив; замыкание, захватившее обёртку, не образует цикла.
`ReverseAnimation` отображает статус.
Тест: `status_subscription_identity`, строки `wrapper_capture_freed_on_drop` (счётчик drop),
`reverse_maps_status`.

**R8 (последний владелец).** КОГДА слушатель освобождает последний handle источника
(контроллер, proxy, switch), на который подписан, СИСТЕМА ДОЛЖНА завершить раздачу текущего статуса
остальным слушателям без panic и deadlock; захваты освобождаются после раздачи вне замков.
Тест: `status_listener_removal`, строки `listener_drops_last_{controller,proxy,switch}`.

## Отказы
**R9 (D-01, раунд и первый отказ).** КОГДА слушатель статуса паникует, СИСТЕМА ДОЛЖНА поймать
panic, удержать payload и конверт этого слушателя (ADR-0127), вызвать остальных слушателей с тем
же статусом, опустошить очередь канала и затем пробросить первый payload (`resume_unwind`) из
`forward`/`stop`/`tick_at`/`set_parent`; последующие payload — `tracing::error!` (текст через
`flui_foundation::panic::payload_text`, за отдельной границей unwind) и удержание. Переход
закоммичен до вызова. Упавший слушатель остаётся подписан и получает следующий статус; следующий
кадр тикает (решение владельца 2026-10-06; одна политика с send-flip T6b/R11).
Тест: `status_listener_failures` (child process), строки `panic_first_of_three`,
`every_listener_panics`, `panic_then_restart_next_frame`, `hostile_payload_and_capture_retained`,
`failing_log_subscriber_retained`.

**R10 (D-01, обход Vsync).** КОГДА `tick_at` контроллера паникует внутри `Vsync::tick_all`
(кривая, симуляция, продолжение, деструктор пользователя), СИСТЕМА ДОЛЖНА тикнуть остальные
контроллеры этого реестра, дочерних и родительского, затем возобновить первый payload; остальные
payload удерживаются и логируются; следующий `tick_all` тикает всех.
Тест: `vsync_walk_containment`, строки `panicking_curve_before_sibling`,
`child_registry_panic_parent_still_ticks`, `two_panics_first_wins`, `next_frame_ticks_all`
(время `0.0`, `0.05`, `0.1`).

## AnimationSwitch
**R11 (D-02).** КОГДА родитель switch'а (пользовательская `Animation`) из `value()`/`status()`/
`add_status_listener` обращается к самому switch'у (`value`, `status`, `current`, подписка, `Debug`),
СИСТЕМА ДОЛЖНА завершиться без deadlock и без panic заимствования.
Тест: `switch_contract` (child process), строки `parent_value_reads_switch`,
`parent_status_subscribes_switch`, `debug_reenters`.

**R12 (D-09).** КОГДА switch пересаживается на новый поезд, СИСТЕМА ДОЛЖНА сообщить слушателям
статус нового поезда, если он отличается от последнего сообщённого, ровно один раз; порядок —
статус → значение → `on_switched`.
Тест: `switch_contract`, строки `hop_to_completed_train_announces`, `hop_same_status_silent`,
`hop_order_status_value_callback`.

**R13 (D-44).** КОГДА тикает неактивный поезд без пересадки или меняется только статус, СИСТЕМА
ДОЛЖНА не уведомлять слушателей значения switch'а; при пересадке — одно уведомление, если значение
изменилось (сравнение по битам, NaN не повторяется).
Тест: `switch_contract`, строки `inactive_train_tick_silent`, `status_change_no_value_notify`.

**R14 (D-29).** КОГДА вызван `AnimationSwitch::dispose`, СИСТЕМА ДОЛЖНА сразу освободить
`on_switched`, слушателей статуса и подписки на оба поезда; повторный `dispose` — no-op; после него
нет уведомлений; цикл proxy → switch → `on_switched` → proxy (transition_route.rs:443-450)
освобождается без переподвешивания proxy.
Тест: `switch_contract`, строки `dispose_releases_callbacks` (счётчики drop),
`dispose_breaks_proxy_cycle`.

## ProxyAnimation
**R15 (D-10).** КОГДА `set_parent` возвращается, СИСТЕМА ДОЛЖНА гарантировать, что `value()`,
уведомления значения и статуса идут от одного родителя; события заменённого родителя, даже
находящегося посреди раздачи, игнорируются; при реентерабельном или конкурирующем `set_parent`
побеждает последний зафиксированный; новый статус объявляется один раз, если отличается от
последнего объявленного.
Тест: `proxy_binding`, строки `old_parent_events_ignored`, `set_parent_from_parent_status`,
`set_parent_from_listener`, `status_announced_once`; `concurrent_set_parent_converges` — пока
`Animation: Send + Sync` (удаляется вместе с ним в frame-path-state).

**R16.** КОГДА снятие подписки со старого родителя паникует в `set_parent`, СИСТЕМА ДОЛЖНА
сохранить зафиксированную замену, выдать уведомления значения и статуса, затем возобновить первый
payload. Тест: `proxy_binding`, строка `retirement_failure_still_notifies`.

## Жизненный цикл и время
**R17 (realm остановлен).** КОГДА `Vsync` уничтожен посреди прогона, СИСТЕМА ДОЛЖНА не выдавать
статусов сама; при последнем владельце — снять слушателей без вызова и доставить отмену; при живом
владельце — следующий `dispose` не вызывает слушателей. Тест: `status_delivery_lifecycle`, строки
`vsync_dropped_last_owner`, `vsync_dropped_owner_disposes_later`.

**R18 (время).** КОГДА `tick_at` повторяет время (dt = 0), получает огромное время (`1e300`),
время назад после завершения или `NaN`, СИСТЕМА ДОЛЖНА выдать каждый переход ровно один раз и не
выдавать статус без перехода. Неконечный `now_secs` реестра — тема motion-clock (D-33).
Тест: `status_delivery_lifecycle`, строки `repeated_time_no_duplicate`, `huge_dt_completes_once`,
`time_backwards_after_complete_silent`, `nan_time_no_flip`.

**R19 (замки).** СИСТЕМА ДОЛЖНА вызывать слушателей, продолжения и деструкторы колбэков без замков
и заимствований канала и владельца. Тест: строки R1, R3, R5, R11 (реентри из колбэка завершается).
