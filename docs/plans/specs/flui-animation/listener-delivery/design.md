# listener-delivery — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)

## Итог

Один крейт-приватный механизм — `StatusChannel`: очередь событий источника (статус или
`RunDelivery`, бывш. `TickerDelivery`), раздача FIFO из самого внешнего кадра, живой поиск слушателя перед каждым
вызовом; panic слушателя сдерживается, раунд дорабатывает, первый payload пробрасывается после
раунда (решение владельца 2026-10-06, одна политика с send-flip T6b/R11; заменяет ADR-0109 §3 для
статуса). Канал есть у тех, кто
владеет статусом: `AnimationController`, `ProxyAnimation`, `AnimationSwitch`. Обёртки без состояния
(`Reverse`, `Curved`, `Tween`) подписывают на канал родителя. Слушатель живёт столько, сколько его
`#[must_use] StatusSubscription`; `ListenerId` и `remove_status_listener` уходят из канала статуса.
`Vsync::tick_all` сдерживает panic каждого `tick_at` и возобновляет первый после обхода.
`StatusChannel` — `pub(crate)`: production-потребителя вне крейта нет.

## Текущее состояние (чтением)

- Контроллер: `take_status_change` двигает маркер под замком (controller.rs:2469-2480), `finish`
  отпускает замок, уведомляет значения, затем `fire_status` по снимку без `catch_unwind`
  (2372-2376, 2403-2423). Panic обрывает хвост, маркер уже сдвинут — переход потерян (D-01).
  Вложенный `finish` из слушателя раздаёт новый статус, затем внешний цикл раздаёт старый (D-06).
  Снятые и освобождённые слушатели вызываются из снимка (D-07). Id — `next_listener_id: 1` и
  `+= 1` (658, 2696-2697) (D-27).
- Proxy: свой реестр и `fan_out_status` без сдерживания и проверки членства (proxy.rs:19-46);
  `set_parent` меняет три `RwLock` по очереди (193-209) (D-10); при panic `retirement.finish()`
  (221) уведомления 222-225 теряются.
- Switch: `last_status` фиксируется до раздачи (switch.rs:259-260), раздача без сдерживания
  (270-272); `value()`, `status()`, `Debug` и обработчик значения зовут родителей под своим
  `Mutex` (294-295, 445, 450, 494-498) (D-02); пересадка не объявляет статус (313-337) (D-09);
  `last_value` мёртв (78-79), статус дёргает уведомление значения (269) (D-44); `dispose` держит
  `on_switched` и слушателей (402-430) (D-29).
- Обёртки отдают id родителя: compound.rs:216-223, reverse.rs:91-108, curved.rs:191-197,
  tween.rs:111-116; `ConstantAnimation` — глобальный счётчик (constant.rs:15, 114).
- `Vsync::tick_all`: дети, затем свои (vsync.rs:462-511) без сдерживания: panic пропускает
  остаток кадра и, из детского реестра, всех родительских.
- Сверка (первичные источники прочитаны `gh api` 2026-10-06; контрактом не являются — тесты
  пинят поведение, выведенное здесь, а не «как во Flutter»): Flutter `notifyStatusListeners` (`animation/listener_helpers.dart`) — снимок, пропуск
  снятых (`contains`), `catch` + `reportError`, продолжение; очереди нет, порядок при реентри тот же,
  что у нас сейчас. `TrainHoppingAnimation._valueChangeHandler` (`animation/animations.dart`) при
  пересадке зовёт `_statusChangeHandler(_currentTrain.status)`, уведомляет значение только при
  изменении, затем `onSwitchedTrain`. GPUI `Subscription` (`crates/gpui/src/subscription.rs`) —
  `#[must_use]`, `Drop` отписывает, `detach()` оставляет.

## Варианты

**(a) Точечные заплатки:** `catch_unwind` и проверка членства в трёх циклах, `checked_add` у id.
Дёшево; не чинит порядок (D-06), идентичность (D-27), времена жизни (D-28); три копии политики.

