# teardown — дизайн

- **Статус:** черновик (ревью: два раунда rework, решения оркестратора D-L1…D-L6 внесены)
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md); уровень 0 —
  [release/requirements.md](../release/requirements.md) R10, R11, D4; смежные —
  [persistence](../persistence/requirements.md) (R3, R12, R14, R15, R21, R24) и её
  [design](../persistence/design.md), [text-ime](../text-ime/requirements.md) (R8, F5) и её
  [design](../text-ime/design.md)

Teardown **владеет** общим контрактом закрытия и завершения сеанса: close guard, причина
закрытия, реестр сброса, порядок доставки. Persistence его потребляет.

## Текущее состояние

Только то, на что опирается дизайн.

- **Граница FFI.** `window_proc` ловит панику, пишет `tracing::error!` и вызывает
  `std::process::abort()` (`crates/flui-platform/src/platforms/windows/platform.rs:948-975`).
  `WM_CLOSE` спрашивает вето (`dispatch_should_close`, `:1027-1052`), `WM_DESTROY` вызывает
  `on_close` (`:1055-1060`), обёртка уходит из карты окон в `:1090-1125`. `WM_QUERYENDSESSION`
  и `WM_ENDSESSION` не обрабатываются нигде в `crates/`. Входы COM в TSF по design text-ime
  каждый под `catch_unwind`.
- **Вход в realm.** Все platform-callback'и `flui-app` идут через `dispatch_platform_realm`
  (`crates/flui-app/src/app/runner/realm_dispatch.rs:1012`). Пока задача выполняется, realm
  **извлечён из слота** (`:1409-1427`); повторный вход в тот же realm ставит задачу в очередь
  (`:1410`). Задача идёт под `catch_unwind` (`:1448`), realm возвращается в слот, отложенные
  мутации применяются, удалённые realms уничтожаются под catch (`drop_removed_realms`,
  `:1730-1738`), затем первая паника **перевыбрасывается** (`:1722`).
- **Закрытие единственной presentation** (`realm_dispatch.rs:1458-1520`): реестр, роутер вето,
  `stop_presentations` (`begin_close` + Detached,
  `crates/flui-runtime/src/ui_realm/presentation_lifecycle.rs:162-181`;
  `LifecycleSource::begin_close` — `crates/flui-view/src/lifecycle.rs:276-281`), затем
  `request_realm_uninstall`; `UiRealm` уничтожается в хвосте той же dispatch. `on_close`
  (`desktop.rs:599-622`) ловит панику и делает `resume_unwind` (`:620`), а на Win32 это abort.
- **`UiRealm::drop`** (`crates/flui-runtime/src/ui_realm/mod.rs:326-422`): отзыв полномочий,
  ключи, закрытие presentations, сброс; первая паника перевыбрасывается **из `Drop::drop`**
  (`:415-420`), и остальные поля дропаются уже в unwind.
- **Выход из цикла.** `run_with_platform` (`crates/flui-app/src/app/runner/main_window.rs:516-645`):
  `platform.run` → `shutdown_main_window` → `teardown_platform_realm` (`realm_dispatch.rs:1980-2104`):
  `drop(realms)` (`:2034`) без catch, сервисы (`SERVICE_SHUTDOWN_DEADLINE` 5 с, `:1970`), пулы
  (`EXECUTION_SHUTDOWN_GRACE` 5 с на каждый из двух, последовательно; `crates/flui-runtime/src/execution.rs:599-622`).
- **Вето.** `CloseRequest` несёт только адрес (`crates/flui-app/src/app/close_request.rs:105-123`);
  `KeepOpen` не оставляет обязательств. `AppHandle::request_quit` вето не спрашивает
  (`crates/flui-app/src/app/application_control.rs:136-144`).
- **Пробуждение.** `UiRealm::next_wake` — минимум дедлайнов presentation
  (`crates/flui-runtime/src/ui_realm/frame_clock.rs:458-463`), хост агрегирует его в
  `AppRuntime::next_wake` (`crates/flui-app/src/app/runtime.rs:1227-1233`) для wake-deadline hook.
  Повтор неудачного кадра темпирует только `FallbackGate` (`frame_pacing.rs:234`), без отката.
