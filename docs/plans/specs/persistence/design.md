# Persistence: сохранение и восстановление состояния Notes — design

- **Статус:** черновик, вторая редакция после adversarial-ревью
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md)
- **Связанные:** [teardown/design.md](../teardown/design.md) (владеет контрактом закрытия и
  завершения сеанса), [send-flip/requirements.md](../send-flip/requirements.md) (разделение
  `AsyncDriver` на `Send`-waker и owner-local очередь)

## Текущее состояние

- **Capability-шов.** Новая capability — метод `LifecycleContext` (ADR-0078 §1). Образец —
  буфер обмена: поле `RealmServices::clipboard` (`crates/flui-runtime/src/realm_services.rs:19`–`34`),
  установка в `BuildOwner` (`crates/flui-runtime/src/presentation.rs:89`–`91`, `:586`), выдача через
  `clipboard_handle` (`crates/flui-view/src/context/build_context.rs:474`); `lifecycle_handle` имеет
  реализацию по умолчанию `None` (`build_context.rs:540`).
- **Async.** `AsyncDriver` опрашивается **внутри кадра**, в слоте `MidFrameMicrotasks`
  (`crates/flui-scheduler/src/scheduler.rs:1371`–`1380`; `crates/flui-runtime/src/execution.rs:8`–`12`).
  Сейчас `BoxedTask` и future `FutureBuilder` обязаны быть `Send`
  (`crates/flui-scheduler/src/async_driver.rs:106`, `crates/flui-view/src/element/future_builder.rs:62`);
  send-flip классифицирует их как «разделить: `Send` waker + owner-local очередь `!Send`», через
  границу IO ходят только `Send`-байты, как `IoFuture`.
- **IO-пул.** `ExecutionServices::spawn_io` (`execution.rs:516`) доступен только `flui-app`;
  ADR-0047 обещает realm'ам capability поверх него, не пулы.
- **Закрытие.** Сейчас вето — `AppConfig::with_close_request_handler` и `CloseRequestRouter::consult`
  (`crates/flui-app/src/app/close_request.rs:432`–`471`). Teardown design вводит `CloseReason`,
  отзыв запроса и двухфазное завершение сеанса с бюджетом 3 с, где persistence пишет первой
  (`teardown/design.md`, раздел (d)); общий контракт close guard задан оркестратором и
  потребляется здесь как есть (раздел «Из teardown» ниже).
- **Видимость Win32.** Сворачивание идёт в `dispatch_visibility_status_change(false)`
  (`crates/flui-platform/src/platforms/windows/platform.rs:1295`–`1318`), а невидимое окно даёт
  `AppLifecycleState::Hidden` (`crates/flui-runtime/src/ui_realm/presentation_lifecycle.rs:220`–`223`).
  Проверено чтением; исполняется только нативно.
- **Router** хранит начальный стек как `Vec<R>` (`crates/flui-widgets/src/router/router.rs:119`–`149`),
  наружу отдаёт только верх (`crates/flui-widgets/src/router/handle.rs:211`–`228`); `Routable`
  имеет `to_path`/`from_path` (`crates/flui-widgets/src/router/routable.rs:50`, `:59`);
  `RouterError` — `#[non_exhaustive]` (`handle.rs:239`).
- **Notes.** Состояние в `Shared` (`examples/two_screens/tree.rs:35`–`44`), заметки адресуются
  индексом `usize` (`Route::Note { id: usize }`), загрузка — внутри Home (`tree.rs:260`), Router —
  над ней (`tree.rs:137`), Retry/Reload — `saturating_add` (`tree.rs:245`, `:272`).
- **Граф facade.** `default = []` (`Cargo.toml:753`); `serde_json` нет в обычном графе `flui` ни
  по умолчанию, ни с `material` (`cargo tree … -i serde_json`: «nothing to print»).
