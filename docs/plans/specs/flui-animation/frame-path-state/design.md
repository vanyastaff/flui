# frame-path-state — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Требования:** [requirements.md](requirements.md) (R1–R8)
- **Связанные:** [send-flip/design.md](../../send-flip/design.md) — вариант a1, строки 11–14, 17–22, 31,
  задачи T4, T5, T6b, T6c; [controller-robustness](../controller-robustness/requirements.md) R1, R3, R6.

## Итог

| Вопрос | Решение |
|---|---|
| Хранение | Владелец-локальное ядро `Rc<ControllerCore>`: горячий снимок `Cell<Published>` (значение, статус, признак прогона, поколение) + холодное состояние `RefCell<ControllerState>`, заимствуемое только кодом крейта и отпускаемое до любого вызова наружу |
| Реестр | `Vsync = Rc<VsyncCore>`; обход ADR-0125 сохранён; детей обходит курсор по слотам — ноль аллокаций в кадре |
| Render objects | Push-кэш для paint-эффектов (`Rc<Cell<u8>>`); layout читает O(1) `value()` |
| Отношение к send-flip | Уточняет a1 и **заменяет текст T6c**; публичный flip трейта — в send-flip T5 |
| Порядок | F2 (форма хранения, `main`) — **первой** среди тем, правящих внутренности контроллера; F3 (flip) — в `send-flip/core` после T5 |
| ADR | да: новый «Состояние анимации принадлежит потоку realm»; поправки ADR-0064 §4, ADR-0125 |

## Текущее состояние (чтением)

- Трейт: `Animation<T>: Listenable + Send + Sync + Debug where T: Clone + Send + Sync`
  (`crates/flui-animation/src/animation.rs:68`); `StatusCallback = Arc<dyn Fn + Send + Sync>` (`:11`);
  `ParentSubscription` держит `Mutex<Option<Box<dyn FnMut + Send>>>` только ради `Sync` (`:126-128`).
- Контроллер: `Arc<Mutex<AnimationControllerInner>>` + `Arc<ChangeNotifier>` (`controller.rs:256-260`);
  `value()`/`status()` — замок (`:2684-2691`), `is_animating()` — замок + замок тикера (`:2722`),
  `walk_probe` — замок (`:1844`); `tick_at` — 1 снимок + 1–2 коммита, у симуляции 3 (`:1874-1940`),
  на каждом тике атомарные клоны `Arc` кривой/симуляции (`:1887-1890`).
- Обёртки: Proxy — три `RwLock` + `Mutex` (`proxy.rs:85-90`), `value()` = чтение `RwLock` + клон `Arc`
  (`:186`, `:234`); Curved — `Mutex` направления (`curved.rs:65`, `:166`); Switch — `Mutex`, родители
  читаются под ним (`switch.rs:294-295`, `:444-451`).
- Реестр: `Arc<Mutex<VsyncInner>>` (`vsync.rs:141-144`); `has_running` под замком реестра берёт
  замок каждого контроллера (`:338-363`) и аллоцирует снимок детей (`:352-356`); `tick_all` —
  снимок детей (`:442-450`), затем шаг курсора с замком реестра и вложенным замком контроллера
  (`:469-483`). Вызовы: `frame.rs:119-120` и `:923` — `has_running` дважды за кадр на presentation.
- Presentation держит `RefCell<Vsync>` и отдаёт клон (`crates/flui-runtime/src/presentation.rs:337`, `:853`).
- Push-модель уже есть: `RenderAnimatedOpacity` пересчитывает alpha в слушателе в `Arc<AtomicU8>`,
  paint читает кэш (`crates/flui-objects/src/proxy/animated_opacity.rs:103`, `:319-336`, `:361-376`);
  так же `sliver/sliver_animated_opacity.rs:60`. Комментарий «listener runs off the owning thread»
  (`animated_opacity.rs:95-103`) ложен: тикает `Vsync` на owner-потоке.