- **Отчёт о сбое.** `FrameFailureKind` (`#[non_exhaustive]`, `crates/flui-runtime/src/frame_failure.rs:238`),
  `report_frame_failure` (`crates/flui-runtime/src/ui_realm/construct.rs:357`), в фасаде
  `flui::app::FrameFailureKind`.
- **Удержание.** `retain_opaque_payload` (`crates/flui-foundation/src/panic.rs:58`),
  `preserve_first_lifecycle_panic`, `TaskToken::drop` (`crates/flui-scheduler/src/async_driver.rs:300-330`).
- **Observer.** `BuildOwner::drop` только возвращает claims ключей
  (`crates/flui-view/src/owner/build_owner.rs:2921-2933`); `detached()` шлют лишь
  `set_/clear_tree_observer` (`:1326-1344`).
- **IME.** Design text-ime: при dispose поле подтверждает preedit на месте в контроллере, затем
  `store.detach()`. Её утверждение SAFETY опирается на то, что вложенного доступа к realm нет.
- **Persistence design** (до этой правки): `CloseGuard`/`CloseHold` в `flui-view`, запись
  циклом документа в `AsyncDriver` realm.

## Варианты

### (a) Где локализуется паника пользовательского callback

| | a1. Per-task catch в диспетчере `flui-app` | a2. На трамплине платформы | a3. В диспетчере `flui-interaction` |
|---|---|---|---|
| Суть | Каждая `RealmTask` под своим catch; паника даёт один отчёт, realm возвращается в слот, хвост FIFO исполняется | `window_proc` ловит и решает сам | catch вокруг жест/клавиатурных handler'ов |
| Требования | R9, R11, R15, R23; все виды callback | Только «не abort» | Только жесты и клавиши |
| Риск | Post-frame паника должна оставлять scheduler в `Idle` | Продолжение после порванного `WM_DESTROY` | Неполное покрытие |

**Выбор: a1 как граница, плюс минимальный a2 как сетка.** `window_proc`: не-`BUG:` payload
уходит в терминальный слот платформы (поле owner-состояния Win32, без `static`). С этого момента
окно отвечает только `DefWindowProcW`, ставится `PostQuitMessage`. `Platform::run` возобновляет
payload после цикла, `run_with_platform` ловит его вокруг `platform.run`, проводит teardown, и
процесс выходит с кодом 101. `BUG:` по-прежнему abort'ит. Нажатая клавиша, чей handler
запаниковал, считается обработанной, поэтому Alt+F4 «по панике» не срабатывает.

### (b) Как паника teardown становится кодом 101 после flush

| | b1. Терминальный слот хоста + `resume_unwind` | b2. `process::exit(101)` | b3. `Err(AppRunError::…)` |
|---|---|---|---|
| Код | 101 по контракту std, hook не повторяется | 101, но деструкторы `main` пропущены | Зависит от `main` пользователя |

**Выбор: b1.** Терминальны (D4) только сбои последнего realm и выхода из цикла: задача
`ClosePresentation` последнего realm целиком (доставка закрытия и уничтожение), уничтожение
realm в `teardown_platform_realm`, `queued_turns`, `on_quit`, ошибка сброса реестра или паника
сервиса при shutdown, если слот пуст. Сбой при закрытии не последнего realm даёт один
`CallbackPanic` и удержание. Возобновляется инертный payload (`&str`/`String`); непрозрачный
удерживается, а возобновляется `String` с его текстом.

### (c) Учёт `Drop` (вопрос 1)

| | c1. Debug-счётчики во фреймворке | c2. Сторожа владения в фикстуре | c3. `TreeObserver` |
|---|---|---|---|
| Доказывает | Счётчик, не `Drop`; новый `static` | Освобождение в `Drop`; slab-узлы переживают сброс только через `forget` или утечку realm, и оба случая оставляют сторожей живыми | Unmount ≠ `Drop` (ADR-0040) |

**Выбор: c2** (`DropLedger`, как флаг `retired` у `Readiness` в `tests/fixtures/notes_flow.rs`),
плюс типизированные отказы устаревших handle (R19) как доказательство того, что realm ушёл.

### (d) Завершение сеанса Windows