**(b) Общий `StatusChannel` у владельцев статуса + RAII-подписка.** Очередь закрывает D-06 и
порядок ADR-0064 при вложенности; живой поиск — D-07; одна политика panic — D-01; guard держит
`Weak` своего канала — чужую регистрацию снять нельзя по построению (D-27), время жизни слушателя
определяет подписчик (D-28). Цена: ломающее изменение трейта, 10 production-мест.

**(c) `flui_foundation::Notifier<AnimationStatus>` + внешняя очередь.** Готовые сдерживание и
порядок регистрации, но: снимок после `dispose` дочитывается до конца (notifier_generic.rs:319-326,
противоречит R5), нет элемента «доставка», хранение `Arc`+`Mutex`+`AtomicBool` зашито
(конфликт с frame-path-state), id — тот же `ListenerId` с 1.

**Выбор — (b).** Контракт брифа «канал у шести типов» уточнён: `CompoundAnimation` удаляется
(controller-robustness T10), а `Reverse`/`Curved`/`Tween` своего статуса не имеют — их канал был бы лишним
форвардером на родителе. С RAII-подпиской делегирование больше не держит слушателя дольше
подписчика, поэтому D-28 закрывается без собственного канала.

**Идентичность:** типизированный `StatusListenerId { registry, slot }` с `remove(&id)` (как токены
ADR-0125) оставляет ручную пару add/remove, а 5 из 10 production-подписок id выбрасывают
(scaffold_messenger.rs:562, 703; ink_well.rs:536; drawer.rs:742; transition_route.rs:646), то есть
живут до `dispose` контроллера или вечно. Guard заставляет решить: хранить или `detach()`.

## Хранение (зависимость от frame-path-state)

Логика канала — `StatusQueue`, обычная структура без внутренней изменяемости и без вызовов
пользовательского кода: слушатели `Vec<(Slot, StatusCallback)>` по возрастанию `Slot`, счётчик
мест, `VecDeque<Outbound>`, флаг `draining`, флаг `disposed`. Канал — одна ячейка вокруг неё:

```rust
// примитивы — из приватного `share.rs` frame-path-state F2 (один модуль на крейт):
use crate::share::{Shared, StateCell};   // F2: Arc / Mutex; F3: Rc / RefCell
struct Channel { queue: StateCell<StatusQueue> }   // подписка держит `Weak` от `Shared<Channel>`
```

Весь доступ — `fn with<R>(&self, f: impl FnOnce(&mut StatusQueue) -> R) -> R`; `f` не вызывает
пользовательский код и возвращает изъятые колбэки наружу, поэтому замок и `borrow_mut` имеют одну
и ту же область, и реентерабельный вызов никогда не застаёт их занятыми. Своих псевдонимов канал не
заводит: переход на `Rc`/`RefCell` — правка `share.rs` в F3. Авто-трейты `StatusSubscription`
следуют за `Animation` (`Send + Sync` сейчас, `!Send` после). Связка proxy и
состояние switch устроены так же (одна ячейка, вызовы вне `with`).

Правило заимствования (edition 2024): скрутини `match` держит guard во всех ветках, поэтому
владелец сначала копирует (`let st = self.with(|q| q.snapshot());`), затем `match st`; `if let`
освобождает временное до `else`, но let-chain `if let Some(cb) = q.take() && …` держит его во
всей then-ветке — колбэк никогда не вызывается из then-ветки такого `if let`.

**Порядок замков до F3** (пока `StateCell` — `Mutex`): владелец (контроллер/proxy/switch) →
канал; канал — листовой, под ним ничего не захватывается и никто не вызывается. Реестр `Vsync` →
контроллер (обход), обратного ребра нет. После F3 — те же рёбра для `borrow_mut`.

## Раздача

```text
enqueue(item)            // под замком/заимствованием владельца, канал — листовой
  Status(s): push (s, fence = next_slot)   Delivery(d): push d     // после dispose Status → drop
drain()                  // после того как владелец отпустил своё состояние
  if !with(|q| take draining token) { return }      // внешний кадр уже раздаёт
  loop:
    item = with(|q| pop_front, и если пусто — draining = false атомарно с pop)
    Status(s, fence): cursor = 0
      loop: (slot, cb) = with(|q| первый слушатель со slot > cursor и slot < fence) else break
            cursor = slot; first.run(|| cb(s))    // Arc-клон, вызов без замков
    Delivery(d): first.run(|| d.deliver())        // ADR-0106: хвост уже доставлен внутри deliver
  first.finish()          // первый payload (слушатель или продолжение) — resume; при panicking — retain
```

