# frame-path-state — требования

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Решение владельца (2026-10-06):** замки на per-animation состоянии в пути кадра нарушают AGENTS.md
  «Frame path is synchronous» и чинятся в этом проходе ([../orchestration.md](../orchestration.md)).
- **Связанные:** [send-flip](../../send-flip/design.md) (строки 11–14, 17–22, 31; T4, T5, T6b, T6c),
  [controller-robustness](../controller-robustness/requirements.md), listener-delivery, ownership, retarget.

## Зачем

UI-состояние realm однопоточно (ADR-0027 §2, §9; ADR-0091 §1), но каждое чтение анимации в
build/layout/paint берёт замок (`controller.rs:2684-2691`; `proxy.rs:186`; `curved.rs:166`;
`switch.rs:444-451`), тик — 2–4 замка на контроллер, `has_running` — замок каждого контроллера
под замком реестра дважды за кадр (`vsync.rs:338-363`; `flui-runtime` `ui_realm/frame.rs:119`, `:923`).
`Send` вынужден хранением, а не вызовами; исключение из ADR-0027 записано только в rustdoc
(`controller.rs:227-230`) и `README.md:537-542`. Подробности с `file:line` — [design.md](design.md).

Тесты — через публичный API из `tests/main.rs` (после Q0); семейства — строки раннера Q0
(`run_cases`); строки, которые сегодня виснут, — через `tests/support/child_process.rs`
(`run_rows`: зависание = падение). Время — явные `tick_at`/`Vsync::tick_all` с виртуальными
секундами. `[prop]` — `proptest` (добавляет Q0). Эталоны — аналитические инварианты (концы
диапазона, равенство снимку после коммита), не производственная формула.

## R1. Принадлежность потоку

- **R1.1** КОГДА код пытается передать `AnimationController`, `Vsync`, `VsyncRegistration`,
  `ProxyAnimation<T>`, `CurvedAnimation<C>`, `TweenAnimation<T, A>`, `ReverseAnimation` или
  `AnimationSwitch` в другой поток, СИСТЕМА ДОЛЖНА отказать при компиляции (`!Send + !Sync`).
  Тест: модуль `thread_affinity` (`static_assertions::assert_not_impl_any!`, как
  `crates/flui-view/tests/signal_reads.rs:21`); trybuild `animation_controller_stays_on_its_thread`
  принадлежит send-flip T7 и здесь не дублируется. Сегодня — ошибка компиляции (красный).
- **R1.2** КОГДА пользователь реализует `Animation<T>` с `!Send` состоянием (`Rc<Cell<f64>>`),
  СИСТЕМА ДОЛЖНА принимать его родителем `ProxyAnimation`, `CurvedAnimation`, `TweenAnimation`,
  `ReverseAnimation`, `AnimationSwitch` и регистрировать его status-слушателя `Rc`-замыканием,
  захватившим `Rc`. Тест: строка `owner_local_parent_composes` (компиляция + чтение значения
  через каждую обёртку равно значению в ячейке).

## R2. Путь чтения

- **R2.1** КОГДА читается `value()`, `status()` или `is_animating()` контроллера, СИСТЕМА ДОЛЖНА
  отвечать из опубликованного снимка: без замка, без `RefCell`-заимствования, без атомарной
  RMW-операции, без аллокации и без вызова пользовательского кода. `velocity()` (вне paint-пути,
  читают retarget и fling) может вызвать `Simulation::dx`, но без занятого заимствования.
  Доказательство — тип (`Cell<Published>`, R1.1) и строки R2.3; стоимость — бенч R6.
- **R2.2** КОГДА читается обёртка, СИСТЕМА ДОЛЖНА держать `RefCell`-заимствование своего
  состояния только на время копирования `Rc` родителя и никогда — через вызов родителя, кривой или
  tween. Тест: строки R2.3 с пользовательским родителем, который из своего `value()` зовёт
  `set_parent`/`value()`/`add_status_listener` той же обёртки.