| | d1. Реестр сброса вне realm + доставка только в realm в слоте (D-L1, D-L2) | d2. `DetachNotifier` в извлечённый realm (прежний выбор) | d3. Не обрабатывать |
|---|---|---|---|
| Пользовательский код во вложенном pump | Нет | Да: нарушает SAFETY text-ime и повторный вход | — |
| Что пишется на диск | Последние закодированные байты каждого документа — всё, что приложение отдало через `Persisted::set` | Снимок, если наблюдатель успел | Ничего |
| Путей записи | Один (реестр) | Два (`AsyncDriver` и снимок) | — |

**Выбор: d1.** Подробно — в шагах S1–S6 ниже.

## Публичный контракт

### Close guard и причина закрытия — `flui-view` (D-L4)

```rust
// flui-view; фасад: flui::view::…; CloseReason ещё и flui::app::CloseReason (реэкспорт flui-app)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] #[non_exhaustive]
pub enum CloseReason { User, Program, SessionEnd }

pub trait LifecycleContext: BuildContext {
    fn close_guard(&self) -> Option<CloseGuard> { None }   // ADR-0078: только в lifecycle-хуках
}
#[derive(Clone)] pub struct CloseGuard { /* Arc<состояние presentation> */ }   // Send + Sync
impl CloseGuard {
    pub fn hold(&self) -> CloseHold;             // работа, которая завершится: вето User и Program;
                                                 // SessionEnd пропускает — его байты пишет сброс
    pub fn require_decision(&self) -> CloseHold; // Failed/Conflict/ReadOnly с правками:
                                                 // вето каждой ветируемой причины
    pub fn pending(&self) -> Option<PendingClose>;    // самая ранняя запись; не больше одной на причину
    pub fn changed(&self) -> CloseChanged;            // один future: запрос, отзыв, разрешение
    pub fn can_veto(&self, reason: CloseReason) -> bool;  // спрашивает ли платформа эту причину
}
#[must_use] pub struct CloseHold { /* Arc */ }
pub struct PendingClose { /* причина + эпоха */ }
impl PendingClose {
    pub fn reason(&self) -> CloseReason;
    pub fn discard_and_close(self);   // закрыть мимо holds, операция владельца
    pub fn stay_open(self);           // снять только эту запись
}
pub struct CloseChanged { /* эпоха */ }   // Future<Output = ()> + Send
```

Семантика:

- **Запись о закрытии.** Ветированный запрос (holds или `KeepOpen` обработчика) создаёт
  `PendingClose` для своей причины; повтор той же причины идемпотентен.
- **Отпускание holds.** Когда отпущен последний hold, а записи есть и ни одна не удержана
  `require_decision`, в очередь владельца ставится **одна** операция закрытия этого
  `PresentationAddress`. Обработчик повторно не спрашивается.
- **Отзыв.** `SessionEnd::Cancelled` снимает только запись `SessionEnd`; запись `User`
  (крестик пользователя) остаётся.
- **Без вето** (`can_veto == false`: `Ending`, web, Android, iOS): консультации нет, работает
  только сброс фреймворка; `Hidden`/`Paused` пишут сразу.
- **`Program`.** Это ветируемый запрос кода: `AppHandle::request_quit` спрашивает guard каждой
  presentation с `Program` и при вето откладывает quit до отпускания. `request_presentation_close`
  остаётся неветируемым завершением.
- **Реализация.** Источник (`CloseGuardSource`) лежит в `flui_view::__runtime` и в фасад не
  входит. `CloseGuard` — одно состояние на presentation, внутри него защёлка «доставка закрытия
  выполнена или идёт» (D-L1).

`flui-app`: `CloseRequestHandler` остаётся синхронным `Close`/`KeepOpen`; добавляется
`CloseRequest::reason(&self) -> CloseReason`. `KeepOpen` записывает `PendingClose` для этой
причины. `with_withdrawal` и `DetachNotifier` удалены.

### Реестр сброса — `flui-view` (D-L2)

```rust
// flui-view, internal (флаг persist не нужен); фасад: нет (его читает Persisted в flui-widgets)
#[derive(Clone)] pub struct FlushRegistry { /* Arc<Mutex<…>>, принадлежит хосту, вне realm */ }
impl FlushRegistry {
    /// Owner-поток: заменить последние байты документа (latest wins), пробудить писателя.
    pub fn publish(&self, name: StorageName, bytes: Arc<[u8]>, base: StoredVersion) -> Revision;
    pub fn committed(&self, name: StorageName) -> Option<(Revision, StoredVersion)>;
    pub fn outcome(&self, name: StorageName) -> FlushState;   // Clean / Pending / Failed(StorageError)
}
pub trait LifecycleContext: BuildContext { fn flush_registry(&self) -> Option<FlushRegistry> { None } }
```

