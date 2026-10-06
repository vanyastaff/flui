# `!Send` UI-поверхности и сигнатура event-callback — требования (уровень 1)

- **Статус:** черновик
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) R2, D3 уровня 0
- **Связанные:** [migration plan](../../2026-09-25-architecture-migration-plan.md) W5-A1..W5-A5;
  ADR-0027 §2, §9; ADR-0074; ADR-0075; ADR-0083; ADR-0086; ADR-0089; ADR-0091 §1;
  `design/decisions.md` D9, O7, A1, A3; `design/open-questions.md` №4, №8.

## Зачем

UI живёт на одном owner-потоке realm (ADR-0027, ADR-0091 §1), но часть публичных типов
требует `Send + Sync`: `Signal<T>`, `WriterSource` или `Rc` нельзя захватить в listener,
post-frame или animation-status callback, остаётся обход через `SignalSender`. Снять `Send` с
параметра — ослабление. Снять его с supertrait (`Listenable`, `Animation<T>`, `CustomPainter`)
— ломающее изменение для generic-кода каждого downstream-крейта. До публикации таких крейтов
нет; после 0.2.0 каждый шаг стоит нового train, правки tutorial и book и миграции пакетов на
`flui-sdk`. Поэтому D3 уровня 0 ставит flip перед публикацией; владелец подтвердил это с
отсечкой (см. «Решения спеки»).

## Текущее состояние

Код проверен на `2c067bbc5` (от базы отличается только `AGENTS.md`). **Уже сделано (W5-A1..A4, итоги 2026-09-26, 09-29, 09-30).** Есть `EventCx`, `Writer`,
`WriterSource`, `EventOutcome`, `callback`/`callback_with`
(`crates/flui-view/src/reactive/writer.rs:295`, `:362`, `:387`). Event-setter уже в целевой
форме `F: Fn(&mut EventCx<'_>) -> R + 'static, R: EventOutcome`
(`crates/flui-widgets/src/interaction/gesture_detector.rs:238`, `raw_button.rs:70`). Все 33 `pub fn on_*` в `packages/flui-material` и `packages/flui-cupertino` принимают
`&mut EventCx`. DragTarget и 14 action-setters Semantics работают через owner-local payload.
Pilot прошёл (ADR-0086 §9); ADR-0086 — Proposed. Gesture arena уже `!Send`
(`crates/flui-interaction/src/recognizers/tap.rs:88`), `RenderTree` тоже
(`crates/flui-rendering/src/storage/tree.rs:23`). Codemod `flui migrate` не написан.

**Handles сегодня.** `!Send` и закреплено: `Signal<T>` (`crates/flui-view/tests/signal_reads.rs:21`),
`WriterSource`, `Writer`, `EventCx` (`writer.rs:437-439`). `!Send` по устройству, без
закрепления: `StateCell` (`Rc`, `crates/flui-view/src/state_cell.rs:141`), `TabController`
(`Rc<Cell<_>>`, `packages/flui-material/src/tab_controller.rs:166`). Сегодня `Send`, и это
нужно изменить: `AnimationController` (`Arc<Mutex<_>>`,
`crates/flui-animation/src/controller.rs:257`), `ScrollController` (`Arc<Mutex<_>>`,
`crates/flui-widgets/src/scroll/scroll_controller.rs:120`), `CupertinoTabController`
(`Arc<AtomicUsize>`, `packages/flui-cupertino/src/tab_scaffold.rs:86`).

**Не сделано (ADR-0086 Status; W5-A5).** Предварительная классификация: «flip» — снять
`Send`/`Sync`, «keep» — оставить, с причиной. Строки с «?» решает design.md.