- **R2.3** КОГДА чтение происходит внутри любого вызова наружу — value-слушатель,
  status-слушатель, `Curve::transform` во время тика, `Simulation::x`, `Simulation::is_done`,
  продолжение future прогона, `Debug` контроллера изнутри слушателя, — СИСТЕМА ДОЛЖНА вернуть
  значение без паники `BorrowError` и без зависания; значение равно снимку последнего коммита.
  Тест: таблица `reads_inside_every_callout` через `run_rows` (строка на вызов × на обёртку:
  контроллер, Proxy, Curved, Tween, Reverse, Switch). Сегодня строка
  `custom_train_reads_switch` виснет (`switch.rs:294-295`, `:444-451`; D-02).
- **R2.4** КОГДА paint-путь читает `Proxy → Curved → Tween → Controller`, СИСТЕМА НЕ ДОЛЖНА
  аллоцировать. Тест: строка `paint_reads_allocate_nothing` в `[[test]] frame_path_allocation`
  (счётный `#[global_allocator]`, шаблон `crates/flui-scheduler/tests/frame_telemetry_allocation.rs`).
  Сегодня проходит — guard; мутация для доказательства: клон `Vec` слушателей в `value()`.

## R3. Согласованность снимка

- **R3.1** КОГДА коммит меняет значение, статус и признак прогона, СИСТЕМА ДОЛЖНА публиковать их одной
  записью до вызова слушателей: любой читатель в этом шаге видит поля одного коммита. `[prop]`
  Тест: `prop_listener_reads_see_one_commit` — случайная последовательность `forward`,
  `reverse`, `stop`, `set_value`, `animate_to`, `tick_at(t)` (t из `[-1, 10]`, повторы, убывание);
  value- и status-слушатели записывают `(value, status, is_animating)`; инварианты (эталон —
  аналитика): последняя запись слушателя в шаге равна чтению после шага; `Completed` ⇒
  `value == upper`, `Dismissed` ⇒ `value == lower` для ограниченного контроллера на
  естественном конце; `is_animating` ⇒ `status().is_running()`.
- **R3.2** КОГДА слушатель внешнего коммита запускает новый прогон (retarget в последнем кадре:
  `animate_to` из `Completed`), СИСТЕМА ДОЛЖНА оставить опубликованным снимок вложенного коммита;
  внешний коммит его не перезаписывает. Тест: строка `nested_commit_is_not_overwritten`
  (после кадра `status() == Forward`, значение — начало нового прогона).

## R4. Путь тика и реестр

- **R4.1** КОГДА кадр без изменений реестра и статусов тикает 1000 контроллеров и вложенный
  реестр (форма `TickerMode`), СИСТЕМА НЕ ДОЛЖНА аллоцировать ни в `has_running`, ни в
  `tick_all`. Тест: `steady_state_frame_allocates_nothing` (`frame_path_allocation`); сегодня
  красный — снимок детей `collect::<Vec<_>>()` в `vsync.rs:352-356`, `:442-450`.
- **R4.2** КОГДА `tick_all` вызывает `tick_at` или `has_running` читает контроллер, СИСТЕМА НЕ
  ДОЛЖНА держать заимствование реестра; признак «идёт прогон» и поколение прогона читаются из
  `Cell`, без заимствования состояния контроллера. Тест: строки `frame_path_reentry` ниже
  (вложенные `tick_all`, `register`, `unregister`, `set_muted` из слушателя).
- **R4.3** КОГДА меняется хранение, СИСТЕМА ДОЛЖНА сохранить семантику обхода ADR-0125
  (забор `fence`, порядок регистрации, «зарегистрирован во время обхода — со следующего кадра»).
  Тест: существующие `vsync_nesting_and_reentrancy` и строки ADR-0125 зелёные без правки assert.

## R5. Отказы (adversarial-матрица для хранения)

Таблица `frame_path_reentry` через `run_rows`; каждая строка дополнительно проверяет: нет
`BorrowError`/`BorrowMutError`, нет зависания, следующий `tick_all` тикает все живые контроллеры,
снимок конечен. Семантику доставки (порядок, первая паника) задаёт `listener-delivery`; здесь —
что хранение её не ломает.

- **R5.1** dispose во время тика: из value- и status-слушателя (`dispose_from_value_listener`,
  `dispose_from_status_listener`); контроллер не тикается со следующего кадра.
- **R5.2** слушатель перезапускает, останавливает, освобождает контроллер (`restart_from_listener`,
  `stop_from_listener`, `dispose_then_read_from_listener`).