**Почему `Send + Sync` сегодня.** Межпоточного вызова нет (`rg thread::spawn` по animation/widgets
production пуст). Bounds навязаны хранением: `RenderView::RenderObject: Send + Sync`
(`crates/flui-view/src/view/render.rs:511`) требует `Send` от `RenderAnimatedOpacity` → `ProxyAnimation`
→ `Animation`; `Listenable: Send + Sync` (foundation) — supertrait. Цена видна у потребителей: слушатель
обязан быть `Send`, поэтому navigator гоняет статусы через data-plane shuttles
(`navigator/transition_route.rs:112-114`, `:537`; `hero_flight.rs:150-160`, `:997-1000`). Realm —
один owner-поток, `!Send` по ADR-0027 §2; render objects уже `!Send` (§9). Исключение записано только
в rustdoc `controller.rs:227-230` и `README.md:537-542`; в ADR его нет — формально заменять нечего,
новый ADR его закрывает.

## Варианты

### (a) Слаб состояний у realm, типизированные поколенные id (GPUI `Entity`)

`Vsync` становится хранилищем `Slab<AnimationState>`; handle = `(Rc<Store>, AnimationId)`.
**За:** плотный обход в тике, явная смерть по unmount, stale-handle ловится поколением.
**Против:** каждое чтение — заимствование всего слаба + проверка поколения; в тике слаб занят
изменяемо, а слушатели читают соседей → нужен split borrow или `Cell` на слот, то есть вырождение
в `Rc<Slot>` с лишним индексом. Контроллер без realm (тесты, `without_ticker` у scroll) требует
хранилища. Stale handle добавляет к `value()` отказ, которого у значения быть не должно. GPUI читает
`Entity` через `cx`, а в paint контекста нет — send-flip a2 отклонён по той же причине. Локальность
кэша не доминирует: тик — это кривая + раздача слушателям. Поколенная идентичность остаётся там, где
нужна, — у токенов регистрации (ADR-0125). **Отклонён.**

### (b) Оставить `Send + Sync`, чтение без замка через атомики

`AtomicU64` (биты `f64`) + `AtomicU8` статуса; мутация под `Mutex`.
**Против:** `Send` по-прежнему вынужден хранением — противоречит ADR-0027 §2, §9 и send-flip
(строка 20, `owner-only` в gate). Согласованность нескольких полей: упаковать `f64` + статус +
поколение в 64 бита нельзя, 128-битные атомики непереносимы (wasm32), seqlock — тот же замок с
повтором; раздельные атомики дают «разорванный» снимок (значение кадра N, статус N−1) для
межпоточного читателя, которого нет — цена без пользы. Тик остаётся на `Mutex` (2–4 на контроллер),
Proxy/Switch требуют lock-free замены `Arc` → новая зависимость (`arc-swap`) на выброс. **Отклонён**,
в том числе как промежуточный шаг: его выбросит F3.

### (c) Push-модель везде: тик пишет в свойство render object

**За:** paint не читает анимацию. **Против:** читатели в build (`AnimatedBuilder`, transitions,
route builders) — не render objects; дублирование состояния ради чтения, которое в (d) стоит одну
загрузку из `Cell`. **Принят как шаблон** для paint-эффектов (уже так у opacity), **не** как
единственный механизм.

### (d) Владелец-локальное ядро со снимком в `Cell` — **выбран**

Уточняет send-flip a1 (`Rc<RefCell<Inner>>`): горячие поля вынесены из `RefCell` в `Cell<Published>`.
Поэтому чтение не заимствует вообще — ни `BorrowError` при реентрантном чтении, ни
«разорванного» снимка (`Cell<T: Copy>` пишется целиком на одном потоке). Мутация — `RefCell`
с правилом «отпустить до вызова наружу», то есть сегодняшняя дисциплина замков (I1 из ledger
zone-1 держится) без замков. Рынок (market-C): GPUI держит скорость пружины в element state окна,
Slint — thread-local `AnimationDriver` с `Property`, Xilem — `on_anim_frame(ctx, …)` в однопоточном
проходе виджетов; ни один не берёт замок на чтение анимации.