- **Атомарная замена.** `std::fs::rename` на Windows — `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`,
  при `ERROR_ACCESS_DENIED` — `FileRenameInfoEx` с `REPLACE_IF_EXISTS | POSIX_SEMANTICS`, без
  `IGNORE_READONLY_ATTRIBUTE`, и путь идёт через `maybe_verbatim` (rust-lang/rust,
  `library/std/src/sys/fs/windows.rs`, `library/std/src/sys/path/windows.rs`).
  `tempfile::NamedTempFile::persist` на Windows зовёт `MoveFileExW` с путём без `\\?\`-префикса
  (Stebalien/tempfile, `src/file/imp/windows.rs`, `fn persist`, `fn to_utf16`).
  `ERROR_SHARING_VIOLATION` в `ErrorKind` не отображается — только по коду ОС.

## Варианты

**A. Только capabilities; хранилище — код Notes.** Framework даёт байтовое хранилище с атомарной
записью и CAS, восстановление стека Router; Notes сам пишет конверт с версией, порядок записей,
повтор, удержание закрытия. R12, R14, R15, R20, R28, R29 каждое приложение повторяет само, а
tutorial объясняет ~400 строк конкурентного кода. Риск — качество showcase.

**B. Capabilities плюс owner-local документ `Persisted<D>` (выбран).** Документ — версионированный
байтовый конверт: приложение само кодирует тело (Notes — через `serde_json`), framework пишет
заголовок с именем, версией и ревизией, держит одну линию записи, повтор, удержание закрытия и
изоляцию паник codec. Ни `serde`, ни `Send` в новых сигнатурах; через границу IO идут только байты.

```rust
struct NotesData { notes: Vec<Note>, next_id: u64, compact: bool }  // Note { id: NoteId, title }
impl Document for NotesData {
    const NAME: StorageName = StorageName::from_static("notes");
    const VERSION: u32 = 1;
    fn initial() -> Self { /* 10 000 заметок, id 0..10 000 */ }
    fn encode(&self) -> Vec<u8> { serde_json::to_vec(self).expect("BUG: plain data encodes") }
    fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError> { /* serde_json */ }
}
// init_state: self.data = Some(Persisted::<NotesData>::open(cx));
// Save: let rev = data.set(snapshot)?; «Saved note N», когда data.committed() >= Some(rev)
```

**C. Реестр восстановления по идентичности виджетов** (Flutter `RestorationManager`,
`packages/flutter/lib/src/services/restoration.dart`; Compose `rememberSaveable`). Восстанавливает
при mount, а R5/R6 требуют сеанс только после загрузки данных; ключи — строковые метки (AGENTS
«Identity is not a label»); данные (R1, R2) не покрыты. Отклонён.

**Выбор: B.** Трудная часть пишется и тестируется один раз; кодирование остаётся за приложением,
поэтому ADR-0089 §2 не расширяется, а R35 выполняется без условий. Следует ADR-0078 §1, ADR-0082
(trait в `flui-platform-api`, бэкенд в `flui-platform`), ADR-0047 (capability поверх IO-пула),
ADR-0083 (у `flui-runtime` нет платформенного ребра), ADR-0097 (новых `static` нет; реестр
сброса — значение хоста), ADR-0123/ADR-0127 (паники и закрытие — существующими путями).
Дополняет шаг 8 ADR-0093 (Proposed): стек восстанавливается, web history остаётся. Ни один
принятый ADR не заменяется.

**Ответы на открытые вопросы.**
1. *Граница.* В `flui`: хранилище (каталоги, атомарная запись, CAS, реестр сброса),
   `LifecycleContext::storage`, `Persisted<D>`, `Router::from_stack`/`RouterHandle::stack`. Вето
   и завершение сеанса — контракт teardown (close guard). В Notes: схемы, кодирование, fallback R7,
   тексты ошибок. `TaskSpawner` widget-коду не выдаётся.
2. *Два файла, без debounce.* `notes.json` (данные, Roaming) пишется на Save и переключение
   compact, с CAS. `session.json` (черновик, стек, смещение, ревизия данных; Local) пишется на
   смену маршрута, правку черновика, `Hidden`/`Paused`, закрытие и завершение сеанса, без CAS.
   Окно потерь при аварии: данные — ноль подтверждённых; сеанс — смещение списка с последней
   смены маршрута или `Hidden` и запись в полёте.
3. *Формат.* Заголовок framework `flui-document <name> <version> <revision>\n`, затем тело
   приложения. Номер версии и миграции — у приложения (`Document::VERSION`, `decode(version, …)`);
   Notes на версии 1, файл старее читается или даёт ошибку с Retry (R31).
4. *Второй экземпляр.* Данные — CAS под короткой межпроцессной блокировкой, конфликт — явная
   ошибка с выбором. Сеанс принадлежит экземпляру и одноразов: последний писатель побеждает.
5. *Повреждённый файл.* Retry и «Начать заново»: байты сохраняются под именем
   `notes-corrupt-N` обычной записью, затем пишется начальное состояние; сообщение называет копию.

## Публичный контракт

Всё достижимое из facade несёт Stable-обещание (ADR-0089 §1). Ошибки — `thiserror`, каждый
вариант с `#[error]`; перечисления `#[non_exhaustive]`.

