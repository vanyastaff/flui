# Распознаватели жестов: API — design

- **Статус:** утверждён и реализован в интеграционной ветке; итоговая проверка — [tasks.md](tasks.md)
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **База кода:** `main` @ `9a4daa3ed`. Строки распознавателей, если не сказано иначе, — по веткам I1
  (`interaction/arena-recognizer-lifecycle` @ `2c7e48c00`, worktree `agent-a4b98e67d784ee379`) и I2
  (`interaction/multi-pointer-recognizers` @ `8c234f670`, worktree `agent-a1f74c902e86029c9`): спека строится
  поверх них. Пути — от `crates/flui-interaction/src/`, кроме помеченных.
- **Проверка формы:** пробы `rustc 1.99.0 --edition 2024` (scratchpad, не в репозитории): dyn-compatible трейт
  с `PointerDispatch<'_>` и `&self` — компилируется; `self: &Arc<Self>` и `self: &Rc<Self>` — E0038;
  `self: Rc<Self>` — dispatchable; `Rc::new_cyclic` с `Weak<T>` → `Weak<dyn GestureArenaMember>` —
  компилируется; upcasting `Rc<dyn GestureRecognizer>` → `Rc<dyn GestureArenaMember>` — компилируется
  (стабильно с 1.86); отправка `Rc<Tap>` в поток — E0277; `Weak::upgrade` после drop последнего `Rc` —
  `None`; правило временных: borrow скрутини `if let` жив в then-блоке, в каждом плече `match`, снят после
  «сначала `let`».

## 1. Что есть сейчас

| Место | Факт |
|---|---|
| `recognizers/recognizer.rs:33-75` (main), `:155-179` (I1) | `GestureRecognizer: GestureArenaMember`; `add_pointer(self: &Arc<Self>, pointer, position, global_position)` — E0038; `add_pointer_down(self: &Arc<Self>, dispatch)` (I1) с дефолтом через `add_pointer`; `dispose(&self)`; `primary_pointer()` без производственного вызова через трейт |
| `recognizers/recognizer.rs:87-122` | `RecognizerBase`: `GestureArena` + 5 полей `Arc<Atomic*>`/`Arc<Mutex<..>>` в `!Send`-объекте; `disposed` + `assert_not_disposed` с `debug_assert!` (`:247-257`) |
| `recognizers/recognizer.rs:273-295` | `start_tracking<T: GestureArenaMember + Clone + 'static>(.., recognizer: &Arc<T>)` — молча перезаписывает текущий контакт (Z1 I19) |
| I1 `recognizer.rs` | `invoke_callback`, `retire_callback` (сдерживание колбэков, ADR-0127), `EventTimeline`, `is_primary_down` |
| I2 `scale.rs:48,61`, `force_press.rs:219`, `tap_and_drag.rs:252` | те же помощники сдерживания (копия, C2 требует вынести в `recognizers/callback_containment.rs`); исходы `Outcome`/`Notice` строятся под guard, колбэки — после |
| `recognizers/one_sequence.rs:23`, `primary_pointer.rs:24` | supertrait-цепочка без потребителя; `deadline()`/`did_exceed_deadline()` не вызываются |
| `with_on_*` | 45 методов `self: Arc<Self> -> Arc<Self>`: tap 13, long_press 7, tap_and_drag 6, drag 5, force_press 4, scale 4, double_tap 3, multi_tap 2, multidrag 1; плюс `drag_variants.rs:126-140` (`on_start/on_update/on_end`), `with_drag_start_behavior`, `with_start_pressure`, `with_peak_pressure`, 10 × `with_settings`, 9 × `set_settings` без производственного вызова |
| колбэки | `callbacks: Rc<RefCell<XCallbacks>>` (tap.rs:129, long_press.rs:104, …); dispose дропает захваты под `borrow_mut` (panic-matrix D3), кроме drag (`drag.rs:857-872`) и multidrag |
| настройки | 10 × `settings: Arc<Mutex<GestureSettings>>` (tap.rs:135, drag.rs:181, …); `GestureSettings` — `Clone`, не `Copy` (`settings.rs:129`), все поля `f64`/`Duration` |
| `arena/mod.rs:134-184` | `GestureArenaMember: sealed::arena_member::Sealed`; дедлайн — три метода (`poll_deadline`, `has_pending_deadline`, `next_deadline`) с конвенцией «согласованы» |
| `arena/mod.rs:192-202` | blanket `impl<T: CustomGestureRecognizer> GestureArenaMember for T` — теряет дедлайны |
| `arena/mod.rs:469, 480, 304` | слот держит `Arc<dyn GestureArenaMember>` сильно; `eager_winner` и `DeadlinePoll` — тоже; `GestureArenaEntry` и `DeadlineWatcher` — `Weak` (`:270-271, :298`) |
| `sealed.rs:82, 179-226`; `lib.rs:153, 287, 340` | `CustomGestureRecognizer`; `pub mod sealed` — запечатка ложная |
| `traits.rs:60, 146, 188` | `GestureCallback` (GAT, 0 impl, E0038), `GestureRecognizerExt` (0), `Disposable` (0) |
| flui-widgets `gesture_detector.rs:458-478, 701-847` | 5 × `Arc<Recognizer>`, 22 вызова `with_on_*` на слоты состояния |
| flui-widgets `gesture_detector.rs:903-914` | `dispose` пяти распознавателей подряд без сдерживания (panic-matrix R1 C3) |
| flui-widgets `gesture_detector.rs:949-1145` | `RecognizerGroup`: ручная проводка, `mounted`-гейт, `add_pointer_with_kind`, гейт `forward` по живым слотам (кроме double tap — комментарий `:1121-1136` объясняет почему; drag и long press тот же дефект не обходят) |
| flui-widgets `back_gesture.rs:286-311, 597-611, 631-655` | ручная проводка + предварительный гейт (`enabled`, «жест уже идёт»), `horizontal_drag(arena).with_on_*` |
| flui-widgets `draggable.rs:1353-1355, 1392-1414, 1470-1478` | `MultiDragGestureRecognizer::new(..).with_on_start(..)`, ручная проводка + гейт `max_simultaneous_drags`, `dispose` → отмена сессий |
| flui-widgets `scroll/scrollable.rs:593`, `text/editable_text.rs:970`; flui-material `ink_well.rs:379` | только `GestureDetector::new().on_*` — распознавателей не касаются |
| ADR-0086 §4 | «`GestureArenaMember` и алиасы колбэков сохраняют сигнатуры… custom recognizers do not break» — эта спека его заменяет (§11) |

## 2. Решения

### D1. Конструирование: builder до `Rc` (scope a)

