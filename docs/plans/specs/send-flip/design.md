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

- **Планировщик целиком `Send + Sync`.** `UpdateScheduler` — `Arc<SchedulerInner>` под
  `parking_lot::Mutex` (`crates/flui-scheduler/src/scheduler.rs:1007`); очереди хранят
  `Send`-замыкания: microtask и idle (`scheduler.rs:836-838`), transient/persistent/post-frame
  (`frame.rs:712`, `:720`, `:726`), lifecycle (`frame.rs:416`), timings (`config.rs:29`), ticker
  (`ticker.rs:77`, `:92`). Закреплено `assert_impl_all!(UpdateScheduler: Send, Sync)`
  (`post_frame.rs:304-305`). Межпоточного вызова в production нет: `thread::spawn` рядом с
  планировщиком встречается только в тестах (`async_driver.rs:1121`).
- **Owner-local дорожка уже есть.** `LocalPostFrameLane`/`LocalPostFrameHandle` — `!Send`, на
  `Rc` (`post_frame.rs:45`, `:169`); кадр получает дорожку параметром
  (`scheduler.rs:1542` `end_frame_with_lane`). `LifecycleContext` выдаёт оба handle
  (`crates/flui-view/src/context/build_context.rs:429`, `:448`). Это прецедент разделения,
  которое (b) доводит до конца.
- **`AsyncDriver` внутри планировщика.** `BoxedTask: Send` (`async_driver.rs:106`), задачи и
  очередь готовности под одним `Mutex` в `Arc<Inner>` (`:181`), `TaskToken` держит `Weak<Inner>`
  (`:249`). Опрос — в `handle_begin_frame`, слот `MidFrameMicrotasks` (`scheduler.rs:1371-1380`,
  `:2161`). `RealmServices` хранит клон (`crates/flui-runtime/src/realm_services.rs:21`, `:57`),
  `UiRealm` дёргает `drive_async_tasks` (`ui_realm/pump.rs:123`).
- **`FutureBuilder`.** `BoxedResultFuture: Send` (`crates/flui-view/src/element/future_builder.rs:62`),
  `K: Send + Sync`, `T, E: Send` (`:121-127`, `:156-158`), снимок в `Arc<Mutex>`, задача —
  `spawn_local_eager` (`:278`). Notes грузит через него (`examples/two_screens/tree.rs:260`).
- **Контроллеры.** `AnimationController` — `Arc<Mutex<Inner>>` + `Arc<ChangeNotifier>`
  (`crates/flui-animation/src/controller.rs:257`), `Vsync` — `Arc<Mutex>` (`vsync.rs:141`),
  `ScrollController` (`scroll_controller.rs:120`), `CupertinoTabController` (`tab_scaffold.rs:86`),
  а также `TextEditingController` (`text/controller.rs:256`), `WidgetStatesController`
  (`widget_state.rs:340`), `TransformationController`, `RefreshController` — `Send` через `Arc`.
  `TabController` уже `Rc<Cell>` (`packages/flui-material/src/tab_controller.rs:166`).
- **Слушатели.** `ListenerCallback = Arc<dyn Fn() + Send + Sync>` и `Listenable: Send + Sync`
  (`crates/flui-foundation/src/notifier.rs:45`, `:76`); `ChangeNotifier` на `Mutex` (`:142`).
  `add_listener(`: 138 строк, из них 54 — `add_listener(Arc::new(`; `add_status_listener(`: 52.
- **Путь записи.** `WriterSource::write` открывает новый `Writer` на каждый вызов, вложенность не
  проверяется (`crates/flui-view/src/reactive/writer.rs:233`). Запись не транзакция: при панике
  частичное значение коммитится и читатели планируются (`reactive/mod.rs:61-66`); ADR-0086 §6:
  независимо открытые `EventCx` — не одна транзакция. `&Reactive` пока `WriteTarget`, 30 вызовов
  `.reactive()` (почти все в тестах). `UiCommand::SignalWrite` несёт
  `Box<dyn FnMut(&Reactive) + Send>` (`crates/flui-runtime/src/ui_realm/commands.rs:109-113`).
- **Идентичность графа.** `NEXT_GRAPH_ID.fetch_add` переполняется и переиздаёт id
  (`reactive/mod.rs:192`): после 2³² графов поздний `SignalSender` может попасть в новый граф с тем
  же id. Нарушает правило «Identity is not a label» AGENTS.md.