| Поверхность | Где | Класс |
|---|---|---|
| `ListenerCallback`, `Listenable`; `ArgCallback` | `crates/flui-foundation/src/notifier.rs:45`, `:76`; `notifier_generic.rs:29` | flip |
| `VoidCallback`, `ValueChanged`, `ValueGetter`, `ValueSetter`, `Predicate`, `ValueTransformer`, `FallibleCallback` | `crates/flui-foundation/src/callbacks.rs:72-189` | flip или удалить |
| `ViewKey` | `crates/flui-foundation/src/key.rs:363` | flip |
| `StatusCallback`, `Animation<T>`; `Simulation`; `impl Curve + Send + Sync` в сеттерах `animated/*` и `Hero` | `crates/flui-animation/src/animation.rs:11`, `:68`; `simulation.rs:72`; `crates/flui-widgets/src/animated/`, `navigator/hero.rs` | flip |
| Внутренности `AnimationController`, `ScrollController`, `CupertinoTabController` | см. «Handles сегодня» | flip: owner-local, без `Arc<Mutex>` |
| `OneShotFrameCallback`, `RecurringFrameCallback`, `PostFrameCallback`; `LifecycleStateCallback` | `crates/flui-scheduler/src/frame.rs:712`, `:720`, `:726`; `:416` | flip |
| `UpdateScheduler`, `PostFrameHandle` (закреплены `Send + Sync`) | `crates/flui-scheduler/src/post_frame.rs:304-305` | разделить: `Send` wake-handle + owner-local очередь |
| `BoxedTask`, `BoxedResultFuture`, `FutureBuilder`, `AsyncDriver` (futures виджетов `Send`) | `crates/flui-view/src/.../async_driver.rs:106`, `crates/flui-widgets/src/.../future_builder.rs:62` (уточнить пути в design) | разделить: `Send` waker + owner-local очередь `!Send` задач. Через границу IO ходят только `Send`-байты, как `IoFuture` (найдено ревью persistence) |
| `TickerCallback`, `TickerProvider` | `crates/flui-scheduler/src/ticker.rs:77`, `:92` | flip |
| `TimingsCallback` | `crates/flui-scheduler/src/config.rs:29` | ? зависит от потока вызова |
| `RenderView::RenderObject` | `crates/flui-view/src/view/render.rs:511` | flip |
| `metadata()`, `HitTestEntry::metadata`, `MetaDataPayload` | `crates/flui-rendering/src/traits/render_object.rs:659`, `render_box.rs:484`, `:842`, `render_sliver.rs:310`, `:574`; `crates/flui-interaction/src/routing/hit_test.rs:189`; `crates/flui-objects/src/interaction/meta_data.rs:37` | flip |
| `HitTestTarget`, `CustomHitTestable`; `SemanticsActionHandler` | `crates/flui-interaction/src/traits.rs:26`, `sealed.rs:136`; `crates/flui-semantics/src/action.rs:52` | flip |
| `CustomPainter` и 4 делегата layout/grid | `crates/flui-rendering/src/delegates/custom_painter.rs:103`, `flow_delegate.rs:71`, `multi_child_layout_delegate.rs:71`, `single_child_layout_delegate.rs:51`, `sliver_grid_delegate.rs:158` | flip |
| `ViewportOffset`, слушатели `ScrollPosition`, `StopHook` | `crates/flui-rendering/src/view/viewport_offset.rs:49`, `scroll_position.rs` | flip |
| `ScrollPhysics`; `BuildDuringLayoutCell`; `PhysicalClipShape` | `crates/flui-widgets/src/scroll/scroll_physics.rs:150`; `crates/flui-objects/src/layout/layout_constraints_cell.rs:61`; `proxy/physical_model.rs:66` | flip |
| `Notification` | `crates/flui-view/src/element/notification.rs:43` | flip |
| `ImageProvider` | `crates/flui-widgets/src/image/provider.rs:56` | ? keep, если resolve идёт на IO (`decode_cache.rs:165`) |
| `WidgetsLocalizations`, `Localizations::of<R: Send + Sync>` | `crates/flui-widgets/src/localization/widgets_localizations.rs:20`, `localizations.rs:333` | ? keep: ресурсы-данные, грузятся async |
| `AnnotationValue` | `crates/flui-layer/src/layer/annotated_region.rs:9` | keep: слой уходит на raster-поток |
| `Draggable<T: Send + Sync>` | `crates/flui-widgets/src/interaction/draggable.rs:190` | keep: данные (`ErasedDragData`, W5-A5) |
| `&Reactive`-путь записи | `crates/flui-view/src/context/build_context.rs:132`, `owner/build_owner.rs:1045`, `crates/flui-testing/src/lib.rs:601` | удалить |
| `LocalHistoryEntry::on_remove(impl Fn() + 'static)` | `crates/flui-widgets/src/navigator/local_history.rs:102` | путь записи (R7) |