**`flui-platform-api`** (C, stable) — `flui::platform::…`, без фичи:
```rust
pub trait Storage: Send + Sync + 'static {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored>;
    /// Принять `bytes` как последнее значение `name` синхронно, до возврата; ещё не начатое
    /// предыдущее значение того же имени заменяется (его future — `Superseded`).
    fn publish(&self, name: &StorageName, bytes: Vec<u8>, mode: WriteMode) -> StorageFuture<StoredVersion>;
}
pub type StorageFuture<T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'static>>;
pub struct StorageName(/* &'static str + область */);  // [a-z0-9][a-z0-9_-]{0,63}, не CON/NUL/COM1…
impl StorageName {
    pub const fn from_static(name: &'static str) -> Self;   // роуминговые данные; неверное имя — ошибка const
    pub const fn machine_local(name: &'static str) -> Self; // данные машины и экземпляра (сеанс)
    pub fn as_str(&self) -> &str;
}
pub enum WriteMode { Replace, IfUnchanged(StoredVersion) }
pub struct StoredVersion(/* длина + SipHash; ABSENT */);   // сравнивается только внутри процесса
pub struct Stored { pub bytes: Option<Vec<u8>>, pub version: StoredVersion }
pub enum StorageError {
    Unavailable, Inaccessible { kind: std::io::ErrorKind }, Busy, Full,
    TooLarge { len: u64, limit: u64 }, Conflict, LockUnsupported, Superseded, Cancelled,
}
```
`machine_local` — единственная добавка сверх минимума ревью: без неё сеанс попадает в Roaming
и синхронизируется между машинами, хотя принадлежит экземпляру (R32).

**`flui-view`** (K, internal) — `flui::view::…`:
```rust
pub trait LifecycleContext: BuildContext {
    fn storage(&self) -> Option<Arc<dyn flui_platform_api::Storage>> { None }
    // close_guard() — из teardown, см. ниже
}
pub mod persist {
    pub trait Document: 'static {
        const NAME: StorageName;
        const VERSION: u32;                           // от 1
        const MAX_BYTES: u64 = 16 * 1024 * 1024;      // 10 000 заметок ≈ 0,4 МиБ
        fn initial() -> Self;
        fn encode(&self) -> Vec<u8>;
        fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError> where Self: Sized;
    }
    pub struct DecodeError { /* сообщение */ }        // DecodeError::new(impl Into<String>)
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Revision(u64);                         // постоянная, из заголовка файла; get() -> u64
    pub enum PersistError {
        Storage(StorageError), Corrupt { detail: String }, Decode(DecodeError),
        NewerVersion { found: u32, supported: u32 },
        Panicked { during: &'static str, message: String }, UnsavedChanges, Exhausted,
    }
    pub enum SaveStatus {
        NotLoaded(Option<PersistError>),  // запись запрещена
        Clean, Saving,                    // Saving: опубликовано, ответа хранилища ещё нет
        Failed(PersistError),             // правки в памяти; retry() публикует актуальное
        ReadOnly(PersistError),           // NewerVersion, LockUnsupported: файл не пишется
        Unavailable,                      // хранилища нет (wasm32, не задан каталог)
    }
    pub struct Persisted<D: Document> { /* Rc<RefCell<…>> */ }   // !Send, Clone
    impl<D: Document> Persisted<D> {
        pub fn open(cx: &dyn LifecycleContext) -> Self;
        pub fn load(&self) -> impl Future<Output = Result<Rc<D>, PersistError>> + 'static;
        pub fn set(&self, value: D) -> Result<Revision, PersistError>;
        pub fn committed(&self) -> Option<Revision>;
        pub fn status(&self) -> SaveStatus;
        pub fn retry(&self);
        pub fn start_over(&self);   // Corrupt/Decode: копия notes-corrupt-N, затем initial()
    }
}
```
`load()` возвращает owner-local future; `FutureBuilder` принимает его после разделения
`AsyncDriver` в send-flip (риск 2). `snapshot_bytes()` не нужен: `set` уже кодирует и публикует.