- **Рендер.** `RenderObject` без `Send` (`render_object.rs:176`), `RenderTree` `!Send`
  (`storage/tree.rs:23`), арена раскладки `!Send` по `NodePtr` (`pipeline/owner/subtree_arena.rs:19-25`),
  `BoxLayoutCtxErased` без `Send` (`protocol/box_protocol.rs:832`). Требует `Send` только
  `RenderView::RenderObject` (`crates/flui-view/src/view/render.rs:511`) и
  `sliver_persistent_header.rs:169`. Делегаты принимаются как `Arc<dyn …>` в widget-API
  (`crates/flui-widgets/src/paint/custom_paint.rs:35`, `layout/flow.rs:34`,
  `custom_single_child_layout.rs:23`, `custom_multi_child_layout.rs:84`, `flui-objects/src/sliver/sliver_grid.rs:197`).
- **`unsafe impl Send/Sync`** в коде есть только в `flui-platform` (46), `flui-hot-reload` (4) и
  `tools/desktop-mcp` (2); в `flui-engine`, `flui-rendering`, `flui-scheduler` совпадения — комментарии.
- **Фасад.** `flui::foundation`, `flui::animation`, `flui::widgets`, `flui::view` реэкспортируют
  крейты целиком (`src/lib.rs:163-196`); `flui::view` несёт `AsyncDriver`, `BoxedTask`,
  `PostFrameHandle`, `LocalPostFrameHandle`, `TaskToken` (`crates/flui-view/src/lib.rs:203-206`);
  `flui::painting::CustomPainter` (`src/painting.rs:17`). `flui-sdk` — те же модули
  (`crates/flui-sdk/src/lib.rs:27-43`).

## Варианты

### (a) Owner-local контроллеры

| | a1 `Rc<RefCell<Inner>>` (выбран) | a2 handle в slab realm (GPUI `Entity<T>`) | a3 на сигналах (Dioxus/Leptos Copy signals) |
|---|---|---|---|
| Чтение | `controller.value()` где угодно на owner | `cx.read(&handle)`: нужен контекст | `signal.get(scope)`: нужен `ReadScope` |
| Paint/layout | работает (`Animation<T>::value()` без контекста) | нет контекста в `paint` → ломает `Animation<T>` | нет графа в paint (ADR-0085 §3 не сделан) |
| Тик | ticker пишет напрямую | через `cx` | нужен `Writer` в ticker → `EventCx` в планировщике |
| Teardown | последний `Rc` — `Drop` на owner | realm владеет, детерминирован | граф presentation, `ForeignGraph` между окнами |
| Цена | механическая: `Arc→Rc`, `Mutex→RefCell` | переписать `Animation<T>` и все чтения | то же плюс межграфовые отказы |

Выбор a1: форма как у `TabController` и `StateCell`, API контроллеров не меняется, кроме
auto-trait. Дисциплина заимствований повторяет нынешнюю дисциплину замков: состояние фиксируется,
исходящие значения выносятся, borrow отпускается до вызова пользовательского кода (status-listener
уже зовётся после отпускания замка, `controller.rs:231`). GPUI (`crates/gpui/src/executor.rs`:
`ForegroundExecutor` с `PhantomData<Rc<()>>`, `spawn` без `Send`) подтверждает форму «UI-объекты
на одном потоке, `Send` только у фоновых задач». a3 остаётся аддитивным адаптером на потом
(`controller.as_signal()`), без ломки. Требования: R1, R2, R13. ADR: ADR-0027 §2 выполняется.

### (b) Разделение планировщика: `Send`-wake и owner-local очереди

- **b1.** `UpdateScheduler` остаётся `Send`; на каждое семейство callback добавляется owner-local
  дорожка, как `LocalPostFrameLane`. Две очереди на семейство и правило их чередования
  (`build_context.rs:432-441` уже тратит абзац на порядок двух post-frame очередей). Отклонён.
- **b2 (выбран).** `UpdateScheduler` и `AsyncDriver` — owner-local (`Rc`, `RefCell`/`Cell`).
  Через поток ходит один узкий тип `FrameWaker: Clone + Send + Sync` — «запросить кадр» и
  «пометить задачу готовой»: атомарный флаг, хук платформы (замок инфраструктуры, держится на
  вставку и выемку) и очередь готовых id задач. Post-frame дорожки сливаются в одну очередь;
  `LocalPostFrameLane` и `*_with_lane` исчезают. Waker задачи держит `Weak<WakeShared>`,
  задачи — `Rc<RefCell<TaskStore>>` у realm. Та же форма у GPUI (`ForegroundExecutor`) и у
  `futures::executor::LocalPool`: `Waker` всегда `Send + Sync`, будущее — нет.