`ScrollPhysics`, `ViewKey` и `BuildDuringLayoutCell` к flip относит и
`docs/research/2026-09-25-architecture-review/designs/performance_first.md:376`. **`Send` по замыслу (keep):** `SignalSender<T>` (`crates/flui-foundation/src/read_scope.rs:665`),
`RebuildHandle` (`crates/flui-view/src/owner/rebuild_handle.rs:90`, ADR-0018), `IoFuture`
(`crates/flui-runtime/src/execution.rs:69`), wakes кадров и build, platform hooks (ADR-0082),
платформенный шов accessibility.

**Масштаб.** `.add_listener`/`.add_status_listener`: 152 строки в `.rs` и 15 в `.md`
(72 файла, 15 из них в `packages/`). `add_post_frame_callback`/`schedule_frame_callback`:
24 вызова и 2 определения.

**Расхождения документации.** `crates/flui-rendering/ARCHITECTURE.md:854` пишет, что trait
`RenderObject` требует `Send + Sync` (в коде такого bound нет, `render_object.rs:176`), и что
`RenderTree` автоматически `Send + Sync` (это противоречит `tree.rs:23`). На устаревшую строку
`render.rs:451` (сейчас `:511`) ссылаются ADR-0091, `design/decisions.md:297`,
`design/architecture.md:666` и `design/open-questions.md:136`.

## Решения спеки

- **Полный flip до 0.2.0, с отсечкой (владелец, 2026-10-05; D3 уровня 0).** Весь класс flip
  из таблицы выходит до публикации 0.2.0. Если `send-flip` не слит к 2026-11-24, 0.2.0
  выходит без flip, а ломающий шаг объявляется для 0.3. RC — 2026-12-15.
- **Частичный flip не выпускается никогда (владелец, 2026-10-05).** Вариант «только listener,
  status, post-frame и ticker, а render-слой спрятать за ADR-0089» отклонён:
  `CustomPainter: Send + Sync` не может держать `!Send` `AnimationController` как источник
  repaint.
- **ADR-0086 принят с поправкой §5 (владелец, 2026-10-05).** Listener пишет через
  захваченный `WriterSource`, без `cx` (см. следующий пункт). R17 проверяет, что статус и
  поправка внесены в ADR.
- **Listener без `cx`.** Снимаем `Send`, сигнатура `add_listener` не меняется.
  Listener пишет через захваченный `WriterSource` (`source.write(|cx| ..)`). Получить его можно
  только в `init_state`/`did_change_dependencies` (и `RenderObjectContext`), а не в `build`.
  Причина — слои: `Listenable` живёт в `flui-foundation`, `EventCx` — в `flui-view`.
  **Обязательство:** внести в ADR-0086 поправку к §5, где снятие `Send` идёт в паре с `cx`
  в каждом семействе.
- **#1250 закрыт как rejected.** Он опирается на `get(&r)` через `&Reactive`,
  а этот путь здесь удаляется (R8). Кроме того, он меняет типизированную точку чтения на
  ambient.
- **Параллельный кадр не закрывается (D5 уровня 0, владелец, 2026-10-05).** Обязательство
  для design.md: показать, как после 0.2.0 добавить параллельную раскладку независимых
  поддеревьев без ломки опубликованного API. Например, opt-in маркер для render objects,
  который требует `Send` только от тех, кто его реализует. И перечислить, какие решения
  этого перехода были бы необратимы для такого пути.

## Требования

**Регрессионный guard** проходит на сегодняшнем коде; **новый контракт** сегодня падает.
Consumer-проверка — внешний крейт только на `flui` или только на `flui-sdk`, как
`external_notes_showcase_runs_through_the_facade`.

### Граница потока