`flui_view::__runtime` даёт хосту `FlushRegistry::new(storage, writer)` и
`flush_within(&self, deadline) -> FlushReport` (синхронно, owner-поток). Обычно пишет IO-пул:
одна запись в полёте на документ, новые байты дают ровно одну следующую. CAS от `base` и
атомарная замена (persistence R21) — в `Storage`/`FileStore` persistence. `StorageName`,
`StoredVersion`, `StorageError` — типы persistence из `flui-platform-api`.

### Прочее

| Элемент | Крейт | Фасад | Production-вызов |
|---|---|---|---|
| `FrameFailureKind::CallbackPanic { message: PanicText, internal_invariant: bool }`; `Contained`. Должен появиться в снимке поверхности (release R3); `FrameFailureHandler` документируется как обработчик сбоев realm | `flui-runtime` | `flui::app::FrameFailureKind` | per-task catch |
| `UiRealm::report_contained_panic(&self, address: PresentationAddress, payload: &(dyn Any + Send))`. Адрес задачи: для задач уровня realm — первичный адрес слота; для закрытия не единственной presentation — закрываемая (может быть уже закрыта) | `flui-runtime` | нет | per-task catch |
| `SessionEnd`, `SessionEndAnswer`, `Platform::on_session_end(Box<dyn FnMut(SessionEnd) -> SessionEndAnswer>)` (без `Send`: owner-поток), `HeadlessPlatform::simulate_session_end` (прецедент `simulate_close`) | `flui-platform` | нет, внутреннее для `flui-platform`/`flui-app` | `run_with_platform` |

Без новой поверхности: терминальные слоты, откат повтора кадра, `detached()` в
`BuildOwner::drop`, защёлка доставки. `Application::run` и `run_app` получают `# Panics`.

## Инварианты, владение, порядок teardown

**D-L1. Пользовательский код выполняется только в realm, который лежит в слоте.** Код
фреймворка, который вызывается при извлечённом realm (сеанс во вложенном pump), трогает только
состояние вне realm: реестр сброса, карту хоста, платформу.

**Общая доставка закрытия** — одна функция `flui-runtime`, вызываемая для close, quit, session
end и из `UiRealm::drop`:

1. Защёлка presentation (в `LifecycleSource`) переходит в «идёт»; если она уже выставлена,
   вызов — no-op. Поэтому вложенный pump внутри наблюдателя Detached и повтор из
   `UiRealm::drop` ничего не делают.
2. Сфокусированное поле подтверждает композицию на месте в контроллере (тот же шаг, что dispose
   в design text-ime; повтор в dispose — no-op). Разбор TSF после этого только cleanup.
3. `begin_close`, затем Detached наблюдателям при живом дереве. Приложение может вызвать
   `Persisted::set` — байты уходят в реестр.
4. Паника на шагах 2–3 при закрытии последнего realm терминальна.

**Штатное закрытие последнего окна:**

1. Вето `CloseReason::User` (обработчик, затем guard); `KeepOpen` или hold → `PendingClose`,
   ничего не разбирается (R23).
2. `on_close` → `ClosePresentation`; внутри кадра (R15) задача встаёт в FIFO; повтор получает
   `PresentationClosing` (R18).
3. Общая доставка закрытия.
4. Уничтожение realm в хвосте dispatch под catch: полномочия → ключи → dispose/unmount
   (`TaskToken` ровно один раз, R2) → `detached()` observer'а (R3) → деревья. Сбой уходит в слот.
5. `on_close` возвращается нормально; платформа удаляет обёртку (R5); exit policy.
6. После `Platform::run` (его паника ловится, сетка a2): `shutdown_main_window`;
   `teardown_platform_realm` — каждый realm и `queued_turns` под catch.
7. **Сброс реестра** `flush_within(SERVICE_SHUTDOWN_DEADLINE)`: дождаться записи в полёте и
   записать последние неподтверждённые байты. Выполняется всегда, даже при занятом слоте (D2).
8. Сервисы с остатком дедлайна; 9. пулы с grace; 10. буфер обмена, redraw-окно.
11. **Код:** слот → `resume_unwind` → 101; `AppRunError` → `Err`; иначе 0. Потолок 15 с.