- `first.run`: `catch_unwind`; первый payload сохраняется, последующие — `tracing::error!` за
  отдельным `catch_unwind` и `retain_opaque_payload`; с первой ошибки до конца раздачи снятые
  колбэки удерживаются, а не уничтожаются (ADR-0127). Раунд дорабатывает (каждый слушатель
  получает статус), очередь опустошается, затем первый payload пробрасывается `resume_unwind` из
  вызова, начавшего раздачу (`forward`/`stop`/`tick_at`/`set_parent`). Переход закоммичен до
  раздачи; следующий кадр тикает.
- Прогон, завершающийся синхронно внутри `drain` того же канала (нулевая длительность, нет часов),
  не исполняется внутри неё: settle откладывается на следующий тик (решение по X3); очередь кадра
  конечна, принятая работа не отбрасывается.
- Guard токена сбрасывает `draining` при unwind из кода фреймворка, чтобы канал не заклинило;
  оставшиеся элементы доставит следующая раздача или `Drop` (`RunDelivery` доставляет в `Drop`).
- Дедуп статуса — у владельца при фиксации (маркер контроллера, `last_announced` у proxy и switch).
- Межпоточность (только пока хранение `Arc`+`Mutex`): элементы, поставленные потоком B, пока
  раздаёт A, раздаёт A; `forward()` на B может вернуться до доставки. После frame-path-state
  вопрос исчезает.

Контроллер: `finish` = под замком `take_status_change` → `enqueue(Status)`, затем
`enqueue(Delivery)`; отпустить; уведомление значений под `catch_unwind` (первая ошибка сохраняется);
`drain`; уничтожение `retired`; resume первой ошибки. Значения остаются немедленными
(`ChangeNotifier`): значение — состояние, его читают через `value()`; порядок гарантирован между
статусами и доставками одного канала.

## Proxy и switch

**Proxy.** Одна ячейка `Binding { parent, value_sub, status_sub, generation: u64, events: u64,
last_announced }`. `set_parent(p)`: (1) `with`: `g = generation + 1` зарезервировать; (2) вне
ячейки подписать значение и статус на `p` с форвардерами, несущими `g`; (3) `with`: если
резерв `g` всё ещё последний — заменить всё, `generation = g`, взять старое; иначе выбросить своё
(победил более поздний вызов); (4) вне ячейки снять старое через `Retirement`, уведомить значение,
прочитать `p.status()`; (5) `with`: если `generation == g` и `events` не менялся с шага 3 и статус ≠
`last_announced` — зафиксировать и `enqueue`; `drain`; (6) `retirement.finish()` последним (R16).
Форвардер: `with` — `generation == g`, иначе игнор; `events += 1`; дедуп; `enqueue`; `drain`.

**Switch.** Ячейка `SwitchState { current, next, mode, generation, last_value_bits, last_announced,
on_switched, subs, disposed }`; родителей никогда не зовёт внутри `with` (включая `value()`,
`status()`, `Debug`: клон `Arc` поезда, вызов снаружи). Обработчик значения: снимок
`(current, next, mode, generation)` → значения снаружи → `with`: поколение то же и условие
пересадки → зафиксировать пересадку, `generation += 1`, изъять старые подписки → снаружи снять
старые, подписать статус нового поезда, допустить по поколению → `new.status()` снаружи → `with`:
≠ `last_announced` → `enqueue` → `drain` (статус) → значение `current.value()` снаружи, уведомить
при `to_bits` ≠ (значение) → `on_switched` (колбэк). Форвардер статуса значение не уведомляет.
`dispose`: `with` — `disposed`, изъять `on_switched`, подписки, `channel.dispose()`; снаружи
`Retirement`. Идемпотентен.

**Vsync.** `tick_all`: каждый `child.tick_all` и каждый `tick_at` — под `catch_unwind` через
существующий `Retirement`-подобный `first`; последующие payload логируются и удерживаются; после
своего обхода — resume первого. Замок реестра при вызове не держится (как сейчас, vsync.rs:509).