- **b3.** Регистрации через командный канал. Лишняя задержка на каждый `add_listener`; отклонён.

**Первая задача — только `AsyncDriver`/`FutureBuilder`** (порядок оркестратора). Пока
`UpdateScheduler` ещё `Send`, хранить `!Send`-хранилище задач в нём нельзя (R5 запрещает
`unsafe impl Send`). Поэтому драйвер переезжает из `SchedulerInner` в owner-local
`LocalPostFrameLane` (переименование в `OwnerLanes`, `#[doc(hidden)]`), а в планировщике остаётся
`Send`-сторона пробуждения. Опрос остаётся в `MidFrameMicrotasks`: кадр с дорожкой опрашивает её
задачи там же, где сейчас (`scheduler.rs:1380`). Это ослабление контракта (`!Send`-future
принимаются), а не противоречие: оно может уйти в `main` в октябре и выйти даже без остального
перехода. Persistence получает `load() -> impl Future + 'static` без временного `Arc<Mutex>`.
Требования: R4 (wake), R9, R10, R13. Цена: 4 дня. ADR: новый ADR (ниже) §2; ADR-0047 не меняется
(`IoFuture` остаётся `Send`).

### (c) Запись из listener через захваченный `WriterSource`; вложенный `EventCx`

- **Сигнатуры слушателей не меняются** (решение спеки): `add_listener(Rc<dyn Fn()>)`,
  `add_status_listener(Rc<dyn Fn(AnimationStatus)>)`, post-frame `FnOnce(&FrameTiming)`. Пишут
  через `WriterSource`, полученный в `init_state`/`did_change_dependencies`/`RenderObjectContext`.
- **Вложенный `WriterSource::write` внутри живого `EventCx`** (`TabController::set_index` из
  `on_tap` → listener → `source.write`): c1 — разрешить, записи применяются сразу в порядке вызова
  (как сегодня; ADR-0086 §6 уже говорит, что независимые `EventCx` не транзакция); c2 — отказ
  `EventContextError::Nested`, ломает `TabController`-сценарий R7; c3 — присоединять к внешней
  транзакции, требует журнала. **Выбран c1**, порядок документируется: запись видна следующему
  чтению сразу, читатели планируются на кадр, вложенный listener выполняется синхронно до возврата.
- **Откат записей при панике (R11).** Журнал отката возможен только для `set`: `update` меняет `T`
  на месте, и без `T: Clone` прежнее значение не восстановить. Смешанное правило (`set`
  откатывается, `update` нет) даёт несогласованные пары сигналов — хуже, чем «всё применено».
  **Выбор:** записи до паники остаются применёнными и видимыми (ADR-0074 unwind consistency,
  `reactive/mod.rs:61-66`), паника локализуется, следующий dispatch работает. Это расходится с
  буквой R11 — вопрос владельцу (см. «Риски»).
- **`UiCommand::SignalWrite`** несёт `Box<dyn FnOnce(&mut EventCx<'_>) + Send>`; realm открывает
  `EventCx` через `WriterSource` presentation, чей граф выпустил слот (ADR-0085 §1). Envelope —
  по ревизии ADR-0086 от 2026-09-28 (при панике удерживается).
- **`LocalHistoryEntry::on_remove(impl Fn() + 'static)`** — сигнатура та же; контракт доставки:
  никогда внутри `build`. Pop, применённый во время сборки, доставляет `on_remove` в owner-local
  post-frame очередь этого кадра (как `PageView::on_page_changed`, ADR-0086 §6).
- **`&Reactive` удаляется** (ADR-0086 §8 шаг 3): `WriteTarget for Reactive`,
  `BuildContext::reactive`, публичный `BuildOwner::reactive`, `HeadlessBinding::reactive`.
  Тесты пишут через `writer_source().write(..)`. `StateCell::schedule` во время `build`
  отказывает тем же guard'ом (ADR-0086 §7).
- Требования: R6–R8b, R12. ADR: ADR-0086 принимается с поправкой §5.

### (d) Gate R3: перечень `Send`/`Sync` поверхности

| | d1 rustdoc JSON + `rustdoc-types` в xtask (выбран) | d2 `cargo-public-api` | d3 только trybuild |
|---|---|---|---|
| Видит | supertrait, bound параметра и where, alias (`DynTrait`), поле, **синтетические auto-trait impl** каждого типа | текст сигнатур; auto-trait impl строками | только заранее перечисленные типы |
| Классификация | по позиции и пути элемента | регэкспы по тексту | — |
| Новое вхождение | падает | падает, если регэксп его узнал | не видит |
| Зависимости | `rustdoc-types` (MIT/Apache), toolchain из `rust-toolchain.toml` + `RUSTC_BOOTSTRAP=1` | внешний бинарник и nightly | нет |
| Общий код | загрузчик JSON делит с `api-closure` ADR-0089 §6 | — | — |