**`flui-widgets`** — `flui::widgets::…`, без фичи:
`Router::from_stack(stack: Vec<R>, page) -> Result<Self, RouterError>` (пустой стек —
новый вариант `RouterError::EmptyStack`) и `RouterHandle::stack(&self) -> Vec<R>`.

**`flui-app`** (H) — `flui::AppConfig`, фича `persist`: `with_storage_dir(self, name: StorageName)
-> Self` (данные — `<dirs::data_dir>/<name>/`, сеанс — `<dirs::data_local_dir>/<name>/`).

**`flui-testing`** — `flui::testing::…`: `storage::MemoryStorage` (`put`, `contents`,
`hold_writes`, `hold_commits`, `fail_next`, `fail_reads`, `live_requests`),
`widgets::lay_out_with_storage`, `LaidOut::request_close(CloseReason)`, `LaidOut::end_session()`,
`LaidOut::set_lifecycle_state`. Двойник завершает запросы на owner-потоке при `pump`.

**Из teardown** (не определяется здесь): `LifecycleContext::close_guard() -> Option<CloseGuard>`;
`CloseGuard::hold() -> CloseHold` (работа, которая завершится: вето `User`, `SessionEnd`
пропускает), `require_decision() -> CloseHold` (вето любой ветируемой причины),
`pending() -> Option<PendingClose>` с `reason`, `discard_and_close()`, `stay_open()`;
`changed() -> CloseChanged`; `can_veto(CloseReason)`; `CloseReason { User, Program, SessionEnd }`.

**Вне facade.** `flui-platform`, фича `storage`, нет на wasm32: `storage::FileStore`
(синхронный протокол ниже), `storage::data_dirs()` через `dirs` 6, временные файлы через
`tempfile`. `flui-app`: `FlushRegistry` — реализация `Storage` для хоста.

**Фичи.** facade `persist = ["flui-app/persist"]` → `flui-platform/storage`. Без неё нет `dirs`
и `tempfile`, `storage()` даёт `None`, документ — `Unavailable`. `serde_json` во framework не
входит ни под какой фичей (R35). `two_screens` — `required-features = ["material", "persist"]`,
Notes зависит от `serde` и `serde_json` сам.

Итого: 13 новых типов (`Storage`, `StorageFuture`, `StorageName`, `WriteMode`, `StoredVersion`,
`Stored`, `StorageError`, `Document`, `DecodeError`, `Revision`, `PersistError`, `SaveStatus`,
`Persisted`) плюс тестовый `MemoryStorage`; 11 методов (`storage`, семь у `Persisted`,
`from_stack`, `stack`, `with_storage_dir`) и три const-конструктора/аксессора у `StorageName`.

## Инварианты, владение, lifecycle

- **Владение и потоки.** `Persisted<D>` — `Rc<RefCell<_>>` в `ViewState` открывшего, `D: 'static`.
  `encode` выполняется синхронно в `set`, на owner-потоке, в обработчике события (~1 мс на
  10 000 заметок); `decode` — в owner-local задаче загрузки, которую `AsyncDriver` опрашивает
  **внутри кадра** (несколько мс один раз). Замеры — спека `measurements`. Через границу IO идут
  только `Vec<u8>`, `StoredVersion` и `StorageError`. `RefCell` не заимствуется во время codec,
  публикации и пробуждения.
- **Реестр сброса** (`flui-app`, одна на хост, вне realm). Карта имя → слот
  `{ последние байты, режим, состояние: idle | queued | writing }` под `parking_lot::Mutex` —
  замок общей инфраструктуры, не состояние кадра; держится только на вставку и выемку.
  `publish` синхронно кладёт байты в слот (непринятое прежнее значение получает `Superseded`) и
  будит воркер IO-пула; воркер берёт слот, пишет через `FileStore`, отвечает через oneshot.
  Если во время записи пришло новое значение того же имени с `IfUnchanged(base)` от того же
  издателя, а пишущаяся запись успешна, `base` новой заменяется на версию только что
  записанной: это одна линия правок, не конфликт. Неудача оставляет байты в слоте (долг
  доставки), пока их не заменит новое значение или не запишет сброс при выходе.
- **Сброс при выходе.** Обычный выход: после разбора realms хост дренирует реестр в окне
  сервисов teardown. Завершение сеанса (`Ending`): реестр пишется **синхронно, inline, на
  owner-потоке**, первым шагом бюджета 3 с, без `AsyncDriver` и без пользовательских
  callback'ов realm, который извлечён; его байты всё равно пишутся — то, что было
  опубликовано последним.