**Конфликт с send-flip:** нет по существу. a1 сохраняется (`Rc`, `RefCell`, `!Send`); (d) добавляет
`Cell`-снимок, правила реестра и push-кэши. Текст T6c заменяется ссылкой на эту спеку.
Жёсткие зависимости: T5 (снят supertrait `Send + Sync` у `Listenable`, `Animation`, `Simulation`,
`StatusCallback = Rc`, строки 11–13, 17–19), T4 (строка 31, bound `RenderView::RenderObject`),
T6b (раздача `ChangeNotifier` по снимку без замка — `notifier_generic.rs:307-321` сейчас берёт
замок на слушателя). Без T5 `!Send` контроллер не реализует `Listenable`.

## Внутреннее устройство (приватно)

```rust
pub struct AnimationController { core: Rc<ControllerCore> }          // Clone, !Send, !Sync
struct ControllerCore {
    published: Cell<Published>,          // пишется только в commit(), читается всеми
    state: RefCell<ControllerState>,     // run, bounds, durations, sources, completer, disposed
    status: RefCell<StatusListeners>,    // форма — listener-delivery; снимок до вызова
    notifier: ChangeNotifier,            // value-канал (send-flip T6b)
}
#[derive(Clone, Copy)] struct Published { value: f64, status: AnimationStatus,
    live: bool /* !disposed && run installed */, generation: u64 }
```

- `fn mutate<R>(&self, f: impl FnOnce(&mut ControllerState, &Cell<Published>) -> R) -> R` —
  единственная точка `borrow_mut`; `f` — код крейта, возвращает `#[must_use] Outgoing`
  (смена статуса, delivery future, отставленные источники), который `finish` исполняет после
  возврата. Пользовательские объекты, хранимые в состоянии (`Rc<dyn Curve>`, `Rc<dyn Simulation>`),
  копируются наружу и вызываются вне `mutate` — как сейчас `TickSource` (`controller.rs:1878-1891`).
- Обёртки: `Proxy { parent: RefCell<Rc<dyn Animation<T>>>, subs: RefCell<Subs> }` — одна запись
  заменяет родителя и подписки разом (закрывает форму D-10); `value()` = `let p =
  self.parent.borrow().clone(); p.value()`. Curved: направление в `Cell<Option<AnimationStatus>>`.
  Switch: текущий поезд — `RefCell<Rc<…>>`, чтение по тому же правилу (D-02 по хранению).
  `ParentSubscription`: `Cell<Option<Box<dyn FnOnce()>>>`.
- `VsyncCore { entries: RefCell<BTreeMap<u64, Entry>>, children: RefCell<BTreeMap<u64, Vsync>>,
  next_id: Cell<u64>, muted: Cell<bool> }`. Шаг обхода: заимствовать, найти следующий ключ в
  `cursor..fence`, прочитать `published` контроллера (`live`, `generation`), клонировать `Rc`,
  отпустить, тикнуть. Детей — тем же курсором по слоту (сегодня `Vec`-снимок): семантика «дети
  выбираются на входе» сохраняется забором `children_fence`. `has_running` — то же без клонов.
- **F2 на `main` (форма без flip):** все примитивы разделения названы в одном приватном модуле
  `share.rs` (`type Shared<T> = Arc<T>`, `StateCell<T>` над `Mutex`, `SnapshotCell<T: Copy>` над
  `Mutex`); код крейта пишет только через `mutate`/`publish`/`snapshot`. F3 меняет `share.rs`
  на `Rc`/`RefCell`/`Cell` и снимает `Send` — правка механическая. Аллокации обхода детей
  убираются уже в F2.

## Инварианты

- **I1** Чтение `value/status/is_animating` контроллера — одна загрузка `Cell`; никакой код не
  держит заимствование `state`, `status` или реестра через вызов наружу (слушатель, кривая,
  симуляция, waker, `Drop` пользователя, `tracing`-подписчик).
- **I2** `published` записывается только в `commit()` внутри `mutate`, до раздачи; вложенный
  коммит из слушателя пишет позже внешнего, и внешний после раздачи не пишет (R3.2).
- **I3** `published.value` конечен; на ограниченном контроллере — в `[lower, upper]`
  (`debug_assert!` в `commit`; поведение на неконечном входе — controller-robustness R4).