Выбор d1: команда `cargo xtask thread-boundary`. Строит rustdoc JSON для `flui` и `flui-sdk`
с замыканием реэкспортов, перечисляет каждое вхождение `Send`/`Sync` (позиция, путь, тип) и
auto-trait факты публичных типов. Сверяет с `tools/xtask/allowlists/thread-boundary.toml`:
`keep` — путь, позиция, причина, точные счётчики, только сокращаются; `flip` — запрет на
возврат; `owner-only` — типы, которые не должны быть `Send` (все `*Controller`, `AnimationController`,
`UpdateScheduler`, `AsyncDriver`, `Signal`, `WriterSource`, …); `sendable` — типы, которые обязаны
остаться `Send` (R4). `--self-test` на fixture-крейте (`pub trait T: Send {}`) доказывает, что gate
умеет падать (ADR-0081 §5). Вторая часть команды — скан `unsafe impl Send/Sync` через `syn` (не
регэксп: совпадения в комментариях не считаются) по всем членам с allowlist для `flui-platform`,
`flui-hot-reload`, `tools/desktop-mcp`. Скан работает без сборки и входит в `cargo xtask checks`.
Перечень по rustdoc JSON собирает workspace, поэтому идёт в CI-джоб с `doc-strict` и в
`cargo xtask gate`, а не в `checks`: AGENTS.md требует, чтобы `checks` не собирал workspace для
docs-only изменений. Это отклонение от слова «в `checks`» в R3, суть (gate на merge path) та же.
`deny.toml [bans]`: `send_wrapper`, `fragile`, `send-cell`, `unsend`, `force-send-sync`.
Риск формата: версия `rustdoc-types` привязана к `FORMAT_VERSION` закреплённого toolchain;
`cargo xtask toolchain` сверяет их. Требования: R3, R4, R5. ADR: ADR-0089 §6 (общий загрузчик).

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
  `std::thread::scope`. `Rc` через поток не идёт: каждый узел поддерева либо доказал `Send`
  маркером, либо развилка выполняется последовательно (проверка при вставке, флаг в узле). Рабочий
  поток получает `Send`-подконтекст без owner-возможностей. Нужна новая аргументация
  корректности `NodePtr` (ADR-0027 §10, не-цель «no reintroducing `Send` … without a fresh
  soundness argument»).
- **e2.** Оставить `Send + Sync` на рендер-слое «на будущее». Отклонено владельцем (D3):
  `CustomPainter` не может держать `!Send` контроллер.
- **e3.** Отдельное неизменяемое дерево раскладки (как taffy). Тоже аддитивно, но дублирует
  дерево; оценивается по профилю R13 уровня 0.

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

**Обратимо (можно исправить аддитивно):** делегаты хранятся стёрто (`Rc<dyn Delegate>`), поэтому
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
| 3 | `UpdateScheduler::{async_driver, spawn_local, spawn_local_eager, drive_async_tasks, pending_task_count}` | на `AsyncDriver`; планировщик их не несёт | B |
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
| 26 | `RenderObject::metadata`, `RenderBox::metadata`, `RenderSliver::metadata` → `Option<Arc<dyn Any + Send + Sync>>` | `Option<Rc<dyn Any>>` | B |

**flui-interaction, flui-semantics, flui-objects** (`flui::interaction`, `flui::rendering`, `flui::widgets`):
| 27 | `HitTestTarget: Send + Sync`, `CustomHitTestable: Send + Sync` | без supertrait | B×2 |
| 28 | `HitTestEntry::metadata(Arc<dyn Any + Send + Sync>)` | `Rc<dyn Any>` | B |
| 29 | `SemanticsActionHandler = Arc<dyn Fn(..) + Send + Sync>` | `Rc<dyn Fn(..)>`; к платформе уходит запрос, не handler | B |
| 30 | `MetaDataPayload`, `BuildDuringLayoutCell`, `PhysicalClipShape` | без `Send + Sync` | B×3 |