| Вариант | За | Против |
|---|---|---|
| 1. Оставить `with_on_*` на `Rc<Self>` + `RefCell` | ноль миграции | сеттер под именем builder; interior mutability на каждое поле; drop старого захвата под borrow (ownership §C №7) |
| 2. Публичная структура `TapCallbacks { on_tap: Option<..>, .. }` в `new(arena, callbacks)` | struct-literal стиль | `#[non_exhaustive]` запрещает литерал вне крейта, без него — новый колбэк ломает всех; `Rc::new` у каждого колбэка руками |
| **3. `XGestureRecognizer::builder(arena) -> XGestureRecognizerBuilder`, методы `on_*(impl Fn(..) + 'static)`, `settings(GestureSettings)`, `build() -> Rc<X>` через `Rc::new_cyclic`** | C-BUILDER; добавление колбэка не ломает; колбэки неизменяемы после `build` → поле `callbacks: XCallbacks` без `RefCell`; свой колбэк не может захватить `Rc` собственного распознавателя (его ещё нет) — частый цикл исключён формой | 10 типов builder'ов |

**Выбран 3.** Builder `#[must_use]`, `!Send` (держит `Rc<dyn Fn>`), обязательные параметры — аргументы
`builder(..)` (`arena`; у drag — `DragAxis`, у multi-tap — число указателей; у multidrag — `MultiDragAxis`),
необязательные — методы. `drag_variants::{pan, horizontal_drag, vertical_drag}` возвращают builder.
`MultiDragStartCallback` возвращает `Option<Rc<dyn MultiDragHandle>>` (trait-table 2.6: без переаллокации
`Rc::from(Box)` и без лишнего `'static`).

### D2. Как `&self` регистрирует себя в арене (dyn-compatibility)

| Вариант | Итог |
|---|---|
| `self: &Rc<Self>` / `&Arc<Self>` | E0038 (проба) |
| `self: Rc<Self>` | dispatchable, но каждый вызов — `Rc::clone(&r).add_pointer(d)`; шум у всех вызывающих |
| **`&self` + `Weak<dyn GestureArenaMember>` на себя, полученный в `Rc::new_cyclic` и хранимый в `ArenaMembership`** | все методы `&self`; трейт dyn-compatible; `Weak<T>` → `Weak<dyn ..>` коэрцируется (проба) |

**Выбран третий.** Сторонний распознаватель делает то же: `Rc::new_cyclic(|this| My { contact:
PrimaryContact::new(ArenaMembership::new(arena, this.clone())), .. })`.

### D3. Сильная или слабая ссылка арены на участника

| Вариант | Итог |
|---|---|
| 1. Сильная (сейчас) | цикл арена ↔ распознаватель рвёт только `dispose`; пропуск = утечка и «призрак» в арене (ownership §B, F10) |
| 2. Распознаватель держит слабую арену | арена закрывается раньше распознавателя — ок, но слот всё равно держит участника сильно |
| **3. Слот, `eager_winner`, `DeadlinePoll` держат `Weak<dyn GestureArenaMember>`; каждое уведомление — `upgrade` на время вызова** | цикла нет по построению; `Drop` распознавателя достаточно; мёртвый участник = вышедший |

**Выбран 3.** Следствия: участник жив, пока его держит владелец (виджет, тест). Мёртвый `Weak` при любом
разборе слота считается `reject`; `PrimaryContact::drop` ставит слот в существующую очередь отложенных решений
(`DeferredResolution`, `arena/mod.rs:843`; разбор — `drain_deferred_resolutions`, `:1725`, его зовёт binding),
поэтому оставшийся единственный участник побеждает на ближайшем разборе, а не на следующем событии. Внутри
`Drop` чужой пользовательский код не исполняется. Пер-последовательностный участник tap из I1
(`TapArenaMember` с `Weak<Tap>`, I1 `tap.rs:291`) хранится распознавателем сильно — иначе он умрёт сразу;
это инвариант I6.

### D4. Иерархия → поля-помощники (scope b)

| Вариант | Итог |
|---|---|
| 1. Оставить supertrait-цепочку | Flutter-форма без потребителя, запрещена «Конвенциями Rust» |
| 2. Один `RecognizerBase` на всё (сейчас) | смешивает членство в арене, контакт, флаг dispose; многоуказательным (scale, multidrag, multi-tap) нужна только арена |
| **3. Два помощника: `ArenaMembership` (арена + `Weak` на себя; `join(pointer)`) и `PrimaryContact` (один контакт: `begin/accept/withdraw/finish`, снимок контакта, `ContactId`, дедлайн, slop)** | каждый распознаватель берёт нужное; одна последовательность — тип (`begin -> Result<ContactId, BeginContactError>`), а не перезапись |

**Выбран 3.** `RecognizerBase` удаляется. `primary_pointer()` уходит из трейта в `PrimaryContact::current()`.
Помощники `pub`: это API точки расширения, и их использует сторонний распознаватель (R4) — не неподключённая
поверхность.

### D5. `GestureRecognizer`: форма и запечатка (scope d)

```rust
pub trait GestureRecognizer: GestureArenaMember {
    /// Admit the contact a `Down` starts. Kind, button and both spaces come from the event.
    fn add_pointer(&self, down: PointerDispatch<'_>);
    /// Move / Up / Cancel (and any event) for a pointer; untracked pointers are ignored.
    fn handle_event(&self, dispatch: PointerDispatch<'_>);
    /// End the in-flight sequence, delivering its cancel callback at most once.
    fn cancel(&self) -> CancelOutcome;
}
```

- Нет ассоциированных типов (trait-table §1: `type Details` убил бы разнородное хранение), нет generic-методов,
  нет `Self: Sized`-ограничений; `PointerDispatch<'_>` — параметр-время жизни, dyn допустимо (проба).
- `cancel` без дефолта: no-op по умолчанию у пользовательского распознавателя — тихий отказ отмены.
- **Запечатка: нет.** Варианты: (1) настоящая запечатка — закрывает точку расширения, ради которой трейт и
  существует; (2) запечатка + мост `CustomGestureRecognizer` — статус-кво, мост теряет возможности
  (дедлайн) и дублирует имена; (3) **открытый трейт** — внешняя реализация легитимна по решению владельца, а
  нарушить инварианты фреймворка она не может: арена держит её слабо, уведомления сдержаны, порядок решений
  арены от неё не зависит. Запечатывать нечего — ни один метод не принимает приватный токен состояния.
- `GestureArenaMember` тоже открыт (trait-table 2.2) и тоже dyn.

### D6. Освобождение: `Drop` + явный `cancel()`