- **I4** `generation` растёт `checked_add` и не переиздаётся; исчерпание — `expect("BUG: run
  generations exhausted")` (как счётчики ключей foundation); `sample_epoch` сравнивается на равенство.
- **I5** Ядро контроллера не держит ссылки на реестр; реестр держит сильный `Rc` ядра, пока
  зарегистрирован; токен — `rc::Weak<VsyncCore>` + слот (ADR-0125 без изменения смысла).
- **I6** `Drop` ядра выполняется вне любого `&self`-метода: обход держит свой клон `Rc` на шаг,
  слушатель вызывается с живым handle (как R4 ledger zone-1).
- **I7** Ни одного нового `static`/`thread_local!` (ADR-0097).

## Владение

Presentation владеет `Vsync` (`presentation.rs:700`, `:780`); состояние виджета — handle контроллера;
реестр — сильной ссылкой до `unregister`. Последний `Rc` отпущен → `Drop` ядра на owner-потоке
(send-flip R13): прогон отменяется, `Err(canceled)` доставляется, слушатели уходят через `Retirement`
(ADR-0127). Удаление `Vsync` при живом handle виджета: снимок остаётся читаемым, future pending до
`dispose()` (controller-robustness R9.5). Владеющий handle вместо register/unregister — тема
`ownership`; она строится на I5/I6 и выбирает сильную или слабую ссылку реестра.

## Публичный контракт

Сверх строк send-flip 17–22 (они входят в T5/T6c и здесь не повторяются) — **новых `pub`
элементов нет**.

| Элемент | Было | Стало |
|---|---|---|
| `Animation::value`, `status` (rustdoc) | контракт не сказан | «O(1); без замка и заимствования; можно звать из любого слушателя, кривой, симуляции и paint, в том числе реентрантно; все чтения одного шага видят один коммит» |
| `AnimationController`, `Vsync`, `VsyncRegistration` | `Send + Sync` | `!Send + !Sync` (строка 20 send-flip; `VsyncRegistration` — новая в списке `owner-only`) |
| `ProxyAnimation::new/set_parent`, `CurvedAnimation::new`, `TweenAnimation::new`, `ReverseAnimation::new`, `AnimationSwitch::new`, `AnimatableExt::animate` | `Arc<dyn Animation<T>>` от вызывающего, `self: Arc<Self>` | `impl Animation<T> + 'static` — стирание один раз внутри (R5 аудита абстракций); хранение `Rc<dyn Animation<T>>`; одна пробрасывающая `impl Animation<T> for Rc<dyn Animation<T>>` для уже стёртых; `CurvedAnimation` не generic по кривой (хранит стёртую кривую); `AnimationExt` удаляется (R4, тема composition) |
| `AnimationController::velocity` (rustdoc) | — | «не O(1) для симуляции: вызывает `Simulation::dx` вне заимствования» |
| `RenderAnimatedOpacity::alpha`, sliver-аналог | `Arc<AtomicU8>` внутри | `Rc<Cell<u8>>` внутри; сигнатуры те же |

Bound `Send + Sync` у `Curve`/`ArcCurve` и `Animatable`/`Tween`: send-flip строка 264 оставляет `ArcCurve` `Send`, а строки 21/39 принимают `impl Curve + 'static` (не `Send`) — несовместимо; один стёртый тип кривой для owner-local хранения (R2 аудита) — **решение владельца**, до него bound не меняется.

## Миграция

`rg` вне `crates/flui-animation`, без `*.md`: `Arc<dyn Animation<` — 53 вхождения в 23 файлах
(flui-widgets 16, flui-objects 4, flui-cupertino 2, flui-view 1); `AnimationController` — 235/59;
`Vsync` — 261/59; `add_status_listener(`/`StatusCallback` — 14/11; обёртки — 108/18. Внутри крейта:
`Arc<dyn Animation<` — 75 (ext 24, compound 17, switch 12, proxy 9, tween 5, reverse 4, curved 3,
animation 1), `Mutex|RwLock` — 46 в 7 файлах, `Send + Sync` — 58 в 10 файлах.

