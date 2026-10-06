# text-ime — design (уровень 1)

- **Статус:** черновик (ревью: approve with fixes — правки внесены)
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md) (R1–R24, F1–F8, D1–D5); уровень 0 —
  [../release/requirements.md](../release/requirements.md) R7, R14
- **Общие решения lifecycle (владелец — `teardown`):** пользовательский код выполняется только
  в realm, который стоит в своём слоте; у realm, извлечённого из слота в момент конца сессии,
  callback'и не выполняются и preedit отбрасывается (документированный предел); guard закрытия
  — в `flui-view` (`hold`/`require_decision`/`pending`, `CloseReason { User, Program,
  SessionEnd }`); состояние попадает на диск только через flush-реестр вне realm.

## Итог

| Вопрос | Решение |
|---|---|
| (a) доступ Win32 к store (D1) | Контракт `TextStoreHost` (не `Send`) в `flui-platform-api`; хост выдаёт только owner-доказательство: `OwnerPlatform::text_store_host(&Arc<dyn HostWindow>)`; runner кладёт его в `PresentationWindow`; `PlatformWindow` и `PlatformTextInput` не меняются. Узкий ADR |
| (b) COM-структура | Один `ITfThreadMgr::Activate` на окно; документ TSF на каждый фокус поля, пустой документ окна вне полей; счётчик глубины COM-входов — операции хоста во время входа ставятся в очередь; каждый вход — `catch_unwind` |
| (c) блокировки | `RequestLock` → `LockArbiter`; уведомление владельца (`on_changed`) — после снятия `Held`, до следующего гранта; слот сессии с флагом реентерабельности; долг якоря и отложенная паника — в realm |
| (d) `GetTextExt` | Чистая функция в `flui_platform::shared` (Linux CI): логический rect × `DevicePixelRatio` из `WindowContext.scale_factor` + начало клиентской области, наружное округление с допуском |
| Open Q1 | D2 — поправка к ADR-0090 (правило контракта store) |
| Open Q2 | Решено иначе, чем предлагали требования: программный unmount **подтверждает** видимый preedit. Новая формулировка R8 (правит оркестратор): «убрать композицию, **оставив её текст подтверждённым** в контроллере; `on_changed` после начала dispose не вызывается; отбрасывается только правка IME, не применённая к store до его отсоединения» |
| Спайк | до ~2026-10-16 (жёсткий срок 10-26); go/no-go 2026-11-20 |
| Оценка | ≈ 12 инженеро-недель; критический путь до go/no-go ≈ 4,9 нед, два исполнителя |

## Текущее состояние (чтением)

- Win32-окно: `WindowContext` (`crates/flui-platform/src/platforms/windows/platform.rs:308`)
  живёт за `GWLP_USERDATA`, доступен только на owner-потоке через
  `with_window_context_checked` (`platform.rs:560`), освобождается в `WM_DESTROY`
  (`platform.rs:1055`); `!Send + !Sync` (ADR-0082 §4 шаг 1, Win32). Owner-поток — STA
  (`platform.rs:770`). Масштаб окна — `WindowContext.scale_factor` (`platform.rs:334`), им же
  пересчитываются pointer-события (`platform.rs:1654`).
- `window_proc` ловит панику и вызывает `abort` (`platform.rs:949-975`). Мягкий барьер —
  `contain_owner_callback` (`crates/flui-platform/src/shared/panic_boundary.rs:48`).
- Клавиатура: `WM_KEYDOWN` (`platform.rs:1660-1722`), stray `WM_CHAR` (`platform.rs:1741-1764`);
  `vk_to_key(0xE5)` уходит в `NamedKey::Unidentified` (`shared/keys.rs:218`). `platform.rs` —
  2585 строк при лимите 3000: TSF-код — отдельный модуль.
- Фичи `windows` 0.62 без `Win32_UI_TextServices` (`crates/flui-platform/Cargo.toml:94-111`).
- Owner-доказательство уже есть: `OwnerPlatform` — `!Send`, выдаётся только backend'ом
  (`crates/flui-platform/src/traits/owner.rs:61-88`); runner читает мост доступности с
  host-окна один раз и кладёт рядом с окном в `PresentationWindow`
  (`crates/flui-runtime/src/presentation.rs:110-149`, `runner::presentation_window`).
- Владелец сессии: `TextInputOwner::new(Option<Arc<dyn PlatformTextInput>>)`
  (`crates/flui-interaction/src/text_input.rs:254`), attach без платформы → `Unsupported`
  (`:306`); pull-подключение ждёт ADR-0082 (`:39-42`); `active_store` — `#[doc(hidden)]`
  без production-вызова (`:549-557`). Presentation: `presentation.rs:575`, `:729`.
- Якорь: `drive_frame` (`crates/flui-runtime/src/ui_realm/frame.rs:343-352`) — при unwind якорь
  пропускается; у скрытого окна кадров нет, есть `pump_background` (`ui_realm/pump.rs:120`).
- Блокировка: `LockArbiter::run_one` ставит `Held` и вызывает `open(grant)`
  (`crates/flui-platform-api/src/text_store/lock.rs:339-343`); у поля `open` и **внутри него**
  `write_back` → `report_if_changed` → `on_changed` (`crates/flui-widgets/src/text/text_store.rs:324-362`),
  то есть пользовательский код сегодня работает под блокировкой. `report_if_changed` сравнивает
  весь текст вместе с preedit (`editable_text.rs:1183-1192`) — D2 нарушен.
- `RefMut` живёт через вызов во владельца: blur — `take()` в scrutinee `if let`
  (`editable_text.rs:1431-1438`), dispose — то же в let-chain (`editable_text.rs:1709-1717`).
  Dispose не решает судьбу композиции; paste отказывает во время композиции (`:1252`).
- Pointer: `handle_input_addressed` (`crates/flui-runtime/src/ui_realm/input.rs:268`) удерживает
  или диспатчит; хука «разрешить композицию» нет.
- Внешние реализации (проверки, не шаблоны): winit и Zed — IMM32. Chromium `TSFTextStore`
  (`ui/base/ime/win/tsf_text_store.cc`): очередь только внутри выданной блокировки,
  `TS_E_NOLAYOUT`, rect нулевой ширины при пустом диапазоне в композиции (Pinyin), защита от
  реентерабельного `OnLayoutChange`; `TSFBridge` (`tsf_bridge.cc`): `AssociateFocus` не держит
  ссылку на менеджер, `SetFocus` вызывается явно. Firefox — документ на каждый фокус.