- **R5.3** паника в value-слушателе, status-слушателе, `Curve::transform`, `Simulation::x`
  (`panic_in_<callout>_leaves_storage_usable`): после раскрутки ни одно заимствование не занято,
  контроллер принимает `stop`/`forward`, следующий кадр тикает остальные. Выходит ли паника
  слушателя наружу и какая первой — политика `listener-delivery` (R9, R10; открытое решение
  владельца в `review.md`); здесь строка проверяет только хранение.
- **R5.4** добавление и снятие слушателя во время уведомления, обоих каналов
  (`listener_add_remove_during_notification`).
- **R5.5** последний владелец отпущен из колбэка: `unregister` и drop последнего handle внутри
  слушателя (`last_owner_released_from_callback`): `Drop` ядра — после возврата шага обхода,
  future прогона — `Err(canceled)`, порядок по `DropLedger`.
- **R5.6** два контроллера на одном `Vsync`: слушатель A перезапускает, снимает с регистрации,
  освобождает B, регистрирует C, глушит реестр, вызывает вложенный `tick_all`
  (`two_controllers_one_vsync`, 6 строк; ожидания — controller-robustness R7.4).
- **R5.7** realm остановлен посреди анимации: `Vsync` заменён из слушателя
  (`vsync_replaced_mid_walk`) и уничтожен при живом handle виджета (`vsync_dropped_with_live_handle`):
  обход завершается, handle читает последний снимок, future pending до `dispose()`.
- **R5.8** время: `dt = 0`, `tick_at(f64::MAX)`, время назад, `NaN`/`±inf` в `tick_all`
  (`time_edges_keep_snapshot_finite`): снимок конечен, повтор того же времени не уведомляет
  дважды. Значения на этих входах — controller-robustness R9.4 и motion-clock.
- **R5.9** переполнение: поколение прогона и счётчик выборок не переиздают значение; на границе
  `u64::MAX` поколение отказывает навсегда (`expect("BUG: …")`), выборка сравнивается на
  равенство. Тест: in-src строка через приватный seed, как `vsync.rs:649`
  (`run_generation_exhaustion_is_permanent`).

## R6. Измерения

- **R6.1** СИСТЕМА ДОЛЖНА иметь бенч `benches/frame_path.rs` с группами `tick_all_running`,
  `has_running`, `paint_reads` (`Proxy → Curved → Controller`, `value()` + `status()`) и `frame`
  (`has_running` + `tick_all` + чтения + `has_running`, форма `frame.rs:119-120`, `:923`) на 1 000 и
  10 000 работающих контроллеров. «До» — на `main` после Q0, «после» — на ветке, один хост; PR
  приводит таблицу. Приёмка: каждая строка «после» быстрее «до»; R2.4 и R4.1 — ноль аллокаций.

## R7. Render objects

- **R7.1** КОГДА paint/compositing решение render object зависит от анимации, СИСТЕМА ДОЛЖНА
  брать значение из кэша, обновлённого слушателем (`RenderAnimatedOpacity`,
  `RenderSliverAnimatedOpacity`: `Rc<Cell<u8>>` вместо `Arc<AtomicU8>`), либо O(1) `value()`.
  Тест: строка `animated_opacity_paint_reads_the_cached_alpha` в `render_object_harness`
  (родитель-счётчик вызовов `value()`: paint, `paint_effects`, `skip_paint` его не вызывают).
  Сегодня проходит — guard; мутация: `paint_effects` читает `animation.value()`.

## R8. Миграция, ADR, документация

- **R8.1** СИСТЕМА НЕ ДОЛЖНА содержать `Arc<dyn Animation<` и `Arc<dyn Simulation` в `crates/`,
  `packages/`, `examples/`, `src/` (проверка — `rg` в PR и `owner-only` allowlist
  `thread-boundary` из send-flip).
- **R8.2** СИСТЕМА ДОЛЖНА иметь ADR «Состояние анимации принадлежит потоку realm»
  ([design.md](design.md), «Черновик ADR»), поправки к ADR-0064 и ADR-0125, без абзаца-исключения
  в `controller.rs` и с переписанными «Thread Safety» в `README.md` и `docs/ARCHITECTURE.md` крейта.