| Вариант | Итог |
|---|---|
| 1. `dispose(&self)` + флаг (сейчас) | use-after-dispose ловит только `debug_assert!`; пропуск = утечка (D3-1) |
| 2. Только `Drop` | отмена с колбэком (Draggable обязан вызвать `on_draggable_canceled`, back gesture — `on_drag_cancel`) из `Drop` — пользовательский код в деструкторе, во время раскрутки — abort |
| **3. `Drop` — тихое освобождение (выход из арены, ретайр захватов, без колбэков); `cancel() -> CancelOutcome` — доставленная отмена, распознаватель остаётся пригодным** | освобождение — одно, по владению; доставка — явная, вне деструктора |

**Выбран 3.** `#[non_exhaustive] pub enum CancelOutcome { Idle, Cancelled }`. **Без `#[must_use]`:** отмена
уже состоялась, игнорировать исход законно (виджет в `dispose`); `#[must_use]` ставится на исход, пропуск
которого — ошибка. Для набора — `pub fn cancel_all<'a>(impl IntoIterator<Item = &'a dyn GestureRecognizer>)
-> CancelOutcome`: отменяет каждого под `RoutePanic::capture`, первая паника возвращается после всех (F6).

### D7. Дедлайн — один запрос

```rust
pub trait GestureArenaMember {
    fn accept_gesture(&self, pointer: PointerId);
    fn reject_gesture(&self, pointer: PointerId);
    /// Pure query: no user callbacks, no arena calls. `None` = no armed deadline.
    fn deadline(&self) -> Option<Instant> { None }
    /// Called by the arena only once `now >= deadline()`, with the arena clock reading it took.
    fn poll_deadline(&self, now: Instant) { let _ = now; }
}
```

Варианты: (1) тройка методов (сейчас) — «согласованы» по конвенции, представимо рассогласование;
(2) **`deadline()` + `poll_deadline(now)`** — `has_pending == deadline().is_some()` по построению; часы
читает арена один раз до вызова (ownership §C №10: пользовательские часы под замком распознавателя);
(3) таймер-сервис с регистрацией и токеном — точнее, но новый механизм ради двух распознавателей.
**Выбран 2.** `GestureArena::{has_pending_deadlines, next_deadline, poll_deadlines}` сохраняют имена и
сигнатуры (их зовёт binding — зона T6d — и flui-runtime `frame_clock.rs:442,461`, `frame.rs:133`), реализация
— через `deadline()`. Опрос сдержан по участнику (F13); переполнение `Instant + Duration` → `checked_add`,
`None` = дедлайна нет (I9).

### D8. Настройки

| Вариант | Итог |
|---|---|
| `Arc<Mutex<GestureSettings>>` (сейчас) | замок без потоков |
| **builder `.settings(s)` → поле `GestureSettings` сейчас; `Cell<GestureSettings>` + `set_settings(&self)` — в I11 вместе с производственным вызовом из `did_change_dependencies` (`GestureSettingsScope`)** | нет неподключённого сеттера до I11; `Cell` требует `GestureSettings: Copy` — добавить `Copy` (все поля `Copy`) в I11 после I3 (ветка I3 меняет `settings.rs`) |
| пересоздавать распознаватель при смене настроек | теряет жест в полёте |

**Выбран средний.** Контакт снимает настройки и тип устройства в `begin` (R8): смена настроек действует со
следующей последовательности. 9 × `set_settings`, 10 × `with_settings` и геттеры `settings()` без
производственного вызова удаляются здесь.

### D9. Подключение к Listener (scope e)

| Вариант | Итог |
|---|---|
| 1. Ручная проводка (сейчас) | три копии, расхождения гейтов (R6a) |
| 2. `Listener::recognizer(Rc<dyn GestureRecognizer>)` с сильной ссылкой | кэшированный маршрут держит распознаватель после unmount → нужен флаг `mounted` в каждом виджете |
| 3. Отдельный виджет `RawGestureDetector` (Flutter) | второй примитив рядом с Listener ради одного поля |
| **4. `RecognizerSet` в flui-interaction (порядок, допуск, сдерживание) + `Listener::recognizer(&Rc<R>)` / `Listener::recognizer_when(&Rc<R>, admit)`; набор хранит `Weak<dyn GestureRecognizer>`** | владелец — состояние виджета; unmount = drop = молчание маршрута без флага; общий код — слоем ниже (AGENTS «Layers») |

**Выбран 4.**

```rust
// flui-interaction, recognizers/set.rs
#[derive(Clone, Default)]
pub struct RecognizerSet { entries: Vec<Attached> }       // !Send: Weak<dyn ..> + Rc<dyn Fn>
struct Attached { recognizer: Weak<dyn GestureRecognizer>, admit: Option<Rc<dyn Fn(PointerDispatch<'_>) -> bool>> }
impl RecognizerSet {
    pub fn attach<R: GestureRecognizer + 'static>(&mut self, recognizer: &Rc<R>);
    pub fn attach_when<R: GestureRecognizer + 'static>(&mut self, recognizer: &Rc<R>,
        admit: impl Fn(PointerDispatch<'_>) -> bool + 'static);
    /// Down: admitted → add_pointer. Anything else: handle_event on every live entry.
    pub fn dispatch(&self, dispatch: PointerDispatch<'_>);
    pub fn is_empty(&self) -> bool;
}
// flui-widgets, Listener
pub fn recognizer<R: GestureRecognizer + 'static>(self, recognizer: &Rc<R>) -> Self;
pub fn recognizer_when<R: GestureRecognizer + 'static>(self, recognizer: &Rc<R>,
    admit: impl Fn(PointerDispatch<'_>) -> bool + 'static) -> Self;
```

- `&Rc<R>`, а не `Rc<..>` по значению: сигнатура говорит «не забираю владение». `R: Sized` нужен для
  коэрции в `Weak<dyn ..>`; вариант для уже стёртого `Rc<dyn GestureRecognizer>` не добавляется, пока у него
  нет производственного вызова.
- Предикат — `bool`, а не enum: это фильтр (`Iterator::filter`, `Vec::retain`), а не флаг-параметр.
- Порядок доставки — порядок подключения (`Vec`), он же порядок вступления в арену (tap первым — передний
  участник, `gesture_detector.rs:464-466`).
- Распознаватели вызываются после `on_pointer_*` колбэков Listener и вне его `writer.write`: им `cx` не
  нужен, их колбэки открывают свой write через `WriterSource` виджета (вложенный write допустим — поправка
  ADR-0086 §5 в [../../send-flip/design.md](../../send-flip/design.md), «Публичный контракт» п. 1, — но не нужен).
- Каждый вызов (`admit`, `add_pointer`, `handle_event`) — под `RoutePanic::capture`, первая паника
  возвращается после обхода всех (F4); последующие payload удерживаются (ADR-0127).

### D10. Удаление (scope c)