**Завершение сеанса Windows** (бюджет `SESSION_END_BUDGET = 3 с` на работу фреймворка):

- **S1. `WM_QUERYENDSESSION`.** Для каждой presentation в слоте: вето с `CloseReason::SessionEnd`
  (обработчик и guard; `hold` пропускает, `require_decision` ветирует). Есть вето → `FALSE` и
  `ShutdownBlockReasonCreate`, запись `PendingClose(SessionEnd)`. Кэш ответа живёт **одну пачку
  сообщений** (D-L5): он нужен, потому что Windows шлёт запрос каждому окну верхнего уровня.
  Кэш и причина блокировки снимаются, когда owner-цикл вернулся к ожиданию сообщений, или на
  любом `WM_ENDSESSION`. Для извлечённого realm вето не спрашивается (D-L1); ответ `TRUE`,
  потому что его байты пишет сброс.
- **S2. `WM_ENDSESSION(FALSE)`.** Причина блокировки снимается, отзывается только
  `PendingClose(SessionEnd)`, `changed()` будится.
- **S3. `WM_ENDSESSION(TRUE)`, доставка.** `ShutdownBlockReasonCreate` держится до конца S6.
  Realm в слоте получает общую доставку закрытия (пользовательский код). Извлечённый realm
  получает в FIFO задачу `SessionEnding`, которая выполнится, только если процесс выживет;
  пользовательского кода нет, preedit этого realm теряется (документированный предел).
- **S4. Сброс реестра** синхронно, inline, с дедлайном «остаток бюджета». Работа фреймворка,
  выполняется всегда. `AsyncDriver` realm не участвует.
- **S5. Разбор realms** — только если ни один realm не извлечён и осталось ≥ 1 с. Иначе realms
  оставляются, и процесс завершает ОС.
- **S6.** Сервисы отменяются без ожидания, пулы — без grace; `PostQuitMessage` всегда (включая
  `ENDSESSION_CLOSEAPP`); затем `ShutdownBlockReasonDestroy`.

**Порог 1 с (D-L3).** Шаги с пользовательским кодом (S3 начинается при полном бюджете; S5 —
`Drop`) пропускаются, если до конца бюджета меньше 1 с. Обоснование: S4 должен успеть записать
документы. Для Notes это ≈ 0,3 МиБ, `FlushFileBuffers` и `MoveFileExW` — десятки–сотни мс на
HDD; ещё ~0,5 с — запас на `ShutdownBlockReasonDestroy` и `PostQuitMessage` до UI «приложение
мешает завершению», который Windows показывает примерно через 5 с. Код пользователя прервать
нельзя: наблюдатель, который блокирует S3 дольше бюджета, съедает время сброса. Это предел
приложения, он задокументирован. Тест утверждает только долю фреймворка.