- Вызовы `Arc<dyn Animation<…>>` → `Rc<…>`: рецепт send-flip T5 (ast-grep), не эта спека; при правке тех же строк `Arc::new(controller.clone())` и `as Arc<dyn Animation<f64>>` (55 мест) уходят — конструкторы принимают `controller.clone()` (R5).
- Эта спека (F3): `controller.rs` (после Q0 — `controller/*`), `vsync.rs`, `proxy.rs`, `switch.rs`,
  `curved.rs`, `animation.rs` (`ParentSubscription`), `reverse.rs`, `tween.rs`, `constant.rs`,
  `share.rs`; `benches/*`; `docs/{ARCHITECTURE,PERFORMANCE,PATTERNS}.md`, `README.md`.
- F4: `crates/flui-objects/src/proxy/animated_opacity.rs`, `sliver/sliver_animated_opacity.rs`
  (кэш `Rc<Cell<u8>>`, правка ложного комментария) — файлы send-flip T6d, по согласованию с ним.
- Упрощение shuttles navigator (`transition_route.rs` `pending_statuses`, `hero_flight.rs`
  `settled_status`) — **не здесь**: меняет порядок применения статусов в build; карточка в
  [../tasks.md](../tasks.md) после F3.

## Порядок относительно других тем

```text
Q0 ─► F1 контракт + бенч «до» ─► F2 форма хранения (main) ─┬─► A listener-delivery (main)
                                                         ├─► B controller-robustness (main; Ticker удалён)
send-flip T4, T5 (core) ─────────────────────────────────┴─► F3 flip = T6c (core) ─► F4 ─► F5 ADR, бенч «после»
                                                              W2 ownership, retarget — на I5/I6
```

Тема **определяет хранение**, на котором строятся listener-delivery, controller-robustness,
ownership и retarget: они правят внутренности только через `mutate`/`commit` из F2 и не добавляют
`Mutex`. F3 предпочтительно после удаления `Ticker` (B): тогда под заимствованием не остаётся
вызова планировщика (D-32). Темы physics, curves, interpolation, derive, motion-clock от хранения не
зависят. Ядро send-flip ребейзится на `main` еженедельно; F3 переигрывается по `share.rs`.
Согласование: канал listener-delivery берёт `Shared`/`StateCell` из `share.rs` (один модуль на
крейт; свои псевдонимы `Shared`/`Cell` из его прежней редакции сняты).
`CompoundAnimation` удаляет controller-robustness T10, а не эта тема (ссылка в listener-delivery исправлена).

## Adversarial review

- **Реентрантность слушателя в контроллер и реестр.** Слушатель зовёт `forward/stop/dispose/
  set_value/tick_at`, `register/unregister/set_muted/tick_all`: ни одно заимствование не занято
  (I1), вложенный коммит побеждает (I2), обход видит удалённый ключ как отсутствующий, новый —
  за забором. Тесты: `frame_path_reentry` R5.1, R5.2, R5.6; `nested_commit_is_not_overwritten`.
  Мутация: раздача внутри `mutate` → `BorrowMutError` в R5.2.
- **Снятие/добавление слушателей во время раздачи.** Раздача по снимку, реестр слушателей
  отпущен; семантика (вызывать ли снятого) — listener-delivery. Тест R5.4.
- **Последний владелец из колбэка.** I6: обход держит клон; `Drop` после шага. Тест R5.5 с
  `DropLedger`. Остаток C7: слушатель, захвативший клон `Vsync`, + реестр, держащий ядро = цикл
  `Rc`, как сегодня с `Arc`; разрывает `unregister`/`dispose` (controller-robustness R6.3) или
  владеющий handle (ownership, `driven_controller_drop_unregisters`) — зависимость, а не решение этой темы.
- **Два контроллера на одном `Vsync`.** Семантика забора сохранена (R4.3); вложенный `tick_all`
  из слушателя — допустим, второй обход того же реестра в одном кадре тикает контроллер повторно с
  тем же временем (чистая функция, R5.8 — без второго уведомления значения). Тест R5.6.
