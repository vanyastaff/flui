# `!Send` UI-поверхности и путь записи — design

- **Статус:** черновик
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md)
- **Связанные:** [persistence/design.md](../persistence/design.md) (ждёт owner-local
  `AsyncDriver`), [teardown/design.md](../teardown/design.md) (порядок уничтожения полей realm,
  `DropLedger`), [realm-model/experiment.md](../realm-model/experiment.md) (граф на
  presentation, `ForeignGraph`, wake не того окна), [authoring-styles/requirements.md](../authoring-styles/requirements.md) R9, R19.

## Текущее состояние

Только то, что меняет выбор; полная таблица классов — в requirements.

- **Планировщик `Send + Sync`.** `UpdateScheduler` — `Arc<SchedulerInner>` под `parking_lot::Mutex`
  (`crates/flui-scheduler/src/scheduler.rs:1007`); очереди хранят `Send`-замыкания (`:836-838`,
  `frame.rs:416`, `:712`, `:720`, `:726`, `config.rs:29`, `ticker.rs:77`, `:92`); закреплено
  `assert_impl_all!` (`post_frame.rs:304-305`). Межпоточных вызовов в production нет.
- **Owner-local дорожка уже есть:** `LocalPostFrameLane`/`LocalPostFrameHandle` на `Rc`
  (`post_frame.rs:45`, `:169`), кадр получает её параметром (`scheduler.rs:1542`); `LifecycleContext`
  выдаёт оба handle (`crates/flui-view/src/context/build_context.rs:429`, `:448`).
- **`AsyncDriver` внутри планировщика:** `BoxedTask: Send` (`async_driver.rs:106`), задачи и
  готовность под одним `Mutex` (`:181`), `TaskToken` держит `Weak<Inner>` (`:249`). Опрос — в
  `handle_begin_frame` (`scheduler.rs:1266`, `:1380`) и в `UiRealm::pump_background` без кадра
  (`crates/flui-runtime/src/ui_realm/pump.rs:123`); `RealmServices` хранит клон (`realm_services.rs:57`).
  Защёлка кадра — `swap(.., SeqCst)` с повторной проверкой (`scheduler.rs:1080`, `:2185-2215`).
- **`FutureBuilder`:** `BoxedResultFuture: Send` (`crates/flui-view/src/element/future_builder.rs:62`),
  `K: Send + Sync`, `T, E: Send` (`:121-127`), драйвер берётся в `init_state` (`:307`); так же
  `image/resolve.rs:142`. Notes грузит через него (`examples/two_screens/tree.rs:260`).
- **Контроллеры:** `AnimationController` (`crates/flui-animation/src/controller.rs:257`), `Vsync`
  (`vsync.rs:141`), `ScrollController` (`scroll_controller.rs:120`), `CupertinoTabController`
  (`tab_scaffold.rs:86`), `TextEditingController` (`text/controller.rs:256`), `WidgetStatesController`,
  `TransformationController`, `RefreshController` — `Send` через `Arc`. `TabController` уже на `Rc`.
- **Слушатели:** `ListenerCallback = Arc<dyn Fn() + Send + Sync>`, `Listenable: Send + Sync`
  (`crates/flui-foundation/src/notifier.rs:45`, `:76`); паника слушателя ловится и только
  логируется (`:256-259`). `add_listener(`: 138 строк (54 — `Arc::new(`), `add_status_listener(`: 52.
- **Путь записи:** `WriterSource::write` открывает новый `Writer` на вызов, вложенность не
  проверяется (`crates/flui-view/src/reactive/writer.rs:233`); `WrittenDuringBuild` — `:221`.
  Запись не транзакция (`reactive/mod.rs:61-66`). `.reactive()` — 30 вызовов; `UiCommand::SignalWrite`
  несёт `FnMut(&Reactive) + Send` (`crates/flui-runtime/src/ui_realm/commands.rs:109-113`).
  `NEXT_GRAPH_ID.fetch_add` (`u32`) переиздаёт id после переполнения (`reactive/mod.rs:192`).
- **Рендер:** `RenderObject` без `Send` (`render_object.rs:176`), `RenderTree` и арена раскладки
  `!Send` (`storage/tree.rs:23`, `pipeline/owner/subtree_arena.rs:19-25`), `BoxLayoutCtxErased` без
  `Send` (`protocol/box_protocol.rs:832`). `Send` требуют только `RenderView::RenderObject`
  (`crates/flui-view/src/view/render.rs:511`) и `sliver_persistent_header.rs:169`. Делегаты
  принимаются как `Arc<dyn …>` (`crates/flui-widgets/src/paint/custom_paint.rs:35`, `layout/flow.rs:34`).
- **`unsafe impl Send/Sync`** — только `flui-platform` (46), `flui-hot-reload` (4), `tools/desktop-mcp` (2).
- **Фасад** реэкспортирует крейты целиком (`src/lib.rs:163-196`); `flui::view` несёт `AsyncDriver`,
  `BoxedTask`, `PostFrameHandle`, `LocalPostFrameHandle`, `TaskToken`
  (`crates/flui-view/src/lib.rs:203-206`); `flui-sdk` — те же модули (`crates/flui-sdk/src/lib.rs:27-43`).

## Варианты

### (a) Owner-local контроллеры

- **a1 `Rc<RefCell<Inner>>` (выбран).** `controller.value()` читается где угодно на owner, в том
  числе в paint (`Animation<T>::value()` без контекста); ticker пишет напрямую; последний `Rc` —
  `Drop` на owner. Цена механическая (`Arc→Rc`, `Mutex→RefCell`), форма как у `TabController`.
- **a2 handle в slab realm (GPUI `Entity<T>`).** Чтение только через `cx`; в `paint` контекста нет
  → ломает `Animation<T>` и все чтения. Отклонён.
- **a3 на сигналах (Dioxus/Leptos Copy signals).** Нужен `ReadScope` в paint (ADR-0085 §3 не
  сделан) и `Writer` в ticker, плюс `ForeignGraph` между окнами. Отклонён; позже — аддитивный
  адаптер `controller.as_signal()`.

Дисциплина заимствований a1 повторяет нынешнюю дисциплину замков: borrow отпускается до
пользовательского кода (status-listener уже зовётся после замка, `controller.rs:231`). GPUI
(`crates/gpui/src/executor.rs`: `ForegroundExecutor` с `PhantomData<Rc<()>>`, `spawn` без `Send`)
подтверждает форму. Требования: R1, R2, R13. ADR: ADR-0027 §2 выполняется.

### (b) Разделение планировщика: `Send`-wake и owner-local очереди