## Варианты

### (a) Доступ backend Win32 к store на owner-потоке (D1)

| | A1. Owner-доказательство + `PresentationWindow` (выбран) | A2. Метод `PlatformWindow` | A3. Метод `PlatformTextInput` |
|---|---|---|---|
| Суть | `OwnerPlatform::text_store_host(&self, &Arc<dyn HostWindow>) -> Option<Rc<dyn TextStoreHost>>`; backend отвечает через запечатанный метод `HostWindow`, который требует токен, чеканимый только `OwnerPlatform`; runner читает хост в `runner::presentation_window` | default-метод на `Send + Sync`-трейте, `None` вне owner-потока | — |
| Плюсы | Поток доказан типом (`OwnerPlatform: !Send`), как ADR-0082 §4 шаг 2 (`OwnerPlatform::window(..) -> OwnerWindow`); путь как у моста доступности | Runtime подключает сам | Мало имён |
| Минусы | Временное место до `OwnerWindow`; при шаге 2 метод переезжает | `!Send` из `Send + Sync`-трейта — правило в документации, не в типе | Против D1 |

**Выбор A1.** `TextInputOwner::new(TextInputBackend)`, где `TextInputBackend { Push(Arc<dyn
PlatformTextInput>), Pull(Rc<dyn TextStoreHost>), None }`; Win32 — `Pull`, winit/macOS —
`Push`, тестовый realm — `Pull` с записывающим хостом. `PresentationWindow` несёт
`TextInputBackend`; если ему нужно остаться `Send`, backend идёт отдельным аргументом
конструктора presentation (проверяет W2). Требования: R16–R18, F1, F5, F7, D1. ≈1 нед. ADR:
узкий ADR; следует ADR-0082 §4 (шаг 1 хранение, шаг 2 форма доказательства), ADR-0097,
ADR-0081. Ломающее изменение `TextInputOwner::new` → фрагмент `changelog.d/` (`### Changed`).

### (b) Структура COM-объектов

| | B1. Документ на окно, store подменяется | B2. Документ на фокус поля (выбран) | B3. Документ на тип ввода (Chromium) |
|---|---|---|---|
| Идентичность (F1) | Генерация в каждом гранте | Структурная: грант захватывает свой `Document` | Как B1 |
| Цена | Дешевле; TIP видит тот же контекст с чужим текстом | `CreateDocumentMgr` + `CreateContext` + `Push` на фокус | Тип у Notes один |

**Выбор B2.** Владение: `WindowContext.text_services: Option<Rc<TextServices>>`;
`TextServices` — `ITfThreadMgr`, `TfClientId`, пустой `ITfDocumentMgr`,
`RefCell<Option<Document>>`, `entry_depth: Cell<u32>`, очередь операций хоста,
`poisonings: Cell<u8>`. `Document` — `ITfDocumentMgr`, `ITfContext`, cookie, COM-объект
`TsfStore` (`#[implement(ITextStoreACP, ITfContextOwnerCompositionSink)]`) с `Rc<DocumentState>`
(store `Rc<dyn TextStore>`, sink и маска, слот сессии, `closed`, `notifying`). Хост —
`Rc<Win32TextStoreHost>` со `Weak<TextServices>`.

**Не закрывать документ под собственным вызовом (B1 ревью).** Цепочка `RequestLock` →
`on_changed` → снятие фокуса → `detach` → `focus_store(None)` → `Pop` и освобождение store
`Rc` возможна внутри COM-входа. Поэтому: каждый вход `com_entry` увеличивает `entry_depth` и
кладёт на стек клоны `Rc<DocumentState>` и store `Rc`; `focus_store`/`complete_composition`,
пришедшие при `entry_depth > 0`, ставятся в очередь `TextServices` и выполняются, когда
внешний вход возвращается (явно после тела, не в `Drop`); при размотке очередь остаётся и
выполняется постом `WM_APP` в окно. Строка `on_changed_unfocus_inside_request_lock`.