- **Realm остановлен.** Замена `Vsync` из слушателя: текущий обход работает на клоне
  (`frame.rs:96`), новый реестр — со следующего кадра. Тест R5.7.
- **Panic в пользовательском коде на каждом вызове наружу** (value/status-слушатель, кривая,
  `x`, `is_done`, `dx`, продолжение future, `Drop` захвата): вызов вне заимствования, раскрутка не
  оставляет занятых `RefCell`; `RefCell` не «отравляется», полузаписанного состояния нет, потому
  что внутри `mutate` пользовательского кода нет. Паника внутри `mutate` возможна только как дефект
  крейта (переполнение `Duration` — controller-robustness R5.1). Раунд дорабатывает, первая паника
  пробрасывается после раунда, остальные удерживаются; следующий кадр тикает
  (listener-delivery). Тест R5.3 (строка на вызов).
- **Overflow и NaN.** I3, I4; NaN-кривая и NaN-время не доходят до `published` (R5.8, R5.9;
  значения — controller-robustness R4.2, motion-clock D-33).
- **`Debug` изнутри слушателя** — `try_borrow`, при занятом — `<borrowed>`, не паника (R2.3).
- **Межпоточный вызов** невозможен по типу (R1.1); `RenderInvalidationHandle` остаётся `Send`
  (канал), слушатель его захватывает — разрешено.
- **Остаточные риски.** (1) Срок: исправление попадает в `main` со слиянием ядра send-flip
  (~11-09); до этого `main` живёт с замками (F2 убирает только аллокации). (2) Конфликты ребейза
  ядра с A/B — смягчено `share.rs`. (3) Цикл `Rc` через слушатель (выше). (4) Цифры выигрыша не
  измерены: оценка ~6 замков × 10 000 контроллеров за кадр — гипотеза, бенч R6 её проверяет.

## Черновик ADR

**ADR-NNNN: Состояние анимации принадлежит потоку realm** (номер — оркестратор).
Status: Proposed. Amends: ADR-0064 §4 (замок контроллера читается как заимствование его
состояния), ADR-0125 (токен держит `rc::Weak` реестра; «mutex реестра» → «заимствование реестра»).
Реализует ADR-0027 §2, §9 для `flui-animation`; закрывает исключение из rustdoc `AnimationController`.

*Решение.* Контроллеры, реестр `Vsync` и обёртки анимаций — `!Send + !Sync` и живут на owner-потоке
realm. Горячее состояние (значение, статус, признак прогона, поколение) публикуется одной записью
`Cell` при коммите и читается без замка и заимствования; холодное — в `RefCell`, которое заимствует
только код крейта и отпускает до любого вызова наружу. Обёртки держат заимствование лишь на копию
`Rc` родителя. Обход реестра не держит заимствование через `tick_at` и не аллоцирует в кадре без
изменений. Render object для paint-эффекта кэширует значение в слушателе. *Отвергнуто:* атомики при
`Send + Sync` (нет согласованного снимка, тик под замком, `Send` вынужден хранением); слаб у realm с
поколенными id (чтение требует хранилища, в paint контекста нет). *Проверка:* `thread_affinity`,
`reads_inside_every_callout`, `frame_path_reentry`, `frame_path_allocation`, бенч `frame_path`.

## Фрагмент `changelog.d`

Если F3 сливается в ядро send-flip — в его фрагмент; иначе `changelog.d/frame-path-state.md`:

```markdown
### Changed
- `flui::animation`: `AnimationController`, `Vsync` and `VsyncRegistration` are owner-thread values
  (`!Send + !Sync`); reading `value()`/`status()` takes no lock and is safe from any listener, curve,
  simulation or paint call.
- `ProxyAnimation`, `CurvedAnimation`, `TweenAnimation`, `ReverseAnimation`, `AnimationSwitch`
  accept any `impl Animation<T> + 'static` (no `Arc::new(..)` or `as Arc<dyn ..>` at the call site).