Удаляются: `recognizers/one_sequence.rs`, `recognizers/primary_pointer.rs`, `RecognizerBase`,
`sealed::{gesture_recognizer, arena_member}`, `CustomGestureRecognizer` и blanket `arena/mod.rs:192-202`,
`traits::{GestureCallback, BoxedCallback, GestureRecognizerExt, Disposable}`, их реэкспорты (`lib.rs:287,
306, 340, 346`; facade `src/interaction.rs:27-29`). `DragAxis` остаётся в `traits.rs` (перенос — не в этой
спеке). `sealed.rs` после удаления содержит только hit-test-часть T6d; файл удаляет T6d (передача, §12).

### D11. Ретайр захватов при `Drop`

Колбэки неизменяемы (D1), поэтому под `RefCell` их не берут ни при вызове, ни при освобождении. `Drop` каждой
структуры `XCallbacks` ретайрит поля по одному через `retire_callback` из I1 (вынесенный по C2 в
`recognizers/callback_containment.rs`): до первой паники — обычный drop под `RoutePanic::capture`, после неё
или при `thread::panicking()` — удержание; первая паника возвращается в конце `Drop`, если раскрутки нет
(F5). Повтор кода в 10 структурах — приватный декларативный макрос `retire_callbacks!(self; on_tap,
on_tap_up, ..)` в том же модуле.

## 3. Публичный контракт (итог)

```rust
// arena
pub trait GestureArenaMember { /* D7 */ }
impl GestureArena {
    pub fn add<M: GestureArenaMember + 'static>(&self, pointer: PointerId, member: &Rc<M>) -> GestureArenaEntry;
    pub fn resolve(&self, pointer: PointerId, winner: Option<&Rc<dyn GestureArenaMember>>);
    // accept / reject / reject_member / resolve_team: &Rc<dyn GestureArenaMember> вместо Arc
}
impl GestureArenaEntry { pub fn member(&self) -> Option<Rc<dyn GestureArenaMember>>; /* остальное без изменений */ }

// recognizers
pub trait GestureRecognizer: GestureArenaMember { /* D5 */ }
#[non_exhaustive] pub enum CancelOutcome { Idle, Cancelled }
pub fn cancel_all<'a>(recognizers: impl IntoIterator<Item = &'a dyn GestureRecognizer>) -> CancelOutcome;

pub struct ArenaMembership { /* arena: GestureArena, this: Weak<dyn GestureArenaMember> */ }
impl ArenaMembership {
    pub fn new(arena: GestureArena, this: Weak<dyn GestureArenaMember>) -> Self;
    pub fn join(&self, pointer: PointerId) -> Option<GestureArenaEntry>; // None: self dead or arena closed
    pub fn now(&self) -> Instant;
}
pub struct PrimaryContact { /* membership, RefCell<Option<Contact>>, Cell<u64>, Cell<Option<Instant>> */ }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct ContactId(NonZeroU64);
#[derive(Debug, Clone)] #[non_exhaustive]
pub struct ContactSnapshot { pub id: ContactId, pub pointer: PointerId, pub kind: PointerType,
    pub local: Offset<f64>, pub global: Offset<f64>, pub settings: GestureSettings /* Copy после I11 */ }
#[derive(Debug, thiserror::Error)] #[non_exhaustive]
pub enum BeginContactError { #[error("already tracking {current:?}")] Busy { current: PointerId },
    #[error("contact admission requires Down")] NotDown,
    #[error("down position is not finite")] NonFinite, #[error("arena refused the member")] ArenaClosed }
impl PrimaryContact {
    pub fn new(membership: ArenaMembership) -> Self;
    pub fn begin(&self, down: PointerDispatch<'_>, settings: &GestureSettings) -> Result<ContactId, BeginContactError>;
    pub fn current(&self) -> Option<ContactSnapshot>;
    pub fn is_current(&self, id: ContactId) -> bool;
    pub fn tracks(&self, pointer: PointerId) -> bool;
    pub fn accept(&self);
    pub fn withdraw(&self) -> Option<ContactSnapshot>; // reject + clear (bowing out mid-competition)
    pub fn finish(&self) -> Option<ContactSnapshot>;   // stop tracking (self-driven sweep as today)
    pub fn arm_deadline(&self, after: Duration) -> Option<Instant>;
    pub fn disarm_deadline(&self);
    pub fn deadline(&self) -> Option<Instant>;
    pub fn moved_beyond(&self, local: Offset<f64>, slop: f64) -> bool; // non-finite → true
}
pub struct RecognizerSet { /* D9 */ }
// 10 × XGestureRecognizer::builder(..) -> XGestureRecognizerBuilder; build() -> Rc<X>
```

`ContactSnapshot` уже несёт замороженные настройки контакта и выводит `Clone`,
пока `GestureSettings` не имеет `Copy`. Контакт хранит снимок в `RefCell<Option<Contact>>`;
`current()` возвращает самостоятельный снимок без сохранённого borrow. I11 может добавить
`Copy` после изменения настроек. Это устраняет противоречие черновика, не откладывая R8.
`ContactSnapshot` — `#[non_exhaustive]`.

## 4. Инварианты

- **I1.** Все методы `GestureRecognizer`/`GestureArenaMember` — `&self`; `dyn` для обоих трейтов закреплён
  тестом (§9).
- **I2.** Распознаватель создаётся только `build()`/`Rc::new_cyclic`; после него колбэки неизменяемы; у
  `XCallbacks` нет `RefCell`.
- **I3.** Ни один `Ref`/`RefMut` состояния не жив при вызове колбэка, `tracing`-события или drop захвата:
  переход вычисляется под `borrow_mut` в исход (`Outcome`/`Notice`, I2), borrow снимается, затем доставка.
- **I4.** После каждого колбэка доставка проверяет `contact.is_current(id)`: `cancel`, новый контакт или
  drop внутри колбэка делают оставшиеся уведомления события устаревшими (F2). Идентичность — `ContactId`,
  не `PointerId` (мышь переиспользует id).
- **I5.** Арена держит участников только `Weak`; на время уведомления — временный `Rc` из `upgrade`, он же
  не даёт распознавателю умереть посреди своего метода (F1).
- **I6.** Объект-участник, который распознаватель создаёт сам (tap `TapArenaMember`), распознаватель держит
  сильно, пока последовательность жива.
- **I7.** `add_pointer` фиксирует допуск (вступление в арену + контакт) до любого пользовательского кода;
  паника колбэка позже не оставляет полу-допущенного контакта.
- **I8.** `begin` отказывает (`Busy`), а не перезаписывает: одна последовательность на `PrimaryContact`.
- **I9.** Дедлайн: `deadline()` — чистый запрос; `poll_deadline(now)` вызывается только при
  `now >= deadline()`; `arm_deadline` — `checked_add`, переполнение = `None`.