## Инварианты

1. Слушатель вызывается только если на момент вызова зарегистрирован и подписан до фиксации
   события (`slot < fence`).
2. Внутри канала события раздаются в порядке фиксации; статус нового прогона — до доставки
   вытесненного (ADR-0064 §4) при любой вложенности.
3. Ни замок, ни заимствование канала, владельца или реестра не удерживаются при вызове слушателя,
   продолжения, `Curve`/`Simulation`, деструктора пользовательского значения.
4. Panic слушателя статуса, продолжения и `tick_at` в обходе сдерживается до конца раунда; первый
   payload пробрасывается после завершения раздачи/обхода, остальные удерживаются (ADR-0127).
5. `Slot(NonZero<u64>)` монотонен в пределах канала (`checked_add`; исчерпание — вечный отказ, `u64::MAX` не выдаётся).

## Публичный API

```rust
pub trait Animation<T> { /* … */
    /// Subscribes to status transitions committed after this call, in commit order.
    /// The listener runs with no lock held, may re-enter the animation, and is never
    /// called after its subscription drops or the source is disposed. A panic in the
    /// listener does not stop the round: every other listener gets the status, then the
    /// first panic resumes from the call that started the delivery.
    fn add_status_listener(&self, callback: StatusCallback) -> StatusSubscription;
    // `remove_status_listener` удалён
}

#[must_use = "dropping a StatusSubscription removes the listener; call `detach` to keep it"]
pub struct StatusSubscription { /* Weak<канал>, Slot */ }
impl StatusSubscription {
    /// Keeps the listener until the source is disposed or dropped.
    pub fn detach(self);
    pub(crate) fn inert() -> Self;     // ConstantAnimation, подписка после dispose
}
impl Drop for StatusSubscription;      impl fmt::Debug for StatusSubscription;
// Drop: изъять колбэк в `with`, уничтожить после отпускания ячейки (деструкторы захватов — код
// пользователя); под unwind — удержание (ADR-0127). Сам `Drop` не паникует.

pub(crate) struct StatusChannel;       // контроллер, proxy, switch
impl StatusChannel {
    pub(crate) fn new() -> Self;
    pub(crate) fn subscribe(&self, callback: StatusCallback) -> StatusSubscription;
    pub(crate) fn enqueue(&self, item: Outbound);   // под состоянием владельца
    pub(crate) fn drain(&self);                     // после отпускания состояния
    pub(crate) fn dispose(&self);
}
```

`StatusChannel` и `StatusSubscription::inert` — `pub(crate)` (правило AGENTS «Unwired surface»:
production-реализаций `Animation` вне крейта 0). Сторонняя `Animation` возвращает подписку
делегированием: хранит `ProxyAnimation`/контроллер и отдаёт его `add_status_listener` (так устроены
обе тестовые реализации `tests/contracts/{proxy,status_delivery}.rs`). Публичный канал вводится
только вместе с названным внешним потребителем (ADR). Запечатать `Animation` (R3 аудита
абстракций, trybuild `animation_is_sealed.rs`) — **решение владельца**, этот design его не
принимает и от него не зависит. Удаляются: `Animation::remove_status_listener`; `StatusCallback`
остаётся. Модуль — `src/status_channel.rs`, реэкспорт `StatusSubscription` в `lib.rs`.

Две модели подписки на одном трейте (значения — `ListenerId` из `Listenable`, статус — RAII)
остаются до сквозной правки foundation (тот же RAII-токен для значений; send-flip T5 правит
`Listenable`) — отмечено в «Остаточных рисках».

## Миграция (rg `add_status_listener|remove_status_listener` по `crates/ packages/ src/`)