```

## Реентри родителя обёртки (edition 2024)

Хвостовое выражение `self.parent.borrow().value()` держит `Ref` на время вызова родителя — та же
форма, что дедлок switch (O1, dl1/dl2): родитель, читающий обёртку, получит `BorrowMutError` при
её `set_parent`. Поэтому чтение — только `let p = self.parent.borrow().clone(); p.value()`, а
`match` никогда не берёт `borrow()` в скрутини (guard живёт во всех ветках; clippy
`significant_drop_in_scrutinee` `RefCell` не видит). В `frame_path_reentry` добавляются строки
`proxy_parent_reads_the_proxy_during_set_parent` и `switch_parent_reads_the_switch_during_hop`
(контрактные строки switch уже в `animation/contract-tests`: `switch_parent_value_reentry`,
`switch_parent_status_reentry`).

## Владение и порядок замков

| Объект | Сильные | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `ControllerCore` | handle (`DrivenController`), обёртки, реестр на время регистрации | `VsyncRegistration` — `rc::Weak<VsyncCore>` | handle снимает и disposes; teardown — drop presentation → `Vsync` → клоны |
| `VsyncCore` | presentation, `VsyncScope`, родительский реестр | токены регистраций | новый реестр со следующего кадра; старый живёт на клоне текущего обхода |
| кэш paint (`Rc<Cell<u8>>`) | render object, его слушатель | — | `detach` снимает слушатель; drop узла |

Циклы: **C5** (`ProxyAnimation` → слушатель → тот же proxy в `RenderAnimatedOpacity::attach`) — F4:
слушатель захватывает только `Rc<Cell<u8>>`, handle инвалидации и слабый proxy
(`ProxyAnimation::downgrade`); «Drop == 1» `animated_opacity_releases_proxy_after_tree_drop`
(drop `PipelineOwner` без `detach`). **C7** — у ownership. Порядок замков F2 (`share.rs` на `Mutex`,
до F3): реестр → контроллер → канал статуса; обёртка → (копия `Rc`, отпустить) → родитель; под
состоянием контроллера ничего чужого. После F3 те же рёбра для `borrow_mut`.

## Паттерн

- **Owner-local core** — `Rc<ControllerCore>`: `Cell<Published>` (горячий снимок) + `RefCell`
  холодного состояния с единственной точкой `mutate`; не arena/slab (вариант (a) отклонён).
- **Generational ID** — только у токенов регистрации (ADR-0125), не у контроллера.
- **Push-кэш** для paint-эффектов (`Rc<Cell<_>>`).
- **Один модуль примитивов разделения** (`share.rs`) — замена `Arc`→`Rc` механическая.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `Animation<T>: Send + Sync` + `Mutex` на чтение | «объект, разделяемый где угодно» (Java/Dart-модель) | `!Send` owner-local, чтение из `Cell` |
| `Arc<dyn Animation>` от вызывающего, `self: Arc<Self>` в ext-трейте | Dart-ссылки на объекты | `impl Animation<T> + 'static`, стирание внутри |
| слаб у realm (GPUI `Entity`) | чтение через контекст | отклонено: в paint контекста нет |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Конструкторы принимают `impl Animation + 'static` | нарушала | design.md:172, 275 | R5: стирание внутри, `CurvedAnimation` не generic |
| Трейт без пользователя удаляется (`AnimationExt`) | нарушала | design.md:172 (`self: Rc<Self>`) | удаляется (composition) |
| Один стёртый тип кривой | открыто | design.md:176; send-flip 21/39 vs 264 | решение владельца (R2) |
| Borrow не живёт через user code (2024: `match`, хвост) | частично | design.md:122-124 | раздел «Реентри родителя», две строки теста |
| Одна политика паники | нарушала | design.md:236-237 | первая — после раунда |
| Таблица владения, C5/C7, «Drop == 1» | нарушала | design.md:154-161 (без таблицы) | раздел «Владение и порядок замков» |
| Порядок замков F2 | нарушала | — | реестр → контроллер → канал |
| Нет новых `static`, `expect("BUG: …")` | соответствует | I4, I7 | — |
| `!Send` фиксирован trybuild | соответствует | R1.1 | — |
| `Debug` через `try_borrow` | соответствует | design.md:240 | — |