- **Удержание закрытия** (контракт teardown). Пока статус `Saving` — `hold()`: закрытие
  пользователем ждёт, завершение сеанса проходит (реестр допишет inline), сообщения «мешает
  завершению» нет. Пока `Failed`, `ReadOnly` или `NotLoaded` при несохранённых правках —
  `require_decision()`: вето любой ветируемой причины; Notes показывает причину, Retry
  (`retry`), «Закрыть без сохранения» (`pending().discard_and_close()`), «Остаться» (`stay_open`).
  `Unavailable` удержания не берёт: сохранить нечего никогда, баннер об этом уже показан. На
  web и Android `can_veto` ложен: теряется изменённое после последнего подтверждения, если
  процесс убит до `Hidden`.
- **Чтение.** Только по `load()`; `Resumed` диск не читает (R13). `load()` при `Saving`/`Failed` —
  `UnsavedChanges`. Применяется только последняя загрузка (поколение плюс отмена в `FutureBuilder`).
- **Запись.** `set` без базовой версии (успешный `load` или `start_over`) — `Err` и статус не
  меняется (R24). Иначе: `encode` под `catch_unwind`, ревизия +1 (`u64::MAX` — `Exhausted`
  навсегда), заголовок, `publish`. `committed()` растёт после ответа хранилища, то есть после
  `rename`. Данные — `IfUnchanged(base)`, сеанс — `Replace`.
- **Повтор `Busy`** — внутри `Persisted`, по часам realm: 50, 100, 200, 400, 800 мс, затем
  `Failed(Storage(Busy))`. Таймер внутренний (дедлайны в `RealmServices`, их учитывает
  `UiRealm::next_wake`), не в facade. `Hidden`/`Paused` и запрос закрытия повторяют сразу.