| Место | Сейчас | После |
|---|---|---|
| scrollable.rs:479-492, 813 | id в поле, снятие | поле `Option<StatusSubscription>`, `take()` |
| dismissible.rs:675, 927; animated_size.rs:208, 281; animated_switcher.rs:361, 397 | то же | то же |
| hero_flight.rs:982, 994, 290, 427 | id в `subscriptions` | guard в `subscriptions`; снятие — `take()` |
| transition_route.rs:646 | id выброшен; при `will_dispose_controller == false` слушатель остаётся на чужом контроллере | guard в `inner`, drop в `dispose` |
| drawer.rs:742; scaffold_messenger.rs:562, 703 | id выброшен | guard рядом с контроллером в состоянии, drop при замене/dispose |
| ink_well.rs:536 | id выброшен; контроллер на нажатие | `.detach()` (контроллер освобождает `cancel_pending_deactivation`) |
| тесты в src: scroll_controller.rs:635, hero_flight.rs:1204, scaffold_messenger/tests/failure_cases.rs:127 | id | guard / `detach` |
| доки: scaffold_messenger.rs:47, navigator.rs:1003, back_gesture.rs:374 | имя метода | без изменения смысла |
| flui-animation: curved.rs:128-136, 191-197; proxy.rs; switch.rs; reverse.rs:91-108; tween.rs:111-116; constant.rs:111-118 | `ParentSubscription`/id | guard; `ConstantAnimation` → `inert()` |
| тесты крейта: contracts/controller_sources.rs (18), contracts/proxy.rs (2), controller_tests.rs (3), vsync.rs (2) | id | guard; см. ниже |
| `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4; SDK реэкспортирует крейт целиком, lib.rs:27) | — | `StatusSubscription` в `measured` (его импортирует flui-material); `ListenerId` остаётся (value-слушатели пакетов) |
| docs: README, GUIDE, PATTERNS, ARCHITECTURE (`## Mapping decisions`), PERFORMANCE | | новый контракт |

Тесты, меняющие ожидание: `a_panicking_status_listener_leaves_the_finished_run_ok`
(controller_tests.rs:591) и `status_failure_retains_retired_source_and_callback`
(contracts/controller_sources.rs:408) ждут panic из `tick_at`/`stop` — panic по-прежнему выходит,
но после раунда: добавляется проверка, что соседи получили статус и захваты удержаны. Строки
`status_listener_panic_finishes_the_round`, `status_listener_panics_compete`,
`vsync_walk_contains_a_sibling_panic` уже лежат в ветке `animation/contract-tests`
(`#[ignore = "contract: …"]`). `CompoundAnimation` не мигрируется (удаляется).

## Черновик ADR

**ADR-NNNN: Animation status delivery — queued channels, contained observers, scoped
subscriptions.** Supersedes: ADR-0064 §4, только порядок статуса и доставки при реентри.

1. Источник статуса (контроллер, proxy, switch) фиксирует
   переход под своим состоянием и ставит событие в очередь канала; раздача идёт из самого
   внешнего кадра в порядке фиксации, каждому слушателю, подписанному до фиксации и не снятому к
   моменту вызова. Вложенные эмиссии ждут своей очереди.
2. Доставка `RunDelivery` — элемент той же очереди: статус нового прогона раздаётся раньше
   отмены вытесненного при любой вложенности. Первый payload продолжения возобновляется после
   опустошения очереди (ADR-0106).
3. Panic слушателя статуса (и значения, продолжения, тика в обходе): переход закоммичен до
   вызова, раунд дорабатывает, первый payload пробрасывается после раунда, остальные удерживаются;
   следующий кадр тикает. Заменяет ADR-0109 §3 «перехват без проброса»; совпадает с send-flip
   T6b/R11 (отчёт realm).
4. Подписка — `#[must_use] StatusSubscription`; drop снимает, `detach` оставляет до `dispose`
   источника. Обёртки без собственного статуса подписывают на канал родителя.
5. `Vsync::tick_all` сдерживает отказ каждого `tick_at` и дочернего реестра, доводит обход и
   возобновляет первый payload.
6. Хранение канала следует за хранением анимаций (ADR темы frame-path-state); область замка или
   заимствования не пересекает пользовательский код.

## Фрагмент changelog (`changelog.d/<branch-slug>.md`)

```markdown
### Changed

- **`flui-animation`**: `Animation::add_status_listener` returns a `#[must_use]`
  `StatusSubscription`; dropping it removes the listener, `detach()` keeps it.
  `remove_status_listener` is removed. Status transitions arrive in commit order even when a
  listener restarts the animation; a panicking status listener no longer stops later listeners —
  the panic is re-raised after every listener got the status; `Vsync::tick_all` ticks every
  controller before re-raising a tick's panic.