- **I10.** `RecognizerSet` доставляет не-`Down` события всем живым элементам независимо от предиката (R6, R6a);
  распознаватель игнорирует неотслеживаемый указатель.
- **I11.** `Drop` распознавателя: выход из арены через отложенное решение, ретайр захватов по ADR-0127; ни
  колбэков распознавателя, ни уведомлений других участников внутри `Drop`.
- **I12.** `ContactId` выдаётся `strict_add` от `Cell<u64>` (1.91): исчерпание — `BUG:`; `PointerId` и
  `ContactId` не смешиваются.

## 5. Владение

```
GestureDetectorState ──Rc──▶ TapGestureRecognizer ──▶ TapCallbacks ──Rc──▶ замыкание ──Rc──▶ tap_slot (у состояния)
        │                         │  └─ ArenaMembership ──▶ GestureArena (handle) ──▶ слоты ──Weak──▶ участник
        │                         └─ PrimaryContact ──▶ GestureArenaEntry (Weak slot, Weak member)
        └─ build: Listener ──▶ RecognizerSet ──Weak──▶ распознаватель
                        └─ lane HandlerCell (кэш маршрута до Up/Cancel) ──▶ RecognizerSet (тот же Weak)
```

- **Сильные ссылки на распознаватель** — только у владельца (состояние виджета; в тестах — тест). Арена,
  `RecognizerSet`, `GestureArenaEntry`, реестр дедлайнов — `Weak`.
- **Циклы.** арена ↔ распознаватель — нет (I5). распознаватель → колбэк → свой распознаватель — форма
  builder'а не даёт захватить `Rc`, которого ещё нет; путь через позже заполняемый слот
  (`Rc<RefCell<Option<Rc<Self>>>>`) остаётся возможным, rustdoc builder'а называет его и предлагает `Weak`.
  распознаватель → слот → пользовательское замыкание → состояние — состояние пользователю недоступно, слоты
  держит состояние.
- **`Drop` ровно один раз** — счётчик `Rc`; флага нет. Порядок: поле `PrimaryContact`/`ArenaMembership`
  (выход из арены, только фреймворк-состояние) объявляется **первым**, `XCallbacks` — последним, чтобы
  паникующий захват не помешал выходу из арены.
- **Unmount.** `dispose` виджета: `cancel_all(..)` (колбэки отмены, со сдерживанием) → `take()` и drop `Rc`
  → кэшированный маршрут при следующем `Move`/`Up` видит мёртвый `Weak` и молчит. Флаг `mounted` больше не
  гейтит распознаватели (остаётся у доставки семантики в `GestureDetector`).
- **Owner-local.** Ни `Arc`, ни атомиков, ни `parking_lot` в распознавателях: `Cell`/`RefCell`/`Rc`.
  `GestureArena` переходит на однопоточное хранилище в I10; эта спека меняет только тип участника.

## 6. Правило edition 2024 в распознавателе

Факт (проба и «Конвенции Rust»): временные скрутини `if let` живут весь then-блок, `match` — все плечи,
`let x = c.borrow().f();` — до `;`.

```rust
fn handle_event(&self, dispatch: PointerDispatch<'_>) {
    let Some(contact) = self.contact.current() else { return };   // самостоятельный снимок; Ref снят на `;`
    if !self.contact.tracks(pointer_of(dispatch.local)) { return }
    let notices = self.state.borrow_mut().step(contact, dispatch); // RefMut снят на `;`
    for notice in notices {
        if !self.contact.is_current(contact.id) { break }         // I4
        self.deliver(notice);                                      // borrow'ов нет
    }
}
fn deliver(&self, notice: Notice) {
    match notice {                                                 // скрутини — значение, не guard
        Notice::Tap(details) => invoke(&self.callbacks.on_tap, details), // поле без RefCell
        ..
    }
}
```

Запрещено (ревью и `clippy::significant_drop_in_scrutinee`): `match self.state.borrow().phase { .. =>
callback() }`; `if let Some(cb) = self.callbacks.borrow().on_tap.clone() { cb(..) }` (форма исчезает вместе с
`RefCell` колбэков); `self.slot = cell.borrow_mut().take()` (старое значение дропается под `RefMut`).

## 7. До/после на реальных вызовах

### `GestureDetector` (flui-widgets `interaction/gesture_detector.rs`)

До — `:722-734` (из 22 вызовов `with_on_*`), `:903-914`, `:949-981`, `:1053-1145`:

```rust
TapGestureRecognizer::new(arena.clone())
    .with_on_tap(move |_details| { let handler = primary_slot.borrow().clone(); /* write */ })
    .with_on_secondary_tap(move |_details| { /* … */ })
// dispose
recognizers.tap.dispose(); recognizers.long_press.dispose(); /* … ×5, без сдерживания */
// make_listener: RecognizerGroup + 4 замыкания; handle_down: add_pointer + handle_event(Down),
// add_pointer_with_kind + is_primary_button_down; forward: гейт drag/long_press по слоту
```

После:

```rust
// init_state
let tap = TapGestureRecognizer::builder(arena.clone())
    .on_tap(slot_callback(&self.tap_slot, &writer))              // помощник: slot → writer.write
    .on_secondary_tap(slot_callback(&self.secondary_tap_slot, &writer))
    .build();
// … long_press, double_tap, drag (DragAxis::Free), horizontal_drag — так же
// build
let gates = Rc::clone(&self.gates);                               // слоты — только для предикатов
Listener::new()
    .recognizer_when(&r.tap, { let g = Rc::clone(&gates); move |_| g.tap_active() })
    .recognizer_when(&r.long_press, { let g = Rc::clone(&gates); move |_| g.long_press_active() })
    .recognizer_when(&r.double_tap, { let g = Rc::clone(&gates); move |_| g.double_tap_active() })
    .recognizer_when(&r.drag, { let g = Rc::clone(&gates); move |_| g.drag_active() })
    .recognizer_when(&r.horizontal_drag, move |_| gates.horizontal_drag_active())
    .behavior(view.behavior)
// dispose
self.mounted.set(false);                                          // для семантики
if let Some(r) = self.recognizers.take() {
    let _ = cancel_all([&*r.tap as &dyn GestureRecognizer, &*r.long_press, &*r.double_tap,
                        &*r.drag, &*r.horizontal_drag]);
}
```

Исчезают: `RecognizerGroup`, `handle_down`, `forward`, `is_primary_button_down` (кнопку фильтрует
распознаватель, I1), `add_pointer_with_kind`, второй `handle_event(Down)` у tap, гейт `forward` (R6a чинится
формой: `RecognizerSet` доставляет терминальные события всегда).

### `BackGestureDetector` (flui-widgets `navigator/back_gesture.rs`)