**Паники.** `com_entry(state, || …) -> HRESULT`: `catch_unwind` на каждом методе.
Пользовательские паники до COM не доходят (см. (c), п. 5 ревью). Паника с `BUG:` внутри
адаптера: документ отравлен (все методы `E_UNEXPECTED`), переключение на пустой документ
поставлено в очередь операций (B1), новый документ — на следующем owner turn (пост в окно);
после 3 отравлений в окне — `text_services = None`, путь `WM_CHAR`, `warn!`. Предыдущий
менеджер, который возвращает `AssociateFocus` (`ppdimPrev`, AddRef'нутый), освобождается
сразу (в windows-rs — drop возвращённого `Option<ITfDocumentMgr>`). Требования: R16–R20, F1,
F3, F5, F7. ≈2,5 нед. Риск: `#[implement]` с `Rc`-полями (спайк).

### (c) Арбитраж блокировок: TSF `RequestLock` → `LockArbiter`/`CommitGate`

| | C1. `LockArbiter` + якорь (выбран) | C2. Синхронно всегда (Chromium) | C3. Свой арбитр в backend |
|---|---|---|---|
| Против | Внутри кадра TIP получает `TS_S_ASYNC` | Правка посреди кадра — против ADR-0027 §3 | Второй автомат, kit не проверяет |

**Выбор C1.**
1. **Перевод ACP** — чистый cfg-free модуль `flui_platform::shared::tsf_acp` (Linux CI): флаги
   `TS_LF_*` → `(LockKind, LockTiming)`; `TextStoreError`/`LockOutcome` → коды HRESULT как
   числовые константы (без типов `windows`); правило «смещение внутри пары → `TS_E_INVALIDPOS`»;
   ответ `TF_IAS_QUERYONLY` из выделения. Win32 только оборачивает.
2. **Ответы.** `Granted` → `*phrSession` = HRESULT `OnLockGranted`, возврат `S_OK`; `Deferred` →
   `TS_S_ASYNC`; `SyncLockUnavailable` → `*phrSession = TS_E_SYNCHRONOUS`, `S_OK`;
   `DeferredQueueFull` → `E_FAIL`; `Detached` → `E_UNEXPECTED`.
3. **Слот сессии.** Грант открывает `SessionSlot` (`Read(*const dyn TextStoreRead)` /
   `Write(*mut dyn TextStoreEdit)`) на время `OnLockGranted`, guard закрывает его и при unwind.
   Метод без слота → `TS_E_NOLOCK`; правка под `Read` → `TS_E_NOLOCK`. Слот несёт флаг
   `in_use`: метод, который берёт слот, пока его держит другой метод, получает
   `E_UNEXPECTED` — второго `&mut` не бывает. SAFETY опирается на инварианты этого кода:
   указатель выведен из ссылки сессии и живёт только внутри кадра замыкания (guard);
   `DocumentState` и store живы весь вход (клоны на стеке, B1); доступ — только owner-поток
   (`!Send`); одновременно не больше одного заимствования (флаг `in_use`). Единственный
   `unsafe` фичи.
4. **Композиция.** `OnStart/OnUpdate/OnEndComposition` вызывают `set_composition` на открытом
   слоте; вне сессии — `E_UNEXPECTED` (спайк подтверждает, что TSF так не делает).
5. **Уведомление владельца вне блокировки (B2 ревью).** `write_back` выполняется внутри `open`
   под `Held` (`lock.rs:339-343`), поэтому `on_changed` там вызываться не может. Store
   фиксирует контроллер и ставит «уведомление владельца должно» (`owed_owner_notification`);
   `LockArbiter::request`/`run_deferred` получают второй аргумент `settle: &mut dyn FnMut()`,
   который арбитр вызывает после снятия `Held` и **до** следующего гранта очереди. В `settle`
   store вызывает `on_changed` и сбрасывает отложенные уведомления observer'у. Вложенный
   запрос из `settle` видит незаблокированный store и не обгоняет очередь (FIFO арбитра).
6. **Паника в `on_changed` (п. 5 ревью).** Паника в `settle` перехватывается store'ом;
   грант уже выполнен, поэтому `RequestLock` возвращает `S_OK` и `*phrSession` от
   `OnLockGranted`; payload (первый) кладётся в канал отказа `CommitGate`, realm на ближайшем
   owner turn сообщает его через свой отчёт о панике (общий с `teardown`) и ставит
   `anchor_owed`; очередь грантов продолжает выполняться.
7. **Правка приложения во время блокировки (п. 7 ревью).** Контроллер получает счётчик
   поколений; сессия запоминает его при открытии. Если к `write_back` поколение сменилось
   (вложенный модальный цикл STA, async-задача), результат сессии платформы отбрасывается,
   правка приложения сохраняется и после снятия блокировки уходит в TSF `OnTextChange`; TIP
   перечитывает документ. Слияния нет — нет базы для трёхстороннего сравнения.
8. **Реентерабельность (F7).** Ни `TextInputOwner`, ни `TextServices`, ни виджет не держат
   borrow через вызов TSF или владельца: `take()` выносится в отдельный оператор в blur и в
   dispose; вложенный sync-лок → `TS_E_SYNCHRONOUS`; уведомления sink не вкладываются (флаг
   `notifying`, отложенная пометка).
9. **Вход COM и realm (п. 10 ревью).** COM-входы TSF не вызывают `UiRealm::enter`: backend не
   имеет ссылки на realm. Обоснование и правило: runtime держит `CommitGate` открытым, только
   пока realm стоит в своём слоте и не выполняет кадр; извлечённый realm (кадр, конец сессии)
   — gate закрыт, sync-лок отказан, async ждёт якоря, пользовательский код не выполняется. Так
   COM-вход доходит до виджетного кода только при realm в слоте на owner-потоке — тот же
   статус, что у OS-callback ввода. `on_changed` пишет через `WriterSource` (ADR-0086); W3
   проверяет, что запись вне `enter` разрешена, иначе доставка `settle` идёт через owner turn
   realm. Строка `tsf_entries_while_the_realm_is_checked_out_run_no_user_code`.

Требования: R18, F2, F3, F4, F7, F8. ≈1,25 нед (с W3). ADR: поправка ADR-0090 (`settle`,
канал отказа, поколение, долг якоря).

### (d) Координаты для `GetTextExt`

| | D-1. Чистая функция (выбран) | D-2. `MapWindowPoints` на месте | D-3. Store в физических пикселях |
|---|---|---|---|
| Тестируемость | Linux CI, таблица | Только Windows-хост | Ломает ADR-0030 §7 |

**Выбор D-1.** `range_rect_to_screen(rect: Bounds<f64>, client_origin: DevicePoint, ratio:
DevicePixelRatio) -> Result<ScreenRect, ScreenRectError>` в `flui_platform::shared::text_geometry`:
`origin + cover(rect × ratio)`, где перед `floor`/`ceil` значения, отстоящие от целого меньше
чем на `1e-6` device px, прижимаются к нему (шум float не добавляет пиксель), а отрицательные
координаты округляются через `floor`, не усечением. Не-конечное или вне `i32` → ошибка →
`TS_E_NOLAYOUT` + `debug!`. Масштаб — `WindowContext.scale_factor` (тот же источник, что у
pointer-ввода), начало — `ClientToScreen(hwnd, (0,0))` в момент вызова. Пустой диапазон →
rect каретки; при композиции — нулевая ширина у правого края предыдущего символа (Pinyin).
Запасной rect при `NoLayout` — **состояние store** (последний раскладанный rect начала
композиции), не часть функции. Требования: R12, F6 (R13 — после go/no-go). ≈0,3 нед.

## Публичный контракт

`flui-platform-api` (`src/text_store/host.rs`, реэкспорт из `text_store`):

```rust
/// A pull-model platform's side of one window's text input (ADR-0090 §3).
/// Owner thread only, shared as `Rc<dyn TextStoreHost>`; not `Send`.
pub trait TextStoreHost {
    /// `store` now receives this window's text input; `None` when no field does.
    /// Called with no borrow held. Inside a platform call into a store the host
    /// queues it and applies it when that call returns.
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>);
    /// End the platform's composition in the focused store.
    /// # Errors
    /// [`TextStoreHostError::Unavailable`] when the window's text services are gone.
    fn complete_composition(&self) -> Result<CompositionEnd, TextStoreHostError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionEnd {
    /// The platform committed its composition into the store.
    Committed,
    /// The platform could not (refused lock); its composition is discarded and
    /// the caller must clear the store's composition, keeping the text.
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextStoreHostError {
    #[error("the window's text services are unavailable")]
    Unavailable,
}
```

`LockArbiter` (там же): `request(grant, timing, open, settle: &mut dyn FnMut())` и
`run_deferred(open, settle)`; `CommitGate::defer_failure(Box<dyn Any + Send>)` /
`take_failure()` (первый отказ сохраняется, следующие удерживаются по ADR-0127).

`flui-platform`: `OwnerPlatform::text_store_host(&self, window: &Arc<dyn HostWindow>) ->
Option<Rc<dyn TextStoreHost>>` — временное место до `OwnerWindow` (ADR-0082 §4 шаг 2);
backend-метод на `HostWindow` запечатан токеном, который чеканит только `OwnerPlatform`.

`flui-interaction`:

```rust
pub enum TextInputBackend {
    Push(Arc<dyn PlatformTextInput>),
    Pull(Rc<dyn TextStoreHost>),
    None,
}
impl TextInputOwner {
    pub fn new(backend: TextInputBackend) -> Rc<Self>;  // ломающее: было Option<Arc<..>>
    /// Resolve the active composition by committing it (pointer-down, close request).
    pub fn complete_composition(&self);
}
impl TextInputHandle {
    /// Commit `token`'s composition, keeping its text; a stale token is a no-op.
    pub fn complete_composition(&self, token: ClientToken) -> Result<(), TextInputError>;
}
```

`active_store` удаляется (единственные вызовы — тесты и harness; Win32 получает store через
`focus_store`). `flui-runtime`: `PresentationWindow` несёт `TextInputBackend`.
`flui-widgets`: `TextEditingController::committed_text(&self) -> String`.
`flui-testing`: `KIT_VERSION = 2`; `owner_notifications` считает изменения подтверждённого
текста; тестовое окно realm'а — `Pull` с записывающим хостом, `Harness::store_host_calls()`.

**Нет новой публичной поверхности:** весь TSF — приватный `platforms::windows::text_services`;
`shared::text_geometry`, `shared::tsf_acp` и правило `VK_PROCESSKEY` — функции внутри уже
публичного `shared` без типов `windows`; `flui-app` (только подключение в
`runner::presentation_window`); `flui-sdk` (метод на экспортированном типе не меняет
`tests/surface.rs`). Ни один тип `windows::*` не выходит из `flui-platform`.

## Инварианты, владение, lifecycle

- **Поток.** Всё TSF — на owner-потоке (STA); `TextServices`, `Document`, `DocumentState`,
  хост — `!Send + !Sync` (`assert_not_impl_any!`). Получить хост можно только с
  `&OwnerPlatform`.
- **Активация.** После установки `WindowContext`: `ITfThreadMgr` → `Activate` (парный
  `Deactivate`) → пустой менеджер → `AssociateFocus(hwnd, empty)`. Отказ → `None`, один
  `warn!` на платформу (флаг в `OwnerControlContext`), путь `WM_CHAR` (R21).
- **Фокус поля (R17).** После коммита состояния `TextInputOwner` без borrow вызывает
  `focus_store(Some(store))`: закрыть прежний `Document`, создать новый (`CreateDocumentMgr`,
  `CreateContext`, `Push`), `AssociateFocus(hwnd, dim)` и, если `GetFocus() == hwnd`, явно
  `ITfThreadMgr::SetFocus(dim)` (как `TSFBridge`; побочный эффект `AssociateFocus` не
  считается доказанным). Тот же `Rc` (`Rc::ptr_eq`) — no-op. Alt+Tab переключает фокус TSF
  сам; FLUI композицию при деактивации не трогает (R7).
- **Закрытие документа** (не под его собственным входом — очередь B1): `closed = true` →
  `AssociateFocus(hwnd, empty)` (+ `SetFocus(empty)` при фокусе окна), `ppdimPrev`
  освобождается → `Pop(TF_POPF_ALL)` → освобождение sink, store `Rc`, контекста, менеджера.
- **Разрешение композиции подтверждением (R6, R8, R10, B3, B4).**
  1. Pointer-down в presentation — **в том числе внутри поля с композицией** — runtime вызывает
     `TextInputOwner::complete_composition()` до удержания и до hit-test, затем заново
     проверяет, что presentation жива (`closing_requested`, lifecycle, есть в realm); если нет,
     событие отбрасывается (п. 11 ревью).
  2. Принятый запрос закрытия (`CloseReason::User/Program`) и `WM_QUERYENDSESSION`
     (`SessionEnd`): runtime вызывает `complete_composition()` во всех presentation realm
     **до** guard'а закрытия (`hold`/`require_decision`) и до сохранения, поэтому проверка
     «несохранённые изменения» и черновик видят preedit как текст. Realm, извлечённый из слота
     в момент конца сессии, не выполняет ничего: preedit отбрасывается (предел `teardown`).
  3. Blur — `complete_composition(token)`, затем `detach`; Ctrl+V/Ctrl+Z — подтверждение, затем
     действие.
  4. Dispose/unmount — **страховка**: если композиция ещё есть, виджет снимает composing range
     на месте, текст остаётся, слушатели контроллера уведомляются, `on_changed` не вызывается,
     затем `store.detach()`. Отбрасывается только правка IME, не применённая к store до
     отсоединения (`Detached`).
- **Операции хоста упорядочены и привязаны к цели.** Внутри транзакции кадра `TextInputOwner`
  ставит операцию в очередь и выполняет на якоре до отложенных грантов. Отложенная
  `complete_composition` захватывает **хост-объект целевого документа** в момент постановки
  (`Win32TextStoreHost` отдаёт дескриптор `Document`), а не токен: последующий `detach` не
  делает её no-op. Закрытие владельца выполняет хвост завершений, затем `focus_store(None)`.
- **Отказ `TerminateComposition` (B3).** `ITfContextOwnerCompositionServices::
  TerminateComposition(None)` вернул ошибку или sync-лок отказан (`TS_E_SYNCHRONOUS`): хост
  закрывает документ и создаёт новый для того же store (TIP теряет свою композицию) и
  возвращает `Abandoned`; владелец запрашивает async read-write лок `set_composition(None)` —
  текст остаётся. Вне кадра лок выдаётся сразу, поэтому вставка R10 видит подтверждённый
  текст. Без хоста (проекция) — тот же лок.
- **Долг якоря и отложенный отказ (F8, п. 5).** `UiRealm.anchor_owed` ставится, если drive
  размотался, якорь прошёл при закрытом gate или в `CommitGate` лежит отказ; снимается только
  выполненным якорем. При постановке — запрос redraw; `drain_owner_inbox`, `pump_background`
  и `pump` первым делом выполняют задолженный якорь и отчёт об отказе. Неудачный wake долг не
  снимает.
- **Teardown (F5; teardown R21).** Принятое закрытие → `complete_composition()` (п. 2) →
  guard/сохранение → `UiRealm` закрывает presentation → dispose (страховка) и `detach` →
  `TextInputOwner::close` → `focus_store(None)` → `WM_DESTROY`: `AssociateFocus(hwnd, None)`,
  пустой менеджер, `Deactivate`, retire `WindowContext`. Если `WM_DESTROY` первым, `Weak`
  хоста мёртв — no-op. Учёт `Drop` — счётчики `teardown`.
- **Уведомления.** Observer → `OnTextChange`/`OnSelectionChange`/`OnLayoutChange(TS_LC_CHANGE)`/
  `OnStatusChange` только по маске sink, не во время уведомления и не под блокировкой.
- **Клавиши.** `VK_PROCESSKEY` не становится ни символом, ни клавишей (`shared::keys`,
  ADR-0069); `WM_IME_*` → `DefWindowProcW`; `Win32_UI_Input_Ime` не включается (R19).
- **D2.** `on_changed`, автовалидация, `TextFormField::validate/save`, черновик Notes читают
  `committed_text()`; сессия, менявшая только preedit, владельца не уведомляет.

## Ошибки и отказы

| Сценарий | Механизм | Где пиннится |
|---|---|---|
| F1 stale store | Грант захватывает свой `Document`; закрытый → `OnLockGranted` не вызывается, методы `E_UNEXPECTED`; отсоединённый store → `E_UNEXPECTED` | Win32-unit, виджет |
| F2 правка из `on_changed` | `on_changed` в `settle` после снятия `Held`; `set_text` — правка приложения, уходит в `OnTextChange`; sync внутри сессии → `TS_E_SYNCHRONOUS` | виджет |
| F3 паника в `on_changed` | Текст уже записан; `RequestLock` → `S_OK`/`*phrSession`; отказ через `CommitGate` в отчёт realm, `anchor_owed`; очередь продолжается | виджет + Win32-unit |
| F4 очередь полна | `E_FAIL`; очередь цела (тест после go/no-go) | Win32-unit |
| F5 закрытие во время композиции | Порядок teardown; preedit подтверждён до guard'а | Win32-unit + teardown R21 |
| F6 DPI посреди композиции | Масштаб из `WindowContext.scale_factor`, `OnLayoutChange` (с R13) | Linux-таблица + Win32-unit |
| F7 FLUI → TSF → store | Нет borrow через вызов; очередь B1; слот `in_use`; запись один раз | Win32-unit |
| F8 грант без кадров | `anchor_owed` + redraw; `pump`/`pump_background`/`drain_owner_inbox` | runtime + Win32-unit |
| Документ закрывается под своим входом | Очередь операций по `entry_depth`, клоны `Rc` на стеке | Win32-unit |
| Правка приложения во время блокировки | Поколение контроллера, сессия платформы отбрасывается | виджет |
| `BUG:` в адаптере | Отравление → пустой документ (очередь) → пересоздание на owner turn → после 3 раз `WM_CHAR` | Win32-unit |
| Отказ `TerminateComposition` | `Abandoned` → пересоздание документа → async `set_composition(None)` | Win32-unit, виджет |
| Отказ активации TSF | `None`-хост, `warn!` один раз, `WM_CHAR` | Win32-unit |

## Тестовая стратегия

Уровни: **Linux** — модуль `crates/flui-platform/tests/text_input_mapping.rs` в `tests/main.rs`,
таблица `win32_text_input_mapping_rows` (`(&str, fn())`): D-1, `tsf_acp`, `VK_PROCESSKEY`;
**Win32-unit** — `#[cfg(all(test, target_os = "windows"))]` модуль
`platforms/windows/text_services/tests.rs`, таблица
`the_text_services_bridge_honours_the_tsf_contract` (как
`the_owner_thread_machinery_honours_its_contracts`): mock-sink `#[implement(ITextStoreACPSink)]`
со сценарием в `OnLockGranted`, mock `TsfThread` для отказов, настоящий `ITfThreadMgr` для
фокуса; CI — clippy, прогон на Windows прикладывается к PR; пишется до кода (test-first, W6);
**виджет** — `crates/flui-widgets/tests/editable_text.rs`, модуль `text_store`
(`common::cases::run_cases`); **material** — `packages/flui-material/tests`; **kit** — v2;
**Notes** — `notes_public_input_flow_matrix`; **runtime** — таблица с
`a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns`;
**нативный** — `cargo xtask device windows-ime`.

| R/F | Тест (строка) | Уровень | Без изменения падает, потому что |
|---|---|---|---|
| R1 | `ime_preedit_is_underlined_at_the_caret_and_hides_it_on_request` | Notes | закрепляет поведение через facade для R23 |
| R2 | `a_composition_update_replaces_the_whole_preedit_and_places_the_caret` | Win32-unit | без `OnUpdateComposition` composing range старый — хвост |
| R3 | `ime_commit_inserts_the_conversion_and_notifies_once` | Notes | `on_changed` на каждый preedit, валидатор видит кану |
| R4 | `cancelled_composition_keeps_the_committed_text_without_on_changed` | виджет | сравнение полного текста даёт `on_changed` |
| R5 | `composition_over_a_selection_replaces_the_selection` | kit v2 | выделение остаётся |
| R6 | `pointer_down_mid_composition_commits_before_the_save_handler`; `pointer_down_inside_the_composing_field_commits` | Notes; виджет | Save читает committed text без preedit |
| R7 | шаг `alt_tab_mid_composition` | нативный | — |
| R8 | `route_change_mid_composition_commits_the_preedit_into_the_draft`; `a_conversion_queued_for_an_unmounted_field_is_dropped` | Notes; виджет | dispose отсоединяет без подтверждения; грант после dispose доходит до контроллера |
| R9 | `autovalidation_ignores_a_whitespace_preedit_until_commit` | material | валидатор видит preedit |
| R10 | `paste_and_undo_mid_composition_commit_first`; `a_refused_termination_still_commits_before_paste`; `the_enter_that_commits_does_not_submit` | виджет | paste отказывает; при отказе TSF preedit остаётся подчёркнутым |
| R11 | `a_processkey_keydown_produces_no_key` | Linux | `Unidentified` доходит до обработчиков |
| R12 | `screen_rects_follow_scale_and_round_outwards` (100/150/175 %, дробные края, отрицательные координаты, шум `1e-9`, NaN, `i32::MAX`) | Linux | усечение отрицательных, лишний пиксель от шума, NaN наружу |
| R13 | `scroll_move_and_dpi_change_send_layout_change_before_the_next_query` | Win32-unit (после go/no-go) | нет `OnLayoutChange` |
| R14 | `split_surrogate_offsets_are_invalid_positions` (Linux, `tsf_acp`); `astral_and_cluster_commits_count_utf16` (Win32, после go/no-go) | Linux; Win32-unit | `S_OK` вместо `TS_E_INVALIDPOS` |
| R15 | `rtl_commit_in_an_ltr_field_keeps_logical_order_and_a_rect` | виджет | пустой rect |
| R16 | `a_window_activates_the_thread_manager_and_a_focused_field_gets_a_document` | Win32-unit | нет активации |
| R17 | `field_focus_moves_the_tsf_document_focus`; `focus_gain_and_loss_reach_the_store_host` | Win32-unit; виджет | `GetFocus` менеджера не меняется без явного `SetFocus` |
| R18 | `the_bridge_reproduces_the_kit_conversion_script`; `reads_outside_a_lock_answer_no_lock`; `a_lock_inside_the_frame_answers_async`; `lock_flags_and_errors_translate_to_tsf_codes` (Linux) | Win32-unit; Linux | расхождение с kit; неверный код |
| R19 | `ime_window_messages_reach_the_default_procedure` | Win32-unit (после go/no-go) | — |
| R20 | `reconversion_requests_leave_the_document_unchanged`; `query_only_insert_answers_from_the_selection` (Linux) | Win32-unit (после go/no-go); Linux | правка при `TF_IAS_QUERYONLY` |
| R21 | `failed_activation_falls_back_to_wm_char_and_warns_once` | Win32-unit | двойной `warn!`, потеря пары суррогатов |
| R22 | шаг `title_copy_paste_round_trips_through_the_clipboard` | нативный | — |
| R23 | строки R1, R3, R4, R6, R8 + `flui::facade_consumer` | Notes | — |
| R24 | `cargo xtask device windows-ime`; отрицательный контроль на `4915054c8` | нативный | гейт окна кандидатов на старом SHA падает |
| F1 | `a_grant_for_a_replaced_document_answers_unexpected` | Win32-unit + виджет | грант A попадает в store B |
| F2 | `set_text_from_on_changed_lands_after_the_commit`; `on_changed_runs_after_the_lock_is_released` | виджет | `on_changed` под `Held`: вложенный sync-лок отказан |
| F3 | `a_panicking_on_changed_keeps_the_commit_and_reports_through_the_realm`; `a_panic_after_the_grant_returns_ok_and_the_queue_drains` | виджет; Win32-unit | `E_FAIL`, пропуск очереди |
| F4 | `a_full_deferred_queue_answers_fail_and_recovers_after_the_anchor` | Win32-unit (после go/no-go) | — |
| F5 | `closing_the_window_mid_composition_releases_tsf_objects_in_order`; `an_accepted_close_commits_the_preedit_before_the_guard` | Win32-unit; runtime | черновик без preedit |
| F7 | `tsf_reentry_from_terminate_composition_commits_once`; `on_changed_unfocus_inside_request_lock`; `a_nested_slot_entry_answers_unexpected` | Win32-unit | `BorrowMutError`, use-after-free документа, второй `&mut` |
| F8 | `a_grant_deferred_by_a_panicking_frame_runs_without_another_frame` | runtime | грант висит |
| п. 7 | `an_app_edit_during_a_lock_is_not_overwritten` | виджет | `write_back` затирает правку |
| п. 10 | `tsf_entries_while_the_realm_is_checked_out_run_no_user_code` | runtime | `on_changed` в извлечённом realm |
| п. 6 | `a_poisoned_document_is_replaced_then_falls_back_after_three` | Win32-unit | документ остаётся отравленным |

Kit v2: `composition_over_a_selection_replaces_the_selection`,
`composition_only_sessions_do_not_notify_the_owner`, `owner_notification_runs_after_release`
(`since: 2`); v1 не меняется. Различимость: для R3, R6, R8, F2, F8 и `on_changed_unfocus_inside_request_lock`
— прогон с откатом production-ханка в отдельном worktree, вывод в PR.

## ADR

**Новый ADR (номер назначит оркестратор): «Win32 text services hold the text store on the
window's owner thread».** Implements part of ADR-0082 §4; Related: ADR-0090, ADR-0097.
1. `TextStoreHost` — owner-thread сторона pull-платформы; `CompositionEnd { Committed, Abandoned }`.
2. Хост выдаётся только с `&OwnerPlatform` (временно, до `OwnerWindow` шага 2); `PlatformWindow`
   и `PlatformTextInput` не меняются; `TextInputOwner` принимает `TextInputBackend`.
3. Win32 хранит хост, `ITfThreadMgr` и документы в `WindowContext`; без `static`/`thread_local!`;
   `windows::*` не покидает `flui-platform`.
4. Документ TSF на фокус поля; `AssociateFocus` + явный `SetFocus`; документ не закрывается
   под собственным COM-входом (очередь по глубине входов).
5. Каждый COM-вход — `catch_unwind`; `BUG:` отравляет документ, после трёх — путь `WM_CHAR`.
6. `GetTextExt` — чистая функция с наружным округлением; ACP-перевод — cfg-free модуль.
7. Без IMM32.

**Поправка к ADR-0090 (§1, §2, §3, Interim, Verification; ADR-0030 §6 — `Superseded-by` для
правила blur).**
1. Подтверждённый текст — документ без composing range; владелец поля уведомляется только о
   его изменении. Kit v2.
2. Уведомление владельца выполняется после снятия блокировки и до следующего гранта
   (`settle`); паника в нём не отменяет грант и уходит в отчёт realm через `CommitGate`.
3. Сессия платформы, во время которой приложение изменило поле, отбрасывается.
4. Композиция поля, теряющего ввод (pointer-down, принятое закрытие, конец сессии, blur,
   вставка, unmount), подтверждается; отказ платформы → `Abandoned` и подтверждение на месте.
5. Якорь, пропущенный размотанным кадром, — долг realm до следующего owner turn.
6. Gate открыт, только пока realm в своём слоте и вне кадра.
7. §3 Platform mapping обновляется; `active_store` удаляется.

Соблюдаются без изменений: ADR-0027 §3, ADR-0069, ADR-0092, ADR-0097, ADR-0098, ADR-0127.

## Спайк (цель ~2026-10-16, жёсткий срок 10-26)

**Объём** (ветка, без merge до ADR): модуль `platforms/windows/text_services/` в рабочем виде
(активация, пустой документ, `Document` на фокус, `TsfStore` с `RequestLock`, чтениями,
`SetText`, `InsertTextAtSelection`, `GetTextExt`, `GetScreenExt`, `GetWnd`, `AdviseSink`,
composition sink, `SessionSlot`, `com_entry`, очередь по `entry_depth`); `shared::text_geometry`
и `shared::tsf_acp` с таблицами; один `#[ignore]` тест-зонд, который открывает настоящее окно
`WindowsPlatform` и подключает к `TextServices` напрямую **store с запаздывающей раскладкой**:
обёртка над `InMemoryTextStore`, которая отвечает `NoLayout` для текста, вставленного в текущей
сессии, и «раскладывает» его по тику зонда — как `EditableText`. Поле смещено от начала
клиентской области (rect не в (0,0)).

**Доказательства (PASS — все пункты, каждый способен упасть).** Windows 11, Microsoft IME
ja-JP (новый) и, если есть, «previous version», 100 % и 150 %:
1. `Activate` → `S_OK`, `TfClientId`; явный `SetFocus` меняет `GetFocus` менеджера.
2. `toukyou`, Space, Enter → в store `東京`, композиции нет; журнал `RequestLock` →
   `OnLockGranted` → `SetText`/`OnStartComposition`…`OnEndComposition`.
3. Путь `TS_S_ASYNC`: на части ввода зонд держит gate закрытым (имитация кадра) — IME получает
   `TS_S_ASYNC`, после открытия и якоря текст тот же, без потерь и дублей.
4. Окно кандидатов/подсказок: список классов окон IME для нового IME Win11 (окна
   `TextInputHost.exe`, XAML) и для старого установлен по перечислению top-level окон и
   **проверен отрицательным контролем** (IME выключен — гейт не находит окна; окно умышленно
   смещено — гейт падает). Прямоугольник пересекает полосу ±1 строка под rect `GetTextExt`
   **на первом нажатии** (окно подсказок) и после Space; xcap-скриншоты.
5. Реакция MS-IME на `TS_E_NOLAYOUT` (повтор после `OnLayoutChange` или окно в углу).
6. Приходит ли `VK_PROCESSKEY`; нужен ли `ITfKeystrokeMgr`.
7. `TerminateComposition` при открытом gate — `Committed`; при закрытом — путь `Abandoned`.
8. Ни одного abort; `#[implement]` компилируется с `Rc`-полями.

**FAIL** — нет (1)–(4) к 10-26: эскалация с журналом; решение D3 на 2026-11-20.
**Выживает:** модуль, обе чистые функции с таблицами, правило `VK_PROCESSKEY`, список классов
окон IME (переходит в `device windows-ime`). **Выбрасывается:** тест-зонд и обёртка store
(~200 строк). Если (5) — окно в углу: store получает запасной rect (состояние store, строка в
виджетной таблице).

## Работы

| № | Работа | Зависит | Исп. | Нед |
|---|---|---|---|---|
| W1 | Спайк (выше) | — | A | 1,5 |
| W2 | Контракт: `TextStoreHost`, `CompositionEnd`, `OwnerPlatform::text_store_host`, `TextInputBackend`, очередь операций владельца с захватом цели, `complete_composition`, записывающий хост, удаление `active_store`, `changelog.d/`; узкий ADR | W1 (неделя 1) | A | 1,0 |
| W3 | D2 + поправка ADR-0090: `committed_text`, поколение контроллера, `settle` в `LockArbiter`, канал отказа `CommitGate`, `EditObserver`, `FormField`/`TextFormField`, kit v2 | — | B | 1,25 |
| W4 | Runtime: долг якоря, gate «realm в слоте», хуки pointer-down (с перепроверкой) и принятого закрытия/`SessionEnd` | W2 (`complete_composition`) | B | 0,75 |
| W5 | Виджет: blur/paste/undo/unmount, вынос `take()` в blur и dispose, `Abandoned`-путь, R4/R9/R10/R15/F2/F3 | W2, W3 | B | 1,0 |
| W6a | `shared::tsf_acp` + Linux-таблица | W1 | B | 0,5 |
| W6 | Win32 production test-first (бывшие W6+W7): маппинг §3, документ, B1, отравление, R2, R16–R18, R21, F1, F3, F5, F7 | W1, W2, W6a | A | 2,4 |
| W8 | Notes-строки R1, R3, R4, R6, R8 + consumer (R23); teardown R21 | W4, W5 | B | 0,75 |
| W9 | `cargo xtask device windows-ime` + шаг R22 в `windows-notes` | W1 (классы окон) | C (или B после 11-20) | 1,5 |
| W10 | После go/no-go: R13, R14 (Win32), R19, R20, F4 | W6 | A | 0,7 |
| W11 | Нативный прогон R24 на SHA, `docs/BETA.md` | все | A | 0,5 |

Итого ≈ 11,85 нед (≈12).

**Критический путь до go/no-go (2026-11-20, 6,6 нед от 10-06):** W1 (10-06 → 10-16) → W2
(10-16 → 10-23) → W6 (10-23 → ~11-09) = **4,9 нед**, запас ≈1,7 нед на ручной прогон
`toukyou` и правки. Нужна R3 (Notes) к go/no-go — путь исполнителя B: W3 (10-06 → 10-14) →
W6a (10-14 → 10-17) → [ждёт W2 10-23] W4 + W5 (10-23 → 11-04) → W8 (11-04 → 11-07). **Второй
исполнитель обязателен** (B: W3, W4, W5, W6a, W8 ≈ 4,25 нед); W9 — третий исполнитель или B
после 11-20 (для go/no-go хватает ручного `toukyou`, для релиза — нет).

**Порядок с `send-flip`.** `send-flip` переписывает `controller.rs`, `text_store.rs` и
`Arc<Mutex>`-контроллер. W3 и W5 трогают те же файлы малыми правками (`committed_text`,
поколение, `settle`, разрешение композиции). Решение: W3 сливается до 10-17, W5 — до 11-04;
`send-flip` начинает переписывание текстового поля после W5 и переносит `committed_text`,
поколение и `settle` в owner-local контроллер (их тесты — строки kit v2 и `text_store` —
остаются арбитром). Если `send-flip` сливает контроллер раньше, W3/W5 переносятся на новый
контроллер (+0,5 нед исполнителю B, запас критического пути это покрывает). Файлы [P]: A —
`flui-platform` (`platforms/windows/`, `traits/owner.rs`), `flui-platform-api/src/text_store/host.rs`,
`flui-interaction`; B — `flui-widgets`, `packages/flui-material`, `flui-testing`,
`flui-platform-api/src/text_store/lock.rs`, `flui-runtime/src/ui_realm/`,
`flui-platform/src/shared/tsf_acp.rs`; C — `tools/xtask/src/device/`.

## Риски

1. **Окно кандидатов и `TS_E_NOLAYOUT`.** Раскладка запаздывает на кадр; MS-IME может поставить
   окно в угол (у Firefox обход). Снижение: зонд с запаздывающей раскладкой и замером на первом
   нажатии; запасной rect в store; `OnLayoutChange` после кадра.
2. **Реальные TIP против отложенных и вложенных блокировок** (`TS_S_ASYNC` внутри кадра,
   закрытие документа под входом, единственный `unsafe`). Снижение: зонд гоняет путь
   `TS_S_ASYNC`; очередь по `entry_depth`, флаг `in_use`; Win32-строки F7 с реентерабельным
   mock; zh-CN Pinyin в R24.
3. **Нет запаса по графику и проверки вне Windows.** Критический путь 4,9 нед при одном
   Win32-исполнителе, нужен второй; CI — clippy для Win32; конфликт с `send-flip` в файлах
   контроллера. Снижение: всё чистое (D-1, `tsf_acp`, `VK_PROCESSKEY`, D2, F8) — в Linux и
   headless; Win32-прогон к каждому PR; порядок слияния с `send-flip` выше; запасной вариант
   D3 — релиз с «no IME» и проверенной деградацией R21.

## Факты спайка (2026-10-06, ветка `text-ime/tsf-spike` @ `4e14cba8c`)

Прогон на хосте разработки с Microsoft IME ja-JP, масштаб 100 %. Проверено оркестратором по
скриншотам: список кандидатов стоит под полем, а при сдвиге прямоугольника store на 150 px
сдвигается вместе с ним.

- Пункты 1, 2, 3, 5, 6, 7 и 8 прошли. Пункт 4 прошёл при 100 %; прогон при 150 % ждёт ручного
  переключения масштаба владельцем.
- Новый IME Win11 рисует кандидатов внутри одного полноэкранного cloaked-окна `TextInputHost.exe`
  (`Windows.UI.Core.CoreWindow`). Перечисление top-level окон его не находит. Гейт R24 и `device
  windows-ime` поэтому — скриншот-дифф около поля (GDI) или UI Automation по элементам
  TextInputHost, а не прямоугольник окна.
- Compartment open/close не включает MS-IME, включает клавиша `VK_IME_ON`. `ActivateProfile`
  требует `TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE`, флаги `0` дают `E_INVALIDARG`.
- При закрытом gate `TerminateComposition` возвращает `E_FAIL`, путь `Abandoned` реален.
  `ITfKeystrokeMgr` для MS-IME ja не нужен; `VK_PROCESSKEY` до окна не доходит.
- MS-IME повторяет запрос после `OnLayoutChange` (24× `TS_E_NOLAYOUT`), и окно не уходит в угол.
- `AssociateFocus` сам переводит фокус TSF, если у окна есть фокус; явный `SetFocus` безвреден.
- Probe — около 890 строк против плана в 150. Он остаётся `#[ignore]`-тестом в крейте и в `main`
  не вливается; вливается только PR с функцией отображения.

## Изменения контракта после ревью T2a (2026-10-06, решение оркестратора)

Основание — независимое ревью `text-ime/store-contract` @ `739f49dc9`.

1. **Паника в `settle` не теряется.** В продакшене gate установлен всегда, а `take_failure` никто не
   читал: паника `on_changed` молча пропадала. Читатель подключается в T2a, не в T3: после возврата
   `TextInputOwner::run_deferred_grants` и `dispatch` ошибка из gate забирается и пробрасывается внутри
   их существующей изоляции (`RoutePanic::try_run`), то есть доходит до отчёта realm. T3 сохраняет
   это поведение и отвечает за путь через якорь кадра.
2. **Подтверждённый текст при реконверсии.** «Подтверждённый текст» — это текст документа, в котором
   диапазон композиции заменён на текст, занимавший его до начала композиции (для нового preedit —
   пустая строка, поведение прежнее). Сессия, которая только помечает существующий текст как
   компонуемый, не меняет подтверждённый текст и не уведомляет владельца. Это поправка к пункту 1
   amendment ADR-0090; в kit добавляется строка про реконверсию.
3. Строки, которых не хватало: замена контроллера во время read-write гранта и путь отказа
   паникующего `on_changed` на уровне виджета (одиночный сбой, два в конкуренции, следующий грант
   после изоляции).
4. Сброс формы во время композиции оставляет preedit — ограничение до пункта 4 amendment
   (commit композиции), записать в его scope.