### Fixed

- **`flui-animation`**: `AnimationSwitch` no longer queries its trains under its own lock,
  announces the new train's status on a hop, stops spurious value notifications and releases
  its callbacks on `dispose`; `ProxyAnimation::set_parent` swaps parent and subscriptions as one.
```

## Adversarial review

| Сценарий | Что делает design | Тест |
|---|---|---|
| Слушатель реентерит контроллер (restart/stop/retarget) | событие в очередь, внешний кадр раздаёт; ADR-0064 порядок | R1, R2 |
| Слушатель реентерит vsync (unregister/register/mute) | замок реестра не держится при `tick_at`; поведение walk неизменно | R4, существующий `vsync_retirement_reentry` |
| Снятие/добавление во время раздачи | живой поиск, `fence` | R3, R5 |
| Последний владелец из колбэка | `finish(&self)` держит handle; форвардеры апгрейдят `Weak` на время вызова; изъятое уничтожается вне замков | R8 |
| Два контроллера на одном vsync | каналы независимы, вложенная раздача B внутри A | R4 |
| Два контроллера на одном тикере | непредставимо: `Ticker` удалён (controller-robustness) | — |
| Realm остановлен | канал не эмитит сам; `Drop` контроллера доставляет отмену | R17 |
| Dispose во время тика / из слушателя | `channel.dispose()` чистит слушателей и статусы, доставки остаются | R5 |
| Retarget в последнем кадре | `Completed` всем, затем `Forward` всем | R1 `retarget_on_last_frame` |
| dt = 0, огромный dt, время назад, NaN | дедуп по маркеру; переход один раз | R18 |
| Переполнение | `Slot` `checked_add`, исчерпание — постоянный отказ | R6 |
| NaN в значении switch | дедуп по битам, без повторного уведомления | R13 |
| Panic: слушатель статуса | хвост раздаётся, первый payload — после раунда, остальные удержаны | R9 |
| Panic: продолжение `RunFuture` | ADR-0106 внутри `deliver`, первый — после очереди | R2 |
| Panic: `Curve`/`Simulation`/деструктор в `tick_at` | выходит из `tick_at`; Vsync доводит обход | R10 |
| Panic: слушатель значения | foundation после send-flip T6b: та же политика (раунд, первый — после) | `value_listener_panic_finishes_the_round` (contract-tests) |
| Panic: снятие подписки в `set_parent`/`dispose` | `Retirement`, уведомления до resume | R16, R14 |
| Panic: подписчик `tracing` при логе | отдельная граница, удержание | R9 `failing_log_subscriber_retained` |
| Родитель switch/proxy реентерит из `value()`/`status()` | вызовы вне ячейки | R11, `proxy_parent_queries_allow_reentrant_replacement` |

## Остаточные риски

- **Пинг-понг нулевых прогонов** (раньше — переполнение стека): синхронный settle, начатый внутри
  `drain`, откладывается на следующий тик (решение по X3), поэтому кадр конечен, а принятая
  работа доставляется; такой слушатель заказывает кадр каждый кадр. Строка
  `zero_duration_ping_pong_terminates_the_frame`.
- Межпоточная доставка «чужим» потоком — до слияния frame-path-state.
- Уведомления значения не входят в очередь: слушатель значения может увидеть значение нового
  прогона раньше, чем слушатели статуса — старый статус. Сознательно.
- Сторонний `Animation` с собственным статусом (не делегирующий крейтовой анимации) нарушит
  порядок — трейт этого не запрещает, пока владелец не запечатал `Animation`; документируется.
- Слушатели значения (`Listenable`, `ListenerId`) остаются на foundation-модели id — вне темы;
  две модели подписки на трейте сходятся в RAII-токен foundation (send-flip T5).

## Владение (долгоживущие объекты)

| Объект | Сильные ссылки | Слабые | Освобождение: unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `StatusChannel` (`Shared<Channel>`) | владелец статуса (контроллер, proxy, switch) | `StatusSubscription` (`Weak` + `Slot`) | с последним handle владельца; подписка переживает канал инертно |
| колбэк слушателя | очередь канала (`Vec<(Slot, cb)>`), клон на время вызова | — | drop подписки / `dispose` канала; изымается и дропается вне `with` |
| форвардер proxy/switch на родителе | канал родителя | `Weak` на proxy/switch | смена родителя (`Retirement`), drop proxy/switch |
| `on_switched` switch | `SwitchState` | — | `dispose` изымает (C6); drop switch |

Циклы: **C2** (status-колбэк захватывает контроллер) — разрывает `StatusSubscription`, тест
«Drop == 1» `status_subscription_drop_releases_captures`; **C4** (слушатель через обёртку живёт
на родителе) — подписка у подписчика, `wrapper_status_listener_dies_with_subscription`
(Reverse/Curved/Tween); **C6** (switch → `on_switched` → proxy) — `dispose` изымает колбэки,
`switch_dispose_releases_callbacks`; часть C6 в `transition_route` (`Weak` proxy) — карточка в
[../tasks.md](../tasks.md) «Задачи из аудитов».

## Паттерн

- **RAII guard** — `#[must_use] StatusSubscription` (отписка в `Drop`, `detach` — явный отказ).
- **Очередь + один `with`** — состояние канала — обычная структура, доступ одной функцией, колбэки
  изымаются наружу (не «table of functions», а изъятие владения до вызова).