- **R1.** КОГДА пользователь передаёт realm-bound handle в `std::thread::spawn` или в future с
  bound `Send`, СИСТЕМА ДОЛЖНА отказывать при компиляции, а в диагностике называть публичный
  тип. Rustdoc типа указывает путь через поток (R9). Trybuild fixture на каждый тип, `.stderr`
  закоммичен. Новый контракт: `AnimationController`, `ScrollController`,
  `CupertinoTabController`. Регрессионный guard: `Signal<T>`, `WriterSource`, `Writer`,
  `EventCx`, `StateCell`, `TabController`.
- **R2.** КОГДА пользовательский тип или замыкание содержит `!Send`/`!Sync` значение
  (`Rc<Cell<_>>`, `Signal<T>`, `WriterSource`), СИСТЕМА ДОЛЖНА принимать его без `unsafe` и
  обёрток в каждой позиции класса flip. Проверка: consumer-проверка через `flui` и через
  `flui-sdk`, по одному `!Send` реализатору или замыканию на строку таблицы. Новый контракт.
- **R3.** КОГДА меняется публичная поверхность `flui` или `flui-sdk`, СИСТЕМА ДОЛЖНА
  перечислять каждое вхождение `Send`/`Sync` в ней (supertrait, alias, bound параметра,
  поле публичного типа; по rustdoc JSON) и сверять его с таблицей классификации
  (flip/keep с причиной). Gate падает на вхождение, которого нет в таблице, и на возвращённый
  flip. Allowlist keep только сокращается. Проверка: `cargo xtask` gate в `checks`.
  Новый контракт.
- **R4.** КОГДА тип по замыслу пересекает поток (`SignalSender<T>`, `RebuildHandle`,
  `IoFuture`, wake-handle планировщика, platform hooks), СИСТЕМА ДОЛЖНА сохранять его
  `Send` (и `Sync`, где он есть сейчас). Проверка: consumer-проверка, которая отправляет
  каждый тип в `std::thread::spawn`. Регрессионный guard; для wake-handle — новый контракт,
  потому что он отделяется от `UpdateScheduler`.
- **R5.** КОГДА собирается крейт фреймворка или официальный пакет, СИСТЕМА НЕ ДОЛЖНА
  содержать новых `unsafe impl Send`/`Sync` для UI-типов. Существующие остаются только в
  `flui-platform` и `flui-hot-reload`, с `SAFETY:`. `deny.toml` запрещает `send_wrapper`,
  `fragile` и аналоги. Проверка: gate с сокращаемым allowlist в `checks`; `cargo xtask deps`.

### Сигнатура event-callback и путь записи

- **R6.** КОГДА пользователь передаёт inline-замыкание без аннотаций в любой
  framework-dispatched event-setter из facade или `flui-sdk`, СИСТЕМА ДОЛЖНА давать записать
  `Signal<T>` через параметр-контекст, ничего не захватывая; возврат `()` или результат
  записи. Таблица ADR-0086 §6, воспроизводимая её grep-командой, не содержит
  неклассифицированных `pub fn on_*`. Проверка: consumer-проверка по setter на семейство.
  Регрессионный guard.
- **R7.** КОГДА listener, animation-status, post-frame callback или `LocalHistoryEntry::on_remove`
  пишет `Signal<T>` через захваченный `WriterSource`, СИСТЕМА ДОЛЖНА применять запись на
  owner-потоке без `SignalSender` и перестраивать подписчиков не позже следующего кадра. Это
  относится и к listener, которого уведомляет метод приложения (`TabController`).
  `UiCommand::SignalWrite` открывает запись через `WriterSource` realm-владельца, а не через
  `&Reactive`. Проверка: nextest через `flui-testing` на виртуальных часах, строка таблицы на
  семейство. Новый контракт.
- **R8.** КОГДА `build` пытается записать сигнал (через `BuildContext`, `BuildOwner` или
  `flui-testing`), СИСТЕМА ДОЛЖНА отказывать при компиляции: публичного пути к `&Reactive`
  или `Writer` нет. Проверка: trybuild (`signal_write_through_build_context` дополнен
  `reactive()`). Новый контракт.
- **R8b.** КОГДА `StateCell::schedule` вызывается во время `build`, СИСТЕМА ДОЛЖНА отказывать
  так же, как при записи сигнала (ADR-0086 §7). Проверка: nextest, run-time отказ. Новый
  контракт.