- **`FileStore::write`.** (1) `create_dir_all` (R19); (2) только для `IfUnchanged`: `File::try_lock`
  на `root/.flui-storage.lock` — занято → `Busy`; `ErrorKind::Unsupported` (SMB/NFS) →
  `LockUnsupported`, документ становится `ReadOnly`: потерянное обновление второго экземпляра
  хуже явного отказа; (3) для `IfUnchanged`: версия на диске ≠ `base` → `Conflict`;
  (4) `tempfile::Builder::new().tempfile_in(root)` — уникальное имя, `write_all`, `sync_all`;
  (5) `std::fs::rename(temp, target)`, затем `TempPath::keep`; на unix — `fsync` каталога;
  (6) снять блокировку. `NamedTempFile::persist` не используется: его `MoveFileExW` без
  `\\?\` ломает R23 на путях длиннее 260 символов без `longPathAware`; `std::fs::rename` путь
  нормализует и имеет POSIX-fallback.
- **Долговечность.** Без `MOVEFILE_WRITE_THROUGH` потеря питания после «Saved» может вернуть
  прежнее целое состояние; смеси не будет. «Saved» означает «переживёт падение процесса», не
  «переживёт потерю питания» (вне scope требований).
- **Код 5 и sharing violation (R22).** Коды 32, 33 → `Busy`. Код 5 → проба цели: атрибут
  read-only (`metadata().permissions().readonly()`) или отказ в открытии на запись с кодом 5 →
  `Inaccessible { PermissionDenied }` сразу, без повторов (fallback std без
  `IGNORE_READONLY_ATTRIBUTE` read-only цель не заменит); иначе → `Busy`. Временный файл
  удаляется в любом исходе.
- **Пути (R23).** Корни — `dirs::data_dir()`/`data_local_dir()` (учитывают перенаправление и
  OneDrive), имя — `StorageName`; не-ASCII идёт как UTF-16.
- **Версии и повреждения.** Нет заголовка, чужое имя → `Corrupt`; `version > VERSION` →
  `ReadOnly(NewerVersion)`, файл не пишется никогда; иначе `decode(version, body)`, ошибка →
  `Decode`, файл не трогается, Retry перечитывает. `start_over`: байты из последней загрузки
  пишутся в первое свободное `notes-corrupt-1…99` (`IfUnchanged(ABSENT)`, `Conflict` → следующее
  N), затем `initial()` — `IfUnchanged(версия повреждённого)`. Повреждённый сеанс (R8) Notes
  отбрасывает сам; `notes.json` — только по кнопке.
- **Notes: идентичность.** `NoteId(u64)` из `next_id`, никогда не переиспользуется; исчерпание
  `next_id` — отказ создать заметку, не перенос. Маршрут — `Note { id: NoteId }`. Сеанс хранит
  `data_revision` — ревизию данных, против которой снят. При загрузке: ревизия совпала — черновик
  применяется, если его заметка есть; не совпала — черновик отбрасывается. Стек проверяется
  всегда: маршрут с несуществующим id обрезает стек до Home. После Save Notes публикует сеанс с
  новой ревизией тем же обработчиком.
- **Notes: восстановление.** Корневой `FutureBuilder` грузит `data`, затем `session`: до
  готовности «Loading notes», ошибка данных — экран Retry, сеанс не читается и не пишется (R6).
  Затем `Router::from_stack(стек)`; пути хранятся как `to_path().to_string()` и разбираются
  `from_path`, ошибка разбора — Home. `scroll.set_pixels(p)` и черновик задаются до первой
  раскладки (R5). Ссылка запуска, если приложение её получило, побеждает сеанс: стек берётся из
  неё, черновик остаётся, только если его заметка в этом стеке. Retry/Reload — `wrapping_add`
  (R26). Сеанс снимается из `RouterHandle::stack()`, `scroll.pixels()` и текста черновика в
  момент записи.

## Ошибки и отказы

| Требование | Механизм |
|---|---|
| R6, R24 | `NotLoaded` — `set` возвращает `Err`; сеанс — только после загрузки данных |
| R7, R8 | `NoteId` и `data_revision`; обрезка стека; повреждённый сеанс отбрасывается |
| R9, R17 | нет файла → `initial()`; `Corrupt` → Retry / `start_over` с копией |
| R10, R11 | IO на пуле; `set` не ждёт записи; codec — миллисекунды на owner-потоке |
| R12, R14 | `hold()` пока `Saving`; `require_decision()` пока `Failed`/`ReadOnly`/`NotLoaded` с правками |
| R13 | запись на `Hidden`/`Paused`; `Resumed` диск не читает |
| R15 | `committed()` и «Saved» — после `rename` |
| R16, R22 | повтор `Busy` по часам realm; коды 32/33 → `Busy`, 5 → проба |
| R18, R19 | `Inaccessible { kind }` отдельно от `Corrupt`; `create_dir_all` при записи |
| R20, R30 | `Failed`, `Full`, `TooLarge` без чтения (`metadata().len()`, затем `take(limit + 1)`) |
| R21, R27 | уникальный tmp + `sync_all` + `rename`; unmount не отменяет принятые реестром байты |
| R25, R28 | поколение загрузки; реестр: последнее значение побеждает, линия правок перебазируется |
| R29 | `catch_unwind` вокруг `encode`/`decode` → `Panicked`; `payload_text`, `retain_opaque_payload` |
| R31 | `ReadOnly(NewerVersion)`; `decode(version, …)` или `Decode` без записи |
| R32 | данные — блокировка + CAS → `Conflict`; без блокировок — `ReadOnly`; сеанс — `Replace` |
| R33 | `storage()` → `None` → `Unavailable`, работа в памяти с баннером |
| R36 | хранилище — поле realm, документ — поле `ViewState`; реестр различает имена и каталоги |

**Реентерабельность.** `set` фиксирует ревизию и статус, отпускает `RefCell`, затем кодирует и
публикует; `set` из `encode` или из слушателя не вкладывается, а ставит следующую публикацию.
Пробуждения и ответы реестра идут после снятия замка. Действия guard из обработчиков — по
контракту teardown.

**Паника.** `catch_unwind(AssertUnwindSafe(…))` вокруг `encode` и `decode`; при панике в `encode`
ничего не публикуется, память и файл не тронуты, статус `Failed(Panicked)`; следующий `set`
работает. Первый сбой остаётся в статусе до следующего успеха. Payload — `retain_opaque_payload`
(ADR-0127). Предел: значение `D`, чей `Drop` паникует дважды до границы, — за ADR-0127.

## Тестовая стратегия

Двойник `MemoryStorage` повторяет только семантику `publish` (замена непринятого, ответы), не
CAS-алгоритм файла: конфликт проверяется на настоящем `FileStore`. Строки проверяют содержимое
хранилища, не только статус.

**Notes через facade** (C; `tests/fixtures/notes_flow.rs`, таблицы рядом с
`notes_public_input_flow_matrix`; `external_notes_showcase_runs_through_the_facade` требует их
`... ok`). `notes_restart_matrix`: `saved_title_survives_restart_and_others_stay` (R1, R34),
`compact_rows_survive_restart` (R2), `unsaved_draft_reopens_its_note_without_changing_the_list`
(R3), `settings_over_editor_restores_and_backs_out_twice` (R4),
`list_offset_and_route_are_restored_on_the_first_loaded_frame` (R5),
`first_launch_starts_from_initial_state_without_an_error` (R9). Без изменения Notes не читает
хранилище — каждая строка падает на старом значении.
`notes_storage_failure_matrix` (H через harness): `failed_data_load_leaves_session_unread_and_unwritten`
(R6), `deleted_note_then_crash_drops_the_draft_and_trims_the_stack` (R7: сеанс снят на ревизии
r с черновиком заметки 3; данные переписаны без заметки 3 ревизией r + 1; снятие дерева до
записи сеанса; перезапуск — Home, черновика нет, ошибки нет),
`unreadable_session_starts_home_at_zero` (R8), `reload_replaces_a_pending_load` (R10, со счётом
`live_requests`), `a_held_write_keeps_typing_and_scrolling_live` (R11),
`hidden_writes_the_session_and_resume_does_not_reread` (R13),
`close_with_a_failed_write_requires_a_decision` (R14), `retry_counter_overflow_still_starts_a_load` (R26).

**`Persisted<D>`** (H; модуль `persist` в `crates/flui-view/tests/main.rs`, через его `run_table`).
`persisted_document_contract`: `user_close_waits_for_an_in_flight_save` (R12),
`saved_status_appears_only_after_commit`, `drop_before_commit_loses_only_that_edit` (R15),
`busy_retry_follows_the_realm_clock` (R16, R22: на 49 мс повтора нет, на 50 мс есть; пять `Busy`
→ `Failed`), `retry_after_failure_publishes_the_latest_value` (R20),
`repeated_retries_apply_exactly_one_result` (R25), `a_later_edit_supersedes_an_unstarted_one` (R28),
`unmount_during_load_releases_requests` (R27), `no_write_without_a_loaded_base` (R24).
`persisted_document_failure_matrix`: `corrupt_bytes_are_kept_until_start_over` (R17),
`inaccessible_is_reported_apart_from_corrupt` (R18), `full_disk_keeps_memory_and_the_old_file`,
`oversized_file_is_refused_unread` (R30), `older_version_decodes_or_reports_without_writing`,
`newer_version_is_never_written` (R31), `unavailable_storage_runs_in_memory_with_status` (R33),
`codec_panic_is_contained_first_failure_kept` (R29: паника в `encode`, затем в `decode`, payload с
паникующим `Drop`; следующий `set` пишет), `two_realms_keep_independent_documents` (R36).

**Реестр и закрытие** (`flui-app`, in-src: реестр — приватный шов с блокирующим поддельным
`FileStore`). `flush_registry_matrix`: `a_blocked_store_keeps_frames_and_input_live` (R11 через
настоящий реестр), `session_end_during_a_write_writes_the_latest_registry_bytes`,
`session_end_never_blocks_for_work_in_progress` (только `require_decision` даёт блокировку),
`a_failed_write_keeps_its_bytes_until_replaced_or_flushed`, `same_writer_edits_rebase_instead_of_conflicting`.
Строка в `close_request_matrix` teardown: `a_saving_document_holds_user_close_only`.

**`FileStore`** (`flui-platform`, `tempfile::tempdir()`; семейство `file_store_contract` в
существующем `tests/contract.rs`): `first_write_creates_the_directory` (R19),
`directory_in_place_of_the_file_is_inaccessible` (R18), `long_non_ascii_path_round_trips` (R23,
> 260 символов, кириллица и CJK), `two_threads_interleave_and_exactly_one_conflicts` (R32: два
`FileStore` на одном каталоге в двух потоках, барьер между проверкой версии и `rename` через
приватный шов; ровно одна запись получает `Conflict`, файл — целое состояние победителя),
`read_only_target_is_inaccessible_not_busy` (`cfg(windows)`, исполняется только локально).
In-src `storage::tests::file_store_interruption_matrix`: обрыв после tmp и до `rename` — следующее
`read` даёт прежнее целое состояние (R21). `os_error_classification_table` — чистая таблица
отображения кодов 32/33/5/`ENOSPC` в `StorageError`, не поведенческий тест `rename`.
`sync_all` автоматическими тестами не наблюдается: он важен только при потере питания; нативный
`taskkill /F` проверяет атомарность протокола, не `sync_all`.

**Router** — `from_stack_restores_back_order`, `stack_reads_every_committed_edit`,
`empty_stack_is_refused` в существующем семействе Router `crates/flui-widgets/tests/contracts.rs`.

**Facade.** `ordinary_facade_graph_excludes_test_support` требует ещё отсутствия `serde_json `,
`dirs ` и `tempfile ` без `persist` (R35); `cargo xtask wasm-check` с `flui/persist` (R33).

**Нативно** (датированный протокол R14 уровня 0, Windows): R1, R12, R14 (файл только для
чтения), R18, R21 (`taskkill /F` в цикле записей), R22 (держатель без `FILE_SHARE_DELETE`), R23
(не-ASCII профиль, перенаправленный AppData), R32 (два процесса), сворачивание → `Hidden` →
запись сеанса, завершение сеанса во время Save.

## ADR

Нужен: новые контракты `flui-platform-api`, `flui-view`, `flui-app`, `flui-widgets` и facade.
**ADR (номер назначит оркестратор) «Persistence: byte storage capability, flush registry and
versioned documents».** Решение:

1. `flui-platform-api::Storage` — асинхронное чтение и синхронно принимаемая публикация байтов;
   через границу IO идут только байты и версии. Данные пишутся с CAS под короткой
   блокировкой, сеанс — без CAS; без блокировок данные только для чтения.
2. Атомарность — уникальный tmp, `sync_all`, `std::fs::rename`; «Saved» переживает падение
   процесса, не потерю питания.
3. Реестр сброса принадлежит хосту, лежит вне realm, пишется IO-пулом, а при завершении сеанса —
   синхронно inline; `LifecycleContext::storage` выдаёт его realm'у (ADR-0078 §1, ADR-0047).
4. `Persisted<D>` — owner-local; кодирование у приложения (`Document::encode`/`decode`), заголовок
   с именем, версией и постоянной ревизией — у framework. `serde` в сигнатурах нет (ADR-0089 §2).
5. Удержание закрытия — через close guard из ADR teardown: `hold` на запись в полёте,
   `require_decision` на сбой с правками.
6. Стек Router — `from_stack` и `stack()`, пути — `to_path`/`from_path` (шаг 8 ADR-0093).

## Риски и открытые вопросы

1. **R34 (владелец).** Notes с persistence требует фичу `persist` и собственные зависимости
   `serde`/`serde_json`; R34 говорит «только `flui` с `material`». Предлагаю «`flui` с `material`
   и `persist`; кодирование — зависимость приложения». Альтернатива — построчный формат в Notes
   без serde: хуже как образец.
2. **Зависимость от send-flip.** `Persisted` и `load()` — `!Send` и требуют owner-local очереди
   `AsyncDriver`. Если разделение не смержено к 2026-11-24, persistence ждёт его или временно
   держит состояние в `Arc<Mutex<_>>` с `D: Send` и меняет bounds в 0.3. Решает владелец.
3. **Windows-эксперимент.** `LockFileEx` и `MoveFileExW` на OneDrive-каталоге с файлами по
   запросу; укладывается ли удержание файла антивирусом в бюджет повтора (~1,5 с) и inline-сброс
   реестра — в 3 с завершения сеанса. Если нет — бюджеты пересматриваются до RC.

## Изменения контракта после заморозки (2026-10-06, решение оркестратора)

Основание — отчёт P1 (`persistence/contract` @ `b6c419ca7`). Зависимые задачи — P2 (`FileStore`) и P6
(хостовое хранилище).

1. **`StorageName::is_machine_local(&self) -> bool`** входит в контракт: без него реализация `Storage`
   не может выбрать между roaming- и local-корнем. Добавляет P2.
2. **`StoredVersion::of_bytes(&[u8]) -> StoredVersion`** принят: версии нужны реализациям вне
   `flui-platform-api` (`MemoryStorage`, `FileStore`).
3. **Тест «каталог хранения доходит до `LifecycleContext`»** живёт внутри `flui-app`
   (`realm_dispatch/tests.rs`): публичного пути собрать realm runner'а без GPU нет. Тот же предел у
   P6 `a_runner_write_lands_under_the_configured_roots` — он тоже in-crate.