**flui-view** (`flui::view`):
| 31 | `RenderView::RenderObject: RenderObject<P> + Send + Sync + 'static` (и `sliver_persistent_header.rs:169`) | `+ 'static` | R |
| 32 | `trait Notification: Any + Send + Sync + 'static` | `Any + 'static` | B |
| 33 | `BoxedResultFuture<T, E>` (`+ Send`); `FutureBuilder<K: Send + Sync, T: Send, E: Send>` | без `Send`; `K: Clone + PartialEq + Debug + 'static`, `T, E: 'static` | B (alias), R (bounds) |
| 34 | `BuildContext::reactive()`, публичный `BuildOwner::reactive()`, `impl WriteTarget for Reactive` | удалены | B×3 |
| 35 | `LifecycleContext::local_post_frame_handle()`; `post_frame_handle() -> Option<PostFrameHandle>` (Send) | первый удалён; второй отдаёт owner-local handle | B |

**flui-widgets, пакеты** (`flui::widgets`, `flui::cupertino`):
| 36 | `ScrollController`, `TextEditingController`, `WidgetStatesController`, `TransformationController`, `RefreshController`, `PageController`: `Send` | `!Send` | B |
| 37 | `trait ScrollPhysics: Send + Sync + Debug` | `Debug` | B |
| 38 | `CustomPaint::{painter, foreground_painter}(Arc<dyn CustomPainter>)`, `Flow::new(Arc<dyn FlowDelegate>, ..)`, `CustomSingleChildLayout::new`, `CustomMultiChildLayout::new`, `RenderSliverGrid::new` | `Rc<dyn …>` | B×5 |
| 39 | `impl Curve + Send + Sync + 'static` в `animated/*`, `Hero::curve` | `impl Curve + 'static` | R |
| 40 | `CupertinoTabController: Send` | `!Send` (`Rc<Cell>`) | B |

**flui-testing** (`flui::testing`): 41 — `HeadlessBinding::reactive()` удалён (B); `spawn_local`
принимает `!Send` (R). **flui-runtime/flui-app** — изменения `pub(crate)`/`#[doc(hidden)]`:
`UiCommand::SignalWrite`, `OwnerLanes`, `RealmServices`.

**Keep (с причиной, вносится в allowlist gate):** `SignalSender<T>`, `RebuildHandle` (ADR-0018),
`IoFuture`, `ComputeJob`, `FrameWaker`, `set_on_frame_scheduled`-хук, `CloseGuard`/`CloseChanged`
(teardown, состояние presentation, доступное хосту), `Storage`/`StorageFuture`/`FlushRegistry`
(persistence, граница IO), `AnnotationValue` (слой уходит на raster), `Draggable<T: Send + Sync>`
(данные `ErasedDragData`), `ImageProvider` (процессный decode-кэш и IO), `WidgetsLocalizations`,
`Localizations::of<R: Send + Sync>` (ресурсы-данные; снять bound параметра позже — не ломка),
platform hooks (ADR-0082 §4). Строка «?» `TimingsCallback` решена как flip: её вызывает
`report_timings` на owner-потоке. Строки gate, которых нет в таблице requirements, классифицируются тем же правилом:
вызывается на owner → flip; данные, уходящие на raster/IO/worker → keep.

**Итого:** 41 строка; ломающих — **37 строк, 79 элементов API** (типы, трейты, alias'ы и методы
по отдельности), ослаблений — 6 строк. Новые публичные элементы: `FrameWaker`, `UpdateScheduler::frame_waker`,
`PostFrameScheduleError` (переименование `LocalPostFrameScheduleError`).

## Миграция

| Крейт | `add_listener(` | `add_status_listener(` | `post_frame_handle()` | delegate/trait impl | `Send`-токенов |
|---|---|---|---|---|---|
| flui-foundation | 22 | — | — | 3 | 53 |
| flui-scheduler | — | — | — | — | 49 (+46 `spawn_local`) |
| flui-animation | 23 | 39 | — | 12 | 64 |
| flui-interaction | 25 | — | — | — | 25 |
| flui-rendering / flui-objects | 8 / 7 | — | — | 13 / 7 | 60 / 15 |
| flui-view | 3 | — | 2 | 14 | 110 |
| flui-widgets | 26 | 8 | 16 | 7 | 151 |
| flui-runtime / flui-testing / flui-app | 1 / — / — | — | 1 / 12 / — | — | 61 / 10 / 54 |
| flui-material / flui-cupertino / flui-devtools | 17 / 2 / — | 5 / — / — | 1 / — / — | 6 / — / — | 20 / 2 / 1 |
| examples / tests (корень) | 4 / — | — | 4 / 2 | — / 1 | 7 / — |

`Send`-токены — верхняя граница (`rg -c 'Send \+ Sync|\+ Send\b'`); в `flui-platform` (152) и
`flui-engine` (26) почти все остаются. Рецепт (ast-grep и `rg`, как разрешено политикой репозитория):