- **R9.** КОГДА результат приходит с другого потока (IO-future, worker через `SignalSender`),
  СИСТЕМА ДОЛЖНА доставлять его в realm-владелец и применять до следующего кадра.
  Поведение загрузки в Notes задаёт спека `persistence`. Проверка: nextest headless — worker
  пишет, следующий `pump` показывает значение. Регрессионный guard.

### Сценарии отказа

- **R10.** КОГДА асинхронный результат или `SignalSender`-запись приходит после уничтожения
  realm или presentation, СИСТЕМА ДОЛЖНА отбрасывать его без паники, не писать в realm на том
  же слоте и освобождать захваты на owner-потоке. Проверка: nextest, счётчик и поток `Drop`.
- **R11.** КОГДА event-, listener- или post-frame callback паникует, СИСТЕМА ДОЛЖНА действовать
  по PANIC-POLICY: первая паника остаётся главной; записи этого callback, сделанные до
  паники, отбрасываются целиком; следующий dispatch того же handle и следующий кадр
  работают. Проверка: nextest-матрица — одиночный сбой, два сбоя в конкуренции, операция
  после локализации.
- **R12.** КОГДА callback синхронно возвращается в ту же подсистему (запись, чей listener
  снова пишет; замена callback изнутри него; освобождение последнего владельца handle;
  открытие вложенного `EventCx` через `WriterSource::write` внутри живого), СИСТЕМА ДОЛЖНА
  завершать dispatch без `BorrowMutError`, без потерянной записи и с документированным
  детерминированным порядком (или с документированным отказом для вложенного открытия).
  Проверка: nextest через публичный handle на каждый из четырёх путей.
- **R13.** КОГДА callback захватывает `!Send` значение со своим `Drop`, СИСТЕМА ДОЛЖНА выполнять
  `Drop` на owner-потоке ровно один раз (замена, unmount, teardown). Проверка: nextest.

### Миграция и платформы

- **R14.** КОГДА изменение слито, СИСТЕМА ДОЛЖНА собирать и проходить тесты на новых
  сигнатурах для Notes (`notes_public_input_flow_matrix` через внешний consumer),
  `flui-material`, `flui-cupertino`, `flui-devtools`, всех `examples/` и шаблона `flui create`.
  Проверка: `cargo xtask check-changed`, `cli_create::generated_*`, уровень 0 R1.
- **R15.** КОГДА собирается wasm32, СИСТЕМА ДОЛЖНА проходить `cargo xtask wasm-check` и
  собирать wasm32 consumer, который захватывает `Rc` в listener и в post-frame callback.
  Проверка: wasm32 consumer-проверка; `cargo xtask cross-typecheck` для Win32, AppKit,
  Android, iOS.
- **R16.** КОГДА публикуется 0.2.0, СИСТЕМА ДОЛЖНА иметь `changelog.d`-фрагмент, где каждая
  снятая граница записана «было → стало» со строкой миграции. Проверка: `changelog --check`.
- **R17.** КОГДА изменение слито, СИСТЕМА ДОЛЖНА иметь ADR-0086 и ADR-0091 в статусе
  Accepted или Superseded, с поправкой §5 из «Решений спеки», и ADR-0027 §9 в согласии с
  кодом. Проверка: ревью PR по списку.
- **R18.** КОГДА изменение слито, СИСТЕМА ДОЛЖНА исправить «Расхождения документации».
  Проверка: `cargo xtask docs-paths`; `rg "render.rs:451"` пуст вне архивных корней.

## Вне scope

- Параллельный layout и отдельный owner-поток на каждый realm (ADR-0091 §1, spike H2).
- `!Send` у platform hooks (ADR-0082 §4); gesture arena и recognizers (ADR-0086 §4).
- #1248, #1249, #1251–#1254 (bounds не меняют; #1250 отклонён); `flui migrate` (открытый вопрос 1);
  ADR-0075 (запись из compute — run-time `WrittenDuringCompute`).

## Открытые вопросы

1. **Нужен ли `flui migrate` до 0.2.0?** Внешних consumer нет, поэтому предлагаю обойтись
   строками миграции в changelog (R16).