До — `:286-311` (`on_pointer_down` с гейтом и `add_pointer`), `:597-611` (4 замыкания), `:631-636`,
`:640-655` (`horizontal_drag(arena).with_on_*`).

После:

```rust
fn build_recognizer(&self, ctx: &dyn BuildContext) -> Rc<DragGestureRecognizer> {
    let runtime = |r: &Rc<BackGestureRuntime>| Rc::clone(r);
    let (s, u, e, c) = (runtime(&self.runtime), runtime(&self.runtime), runtime(&self.runtime), runtime(&self.runtime));
    horizontal_drag(GestureArenaScope::of(ctx))
        .on_start(move |d| s.on_drag_start(d))
        .on_update(move |d| u.on_drag_update(d))
        .on_end(move |d| e.on_drag_end(d))
        .on_cancel(move || c.on_drag_cancel())
        .build()
}
// build
let runtime = Terminal::new(Rc::clone(&self.runtime));
Listener::new()
    .behavior(HitTestBehavior::Translucent)
    .recognizer_when(recognizer, move |_down| runtime.admits_new_gesture()) // enabled && нет жеста
// dispose
self.runtime.dispose_safety_net();
if let Some(r) = self.recognizer.take() { let _ = r.cancel(); }
```

`on_pointer_down` становится `admits_new_gesture(&self) -> bool` (только предикат). Обёртка `Terminal`
(ретайр по ADR-0127) сохраняется для захвата предиката.

### `Draggable` (flui-widgets `interaction/draggable.rs`)

До — `:1350` (`as Box<dyn MultiDragHandle>`), `:1353-1355`, `:1387-1414`, `:1470-1478`. После:

```rust
self.recognizer = Some(MultiDragGestureRecognizer::builder(arena, MultiDragAxis::Free).on_start(on_start).build());
// on_start возвращает Some(Rc::new(DragSession { .. }) as Rc<dyn MultiDragHandle>)
// build
Listener::new().recognizer_when(&recognizer, move |_| max.is_none_or(|max| active_count.load(Acquire) < max))
// dispose: feedback layer first (как сейчас), затем
if let Some(r) = self.recognizer.take() { let _ = r.cancel(); }
```

### `Scrollable`, `EditableText`, `InkWell` — без изменений

`scroll/scrollable.rs:593` (`GestureDetector::new().on_pan_start/update/end`), `text/editable_text.rs:970`
(`on_double_tap_down`), flui-material `ink_well.rs:379, 416` (`on_tap`) пользуются только API
`GestureDetector`, которое не меняется. Поведенческие изменения для них: R6a (pan-колбэки, снятые посреди
жеста, больше не оставляют drag залипшим — касается `Scrollable` при смене `physics`/`enabled`),
F1/F6 (размонтирование из колбэка, паника отмены). Ручной Listener `EditableText` (`:915`) не трогается.

### Сторонний распознаватель (facade fixture `tests/fixtures/facade_extensions.rs:245-290`)

До: `impl CustomGestureRecognizer` + `impl GestureRecognizer { fn add_pointer(self: &Arc<Self>, ..) {
self.base.start_tracking(.., self) } fn dispose(..) }`, `#[derive(Clone)]` с `RecognizerBase`. После:

```rust
struct DistanceRecognizer { contact: PrimaryContact, threshold: f64, accepted: Cell<usize>, rejected: Cell<usize> }
impl DistanceRecognizer {
    fn new(arena: GestureArena, threshold: f64) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { contact: PrimaryContact::new(ArenaMembership::new(arena, this.clone())),
            threshold, accepted: Cell::new(0), rejected: Cell::new(0) })
    }
}
impl GestureArenaMember for DistanceRecognizer { fn accept_gesture(&self, _: PointerId) { self.accepted.update(|n| n + 1); } /* reject */ }
impl GestureRecognizer for DistanceRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) { let _ = self.contact.begin(down, &GestureSettings::default()); }
    fn handle_event(&self, d: PointerDispatch<'_>) {
        if let Some(c) = self.contact.current() && d.local.position().dx - c.local.dx >= self.threshold { self.contact.accept(); }
    }
    fn cancel(&self) -> CancelOutcome { if self.contact.withdraw().is_some() { CancelOutcome::Cancelled } else { CancelOutcome::Idle } }
}
// и Listener::new().recognizer(&recognizer) в дереве теста
```

## 8. Миграция вызовов workspace

| Файл | Изменение |
|---|---|
| `crates/flui-interaction/src/recognizers/*.rs` (10 распознавателей, `drag_variants.rs`) | builder, `Rc`, `PrimaryContact`/`ArenaMembership`, `cancel`, `Drop`-ретайр, `deadline()`; in-`src` тесты |
| `crates/flui-interaction/src/recognizers/{recognizer,mod,one_sequence,primary_pointer}.rs`, новые `contact.rs`, `set.rs`, `callback_containment.rs` | трейт, помощники, удаление иерархии |
| `crates/flui-interaction/src/arena/{mod,team,signal_resolver}.rs` | `GestureArenaMember` (D7), `Weak`-слоты, `Rc<dyn>`, удаление blanket и `Sealed` |
| `crates/flui-interaction/src/{sealed,traits,lib}.rs` | удаление (D10); hit-test-часть — T6d |
| `crates/flui-interaction/src/binding.rs` (тесты `:1673, :1686`, зона T6d) | `impl CustomGestureRecognizer` → `impl GestureArenaMember`; патч владельцу T6d или правка после его слияния (§12) |
| `crates/flui-interaction/{tests/interaction_lane.rs (47 мест), tests/headless_long_press.rs, tests/multi_tap.rs, benches/{tap_detector,gesture_arena}_bench.rs, examples/custom_recognizer.rs}` | builder/`Rc`; `CustomGestureRecognizer` → `GestureArenaMember`; `stop_tracking_pointer` (`interaction_lane.rs:1270`) → Cancel-dispatch; пример — сторонний распознаватель через `RecognizerSet` |
| `crates/flui-interaction/{README.md, docs/GESTURES.md, docs/ARCHITECTURE.md}` | новый путь расширения; `GESTURES.md:390` (некомпилируемый impl) → компилируемый доктест |
| `crates/flui-widgets/src/interaction/{listener,gesture_detector,draggable}.rs`, `navigator/back_gesture.rs` | §7 |
| `crates/flui-runtime/src/ui_realm/tests/{pump_transaction.rs:121, closing_one_presentation_is_invisible_to_siblings.rs:364}` | builder; `impl GestureArenaMember` |
| `crates/flui-testing/{src/lib.rs:77-83 (доктест), tests/pointer_script_replay.rs:36, tests/owner_scope.rs}` | builder, `add_pointer(dispatch)` |
| facade `src/interaction.rs:10-36`; `tests/fixtures/facade_extensions.rs` | реэкспорт новых типов, удаление старых; fixture §7 |
| `.config/nextest.toml:59` | `binary_id(flui-interaction::compile_fail)` в группу `trybuild` |