1. `ast-grep -p '$R.add_listener(Arc::new($F))' -r '$R.add_listener(Rc::new($F))'`, то же для
   `add_status_listener`; импорт `std::rc::Rc`.
2. `rg -l 'Arc<dyn (CustomPainter|FlowDelegate|MultiChildLayoutDelegate|SingleChildLayoutDelegate|SliverGridDelegate|Simulation|Any \+ Send \+ Sync)>'`
   → `Rc<dyn …>`; `Arc::new(painter)` в этих позициях → `Rc::new`.
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
8. Опрос задач — один раз за кадр в `MidFrameMicrotasks`, никогда в build/layout/paint.

## Ошибки и отказы

| Сценарий | Поведение |
|---|---|
| Поздний результат IO после уничтожения realm | IO-future (`Send`) шлёт в oneshot; приёмник был в `!Send` задаче, уничтоженной вместе с `TaskStore` на owner → отправка отказывает, `Send`-данные освобождаются на рабочем потоке; `!Send` захватов там нет по типам |
| Поздний wake задачи | `Weak<WakeShared>` не поднимается → no-op; id задачи монотонен на драйвер и не переиздаётся; новый realm — новая аллокация, `Weak` старой туда не ведёт |
| Поздний `SignalSender` при переиспользованном id графа | счётчик графов: `checked_add`, исчерпание — `SignalError::GraphIdsExhausted` при создании графа, навсегда; без этого после 2³² графов запись попала бы в чужой граф (сегодня `reactive/mod.rs:192`) |
| `FrameWaker` после закрытия realm | хук через `Weak` — no-op; будит только свой realm (вклад в починку wake из realm-model) |
| Паника в listener/status/post-frame | PANIC-POLICY: catch на границе dispatch, первая паника главная, остальные слушатели раунда вызываются, записи до паники применены (см. (c)), следующий dispatch и кадр работают |
| Паника в `Drop` захвата при замене | заменённое значение выносится и уничтожается после отпускания borrow под тем же catch; первая паника сохраняется до конца восстановления |
| Реентрантная запись: listener пишет сигнал, чей listener снова пишет | применяется сразу, читатели планируются один раз на кадр; повторная запись в слот на займе — `SignalError::Reentrant`, не `BorrowMutError` |
| Замена/удаление callback изнутри него | снимок раунда; заменённый уничтожается после раунда |
| Освобождение последнего владельца handle внутри callback | уведомитель держит сильную ссылку до конца раунда; `Drop` после |
| Вложенный `WriterSource::write` в живом `EventCx` | разрешён (c1), порядок — порядок вызовов |
| `StateCell::schedule` в `build` | отказ guard'ом, `tracing::warn!` на `flui::signals` |
| `on_remove` при pop внутри сборки | доставка в post-frame очередь кадра |
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
| R7 | таблица `owner_local_write_paths` (`flui-widgets/tests`, `flui-testing` на виртуальных часах): `listener_writes_through_a_captured_writer_source`, `animation_status_writes_through_a_captured_writer_source`, `post_frame_callback_writes_through_a_captured_writer_source`, `local_history_removal_writes_after_the_flush`, `tab_controller_listener_writes_from_an_event_callback`, `cross_thread_signal_write_opens_the_owner_writer_source` | nextest | E0277: `WriterSource` в `Send`-callback |
| R8 | trybuild `signal_write_through_build_context` дополнен `ctx.reactive()`; новая `headless_binding_exposes_no_reactive_graph` | trybuild | `reactive()` существует → `.stderr` не совпадает |
| R8b | `state_cell_schedule_during_build_is_refused` | nextest | сегодня планирует |
| R9 | `worker_write_is_visible_on_the_next_pump`; `owner_local_future_completes_after_a_worker_wake` (W1) | nextest | первый РГ; второй — `!Send` future не компилируется |
| R10 | `late_completion_after_realm_drop_drops_captures_on_the_owner` (`DropLedger` + id потока); `exhausted_graph_ids_are_refused_for_good` (in-`src`: нужен приватный шов счётчика) | nextest | второй: счётчик переиздаёт id |
| R11 | матрица `owner_callback_panic_containment`: строки «одиночный сбой», «два сбоя в конкуренции», «следующий dispatch после локализации» для listener, status и post-frame | nextest | `Rc`-захват не компилируется в `Send`-callback |
| R12 | `notifier_reentry`: `listener_write_triggers_a_second_write`, `listener_replaced_from_inside_itself`, `last_owner_released_during_notify`, `nested_writer_source_inside_an_event_cx` | nextest | первые три — `Mutex`-нотификатор недоступен для `Rc`-захвата (не компилируется); четвёртая — РГ порядка |
| R13 | `owner_local_captures_drop_once_on_the_owner_thread`: замена, unmount, teardown | nextest | не компилируется сегодня |
| R14 | `cargo xtask check-changed`, `cli_create::generated_*`, `external_notes_showcase_runs_through_the_facade` | CI | — |
| R15 | `wasm_consumer_captures_rc_in_listener_and_post_frame` в `cargo xtask wasm-check`; `cargo xtask cross-typecheck` | wasm consumer | E0277 |
| R16–R18 | `cargo xtask changelog --check`; ревью по списку; `cargo xtask docs-paths`, `rg "render.rs:451"` пуст | gate/ревью | — |