- **Generation-checked commit** — `generation` у proxy/switch: поздний вызов выигрывает без замков.
- **Monotonic newtype ID** — `Slot(NonZero<u64>)` с `checked_add` и вечным отказом при исчерпании.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `add_status_listener`/`remove_status_listener(id)` | Flutter пара add/remove по колбэку/id | RAII `StatusSubscription` (GPUI `Subscription`), удаление `remove_*` |
| `AnimationLocalStatusListenersMixin` на каждой обёртке | mixin-копия реестра | один крейтовый канал у владельцев статуса, обёртки делегируют |
| `notifyStatusListeners` с `catch` + `reportError` без проброса | Flutter «report and continue» | раунд дорабатывает, первый payload — `resume_unwind` после раунда |
| `StatusChannel` как публичная база для сторонних анимаций | ChangeNotifier-подобный базовый тип | `pub(crate)`; внешние — делегирование |

## Конвенции Rust (аудит 2026-10-06)

Строки `design.md:N` — до правки (`9bf335e07`).

| Пункт конвенции | Статус | Где | Правка |
|---|---|---|---|
| Одна политика паники для всех колбэков | нарушала | design.md:11, 105-107, 155-156, 165-166, 233-234, 251; R9 | раунд дорабатывает, первый payload — после раунда (Итог, «Раздача», инв. 4, ADR п. 3, changelog, R9) |
| Каждый новый `pub` с production-потребителем | нарушала | design.md:175, 181-197 | `StatusChannel`, `inert` → `pub(crate)`; внешние реализации делегируют |
| Sealed для закрытых семейств | открыто | `Animation` (animation.rs:68) | решение владельца (R3), design от него не зависит |
| RAII-guard, `#[must_use]` | соответствует | design.md:171 | — |
| ID — `NonZero<u64>`, `checked_add`, вечный отказ | нарушала | design.md:157 (`u64`) | `Slot(NonZero<u64>)` |
| Ни guard, ни borrow при пользовательском коде | соответствует | design.md:82-87 | добавлено правило edition 2024 (`match`, let-chain) |
| `Drop` без пользовательского кода под guard | уточнено | design.md:179 | колбэк изымается, уничтожается вне ячейки |
| Порядок замков назван | нарушала | — | «Порядок замков до F3» |
| Принятая работа доставляется, кадр конечен | нарушала | design.md:286-288 | settle внутри `drain` → следующий тик (X3) |
| Таблица владения, циклы C2/C4/C6, тест «Drop == 1» | нарушала | — | раздел «Владение» |
| Одна модель подписки | открыто | `Listenable` vs `StatusSubscription` | сквозная правка foundation (send-flip T5) |
| Имена `Ticker*` после удаления `Ticker` | нарушала | design.md:10, 109, 230, 269, 277 | `RunDelivery`/`RunFuture` (решение по controller-robustness) |