Не меняются: flui-material, flui-cupertino, flui-devtools (`rg 'Recognizer::|with_on_|add_pointer'` по
`packages/` пуст), `scrollable.rs`, `editable_text.rs`.

## 9. Тестовая стратегия

- **dyn закреплён двумя способами.** (1) trybuild **pass**-фикстура
  `tests/compile_pass/dyn_compatible_extension_points.rs`: `fn _f(_: &dyn GestureRecognizer, _: &dyn
  GestureArenaMember, _: &dyn MultiDragHandle) {}` и сторонний `impl GestureRecognizer` через публичные пути
  — на базе E0038 (тест красный, вывод — в PR), после RA2 зелёный; регресс dyn-compatibility снова его
  покраснит. Строка не живёт в `tests/main.rs`: некомпилируемая строка сломала бы весь бинарь. (2) Поведение через `dyn`:
  `heterogeneous_set_drives_builtin_and_custom_recognizers` — `RecognizerSet` с tap, drag и
  тестовым сторонним распознавателем; tap побеждает на коротком касании, drag — после slop, сторонний —
  по своему дедлайну. Без `dyn` этот тест не пишется вовсе.
- **trybuild `!Send`** — `crates/flui-interaction/tests/compile_fail.rs` (тест `trybuild_ui`, имя покрыто
  `--skip trybuild_ui` в `gamma.toml:29`), фикстуры `tests/compile_fail/`:
  `recognizer_stays_on_its_thread.rs` (`assert_send::<TapGestureRecognizer>()`), `builder_stays_on_its_thread.rs`
  (builder в `thread::spawn`), `recognizer_set_stays_on_its_thread.rs`. Ожидание — E0277 с `Rc<dyn Fn`
  в цепочке. `trybuild` — новая dev-зависимость крейта (workspace уже её имеет).
- **Матрица отказов** (`tests/recognizer_lifecycle.rs`, таблицы через существующий раннер I1/I2): строки F1–F14
  по каждому из 10 распознавателей там, где применимо (C1 одиночная паника, C2 две, C3 drop/cancel, C4
  следующий жест). Строки «во время раскрутки» — в дочернем процессе, как drag
  (`tests/interaction_lane.rs:859-944`).
- **flui-widgets** (`tests/` крейта, модуль существующего `main.rs`):
  `listener_admits_by_predicate_and_forwards_terminal_events`, `clearing_pan_callbacks_mid_drag_still_finishes_the_drag`,
  `callback_that_unmounts_its_detector_finishes_the_event`, `unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival`,
  `cancel_all_cancels_every_recognizer_before_resuming_the_first_panic` (через `GestureDetector` с двумя
  паникующими `on_*_cancel`).
- **Отказ без изменения:** каждая строка гоняется в изолированном checkout с откатом production-хунка и
  падает по названной причине (tasks.md, колонка «Красный без»).
- **Бенчи.** `tap_detector_bench`: строки `handle_event/static` (вызов на `Rc<TapGestureRecognizer>`) и
  `handle_event/dyn` (через `RecognizerSet`: `upgrade` + vtable + `catch_unwind`). `gesture_arena_bench`:
  `resolve/strong` (база до — `Arc<dyn>` сильный) и `resolve/weak` (`Weak` + `upgrade` на уведомление). Один
  хост, `CARGO_BUILD_JOBS=6`, criterion `--save-baseline before` на базе I1+I2, `--baseline before` на ветке.

## 10. ADR и changelog

**ADR-NNNN (номер назначит оркестратор), «Gesture recognizers are owner-local values attached to a
Listener».** `Supersedes: ADR-0086 §4`; в ADR-0086 у §4 — `Superseded-by: ADR-NNNN`. Черновик:

> **Context.** ADR-0086 §4 froze `GestureArenaMember` and the recognizer callback aliases so that custom
> recognizers would not break. No custom recognizer could be attached to a widget, `GestureRecognizer` was
> not dyn-compatible, and the arena kept members alive strongly, so a skipped `dispose` leaked them.
>
> **Decision.** (1) Recognizers are built by a builder and returned as `Rc<Self>`; callbacks are immutable
> after `build`. Recognizers are `!Send`. (2) `GestureRecognizer` and `GestureArenaMember` are open,
> dyn-compatible traits with `&self` methods; a recognizer reaches its own arena identity through a `Weak`
> taken in `Rc::new_cyclic`. (3) The arena holds members weakly; dropping the last `Rc` withdraws a
> recognizer through the deferred-resolution queue and retires its captures per ADR-0127. Delivered
> cancellation is an explicit `cancel() -> CancelOutcome`. (4) A member owns at most one deadline, exposed by
> `deadline()`; the arena polls only due members with its own clock. (5) `flui-widgets` attaches recognizers
> to a `Listener` through `RecognizerSet` (weak, ordered, admission on `Down` only, terminal events always
> forwarded, per-recognizer panic containment). Widgets own their recognizers; the Listener never does.
>
> **Consequences.** Callback signatures (`Rc<dyn Fn(Details)>`) and the widget-side `WriterSource` wrapping of
> ADR-0086 §4 are unchanged. Custom recognizers implement `GestureRecognizer` with the public
> `PrimaryContact`/`ArenaMembership` helpers. A recognizer nobody owns does nothing.

**Changelog** — `changelog.d/recognizer-api.md`: `### Changed` (builder вместо `with_on_*`, `Rc` вместо
`Arc`, `add_pointer(dispatch)`, `GestureArenaMember::deadline`, `MultiDragStartCallback` → `Rc`),
`### Added` (`Listener::recognizer[_when]`, `RecognizerSet`, `PrimaryContact`, `ArenaMembership`,
`CancelOutcome`, `cancel_all`), `### Removed` (`CustomGestureRecognizer`, `sealed::{gesture_recognizer,
arena_member}`, `OneSequenceGestureRecognizer`, `PrimaryPointerGestureRecognizer`, `RecognizerBase`,
`GestureCallback`, `BoxedCallback`, `GestureRecognizerExt`, `Disposable`, `dispose`, `with_settings`,
`set_settings`, `add_pointer_with_kind`), `### Fixed` (R6a, F10, D1–D3, D5 panic-matrix).

## 11. Adversarial review (principal-architect → security-engineer)