- **b1.** `UpdateScheduler` остаётся `Send`, у каждого семейства — вторая owner-local дорожка: две
  очереди и правило чередования на семейство (`build_context.rs:432-441`). Отклонён.
- **b2 (выбран).** `UpdateScheduler` и задачи — owner-local (`Rc`, `RefCell`/`Cell`). Через поток
  ходит один `FrameWaker: Clone + Send + Sync` («запросить кадр», «задача готова»): атомарная
  защёлка, хук платформы (замок инфраструктуры на вставку/выемку), очередь готовых id. Waker
  задачи держит `Weak<WakeShared>`. Так же у GPUI (`ForegroundExecutor`) и
  `futures::executor::LocalPool`: `Waker` всегда `Send + Sync`, future — нет.
- **b3.** Регистрации через командный канал: задержка на каждый `add_listener`. Отклонён.

**Протокол `FrameWaker`** — сегодняшняя защёлка без изменений (`scheduler.rs:1080`, `:2185-2215`,
issue #1162): `request_frame` — `if !frame_scheduled.swap(true, SeqCst) { hook() }`, хук только на
фронте false→true; снятие — `swap(false, SeqCst)` **до** опроса (начало кадра, `finish_async_pump`),
затем drain, затем повторная проверка: непустая очередь готовых id или живой `end_of_frame`
ожидающий снова выставляет защёлку и зовёт хук; самопробуждение во время drain не стирается.
Переносятся явно, с прежними утверждениями: `finish_async_pump_reissues_a_stranded_live_waiters_demand`,
`frame_scheduled_hook_fires_once_per_transition`, `lifecycle_reenable_edge_schedules_exactly_one_frame`,
`headless_wake_from_another_thread_is_polled_on_the_frame_thread`, таблица `tests/wake_delivery.rs`.

**Первая задача — `AsyncDriver`/`FutureBuilder` в окончательной форме** (порядок оркестратора).
Ломающее (строки 1–3 и 33 контракта), но окончательное: 6a его не переделывает, и промежуточного
async API не бывает, даже если ядро не успеет к 11-24.
- `UpdateScheduler` пока `Send`, а `unsafe impl Send` запрещён (R5), поэтому задачи и post-frame
  очередь живут в отдельном `!Send` `OwnerFrame` (`#[doc(hidden)]`, поглощает
  `LocalPostFrameLane`). Единственная сильная ссылка — у realm (`RealmServices`) и `HeadlessBinding`.
- Публичный `AsyncDriver` (`LifecycleContext::async_driver`, `!Send`, `Clone`) — `Weak` на
  `TaskStore`: состояние виджета (`future_builder.rs:307`, `image/resolve.rs:142`), даже утёкшее,
  не держит задачи и захваты после realm. `spawn_local` на мёртвом handle уничтожает future на
  owner-потоке и возвращает отменённый `TaskToken` с `tracing::warn!`.
- Teardown дренирует `TaskStore` явно в своём порядке полей (`Option`/`ManuallyDrop`, catch,
  первая паника сохраняется).
- Входы кадра без owner-части удаляются в том же PR: `handle_begin_frame(vsync, &OwnerFrame)`,
  `drive_frame(&OwnerFrame, ..)`, `execute_frame(&OwnerFrame)`, `end_frame(&OwnerFrame)`;
  `*_with_lane` и безлейновые варианты исчезают — ни один вход не опрашивает «ничего». Опрос — в
  `MidFrameMicrotasks` (`:1380`); `pump_background` (`pump.rs:123`): `finish_async_pump()`, затем
  `owner_frame.poll_ready()`.
- 6a эти сигнатуры не меняет: `OwnerFrame` остаётся домом owner-local очередей, а
  `UpdateScheduler` сужается до фазы, времени и регистраций.

Persistence получает `load() -> impl Future + 'static` без `Arc<Mutex>`. Требования: R4, R9, R10,
R13. Цена: 6 дней (~80 вызовов входов кадра, почти все в тестах). ADR: новый ADR §2; ADR-0047 без изменений.

### (c) Запись из listener через захваченный `WriterSource`; вложенный `EventCx`

- **Сигнатуры слушателей не меняются** (решение спеки): `add_listener(Rc<dyn Fn()>)`,
  `add_status_listener(Rc<dyn Fn(AnimationStatus)>)`, post-frame `FnOnce(&FrameTiming)`. Пишут
  через `WriterSource`, полученный в `init_state`/`did_change_dependencies`/`RenderObjectContext`.
- **Вложенный `WriterSource::write` в живом `EventCx`** (`set_index` из `on_tap` → listener →
  `source.write`): c1 — разрешить, записи применяются сразу в порядке вызова (ADR-0086 §6:
  независимые `EventCx` — не транзакция); c2 — отказ `Nested`, ломает сценарий R7; c3 — общая
  транзакция, нужен журнал. **Выбран c1**: запись видна следующему чтению сразу, читатели
  планируются на кадр, вложенный listener выполняется синхронно до возврата.
- **Паника в callback (R11).** Записи до паники остаются применёнными (R11 в редакции
  оркестратора; ADR-0074, `reactive/mod.rs:61-66`). Сегодня `notify_listeners` глотает панику
  слушателя в `tracing::error!` (`notifier.rs:256-259`). Новое правило: раунд продолжается,
  первая пойманная паника возобновляется после раунда (`resume_unwind`), последующие —
  вторичные (лог, уничтожение под catch). Граница dispatch realm (событие, фаза кадра,
  post-frame) отдаёт её в отчёт паник realm (`report_contained_panic`, teardown design), как F3
  в text-ime, и сообщает **первую** панику раунда. Паника вложенной записи поднимается через
  `source.write` во внешний callback и становится его первой паникой.
- **Мутация контроллера во время `build`.** Сегодня `set_index` в `build` меняет индекс, а запись
  слушателя теряется (`WrittenDuringBuild`, `writer.rs:221`). Мутаторы `TabController`,
  `CupertinoTabController`, `ScrollController::jump_to`/`animate_to` и `AnimationController`
  отказывают во время сборки, как `StateCell::schedule` (R8b): состояние не меняется,
  `tracing::warn!` на `flui::signals`; `AnimationController` — новый вариант
  `AnimationError::MutatedDuringBuild` (enum `#[non_exhaustive]`). Guard — флаг `building` графа,
  привязка в `init_state` виджета, подписанного на контроллер.
- **Рекурсия слушателей** (A ↔ B через контроллеры) глубиной не ограничивается — это синхронный
  пользовательский цикл; документируется. Мутатор с равным значением не уведомляет, а запись
  сигнала слушателей синхронно не вызывает, поэтому через сигналы цикла нет.
- **`UiCommand::SignalWrite`** несёт `Box<dyn FnOnce(&mut EventCx<'_>) + Send>`; realm открывает
  `EventCx` через `WriterSource` presentation, выпустившей слот (ADR-0085 §1); envelope при
  панике удерживается (ревизия ADR-0086 от 2026-09-28).
- **`LocalHistoryEntry::on_remove(impl Fn() + 'static)`** — сигнатура та же; pop, применённый во
  время сборки, доставляет `on_remove` в post-frame очередь кадра (как `PageView`, ADR-0086 §6).
- **`&Reactive` удаляется** (ADR-0086 §8 шаг 3): `WriteTarget for Reactive`, `BuildContext::reactive`,
  публичный `BuildOwner::reactive`, `HeadlessBinding::reactive`. `StateCell::schedule` в `build`
  отказывает тем же guard'ом (§7).
- Требования: R6–R8b, R12. ADR: ADR-0086 принимается с поправкой §5.

### (d) Gate R3: перечень `Send`/`Sync` поверхности

- **d1 rustdoc JSON + `rustdoc-types` в xtask (выбран).** Видит позиции структурно, включая
  синтетические auto-trait impl; классифицирует по пути и позиции; закреплённый toolchain +
  `RUSTC_BOOTSTRAP=1`; загрузчик общий с `api-closure` (ADR-0089 §6).
- **d2 `cargo-public-api`.** Текст сигнатур и регэкспы; внешний бинарник и nightly. Отклонён.
- **d3 только trybuild.** Не видит вхождений, которых нет в списке фикстур. Отклонён как gate,
  остаётся для R1.

Выбор d1: команда `cargo xtask thread-boundary`. Строит rustdoc JSON для `flui` и `flui-sdk`
с замыканием реэкспортов, перечисляет каждое вхождение `Send`/`Sync` (позиция, путь, тип) и
auto-trait факты публичных типов. Сверяет с `tools/xtask/allowlists/thread-boundary.toml`:
`keep` — путь, позиция, причина, точные счётчики, только сокращаются; `flip` — запрет на
возврат; `owner-only` — типы, которые не должны быть `Send` (все `*Controller`, `AnimationController`,
`UpdateScheduler`, `AsyncDriver`, `Signal`, `WriterSource`, …); `sendable` — типы, которые обязаны
остаться `Send` (R4).

**Позиции**, каждая со своей self-test fixture (крейт-заглушка, на котором gate обязан упасть):
supertrait; bound параметра и where-clause; alias (`DynTrait`); поле публичного типа;
синтетический auto-trait impl; generics и where-clause impl-блоков (blanket impl
`impl<T: Send> Listenable for T`); **приватный sealed supertrait** (`Listenable: sealed::Base`,
`Base: Send`) — JSON строится с `--document-private-items`, и замыкание supertrait
проходит по приватным элементам; `-> impl Trait + Send` в возврате. Вторая линия для трейтов —
consumer R2: `!Send` реализатор каждого flip-трейта не скомпилируется, если bound спрятан
где-то, чего ledger не видит.

**Где работает.** Ledger вызывается изнутри `cargo xtask doc-strict`: джоб `doc` уже идёт на
широкой линии, workflow не меняется; таймаут джоба 25 минут, время второго прохода rustdoc
замеряется в пункте 2 работ (при превышении — по крейтам). `--self-test` — в `cargo test -p xtask`,
то есть в `checks`; оркестратор правит текст R3 под это. Скан `unsafe impl Send/Sync` через `syn`
(комментарии не считаются) с allowlist для `flui-platform`, `flui-hot-reload`, `tools/desktop-mcp`
— без сборки, в `checks`. Загрузчика `api-closure` (ADR-0089 §6) ещё нет: его строит пункт 2.
`deny.toml [bans]`: `send_wrapper`, `fragile`, `send-cell`, `unsend`, `force-send-sync`.
Требования: R3, R4, R5. ADR: ADR-0089 §6.

### (e) Параллельная раскладка после 0.2.0 (D5)

- **e1 (выбран): opt-in маркер и развилка у родителя.** После 0.2.0 аддитивно в `flui-rendering`:
  ```rust
  /// Объект и всё, что он держит, можно раскладывать на другом потоке.
  pub trait IndependentLayout: RenderBox + Send {}
  // provided-метод в RenderObject, default None — minor-изменение:
  fn independent_layout(&mut self) -> Option<&mut dyn IndependentLayout> { None }
  // развилка — метод layout-контекста родителя (ADR-0027 §10: fork point — код объекта):
  ctx.layout_children_independently(&mut children, constraints)
  ```
  Внутри одного checkout `PipelineCell` арена отдаёт непересекающиеся `&mut` поддеревьев в
  `std::thread::scope`; узел без маркера делает развилку последовательной (флаг при вставке).
  Рабочий поток получает `Send`-подконтекст без owner-возможностей. Нужна новая аргументация
  корректности `NodePtr` (не-цель ADR-0027 §10).
- **e2.** `Send + Sync` на рендер-слое «на будущее» — отклонено владельцем (D3).
- **e3.** Отдельное неизменяемое дерево раскладки (как taffy): аддитивно, дублирует дерево.

**Необратимо для e1 — в 0.2.0 не делать:**
1. Не реализовывать публичных трейтов с owner-доступом (`ReadScope`, `ReadGraph`) для
   `BoxLayoutContext`/`SliverLayoutContext` и не добавлять в них методы, отдающие `WriterSource`,
   `Signal`, `Rc`-сервисы или `PipelineCell`. Убрать impl трейта у опубликованного типа — ломка.
   Чтения сигналов в layout (ADR-0085 §3) — только через отдельный подконтекст.
2. Не запечатывать `RenderObject`/`RenderBox`/`RenderSliver` и не добавлять им supertrait,
   который тянет `!Send` (поле-маркер `PhantomData<Rc<()>>` в обязательной базе объекта).
3. Не публиковать в сигнатурах `perform_layout` хранимый `&PipelineOwner`, `&mut RenderTree`
   или `PipelineCell`: барьер ADR-0091 §2 должен остаться деталью реализации.
4. Не возвращать `Send` в снятые supertrait и alias'ы: это ломка. Путь только opt-in.
5. Не делать layout-делегаты (`MultiChildLayoutDelegate` и др.) обязательной частью
   `RenderBox`-трейта: делегат с `Rc` должен лишь выключать маркер своего объекта.
6. `TextCx` остаётся непрозрачным, и отказ «контекст уже одолжен» не обещается как контракт
   (сегодня это паника в `BoxLayoutContext::text`, `protocol/box_protocol.rs:612-622`): рабочему
   потоку понадобится свой контекст шейпинга, а не тот же одолженный.

**Обратимо (можно исправить аддитивно):** делегаты хранятся стёрто (приватный `Rc<dyn Delegate>`), поэтому
объект с делегатом не сможет доказать `Send`. Позже — generic-вариант объекта
(`impl<D: MultiChildLayoutDelegate + Send> IndependentLayout for RenderCustomMultiChildLayoutBox<D>`)
или конструктор с `impl Delegate + Send`. `TextSource` в layout-контексте — заимствование на
вызов, не хранимый handle; параллельная раскладка текста требует контекста шейпинга на поток
(предусловие ADR-0027 §10). Требования: D5 уровня 0, authoring-styles R19.

## Публичный контракт

**B** — ломающее; **R** — ослабление (ломает только код, полагавшийся на `Send` значения).
Фасад: `flui::<модуль>::…`, SDK: `flui_sdk::<модуль>::…`, если не указано иное.

**flui-scheduler** (`flui::view`, `flui::animation`):
| # | Было | Стало | |
|---|---|---|---|
| 1 | `BoxedTask = Pin<Box<dyn Future<Output=()> + Send>>` | без `+ Send` | B |
| 2 | `AsyncDriver`, `TaskToken`: `Send + Sync` | owner-local, `!Send` | B |
| 3 | `UpdateScheduler::{async_driver, spawn_local, spawn_local_eager, drive_async_tasks, pending_task_count}`; входы кадра `handle_begin_frame`, `drive_frame`, `execute_frame`, `end_frame` и их `*_with_lane`; `LocalPostFrameLane`, `new_local_post_frame_lane` | методы задач — на `AsyncDriver` (`Weak`) и `OwnerFrame`; входы кадра принимают `&OwnerFrame` (`#[doc(hidden)]`), безлейновые и `*_with_lane` удалены | B |
| 4 | `UpdateScheduler: Send + Sync` | `!Send`; новый `UpdateScheduler::frame_waker() -> FrameWaker` (`Clone + Send + Sync`: `request_frame()`) | B |
| 5 | `OneShotFrameCallback`, `PostFrameCallback` (`+ Send`) | без `Send` | B |
| 6 | `RecurringFrameCallback`, `TimingsCallback` (`Arc<dyn Fn + Send + Sync>`) | `Rc<dyn Fn(..)>` | B |
| 7 | `LifecycleStateCallback = Box<dyn Fn + Send + Sync>` | `Box<dyn Fn(AppLifecycleState)>` | B |
| 8 | `TickerCallback = Box<dyn FnMut(f64) + Send>`; `TickerProvider: Send + Sync` | без `Send`; без supertrait | B |
| 9 | `PostFrameHandle` (`Send`, `schedule(impl FnOnce + Send)`) и `LocalPostFrameHandle` | один `PostFrameHandle` (`!Send`): `schedule(impl FnOnce(&FrameTiming) + 'static) -> Result<(), PostFrameScheduleError>` | B |
| 10 | `schedule_microtask`, `add_task`, `schedule_idle_callback`, ticker `F: … + Send` | без `Send` | R |
| — | `set_on_frame_scheduled(Arc<dyn Fn() + Send + Sync>)`, `AsyncDriver::set_request_frame` | без изменений (wake-хук хоста) | keep |

**flui-foundation** (`flui::foundation`):
| 11 | `ListenerCallback = Arc<dyn Fn() + Send + Sync>` | `Rc<dyn Fn()>` | B |
| 12 | `trait Listenable: Send + Sync` (и `ValueListenable<T>`) | без supertrait | B |
| 13 | `ArgCallback<A> = Arc<dyn Fn(&A) + Send + Sync>` | `Rc<dyn Fn(&A)>` | B |
| 14 | `ChangeNotifier`, `ValueNotifier<T>`, `Notifier<A>`: `Send + Sync` | `!Send` (`Rc<RefCell>`) | B |
| 15 | `VoidCallback`, `ValueChanged`, `ValueGetter`, `ValueSetter`, `Predicate`, `ValueTransformer`, `FallibleCallback` | удалены (вне реэкспортов не используются; замыкание пишется по месту) | B×7 |
| 16 | `trait ViewKey: Send + Sync + 'static` | `ViewKey: 'static` | B |
| 16a | `SignalSlot::graph() -> u32` (id графа переиздаётся при переполнении) | `-> u64`; счётчик `checked_add` с `expect("BUG: reactive graph identities exhausted")`, как `async_driver.rs:183-192` | B |

**flui-animation** (`flui::animation`):
| 17 | `StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>` | `Rc<dyn Fn(AnimationStatus)>` | B |
| 18 | `trait Animation<T>: Listenable + Send + Sync + Debug where T: Clone + Send + Sync` | `Animation<T>: Listenable + Debug where T: Clone + 'static` | B |
| 19 | `trait Simulation: Send + Sync` | без supertrait | B |
| 20 | `AnimationController`, `Vsync`: `Send + Sync` | `!Send` | B |
| 21 | `animate_to_with_curve(.., curve: Arc<dyn Curve + Send + Sync>)` и ещё два (`controller.rs:1153`, `:1170`, `:1192`) | `curve: impl Curve + 'static` | B |
| 22 | `T: Clone + Send + Sync` в `Tween*`, `Proxy*`, `Constant*`, `CurvedAnimation<C>`, `switch` | `T: Clone + 'static` | R |
| — | `ArcCurve` | keep: стёртая `Send`-кривая как данные | keep |

**flui-rendering** (`flui::painting::CustomPainter`, `flui::rendering`):
| 23 | `CustomPainter: Send + Sync + Debug` | `Debug` | B |
| 24 | `FlowDelegate`, `MultiChildLayoutDelegate`, `SingleChildLayoutDelegate`, `SliverGridDelegate` | без `Send + Sync` | B×4 |
| 25 | `ViewportOffset: Debug + Send + Sync`; слушатели `ScrollPosition` | `Debug`; `Rc` | B |
| 26 | `RenderObject::metadata`, `RenderBox::metadata`, `RenderSliver::metadata` → `Option<Arc<dyn Any + Send + Sync>>` | `Option<HitMetadata>`: непрозрачный `Clone`, `!Send`; `HitMetadata::new(impl Any)`, `downcast_ref::<T>()`; хранение приватно | B |

**flui-interaction, flui-semantics, flui-objects** (`flui::interaction`, `flui::rendering`, `flui::widgets`):
| 27 | `HitTestTarget: Send + Sync`, `CustomHitTestable: Send + Sync` | без supertrait | B×2 |
| 28 | `HitTestEntry::metadata(Arc<dyn Any + Send + Sync>)`; alias `MetaDataPayload` | `metadata(HitMetadata)`; alias удалён | B |
| 29 | `SemanticsActionHandler = Arc<dyn Fn(..) + Send + Sync>` | `Rc<dyn Fn(..)>`; к платформе уходит запрос, не handler | B |
| 30 | `BuildDuringLayoutCell`, `PhysicalClipShape` | без `Send + Sync` | B×2 |

**flui-view** (`flui::view`):
| 31 | `RenderView::RenderObject: RenderObject<P> + Send + Sync + 'static` (и `sliver_persistent_header.rs:169`) | `+ 'static` | R |
| 32 | `trait Notification: Any + Send + Sync + 'static` | `Any + 'static` | B |
| 33 | `BoxedResultFuture<T, E>` (`+ Send`); `FutureBuilder<K: Send + Sync, T: Send, E: Send>` | без `Send`; `K: Clone + PartialEq + Debug + 'static`, `T, E: 'static` | B (alias), R (bounds) |
| 34 | `BuildContext::reactive()`, публичный `BuildOwner::reactive()`, `impl WriteTarget for Reactive` | удалены | B×3 |
| 35 | `LifecycleContext::local_post_frame_handle()`; `post_frame_handle() -> Option<PostFrameHandle>` (Send) | первый удалён; второй отдаёт owner-local handle | B |

**flui-widgets, пакеты** (`flui::widgets`, `flui::cupertino`):
| 36 | `ScrollController`, `TextEditingController`, `WidgetStatesController`, `TransformationController`, `RefreshController`, `PageController`: `Send` | `!Send` (у `TextEditingController` — через его `ChangeNotifier`, строка 14; внутренний `Arc<Mutex>` уходит отдельной задачей, см. «Работы») | B |
| 37 | `trait ScrollPhysics: Send + Sync + Debug` | `Debug` | B |
| 38 | `CustomPaint::{painter, foreground_painter}(Arc<dyn CustomPainter>)`, `Flow::new(Arc<dyn FlowDelegate>, ..)`, `CustomSingleChildLayout::new`, `CustomMultiChildLayout::new`, `RenderSliverGrid::new` | `impl CustomPainter + 'static` (и так же для делегатов); хранение приватное (`Rc<dyn …>`), в публичной сигнатуре `Rc` нет (authoring-styles R12) | B×5 |
| 39 | `impl Curve + Send + Sync + 'static` в `animated/*`, `Hero::curve` | `impl Curve + 'static` | R |
| 40 | `CupertinoTabController: Send` | `!Send` (`Rc<Cell>`) | B |

**flui-testing** (`flui::testing`): 41 — `HeadlessBinding::reactive()` удалён (B); `spawn_local`
принимает `!Send` (R). **flui-runtime/flui-app** — изменения `pub(crate)`/`#[doc(hidden)]`:
`UiCommand::SignalWrite`, `OwnerFrame`, `RealmServices`.

**Keep (с причиной, вносится в allowlist gate):** `SignalSender<T>`, `RebuildHandle` (ADR-0018),
`IoFuture`, `ComputeJob`, `FrameWaker`, `set_on_frame_scheduled`-хук, `CloseGuard`/`CloseChanged`
(teardown, состояние presentation, доступное хосту), `Storage`/`StorageFuture`/`FlushRegistry`
(persistence, граница IO), `AnnotationValue` (слой уходит на raster), `Draggable<T: Send + Sync>`
(данные `ErasedDragData`), `ImageProvider` (процессный decode-кэш и IO), `WidgetsLocalizations`,
`Localizations::of<R: Send + Sync>` (ресурсы-данные; снять bound параметра позже — не ломка),
platform hooks (ADR-0082 §4). Строка «?» `TimingsCallback` решена как flip: её вызывает
`report_timings` на owner-потоке. Строки gate, которых нет в таблице requirements, классифицируются тем же правилом:
вызывается на owner → flip; данные, уходящие на raster/IO/worker → keep.

**Итого:** 42 строки; ломающих — **38 строк, 89 элементов API** (типы, трейты, alias'ы и методы
по отдельности), ослаблений — 6 строк. Новые публичные элементы: `FrameWaker`, `UpdateScheduler::frame_waker`,
`HitMetadata`, `AnimationError::MutatedDuringBuild`, `PostFrameScheduleError` (переименование
`LocalPostFrameScheduleError`). Поведенческие изменения без смены сигнатуры: мутаторы
контроллеров отказывают во время `build`; паника слушателя возобновляется после раунда.

## Миграция

Вызовы по крейтам: `add_listener(` / `add_status_listener(` / `post_frame_handle()` / impl
flip-трейтов / `Send`-токены. foundation 22/–/–/3/53; scheduler –/–/–/–/49 (+46 `spawn_local`);
animation 23/39/–/12/64; interaction 25/–/–/–/25; rendering 8/–/–/13/60; objects 7/–/–/7/15;
view 3/–/2/14/110; widgets 26/8/16/7/151; runtime 1/–/1/–/61; testing –/–/12/–/10; app –/–/–/–/54;
material 17/5/1/6/20; cupertino 2/–/–/–/2; devtools –/–/–/–/1; examples 4/–/4/–/7; корневые tests –/–/2/1/–.

`Send`-токены — верхняя граница (`rg -c 'Send \+ Sync|\+ Send\b'`); в `flui-platform` (152) и
`flui-engine` (26) почти все остаются. Рецепт (ast-grep и `rg`, как разрешено политикой репозитория):

1. `ast-grep -p '$R.add_listener(Arc::new($F))' -r '$R.add_listener(Rc::new($F))'`, то же для
   `add_status_listener`; импорт `std::rc::Rc`.
2. Вызовы `CustomPaint::painter(Arc::new(p))` и конструкторов делегатов →
   `painter(p)` (`ast-grep -p '$W.painter(Arc::new($P))' -r '$W.painter($P)'`, так же для
   `foreground_painter` и `::new(Arc::new($D), ..)` у `Flow`/`Custom*Layout`); прочие
   `Arc<dyn (Simulation|…)>` → `Rc<dyn …>`; `Arc<dyn Any + Send + Sync>` в metadata →
   `HitMetadata::new(v)`.
3. Удалить `+ Send + Sync`/`+ Send` из supertrait и bound'ов flip-класса по списку строк gate
   (`cargo xtask thread-boundary --report`), остальное — по ошибкам компилятора.
4. Захваты `Arc<Mutex<_>>`/`Arc<AtomicX>`, которые существовали ради `Send` listener'а →
   `Rc<RefCell<_>>`/`Rc<Cell<_>>` (ручная правка; `clippy::arc_with_non_send_sync` их находит).
5. Запись из listener через `SignalSender` + `UiCommand` (17 упоминаний) → захваченный
   `WriterSource`: `let source = cx.writer_source();` в `init_state`, в listener —
   `source.write(|cx| sig.set(cx, v))`.
6. `.reactive()` (30, почти все в тестах) → `writer_source().write(..)` или `peek`.
7. `local_post_frame_handle()` → `post_frame_handle()`, `.schedule_local(` → `.schedule(`.

Notes (`examples/two_screens`, `tests/fixtures/notes_flow.rs`): изменений сигнатур не требует,
`FutureBuilder` принимает прежний future; проверяется внешним consumer. Шаблоны `flui create`:
`hot_reload.rs` держит `Arc<AtomicI32>` — компилируется и после (Send-значения принимаются);
проверка `cli_create::generated_*`. `flui migrate` не пишется (открытый вопрос 1 requirements):
строки миграции — в `changelog.d/send-flip.md`, по строке на каждый элемент таблицы выше.

## Инварианты

1. Ни один тип класса flip не реализует `Send`/`Sync`; закрепляет gate `owner-only` и trybuild R1.
2. Через поток ходят только `FrameWaker`, `SignalSender`, `RebuildHandle`, `IoFuture`/`ComputeJob`,
   данные keep. `Waker` задачи держит только `Weak` на `Send`-состояние пробуждения.
3. Каждый `!Send` захват создаётся, вызывается и уничтожается на owner-потоке; `Drop` — ровно
   один раз (замена, unmount, teardown). Это следует из типов: `!Send` значение нельзя переместить.
4. `RefCell` не заимствован во время пользовательского кода: состояние фиксируется, исходящие
   значения выносятся, borrow отпускается, затем вызов или уничтожение (AGENTS.md, reentrancy).
5. Уведомление слушателей — по снимку на начало раунда; удалённый в раунде не вызывается, добавленный
   — со следующего раунда; уведомитель держит сильную ссылку на себя до конца раунда.
6. Запись открывается только через `WriterSource`; в `build` публичного пути к `Writer` нет.
7. Идентичность графа и задачи не переиздаётся: исчерпание счётчика — постоянный отказ.
8. Задачи опрашиваются только в `MidFrameMicrotasks` кадра или в `pump_background` без кадра
   (`pump.rs:123`, после `finish_async_pump`), никогда в build/layout/paint.
9. Единственная сильная ссылка на `TaskStore` — у realm; handle виджетов — `Weak`.
10. Мутатор контроллера во время `build` не меняет состояние и не уведомляет.

## Ошибки и отказы

| Сценарий | Поведение |
|---|---|
| Поздний результат IO после уничтожения realm | IO-future (`Send`) шлёт в oneshot; приёмник был в `!Send` задаче, уничтоженной вместе с `TaskStore` на owner → отправка отказывает, `Send`-данные освобождаются на рабочем потоке; `!Send` захватов там нет по типам |
| Поздний wake задачи | `Weak<WakeShared>` не поднимается → no-op; id задачи монотонен на драйвер и не переиздаётся; новый realm — новая аллокация, `Weak` старой туда не ведёт |
| Поздний `SignalSender` при переиспользованном id графа | id графа — `u64`, `checked_add`, `expect("BUG: …")` (строка 16a); сегодня `u32` переиздаётся (`reactive/mod.rs:192`) |
| `FrameWaker` после закрытия realm | хук через `Weak` — no-op; будит только свой realm (вклад в починку wake из realm-model) |
| Утёкшее состояние виджета с `AsyncDriver` | `Weak`: задачи и захваты уходят с realm; `spawn_local` на мёртвом handle — future уничтожен, токен отменён |
| Паника в listener/status/post-frame | раунд продолжается, первая паника возобновляется после раунда и попадает в отчёт паник realm; записи до паники применены; следующий dispatch и кадр работают |
| Вложенная запись паникует внутри `EventCx` | поднимается через `source.write` во внешний callback, становится его первой паникой; записи до неё применены |
| Паника в `Drop` захвата при замене | заменённое значение выносится и уничтожается после отпускания borrow под тем же catch; первая паника сохраняется до конца восстановления |
| Реентрантная запись: listener пишет сигнал, чей listener снова пишет | применяется сразу, читатели планируются один раз на кадр; повторная запись в слот на займе — `SignalError::Reentrant`, не `BorrowMutError` |
| `!Send` захват в `Drop` при teardown | порядок полей realm из teardown design: post-frame очередь, `TaskStore`, writers — `Option`/`ManuallyDrop`, уничтожаются явно на owner до `resume_unwind` |

## Тестовая стратегия

Новый контракт (НК) сегодня падает; регрессионный guard (РГ) проходит. Consumer — внешний крейт
на `flui` и отдельный на `flui-sdk` (механизм `tests/facade_consumer.rs`).

| R | Тест | Вид | Почему падает сегодня |
|---|---|---|---|
| R1 | trybuild `tests/ui/thread_boundary/` в новом корневом `tests/thread_boundary_ui.rs` (группа `trybuild` в `.config/nextest.toml`): `animation_controller_stays_on_its_thread`, `scroll_controller_stays_on_its_thread`, `cupertino_tab_controller_stays_on_its_thread`, `async_driver_stays_on_its_thread` (НК); `signal_…`, `writer_source_…`, `writer_…`, `event_cx_…`, `state_cell_…`, `tab_controller_stays_on_its_thread` (РГ) | trybuild, `.stderr` со строкой ``within `AnimationController` `` | НК-фикстуры компилируются: trybuild «expected compile failure» |
| R2 | `owner_local_values_fit_every_flip_position` — consumer через `flui` и через `flui-sdk`, по `Rc<Cell<_>>`-реализатору или замыканию на строку таблицы (B-строки выше) | consumer compile | E0277: `Rc<Cell<u8>>` cannot be sent/shared |
| R3 | `cargo xtask thread-boundary` с целевым allowlist; `--self-test` | gate | supertrait `Listenable: Send` и др. не в keep |
| R4 | `cross_thread_handles_stay_sendable` — consumer шлёт `SignalSender`, `RebuildHandle`, `IoFuture`, `FrameWaker` в `thread::spawn` | consumer compile | `FrameWaker` не существует (НК); остальное РГ |
| R5 | скан `unsafe impl` в `thread-boundary`, `--self-test`; `cargo xtask deps` с баном `send_wrapper` | gate | РГ (allowlist равен сегодняшнему) |
| R6 | `event_setters_infer_cx_through_the_facade` — inline-замыкание без аннотаций на setter семейства | consumer compile | РГ |
| R7 | таблица `owner_local_write_paths` (`flui-widgets/tests`, `flui-testing` на виртуальных часах): `listener_writes_through_a_captured_writer_source`, `animation_status_writes_through_a_captured_writer_source`, `post_frame_callback_writes_through_a_captured_writer_source`, `local_history_removal_writes_after_the_flush`, `tab_controller_listener_writes_from_an_event_callback`, `cross_thread_signal_write_opens_the_owner_writer_source` | nextest | не компилируется. Мутация: контроллер уведомляет, держа borrow своего `RefCell` → перестроенный подписчик читает контроллер → `BorrowError` |
| R8 | trybuild `signal_write_through_build_context` дополнен `ctx.reactive()`; новая `headless_binding_exposes_no_reactive_graph` | trybuild | `reactive()` существует → `.stderr` не совпадает |
| R8b | `state_cell_schedule_during_build_is_refused`; `controller_mutation_during_build_is_refused` (строки `TabController`, `CupertinoTabController`, `ScrollController`, `AnimationController`) | nextest | сегодня планирует и мутирует |
| R9 | `worker_write_is_visible_on_the_next_pump`; `owner_local_future_completes_after_a_worker_wake` (W1) | nextest | первый РГ; второй — `!Send` future не компилируется |
| R10 | `late_completion_after_realm_drop_drops_captures_on_the_owner` (`DropLedger` + id потока); `exhausted_graph_ids_are_refused_for_good` (in-`src`: нужен приватный шов счётчика) | nextest | второй: счётчик переиздаёт id |
| R11 | матрица `owner_callback_panic_containment`: «одиночный сбой», «два сбоя в конкуренции», «следующий dispatch после локализации» для listener, status и post-frame; `nested_write_panics_inside_an_event_cx`; `listener_panic_reaches_the_realm_report` | nextest | не компилируется. Мутация: убрать catch раунда (локализация) → второй слушатель не вызван; вернуть «только `error!`» → отчёт realm пуст |
| R12 | `notifier_reentry`: `listener_write_triggers_a_second_write`, `listener_replaced_from_inside_itself`, `last_owner_released_during_notify`, `nested_writer_source_inside_an_event_cx`, `listener_drop_reads_the_controller_during_dispose`, `last_controller_rc_dropped_at_round_end` | nextest | первые три не компилируются. Мутации: обход живой карты под borrow вместо снимка → `BorrowMutError`; снимок/заменённый уничтожается под borrow → `Drop`, зовущий `remove_listener`, паникует; без сильной ссылки на себя → `DropLedger` видит `Drop` до конца раунда. Четвёртая — РГ порядка |
| R13 | `owner_local_captures_drop_once_on_the_owner_thread`: замена, unmount, teardown | nextest | не компилируется. Мутация: заменённый callback уничтожается под borrow реестра или клон снимка утекает → счётчик `Drop` ≠ 1 |
| R14 | `cargo xtask check-changed`, `cli_create::generated_*`, `external_notes_showcase_runs_through_the_facade` | CI | — |
| R15 | `wasm_consumer_captures_rc_in_listener_and_post_frame` в `cargo xtask wasm-check`; `cargo xtask cross-typecheck` | wasm consumer | E0277 |
| R16–R18 | `cargo xtask changelog --check`; ревью по списку; `cargo xtask docs-paths`, `rg "render.rs:451"` пуст | gate/ревью | — |

Мутации из таблицы прогоняются в изолированном checkout (AGENTS.md). `assert_impl_all!(UpdateScheduler:
Send, Sync)` (`post_frame.rs:304`) удаляется в пользу gate и trybuild. `.stderr` R1 называет и
приватные типы (`Rc<RefCell<ControllerInner>>`): переснимок при смене внутренностей принят сознательно.

## ADR

1. **ADR-0086 → Accepted, с поправкой §5.** Новый текст §5: «Listener, animation-status и
   post-frame callbacks теряют `Send`, но не получают `cx`: `Listenable` живёт в
   `flui-foundation` ниже графа. Они пишут через `WriterSource`, захваченный в
   `init_state`/`did_change_dependencies`/`RenderObjectContext`. Вложенное открытие `EventCx`
   разрешено и не транзакционно; записи до паники остаются применёнными; мутация контроллера во
   время `build` отказывает». Outstanding закрыт: `LocalHistoryEntry::on_remove` (вне `build`),
   `UiCommand::SignalWrite` через `WriterSource` (ADR-0074 §5.8), `&Reactive` (§8 шаг 3),
   `StateCell::schedule` (§7).
2. **ADR (номер назначит оркестратор) «UI surfaces are owner-local; one thread-boundary
   ledger».** (1) flip/keep с причинами, ledger `tools/xtask/allowlists/thread-boundary.toml`,
   gate `cargo xtask thread-boundary`; (2) `OwnerFrame` у realm (единственная сильная ссылка),
   `AsyncDriver` — `Weak`, через поток — только `FrameWaker` (защёлка из (b)) и `Waker`; (3) одна
   post-frame очередь; (4) контроллеры `Rc<RefCell>`, мутация в `build` отказывает; (5) паника
   слушателя — после раунда в отчёт realm; (6) никаких `unsafe impl Send/Sync` для UI-типов;
   (7) параллельная раскладка — opt-in `IndependentLayout` и шесть запретов (e). Amends ADR-0027
   §2 и §9 (§10 получает маркер); закрывает абзац ADR-0091 §1 о переходе.
3. **ADR-0091:** §1 заменяет ADR realm-model (D4, номер назначит оркестратор); для R17 — Accepted
   для §2–§6, `Superseded-by` для §1. **ADR-0089 §6:** загрузчик строит `thread-boundary`.

## Работы

Слитое в `main` в любой момент — согласованная модель: окончательный async-драйвер, путь
записи, ослабления — да; противоречивая половина перехода — нет. Ядро идёт интеграционной
веткой `send-flip/core` и сливается одним PR (или не сливается до 0.3).

| # | Работа | Ветка | Дни | Зависит | [P] / файлы | Linux remote |
|---|---|---|---|---|---|---|
| 1 | **`AsyncDriver`/`FutureBuilder` в окончательной форме**: `OwnerFrame` (задачи + post-frame), `Weak` `AsyncDriver`, `FrameWaker`-протокол, входы кадра с `&OwnerFrame`, `pump_background`, явный drain в teardown; trybuild `async_driver_stays_on_its_thread` | `main`, мерж до 10-14 | 6 | — | `flui-scheduler/src/{async_driver,scheduler,post_frame}.rs`, `flui-view/src/element/future_builder.rs`, `flui-widgets/src/image/resolve.rs`, `flui-runtime/src/{realm_services,ui_realm/*}.rs`, `flui-testing/src/{lib,bootstrap}.rs`, тесты входов кадра | да; `cross-typecheck` для runners |
| 2 | Gate `thread-boundary`: загрузчик rustdoc JSON (общий с `api-closure`), ledger в режиме храповика внутри `doc-strict`, self-test fixtures на 8 позиций в `cargo test -p xtask`, скан `unsafe impl`, баны `deny.toml`, замер времени джоба `doc` | `main` | 6 | — | [P] `tools/xtask/**`, `deny.toml`, allowlist | да |
| 3 | Путь записи: `SignalWrite` через `WriterSource`, удаление `&Reactive`/`reactive()`, guard `StateCell`, `on_remove` вне build, id графа `u64` | `main` | 4 | — | [P] `flui-view/src/{reactive,context,owner}/**`, `flui-foundation/src/read_scope.rs`, `flui-runtime/src/ui_realm/commands.rs`, `flui-widgets/src/navigator/{local_history,modal_route}.rs` | да |
| 4 | Ослабления bound'ов параметров (строки R) | `main` | 2 | 1 | [P] `animated/*`, `hero.rs`, `render.rs`, tween/proxy | да |
| 5 | C1: снятие supertrait/alias (B-строки 5–8, 11–13, 15–19, 23–30, 32, 37) и call sites по рецепту 1–3 | core | 4 | 1, 4 | один исполнитель | да |
| 6a | C2: планировщик owner-local, слияние post-frame в `OwnerFrame`, ticker | core | 5 | 5 | [P] `flui-scheduler/**`, `flui-app/src/app/runner/**`, `flui-runtime/src/ui_realm/frame*.rs` | да; runners — `cross-typecheck` |
| 6b | C2: нотификаторы foundation (раунд по снимку, паника после раунда), `ViewKey`, алиасы | core | 3 | 5 | [P] `flui-foundation/**` | да |
| 6c | C2: `AnimationController`, `Vsync`, `Simulation`, `MutatedDuringBuild` | core | 4 | 5 | [P] `flui-animation/**` | да |
| 6d | C2: `HitMetadata`, делегаты, `ScrollPosition`, hit-test, semantics | core | 4 | 5 | [P] `flui-rendering`, `flui-objects`, `flui-interaction`, `flui-semantics` | да |
| 6e | C2: контроллеры виджетов (кроме `TextEditingController`), конструкторы `impl Delegate`, пакеты, examples, шаблоны | core | 4 | 5 | [P] `flui-widgets` (кроме `text/controller.rs`), `packages/**`, `examples/**`, `flui-cli/src/templates` | да |
| 7 | Закрытие: consumer R2/R4/R6/R15, trybuild R1, матрицы R10–R13 (пишутся в 6a–6e), целевой allowlist, docs (R18), changelog, ADR | core | 4 | 6a–6e, 2, 3 | — | да |
| 8 | Нативный smoke Notes на Windows (`cargo xtask device windows-notes`) на SHA PR ядра | — | 0,5 | 7 | — | нет |
| 9 | `TextEditingController`: внутренний `Arc<Mutex>` → `Rc<RefCell>`; `with_inner_silent` (`text/controller.rs:994`) зовёт замыкание под замком — переделка с выносом состояния, не механика | core после text-ime T5 (или `main` после ядра) | 2 | 6e, T5 | `crates/flui-widgets/src/text/controller.rs` | да |

Пункт 9 контракт не меняет: `TextEditingController` становится `!Send` через свой `ChangeNotifier`
(строки 14, 36), поэтому мерж ядра он не держит.

**Не трогаются** (focus-keyboard не блокируется): `crates/flui-widgets/src/interaction/raw_button.rs`
(нет `Send`/`Arc`); сигнатуры 33 `on_*` в `packages/flui-material` и `packages/flui-cupertino` (уже
`EventCx`); `crates/flui-interaction/src/routing/focus_scope.rs` (`FocusNode` уже на `Rc`);
`crates/flui-interaction/src/recognizers/**` и арена (ADR-0086 §4); `crates/flui-platform/**`,
`crates/flui-platform-api/src/text_store/**`, `crates/flui-engine/**`. В `gesture_detector.rs` — одна строка (`:711`).

Итого ≈ 48 человеко-дней. **Критический путь:** 1 (6) → 5 (4) → 6a (5) → 7 (4) → ревью, smoke и
буфер (5) = 24 рабочих дня: 10-06 → 10-13 (1), 10-14 → 10-19 (5), 10-20 → 10-26 (6a),
10-27 → 10-30 (7), 11-02 → 11-06 ревью; **цель мержа ядра ~11-09**. До отсечки 11-24 — 11 рабочих
дней запаса (коэффициент ≈1,45 на критический путь). Пункты 2, 3, 4 и 6b–6e идут параллельно с
непересекающимися файлами; каждый 6x — подветка ядра с CI. Ветка ядра перебазируется на `main`
еженедельно; пункт 5 повторяется по рецепту, а не переносится конфликтами. GPU не нужен; Windows —
только пункту 8.

## Риски

1. **Конфликты интеграционной ветки (главный).** Пункт 5 касается ~100 файлов во всех крейтах,
   пока в `main` идут persistence, teardown, **text-ime** (`flui-widgets/src/text/**`,
   `TextEditingController`, T5) и authoring-styles. Смягчение: механика пункта 5 воспроизводится
   скриптом (ast-grep); `text/controller.rs` вынесен в пункт 9 после T5; широкие рефакторинги
   `flui-widgets` в окне 10-27 → 11-09 согласует оркестратор. Если 11-17 ядро не review-ready,
   оркестратор готовит объявление для 0.3 (D3); в 0.2.0 остаются пункты 1–4, они согласованы.
2. **Формат и время rustdoc JSON.** `RUSTC_BOOTSTRAP=1` на закреплённом stable; смена toolchain
   меняет `FORMAT_VERSION` и требует обновить `rustdoc-types` в том же PR (`cargo xtask toolchain`
   сверяет). Проход с `--document-private-items` — в джобе `doc` (25 минут); иначе — по крейтам.
3. **Новые отказы во время `build`.** Мутация контроллера в `build` раньше проходила молча; код,
   который так делает, меняет поведение. Смягчение: `warn!` с именем контроллера, строка в changelog.