**Первая ошибка главная.** Слот хранит первый payload, следующие логируются. Payload
локализованной паники дропается под catch, если поток не в unwind, и удерживается, если сам
дроп паникует. Значение, чьи поля дважды паникуют в drop glue, abort'ит (ADR-0127).
`UiRealm::drop` не возобновляет панику, пока поля (post-frame lane, `AsyncDriver`, writers)
держат пользовательские значения: они выносятся в `ManuallyDrop`/`Option` и уничтожаются явно
(обход #1165).

**Откат повтора кадра (R10, D-L6).** После `FrameDropped` серия `k` (`u32`, `saturating_add`)
назначает повтор на `T · 2^e`, где `e = min(k − 1, 6)` (`k ≥ 1`), с потолком 1 с. Степень
ограничена до сдвига: переполнения `u32` нет, умножение `Duration` — `checked_mul` с
насыщением к 1 с. Срок — запись в очереди дедлайнов presentation, которую уже читает
`UiRealm::next_wake`, поэтому отдельной проводки в wake hook и `frame_is_dirty` нет. Pump,
разбуженный в срок, делает попытку. Откат обходит только дискретное действие пользователя
(нажатие клавиши или кнопки указателя): одна немедленная попытка на окно за интервал. Запись
сигнала, тикер, движение указателя и записи самого paint только подтверждают срок. В
установившемся режиме — около 1 попытки в секунду.

**Пределы.** `BUG:` в `window_proc` abort'ит. Пользовательский `panic::set_hook`, который входит
в realm, работает до любой границы FLUI и не покрыт. iOS не возвращается из
`UIApplicationMain`: терминальный payload только логируется. Preedit извлечённого realm при
завершении сеанса теряется.

## Ошибки и отказы

| Требование | Сбой | Механизм | Код |
|---|---|---|---|
| R6 | Запись не успевает к дедлайну | `flush_within` → `DeadlineExceeded`; файл цел (persistence R21) | 0 |
| R7, R12 | Паника в `dispose`/`Drop` при закрытии последнего окна | Слот → quit → сброс реестра → сервисы → пулы → `resume_unwind` | 101 |
| R13 | Паника `Drop` захвата при закрытии последнего окна | Первая главная, хвост удержан | 101 |
| R14 | `Drop` внутри идущего unwind | Режим сохранения, `forget` | без abort |
| — | Паника в доставке закрытия последнего realm | Терминальна | 101 |
| — | Паника при закрытии не последнего realm | `CallbackPanic` + удержание (D4) | процесс жив |
| R9 | Паника handler'а ввода, post-frame, lifecycle, команды | Per-task catch, `CallbackPanic` | процесс жив |
| R8, R23 | Паника `build`; паника вето | `ErrorView`; `KeepOpen` | процесс жив |
| R10 | Паника layout/paint | `SegmentPanic` на сбой, откат через `next_wake` | процесс жив |
| R11 | Не-`BUG:` паника в `window_proc` | Слот Win32 → quit → teardown | 101 |
| R16, R19 | Позднее завершение; устаревший handle | Токен сброшен; `Closed`/`OwnerGone` | — |
| R20 | Device lost на закрывающем кадре | Backoff привязан к realm | 0 |
| R22 | Завершение сеанса, в т. ч. при извлечённом realm | S1–S6, реестр | ОС |
| — | Logoff отменён | Отзывается только `SessionEnd`, крестик пользователя ждёт | процесс жив |
| R24 | wasm закрывает вкладку | Строка README | — |

## Тестовая стратегия

«Ребёнок» означает дочерний процесс по образцу `opaque_frame_child`: родитель проверяет успех
ребёнка, маркеры порядка и отсутствие abort. На headless старый код выпускает панику без
teardown, поэтому код 101 проходит в обе стороны; различает маркер сброса до возобновления.
Нативный 101 доказывает `windows-notes`. **Fix** падает на старом коде; **характеризация**
на headless проходит и на старом коде.

| Треб. | Тест (уровень; таблица) | Вид | Без изменения |
|---|---|---|---|
| R1 | `notes_close_releases_every_owner_after_navigation` (headless; `notes_flow`), `last_window_close_releases_the_realm_through_the_runner` (headless-runner; новая `runner_teardown_matrix`) | Характеризация | Сторож жив |
| R2 | `closing_with_a_pending_load_retires_the_future_once` (headless-runner) | Характеризация | `Drop` ≠ 1 |
| R3 | `dropping_an_owner_with_an_observer_detaches_it_once` (headless; `tree_observer_inspector`) | Fix | 0 вызовов |
| R4 | `last_window_close_returns_ok_on_both_routes` (headless-runner); live-smoke Notes; windows | Fix (live-smoke) | — |
| R5 | `win32_close_drains_platform_map_entry` (`#[ignore]`, `tests/contract.rs`) | Характеризация | `Weak` жив |
| R6 | `a_write_past_the_deadline_leaves_a_whole_file_and_exits_ok` (headless-runner, двойник хранилища с барьером) | Fix | Реестра нет |
| R7 | `teardown_panic_flushes_the_registry_before_resuming` (ребёнок) | Fix | Нет маркера сброса |
| R8 | `a_panicking_note_build_shows_error_view_and_other_screens_work` (headless) | Характеризация | — |
| R9 | `a_panicking_tap_handler_reports_once_and_the_next_tap_works`, `a_panicking_post_frame_callback_reports_once_and_the_next_frame_builds` (ребёнок; `runner_teardown_matrix`); windows `notes_faults save-panic` | Fix | Паника выходит из dispatch |
| R10 | `a_repeating_paint_panic_backs_off_but_keeps_retrying` (headless; `headless_frame_driver_matrix`: без ввода 12 с, за последние 10 с от 8 до 12 отчётов); `a_single_paint_panic_redraws_without_input`; `a_pointer_move_does_not_bypass_the_backoff`; `a_failure_streak_past_thirty_three_keeps_the_one_second_cap` (k до 40); `the_runner_sleeps_until_the_failure_retry_deadline` (headless-runner: платформа спит до дедлайна `next_wake`) | Fix | ~60/с; 0 после затишья; переполнение сдвига |
| R11 | `user_panic_exit_code_matrix` (ребёнок, строка на путь); `a_non_bug_panic_at_the_window_proc_resumes_as_101` (Win32-unit, `#[ignore]`) | Fix | abort |
| R12 | `dispose_panic_on_last_window_close_resumes_after_flush` (ребёнок); windows `notes_faults dispose-panic` → 101 | Fix | `0xC0000409` |
| R13 | `capture_drop_panic_keeps_the_first_failure_and_still_flushes` (ребёнок) | Fix | Сброс пропущен |
| R14 | `user_drop_panic_inside_an_unwind_is_retained` (ребёнок; `realm_and_presentation_isolation_matrix`) | Характеризация | abort |
| R15–R20 | `close_requested_inside_a_frame_runs_once_on_the_next_turn`, `a_late_completion_after_close_requests_no_frame`, R17-строки роутера `flui-widgets`, `a_second_close_request_tears_down_once`, `stale_handles_refuse_after_realm_drop`, `device_loss_on_the_closing_frame_still_exits_ok` | Характеризация | — |
| R21 | `closing_during_composition_commits_before_detached` (headless-runner); windows | Fix | Detached без preedit |
| R22 | `session_end_flushes_the_registry_within_the_framework_budget` (доля фреймворка ≤ бюджет); `session_end_with_a_checked_out_realm_writes_registry_bytes_and_calls_no_user_code` (`simulate_session_end` из handler'а; счётчик вызовов наблюдателей = 0, байты на диске); windows: сообщения сеанса окну Notes, перезапуск | Fix | Нет реестра; пользовательский код при извлечённом realm |
| — | `close_delivery_is_idempotent` (headless: вложенная доставка из наблюдателя Detached и повтор из `UiRealm::drop`, одна доставка) | Fix | Двойной Detached |
| — | `a_cancelled_logoff_withdraws_only_the_session_entry` (`close_request_matrix`: крестик `User` остаётся `pending` после `Cancelled`) | Fix | — |
| — | `the_query_cache_ends_with_its_burst` (`close_request_matrix`) | Fix | — |
| — | `releasing_the_last_hold_queues_one_close` (`close_request_matrix`) | Fix | — |
| R23 | `a_vetoed_close_disposes_nothing` (`close_request_matrix`) | Характеризация | — |
| R24 | Строка README | — | — |

R1 доказывается парой «Notes на headless» плюс «пробное дерево через раннер» (см. риск 3).

**Ответы на открытые вопросы требований.**

1. Механизм учёта — c2.
2. Нового бюджета в `measurements` нет: потолок 15 с выводится из констант, для сеанса 3 с.
3. Пример `notes_faults` (`save-panic`, `dispose-panic` — аргументом), seam в `tree.rs` по
   образцу `with_loader`; env в Notes отвергнута.
4. Коридор: от 8 до 12 попыток за 10 с после 2 с разгона. Нижняя граница доказывает
   пробуждение, верхняя — отсутствие горячего цикла.

## ADR

- **ADR (номер назначит оркестратор): «Owner-turn panic boundary and terminal exit».**
  Supersedes ADR-0035 §5 в части «first panic resumes after restoration» и утверждение
  `flui-runtime/ARCHITECTURE.md` «panic leaves the dispatch boundary». Решение:
  (1) per-task catch, `CallbackPanic`; (2) терминальны сбои последнего realm и выхода из цикла
  (D4), включая доставку закрытия; `resume_unwind` после сброса реестра, сервисов и пулов;
  (3) `window_proc`: не-`BUG:` — слот платформы и 101, `BUG:` — abort; (4) `Drop` не
  возобновляет панику при живых пользовательских полях; (5) откат повтора кадра через
  `next_wake`.
- **ADR (номер назначит оркестратор): «Close guard, close reasons and the flush registry».**
  `CloseReason`, `CloseGuard`/`PendingClose`/`CloseChanged` в `flui-view`; одна запись на
  причину; отпускание последнего hold — одна операция закрытия; `FlushRegistry` вне realm как
  единственный путь persisted-состояния на диск; общая идемпотентная доставка закрытия;
  пользовательский код только в realm в слоте. Заменяет соответствующие пункты persistence
  design (`close_now`/`abandon_close`/`requested`, запись из `AsyncDriver`).
- **ADR (номер назначит оркестратор): «Session end».** Фазы S1–S6, кэш одной пачки,
  `ShutdownBlockReason`, бюджет 3 с и порог 1 с для пользовательского кода, `SessionEnding` для
  извлечённого realm, `PostQuitMessage` всегда.
- **ADR (номер назначит оркестратор): «Owner drop ends observation».** Supersedes ADR-0040 в
  части «Teardown honesty». Закрывает #1126.
- **PANIC-POLICY:** раздел «Callback panics and exit codes».

## Работы

‖ — можно параллельно (общих файлов нет).

1. ‖ ADR и PANIC-POLICY.
2. ‖ `flui-view`: `CloseReason`, `CloseGuard` и др., защёлка в `LifecycleSource`,
   `FlushRegistry`, `LifecycleContext::{close_guard, flush_registry}`, `detached()` в
   `BuildOwner::drop`; R3, `close_delivery_is_idempotent`.
3. ‖ `flui-platform`: `SessionEnd`, `on_session_end`, Win32 сообщения сеанса с кэшем пачки и
   `ShutdownBlockReason*`, слот в `window_proc`, `simulate_session_end`, тест R5.
4. ‖ Фикстуры Notes: `DropLedger`, seam в `tree.rs`, `notes_faults`; R1, R8.
5. ‖ `flui-widgets`: строки R17.
6. `flui-runtime` (после 2): `CallbackPanic`, `report_contained_panic`, общая доставка закрытия,
   откат в очереди дедлайнов; R10.
7. `flui-app` (после 6): per-task catch, классификация, слот, catch вокруг `platform.run`,
   `drop(realms)`, `queued_turns`, `on_quit`; сброс реестра в teardown; `on_close` без
   `resume_unwind`; `runner_teardown_matrix`. Android — тот же резюме (только clippy).
8. `flui-app` вето (после 7): `CloseRequest::reason`, `PendingClose` из `KeepOpen`, `Program` в
   `request_quit`, строки `close_request_matrix`.
9. Обход #1165 (после 2 и 7): `resume_unwind` из `Drop`, поля `UiRealm`; R14; `ARCHITECTURE.md`.
10. IME-шаг доставки (после 6 и W5 design text-ime); R21.
11. Session end в `flui-app` (после 3, 8, 10): S1–S6; R22.
12. `tools/xtask` (после 4, 11): live-smoke на Notes; `windows-notes`: программный и двойной
    close, `notes_faults`, сообщения сеанса.
13. README-матрица (R24); датированный прогон Windows.

**Обязательства persistence.** `Persisted::set` кодирует на owner-потоке и публикует байты в
`FlushRegistry`. Это единственный путь на диск: цикла записи в `AsyncDriver` нет. Статус
документа читается из `FlushRegistry::{committed, outcome}`. Требование «дренировать inbox
сервиса» теперь означает «сбросить реестр»: опубликованные, но не записанные байты пишутся в
шаге 7 или S4. Удержание: `hold()` на `Pending`, `require_decision()` на
`Failed`/`Conflict`/`ReadOnly` с правками. `close_now` и `abandon_close` заменяются на
`PendingClose::{discard_and_close, stay_open}`, `requested` — на `changed`. Строка
`a_registry_entry_published_at_detached_is_written_at_teardown` идёт в таблицу persistence.

## Риски и открытые вопросы

1. **Drop glue `UiRealm` после удержанного сбоя.** До работы 9 паника в `dispose` вместе с
   паникующим `Drop` future даёт abort. Эксперимент: ребёнок с обоими сбоями.
2. **Сеанс на реальной ОС.** Доставляет ли Windows `WM_ENDSESSION` во вложенный модальный цикл,
   и укладывается ли S4 в долю бюджета на медленном диске и OneDrive. Нужен датированный прогон
   с модальным диалогом и перетаскиванием.
3. **Уровень R1.** Notes на headless плюс пробное дерево через раннер, а не Notes в раннере.
   Владелец принимает это или просит test-support seam в `flui-app`.