| # | Угроза / сценарий | Что ломается без ответа | Ответ в design | Тест |
|---|---|---|---|---|
| A1 | Реентри: колбэк роняет последний `Rc` распознавателя (unmount) посреди `handle_event` | use-after-free невозможен в safe Rust, но `Drop` посреди метода — нет; сегодня `BorrowMutError` (ownership §C №1) | I5: вызывающий (`RecognizerSet`, арена) держит `upgrade`-`Rc` на время вызова; `Drop` — после возврата | F1 |
| A2 | Реентри: колбэк вызывает `cancel()` своего распознавателя, затем тот же цикл доставки шлёт `on_tap` | двойная доставка, `on_tap` после `on_tap_cancel` | I4: `is_current(id)` после каждого колбэка | F2 |
| A3 | Реентри: `Drop` захвата диспатчит событие в тот же Listener | `upgrade` мёртвого — `None`; набор не держит borrow | I5, `RecognizerSet::dispatch` без `RefCell` | F5 строка «реентерабельный drop» |
| A4 | Отмена посреди последовательности, затем `Move`/`Up` того же указателя | доставка в отменённый контакт | `tracks(pointer)` ложно после `withdraw`; следующий `Down` с тем же id — новая арена (I1 освобождает удержанные слоты) | F2, F7 |
| A5 | Два указателя в одном кадре на один primary-распознаватель | перезапись контакта (Z1 I19) | I8 `Busy` | R9, F9 |
| A6 | Unmount во время арены: `accept_gesture` колбэк размонтирует победителя | арена уведомляет уже мёртвого | `Weak` + `upgrade` на уведомление; мёртвый = `reject` | F8, F10 |
| A7 | Rebuild убирает распознаватель из `Listener` посреди жеста (сторонний виджет) | распознаватель не видит `Up`, залипает | **остаточный риск:** набор заменяется целиком при rebuild. Правило в rustdoc `Listener::recognizer`: снятие attachment = `cancel()` у владельца; виджеты фреймворка подключают всегда и гейтят предикатом (§7) | `listener_admits_by_predicate_and_forwards_terminal_events` (строка «предикат ложен посреди жеста») |
| A8 | Паника: одиночная, в конкуренции, в `Drop`, в `cancel_all`, следующий жест | см. panic-matrix D1–D5 | D6, D9, D11; I7 | F3–F7 |
| A9 | NaN/inf в `Down`/`Move`; переполнение `Instant + Duration` при огромном `long_press_timeout` | NaN-расстояние `> slop` ложно → tap с NaN-позицией; `Instant` паникует при переполнении `+` | `begin` → `NonFinite`; `moved_beyond` → `true`; `checked_add` → без дедлайна (I9) | F11, строка `huge_timeout_arms_no_deadline` |
| A10 | Неподдерживаемое устройство (`PointerType` неизвестен), кнопка не та | slop 0 или паника по `match` | неизвестный тип → slop касания; кнопка — правило I1 | F12, R8 |
| A11 | Сторонний участник: `deadline()` паникует | паника из frame clock (flui-runtime) без сдерживания, остальные дедлайны теряются | обход сдержан по участнику, паникующий = `None`, первая паника — после обхода | F13 |
| A12 | Сторонний участник не сдвигает `deadline()` после `poll_deadline` | кадр за кадром (как бесконечная анимация), CPU | опрос — раз за тик арены; `tracing::warn!` один раз на участника, если дедлайн не изменился после опроса; поведение задокументировано в rustdoc трейта | строка `stale_deadline_is_polled_once_per_tick` |
| A13 | Сторонний распознаватель эагерно `accept` на каждый `Down` | забирает все жесты своего поддерева | легитимно (так работает `EagerGestureRecognizer`); арена — на презентацию, чужие окна не затронуты; `GestureArenaEntry::member()` отдаёт только своего участника | — |
| A14 | Предикат допуска паникует или реентерабельно меняет слоты | пропуск остальных attachment | предикат — под `capture`, как вызовы распознавателя | F4 строка «паника предиката» |
| A15 | Порядок доставки зависит от `HashMap` | недетерминированный победитель на сбросе | `Vec` в порядке подключения | `heterogeneous_set_…` (порядок закреплён) |
| A16 | Исчерпание `ContactId`, повтор id после wrap | старый контакт совпадает с новым | `strict_add` + `BUG:` | F14 (ревью; 2^64 не тестируется) |
| A17 | Цикл через слот: колбэк захватывает `Rc` своего распознавателя, положенный в слот после `build` | утечка | не исключён типом; rustdoc builder'а; все виджеты фреймворка держат `Weak` в таких местах | — |

## 12. Передача T6d

- `sealed::{hit_testable, focus_node}::Sealed`, `CustomHitTestable` (`sealed.rs:136-176, 232-235`),
  `HitTestable` и `routing/event_router.rs` (trait-table 2.10), `HitTestTarget` (`traits.rs:26, :203`) —
  удаление в T6d; после него файл `sealed.rs` исчезает.
- `binding.rs` тестовые `impl CustomGestureRecognizer` (`:1673, :1686`): патч на `impl GestureArenaMember`
  передаётся владельцу T6d вместе с задачей RA5; если T6d слит раньше — правка в RA5 по согласованию.
- `PointerEventExt`/`PointerEventExtTrait` — словарь P2, не здесь.

## 13. Риски

1. **Конфликт с send-flip T6e** (`crates/flui-widgets/**` кроме `text/controller.rs`): `gesture_detector.rs`,
   `draggable.rs` в обоих. Смягчение: RA4 после ядра send-flip или ребейз по рецепту; у send-flip в
   `gesture_detector.rs` одна строка (`:711`).
2. **I10 трогает `arena/mod.rs`** (однопоточное хранилище). RA1 идёт после I10; если I10 задерживается, RA1
   меняет только тип участника и `Weak`-слоты, I10 ребейзится.
3. **Промежуточные ветки не собирают workspace:** смена типов распознавателей ломает flui-widgets до RA4.
   Поэтому интеграционная ветка `recognizer-api/core`, один PR в `main` (tasks.md).
4. **Цена `dyn` + `upgrade` на событие** — измеряется (R13); если строка `handle_event/dyn` хуже на > 10 %,
   `RecognizerSet` переходит на `SmallVec<[Attached; 5]>` (с бенчем, «Конвенции Rust»).

## Открытый вопрос владельцу

Выводить ли точку расширения (`GestureRecognizer`, `PrimaryContact`, `ArenaMembership`, `RecognizerSet`,
`Listener::recognizer`) в `flui-sdk` сейчас (ADR-0088 §4, строка в `crates/flui-sdk/tests/surface.rs`)?
**Рекомендация:** нет — только facade `flui::interaction`/`flui::widgets`, пока официальный пакет не попросит:
SDK — Evolving-поверхность, а неподключённый `pub` в ней — тот же дефект, что и в крейте.