Проверка «тест различает дефект» (AGENTS.md): для R7, R10, R12 — прогон с восстановленным
`Send` bound или `fetch_add` в изолированном checkout. In-`src` `assert_impl_all!(UpdateScheduler: Send, Sync)`
(`post_frame.rs:304`) удаляется: контракт переходит в gate и trybuild.

## ADR

1. **ADR-0086 → Accepted, с поправкой §5.** Новый текст §5: «Listener, animation-status и
   post-frame callbacks теряют `Send`, но не получают `cx`: `Listenable` живёт в
   `flui-foundation` ниже графа. Они пишут через `WriterSource`, захваченный в
   `init_state`/`did_change_dependencies`/`RenderObjectContext`. Вложенное открытие `EventCx`
   разрешено и не транзакционно; записи до паники остаются применёнными». Список Outstanding
   закрыт: `LocalHistoryEntry::on_remove` (доставка вне `build`), `UiCommand::SignalWrite` через
   `WriterSource` (ADR-0074 §5.8), удаление `&Reactive` (§8 шаг 3), `StateCell::schedule` (§7).
2. **Новый ADR «UI surfaces are owner-local; one thread-boundary ledger».** Решение:
   (1) классификация flip/keep с причинами; ledger в `tools/xtask/allowlists/thread-boundary.toml`
   — источник истины, gate `cargo xtask thread-boundary`; (2) планировщик и драйвер задач
   owner-local, через поток — только `FrameWaker` и `Waker`; (3) post-frame — одна очередь;
   (4) контроллеры — `Rc<RefCell>`; (5) никаких `unsafe impl Send/Sync` для UI-типов, бан
   `send_wrapper`-подобных крейтов; (6) параллельная раскладка — только opt-in `IndependentLayout`
   и пять запретов из (e). Amends ADR-0027 §2 (таблица: `UpdateScheduler`, `AsyncDriver` → `!Send`;
   `FrameWaker` → `Send`) и §9 (теперь истинно для `RenderObject`; §10 получает маркер как форму
   opt-in). Закрывает абзац ADR-0091 §1 о переходе как выполненный.
3. **ADR-0091.** §1 заменяет ADR из realm-model (D4); для R17 статус ADR-0091 — Accepted для
   §2–§6 и `Superseded-by` для §1. Согласует оркестратор вместе с ADR realm-model.
4. **ADR-0089 §6.** Строка: загрузчик rustdoc JSON общий для `api-closure` и `thread-boundary`.

## Работы

Слитое в `main` в любой момент должно быть согласованной моделью: ослабления и путь записи —
да, противоречивая половина перехода — нет. Поэтому ядро идёт интеграционной веткой
`send-flip/core` и сливается одним PR (или не сливается до 0.3).

| # | Работа | Ветка | Дни | Зависит | [P] / файлы | Linux remote |
|---|---|---|---|---|---|---|
| 1 | **Разделение `AsyncDriver`/`FutureBuilder`**: owner-local `TaskStore`, `Send` wake, `OwnerLanes`, `FutureBuilder` без `Send`, тесты R9-строки, trybuild `async_driver_stays_on_its_thread` | `main`, мерж до 10-12 | 4 | — | `flui-scheduler/src/{async_driver,scheduler,post_frame}.rs`, `flui-view/src/element/future_builder.rs`, `flui-runtime/src/{realm_services,ui_realm/pump}.rs`, `flui-testing/src/{lib,bootstrap}.rs` | да; `cross-typecheck` для runners |
| 2 | Gate `thread-boundary` (скан `unsafe impl`, rustdoc ledger в режиме храповика), `deny.toml` баны, `--self-test` | `main` | 4 | — | [P] `tools/xtask/**`, `deny.toml`, allowlist | да |
| 3 | Путь записи: `SignalWrite` через `WriterSource`, удаление `&Reactive`/`reactive()`, `StateCell` guard, `on_remove` вне build, исчерпание id графа | `main` | 4 | — | [P] `flui-view/src/{reactive,context,owner}/**`, `flui-runtime/src/ui_realm/commands.rs`, `flui-widgets/src/navigator/{local_history,modal_route}.rs`, тесты с `.reactive()` | да |
| 4 | Ослабления bound'ов параметров (строки R) | `main` | 2 | 1 | [P] `animated/*`, `hero.rs`, `render.rs`, tween/proxy | да |
| 5 | C1: снятие supertrait/alias (строки B 5–8, 11–13, 15–19, 23–30, 32, 37) и Arc→Rc в call sites по рецепту 1–3 | `send-flip/core` | 4 | 1, 4 | один исполнитель: широкая механика | да |
| 6a | C2: планировщик owner-local, `FrameWaker`, слияние post-frame, ticker | core | 5 | 5 | [P] `flui-scheduler/**`, `flui-app/src/app/runner/**`, `flui-runtime/src/ui_realm/frame*.rs` | да; runners — `cross-typecheck` |
| 6b | C2: нотификаторы foundation, `ViewKey`, удаление алиасов | core | 3 | 5 | [P] `flui-foundation/**` | да |
| 6c | C2: `AnimationController`, `Vsync`, `Simulation` | core | 4 | 5 | [P] `flui-animation/**` | да |
| 6d | C2: metadata, делегаты, `ScrollPosition`, hit-test, semantics | core | 4 | 5 | [P] `flui-rendering`, `flui-objects`, `flui-interaction`, `flui-semantics` | да |
| 6e | C2: контроллеры виджетов, setter'ы делегатов, пакеты, examples, шаблоны | core | 4 | 5 | [P] `flui-widgets`, `packages/**`, `examples/**`, `flui-cli/src/templates` | да |
| 7 | Закрытие: consumer R2/R4/R6/R15, trybuild R1, матрицы R10–R13 (пишутся в 6a–6e), целевой allowlist, docs и ARCHITECTURE.md (R18), changelog, ADR | core | 4 | 6a–6e, 2, 3 | — | да, кроме нативного smoke |
| 8 | Нативный smoke Notes на Windows (`cargo xtask device windows-notes`) на SHA PR ядра | — | 0,5 | 7 | — | нет (Windows) |

Итого ≈ 42 человеко-дня. **Критический путь:** 1 (4) → 5 (4) → 6a (5) → 7 (4) → ревью и 8 (3)
= 20 рабочих дней: 10-06 → 10-09 (1), 10-13 → 10-16 (5), 10-19 → 10-23 (6a), 10-26 → 10-29 (7),
PR ядра на ревью 10-30, цель мержа 11-04. До отсечки 11-24 — 14 рабочих дней запаса, это покрывает
коэффициент 1,5 на оценки. Пункты 2, 3, 4 и 6b–6e идут параллельно с непересекающимися файлами;
каждый 6x — подветка в `send-flip/core`, CI на каждой. Ветка ядра перебазируется на `main` каждую
неделю; пункт 5 повторяется по рецепту, а не переносится конфликтами. GPU ничему не нужен; Windows
нужен только пункту 8 и нативным прогонам уровня 0.

## Риски

1. **Конфликты интеграционной ветки (главный).** Пункт 5 касается ~100 файлов во всех крейтах,
   пока в `main` идут persistence, teardown, text-ime и authoring-styles. Смягчение: механика пункта
   5 воспроизводится скриптом (ast-grep), ветка ядра держится коротко (цель мержа 11-04), широкие
   рефакторинги `flui-widgets` в окне 10-26 → 11-04 согласует оркестратор. Если 11-17 ядро не в
   review-ready, оркестратор готовит объявление для 0.3 (D3), а в 0.2.0 остаются только пункты 1–4.
2. **R11 и откат записей.** Требование «записи до паники отбрасываются целиком» невыполнимо для
   `update` без `T: Clone`. Design выбирает «записи применены и видимы» (ADR-0074, ADR-0086 §6).
   Вопрос владельцу: принять формулировку или требовать журнал с `T: Clone` для `update`
   (+5 дней и поправка ADR-0074 §5.1).
3. **Формат rustdoc JSON.** Unstable-флаг на закреплённом stable через `RUSTC_BOOTSTRAP=1`;
   смена toolchain меняет `FORMAT_VERSION` и требует обновить `rustdoc-types` в том же PR.
   Смягчение: `cargo xtask toolchain` сверяет версию; при расхождении gate падает с понятным
   сообщением, а не молча пропускает.
