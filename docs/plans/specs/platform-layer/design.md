# Платформенный слой — дизайн

- **Статус:** черновик на утверждение владельцу
- **Дата:** 2026-10-06
- **База:** `main` @ `d56188c14`
- **Требования:** [requirements.md](requirements.md)
- **ADR (Proposed, на утверждение):**
  [ADR-0151](../../../adr/ADR-0151-platform-layer-boundary-and-names.md) — граница, имена,
  размещение lifecycle и системных настроек;
  [ADR-0152](../../../adr/ADR-0152-capability-seam-revised.md) — шов возможностей
  (заменяет ADR-0084 с изменениями)

Обозначения в фактах: **[R]** прочитано в коде, **[C]** скомпилировано, **[X]** запущено,
**[I]** вывод, **[—]** недоступно на этом хосте.

## 1. Что есть сейчас (сжато)

Полная инвентаризация — §8. Главное:

| Факт | Источник |
|---|---|
| `flui-platform-api`: C/1, layer 1, `stable`, `reach-forbid = [accesskit, tokio]`; фасад делает `pub use flui_platform_api as platform` — весь крейт Stable-поверхность `flui` | [R] `crates/flui-platform-api/Cargo.toml:69-74`, `src/lib.rs:183` |
| В Stable-сигнатурах — `ui-events` 0.3, `keyboard-types` 0.8 (`Key`, `Modifiers`), `dpi::PhysicalPosition`; запрещено ADR-0089 | [R] `flui-platform-api/src/input.rs:29-38,96`, `platform_window.rs:288` |
| ~2 000 строк политики и реализации в контракте: `LockArbiter`, `OwnerCalls`, `CompositionLedger`, `EditGeneration`, `project_ime_event`; тестовые двойники `InMemoryClipboard`, `InMemoryTextStore` (661 строка); backend-слаб `OfferTable` | [R] `text_store/*`, `clipboard.rs:23`, `data_transfer.rs` |
| Unwired в контракте: `Storage` (единственный impl — `flui-testing::MemoryStorage`), `PlatformHaptics` (только `FakeHaptics`), `DataTransferSource` (только winit, без потребителя), `PlatformDisplay`, `WindowBounds`, `WindowMode`, `WindowEvent`, ~10 методов `PlatformWindow` | [R] |
| `flui-platform`: H/1, layer 3, `allowed-dependents = [flui-app]`, 97 файлов / 40 717 строк src; в production его называет только `flui-app` (21 файл) | [R] |
| В бэкенде живут контракты (`PlatformCapabilities`, `PathPromptOptions`, `SessionEndPhase/Answer`, `PlatformExecutor`), мёртвый код (`LinuxPlatform` — `unimplemented!()` во всех методах; `window.rs` с сырым `RawWindowHandle`; `BackgroundExecutor` на tokio) и публичные OS-типы (`win32::HWND`, `NSApplication`, `AndroidApp`, `web_sys::HtmlCanvasElement`, `tokio::runtime::Handle`, `accesskit::TreeUpdate`) | [R] |
| «Unsupported» сообщается по-разному: `Option::None`, `CursorError::Unsupported`, `Ok(None)` у файлового диалога (читается как отмена), молчаливый no-op у `open_url`, выдуманные значения дисплея на Android, паника в `LinuxPlatform` | [R] |
| `AppLifecycleState` — в `flui-scheduler` (ADR-0035), путь OS → runtime → scheduler проведён полностью; app-level сигнала ОС (фон/передний план, minimize, `visibilitychange`) нет | [R] `flui-scheduler/src/frame.rs:230`, `flui-runtime/src/lifecycle_state.rs` |
| `AccessibilityFeatures` — никто не пишет и не читает (`#[expect(dead_code)]`); `text_scale_factor` всегда 1.0; локаль системы не доставляется; `GestureSettings` — константы, `for_platform`/`native()` никто не зовёт | [R] `flui-app/src/app/runtime.rs:92-97`, `flui-widgets/src/app/media_query.rs:60`, `flui-interaction/src/settings.rs` |
| Возможность до виджета: `RealmHostServices` → `RealmServices` → `PresentationState` → `BuildOwner` → `BuildCapabilities` → `LifecycleContext` — ~7 файлов на новую возможность; пакет добавить не может (`LifecycleContext` sealed); ADR-0084 — 0 строк кода | [R] |
| Только Linux/headless исполняются в CI; Win32, AppKit, iOS, Android — clippy cross-typecheck; Windows-job, упомянутый в `ci.yml:405`, не существует | [R] |

## 2. Правила решения

1. **Контрактный крейт — словарь и трейты, нужные ядру.** Без OS-кода, без `unsafe`, без
   upstream-типов (ADR-0089), без `tokio`/`accesskit` (reach-forbid), без тестовых двойников.
   Элемент без потребителя выше бэкенда в контракт не входит: Stable — это обещание.
2. **OS-код — только в бэкенд-крейте**; от него зависит только `flui-app`. OS-типы в нём —
   `pub(crate)`.
3. **Источник системной настройки — бэкенд, потребитель — фреймворк.** Фреймворк не держит
   «умолчание ОС» константой; запасное значение для бэкенда без ответа ОС лежит рядом с типом
   в контракте и документировано по бэкендам.
4. **Тип, который делят производитель и потребитель, — в самом нижнем нужном крейте.** Экземпляры
   — выше (ADR-0083: «types stay low, instances move up»).
5. **Необязательный сервис — пакет через шов**, не метод Stable-трейта.
6. **Owner-local по send-flip:** handle, живущий на owner-потоке, — `!Send`; `Send` — только у
   действительно межпоточного (колбэки окна до ADR-0082 §4 шаг 2, `Storage`, `Clipboard`).
7. **Отсутствие — значение.** `Unsupported { reason }`, не паника, не `None` без причины, не
   `Ok(None)`.

## 3. Целевая карта крейтов

| Крейт | Tier / kind | Layer | Содержит | Не содержит |
|---|---|---|---|---|
| **`flui-platform`** (сейчас `flui-platform-api`) | C/1, `stable` | 1 | `window` (PlatformWindow, WindowId, WindowOptions, WindowAppearance, WindowExecutionState, CursorError), `input` (свой словарь указателя/клавиатуры, ADR-0089 §4), `ime`, `text_store` (трейты и значения), `clipboard`, `data_transfer` (словарь), `storage`, `haptics`, `lifecycle` (AppLifecycleState), `preferences` (SystemPreferences), `locale`, `target_platform`, `capability` (шов) | OS-код, тестовые двойники, backend-таблицы, `ui-events`/`keyboard-types`/`dpi`, реализация lock/ledger-машинерии |
| **`flui-native`** (сейчас `flui-platform`) | H/1, `internal` | 3 | `Platform`, `OwnerPlatform`/`SharedPlatform`/`PlatformProxy`, `HostWindow`, бэкенды `windows`, `macos`, `ios`, `android`, `winit` (Linux), `web`, `headless`; файловое хранилище; кросс-ОС правила маппинга (бывший `shared/`, разложенный по смыслу) | контракты, нужные выше; `LinuxPlatform`-заглушку; `window.rs`; `BackgroundExecutor`; публичные OS-типы |
| `flui-semantics` | S/5 | 3 | без изменений, кроме удаления `AccessibilityFeatures`; `PlatformAccessibility` остаётся до своего словаря дерева (вне объёма) | — |
| `flui-scheduler` | S/2 | 2 | реэкспорт `AppLifecycleState` из контракта (путь не меняется); новое ребро S→C | определение типа |
| `flui-interaction` | S/4 | 2 | `GestureSettings` как конфиг распознавателей, строится из `SystemPreferences::gestures` | константы как «системные» значения |
| `flui-runtime` | K/4 | 6 | `CapabilityRegistry`, `CapabilityRegistrar`, `Plugin`; доставка `SystemPreferences` в `MediaQuery` и в привязки жестов/анимаций | — |
| `flui-view` | K/1 | 5 | `LifecycleContextExt::capability::<C>()` + скрытый `capability_erased` | `clipboard_handle` (удаляется), позже `storage` |
| `flui-testing` | K/6 | 6 | headless-провайдеры встроенных возможностей, `MemoryStorage`, `InMemoryClipboard`, `InMemoryTextStore` (переезжают из контракта) | — |
| `flui-app` | H/2 | 9 | единственный, кто называет `flui-native`; регистрирует встроенные провайдеры; `Application::plugin`/`capability` | — |

Третьего платформенного крейта нет: ни `-core`, ни крейта на ОС (ADR-0082 отверг per-backend
крейты; рыночная норма — §7 — один контракт + бэкенды, а у winit разделение пришло только
вместе с внешними бэкендами, которых у FLUI нет).

## 4. Таблица ответственности

Колонки: понятие → категория → ядро или пакет → сейчас → цель → почему. «C» = контрактный крейт,
«N» = бэкенд-крейт.

### 4.1 Окно и дисплей

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| `PlatformWindow` | контракт окна | ядро | C `platform_window.rs` | C, без изменений формы; `display()`, `window_bounds()`, `set_background_appearance()`, `mouse_position()`, `is_hovered()` → `HostWindow` (N) | методы без потребителя выше бэкенда не должны быть Stable-обещанием |
| `HostWindow` | host-подтрейт | ядро (только host) | N `traits/host_window.rs` | N `host_window` | держит `accessibility()` (accesskit) и `text_store_host` — host-only |
| Мониторы `PlatformDisplay`, `DisplayId` | словарь дисплея | ядро (host) | C, потребитель только N | N | нет потребителя выше бэкенда; вернётся в C с первым (полноэкранный выбор монитора в API приложения) |
| DPR, размер | значение окна | ядро | C `scale_factor`/`on_resize`, проведено | C, без изменений | работает |
| Refresh rate | значение окна | ядро | C `refresh_period`, проведено в pacing | C, без изменений | работает |
| Safe area / insets | значение окна | ядро | C `safe_area_insets`, проведено только iOS | C; провести на Android/web; `view_insets` (клавиатура) — метод C, когда появится производитель | `MediaQueryData.padding/view_insets` есть, производителя нет |
| Курсор | команда окна | ядро, framework-routed | C `set_cursor(CursorIcon)` | C, без изменений | ADR-0089 §2 разрешает `cursor-icon`; виджетам не возможность, а `MouseRegion` |
| Системный chrome (заголовок, полноэкранный режим) | команда окна | ядро | C `set_title`, `toggle_fullscreen` | C | десктоп-база |
| Системный chrome мобильный (status bar, Mica, vibrancy, liquid glass) | стиль ОС | пакет | N `window_ext`, C `WindowBackgroundAppearance::Mica*` | пакет через шов; `Mica*`/`Vibrant*` уходят из C | ОС-специфичные варианты не должны быть в Stable enum |
| `WindowMode`, `WindowEvent`, `WindowBounds` | backend-состояние | — | C, потребитель только N | N | то же |

### 4.2 Ввод

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| Словарь указателя (`PointerEvent`, `PointerId`, `PointerKind`, `ScrollDelta`, фазы жестов) | словарь | ядро | C реэкспорт `ui-events` | C, свой тип (ADR-0089 §4) | ведёт interaction pointer-vocabulary P1; эта спека только фиксирует место |
| Словарь клавиатуры (`KeyEvent`, `Key`, `NamedKey`, `Code`, `Modifiers`) | словарь | ядро | C реэкспорт `keyboard-types` | C, свой тип | то же ADR; focus-keyboard пока живёт на ui-events — переход после P1 |
| Push-IME `ImeEvent`, `PlatformTextInput` | контракт | ядро | C | C, без изменений | ADR-0030/0090 |
| Pull-store `TextStore`, `TextStoreHost`, значения | контракт | ядро | C `text_store` | C | ADR-0090/0135/0142 |
| Машинерия store: `LockArbiter`, `OwnerCalls`, `CompositionLedger`, `EditGeneration`, `project_ime_event`, `commit_composition_in_place` | реализация | ядро | C (~2 000 строк, с `tracing`) | решается после text-ime T6 (см. Q5): вариант A — `flui-interaction` (S), Win32 зовёт через N→S; вариант B — остаётся в C за `#[doc(hidden)]`-модулем | это политика, а не словарь; но text-ime активно её меняет |
| `InMemoryTextStore` | тестовый двойник | — | C | `flui-testing` | двойник не Stable |
| Drag-and-drop: словарь `DataTransferOffer`, `TransferFormat`, … | словарь | ядро | C | C | нужен ядру для DnD-виджетов |
| DnD: `OfferTable`, `OfferRecord` | backend-слаб | — | C | N | реализация winit |
| `device_to_logical`, `logical_to_device`, `offset_from_coords` | хелперы | — | C (Win32-only / мёртвые) | N или удалить | backend-код |

### 4.3 Состояние системы

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| `AppLifecycleState` (тип) | словарь | ядро | `flui-scheduler/src/frame.rs:230` | C `lifecycle`; scheduler реэкспортирует | общий тип производителя (host, `PlatformToUi::Lifecycle`) и потребителя (scheduler); ADR-0082 §1 уже называет `WindowExecutionState` «машиной ADR-0035» — оба типа в одном модуле |
| Lifecycle: источник событий | событие | ядро | N per-window (focus, visibility, execution) + host seed | N per-window + app-level сигнал ОС (фон/передний, minimize, `visibilitychange`) — новый колбэк в C, когда бэкенд его даёт | ADR-0035 «Not implemented» |
| Lifecycle: агрегат | вычисление | ядро | `flui-runtime/src/lifecycle_state.rs` | без изменений | агрегат — решение фреймворка, не ОС |
| Session end | событие | ядро | N `SessionEndPhase/Answer`, `on_session_end` | N (teardown владеет; тип internal к host по её решению) | teardown: «session-end types are internal» |
| Memory pressure | событие | ядро | нет производителя; `WidgetsBinding::handle_memory_pressure` без вызова | C колбэк `Platform`-уровня, когда появится потребитель (кэш изображений) | после 0.2 |
| Энергосбережение | настройка | ядро | нет | поле `SystemPreferences`, когда scheduler `LowPower` начнёт его читать | YAGNI до потребителя |
| Тема (`Brightness`) | настройка | ядро | C + `appearance()`/`on_appearance_changed`, проведено | поле `SystemPreferences` | один источник |
| Контраст | настройка | ядро | мёртвое поле в `AccessibilityFeatures` | поле `SystemPreferences` | material/cupertino ждут `high_contrast` |
| Масштаб текста | настройка | ядро | `MediaQueryData.text_scale_factor` = 1.0 всегда | поле `SystemPreferences` → `MediaQuery` | нет производителя |
| «Меньше движения», bold text, invert | настройка | ядро | мёртвые поля `AccessibilityFeatures` | поля `SystemPreferences`; `AccessibilityFeatures` удаляется | animation планирует `SystemMotion` (ADR-0146) — сводим, Q2 |
| Локаль (список предпочтений) | настройка | ядро | тип `Locale` в C; списка нет | поле `SystemPreferences` | `WidgetsApp::resolve_locale` ждёт список |
| Системные параметры жестов (double-click time, drag threshold, long press) | настройка | ядро | константы `flui-interaction/src/settings.rs` | поле `SystemPreferences::gestures` (`GestureTimings`), `GestureSettings` строится из него | interaction X1 планирует `GestureSettingsSource` — сводим, Q2 |
| Доставка настроек виджету | транспорт | ядро | `MediaQuerySource` для DPR/brightness/padding | `MediaQuerySource` для всех полей + прямая подача в `GestureBinding` и в анимации | push-значения — inherited data, не capability (ADR-0084 их не покрывает) |
| `PlatformAccessibility` | контракт a11y-моста | ядро | `flui-semantics/src/platform.rs:64` | без изменений (вне объёма) | называет `accesskit::TreeUpdate`, в C нельзя до своего словаря |

### 4.4 Данные

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| Clipboard (трейт) | контракт | ядро, встроенная возможность | C `Clipboard`, `Send + Sync` | C | ADR-0039 |
| Clipboard (доставка) | транспорт | ядро | named field: `RealmHostServices` → … → `LifecycleContext::clipboard_handle` | шов: `cx.capability::<dyn Clipboard>()`, встроенный провайдер в `flui-app`, headless в `flui-testing` | первый вертикальный срез |
| `InMemoryClipboard` | двойник | — | C | `flui-testing` (headless-бэкенд N держит свой) | двойник не Stable |
| Storage (трейт) | контракт | ядро | C `Storage` (ADR-0133, persistence) | C, форма не меняется | persistence объявила её стабильной |
| Storage (доставка) | транспорт | ядро | `LifecycleContext::storage()`; `host_storage()` всегда `None` | после слияния persistence — встроенная возможность через шов; `LifecycleContext::storage` удаляется | один вход для платформенных возможностей |
| `FileStore` | backend | ядро | N `storage/`, без `impl Storage` | N, `impl Storage` (persistence) | — |
| URL наружу (`open_url`, `reveal_path`) | сервис | пакет | N `Platform::open_url` (Win32 real) | пакет `launcher` через шов | необязательно для ядра |
| Deep links внутрь | событие | ядро | N `on_open_urls`, без потребителя | C колбэк, когда router начнёт принимать; не раньше | YAGNI |

### 4.5 Шов и необязательные сервисы

| Понятие | Ядро / пакет | Цель |
|---|---|---|
| Шов (`Capability`, `CapabilityProvider`, `Unsupported`, `UnsupportedReason`) | ядро | C `capability` (ADR-0152) |
| Реестр, `Plugin`, `CapabilityRegistrar` | ядро | `flui-runtime`, реэкспорт `flui-sdk` |
| Разрешения (`PermissionState`, запрос, отзыв) | ядро-словарь, только с первым потребителем | C `permission` — вместе с первым пакетом, которому он нужен (R4.2) |
| Haptics | пакет | `PlatformHaptics` уходит из `PlatformWindow` в пакет `haptics` через шов; мёртвые forwarders в runtime удаляются |
| Файловые диалоги | пакет | N `prompt_for_paths` → пакет `dialogs`; `Task`/`BackgroundExecutor` на tokio удаляются, результат — свой future |
| Геолокация, сенсоры, камера, биометрия, connectivity, батарея, уведомления, share, защищённое хранилище, трей/меню | пакеты | каждый — отдельный пакет `packages/<name>` (или вне репозитория) на `flui-sdk` + C; ни одного до 0.2 |

## 5. Шов возможностей (ADR-0152 вместо ADR-0084)

ADR-0084 остаётся основой; изменения:

1. **Имена без заикания.** В крейте `flui-platform` — `flui_platform::Capability`, не
   `PlatformCapability` (N7 спеки naming).
2. **Handle owner-local.** `Capability::Handle: Clone + 'static`, без `Send`; реестр на `Rc`
   (send-flip, ADR-0136). Если возможность сама межпоточная (clipboard), handle — `Arc<dyn _>`.
3. **Push-значения вне шва.** Системные настройки — `SystemPreferences` через inherited data;
   шов только для pull-handle'ов. ADR-0084 этого не различал.
4. **Storage классифицирован**: встроенная возможность через шов после persistence.
5. **Разрешения — забота handle'а, не шва.** Шов отвечает «есть ли возможность»; «разрешено ли» —
   методы handle'а, которым нужно разрешение, возвращают
   `Err(CapabilityError::Permission(PermissionState::Denied | DeniedPermanently | Restricted))`, а
   handle даёт `request() -> impl Future<Output = PermissionState>` и событие отзыва. Тип —
   с первым потребителем.
6. **Устаревшие ссылки** ADR-0084 (`runtime.rs:162`, `platform.rs:423`, «eleven methods»)
   исправлены в новом ADR.
7. **Остаётся как в ADR-0084:** хранение в per-realm реестре, приоритет app > встроенный >
   единственный плагин, конфликт — `AppRunError::CapabilityConflict` до окна, без
   `inventory`/`linkme`, `capability_erased(TypeId, &'static str)` скрытый и object-safe,
   blanket `LifecycleContextExt`, порядок причин `NotRegistered` → `NoWindow` → `NotOnThisPlatform`.

```rust
// flui-platform (контракт)
pub trait Capability: 'static {
    type Handle: Clone + 'static;
    const NAME: &'static str;
}

pub trait CapabilityProvider<C: Capability + ?Sized>: 'static {
    /// # Errors
    /// [`Unsupported`] when this window or platform cannot provide `C`.
    fn provide(&self, window: &Arc<dyn PlatformWindow>) -> Result<C::Handle, Unsupported>;
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{capability} is unsupported: {reason}")]
pub struct Unsupported { pub capability: &'static str, pub reason: UnsupportedReason }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedReason { NotRegistered, NoWindow, NotOnThisPlatform }

impl Capability for dyn Clipboard {
    type Handle = Arc<dyn Clipboard>;
    const NAME: &'static str = "clipboard";
}
```

Виджет: `let clipboard = cx.capability::<dyn Clipboard>()?;` в `init_state`. В `build` —
E0599 (метод на `LifecycleContextExt`, а `BuildContext` его не реализует), закреплено trybuild.

## 6. Системные настройки (`SystemPreferences`)

```rust
// flui-platform::preferences
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SystemPreferences {
    pub brightness: Brightness,
    pub contrast: Contrast,          // Standard | High, #[non_exhaustive]
    pub text_scale: TextScale,       // конечное f64 > 0, валидирующий конструктор
    pub motion: Motion,              // Full | Reduced, #[non_exhaustive]
    pub bold_text: bool,
    pub locales: Arc<[Locale]>,      // в порядке предпочтения, может быть пуст
    pub gestures: GestureTimings,    // double_tap, long_press: Duration; touch_slop, drag_threshold: f64 (logical px)
}
```

- **Производитель:** `PlatformWindow::preferences() -> SystemPreferences` и
  `on_preferences_changed(Box<dyn FnMut(SystemPreferences) + Send>)` (Send — до ADR-0082 §4
  шаг 2). `appearance()`/`on_appearance_changed` остаются для `WindowAppearance` (vibrancy и пр.),
  `Brightness` берётся из настроек. Бэкенд без ответа ОС отдаёт `SystemPreferences::default()` —
  значения задокументированы в контракте рядом с типом.
- **Потребители:** `MediaQuerySource` (всё, что видит виджет), `GestureBinding` (строит
  `GestureSettings` из `gestures` и `PointerType`), анимации (`motion`, длительность).
- **Почему один снимок, а не три шва:** у каждого бэкенда один путь «прочитать настройки ОС и
  подписаться на `WM_SETTINGCHANGE` / `NSWorkspace` / `UIContentSizeCategory` / `matchMedia`»;
  три отдельных шва (animation `SystemMotion`, interaction `GestureSettingsSource`, этот) — три
  подписки на одно событие ОС и три разных ответа на «нет значения». Поле добавляется в
  `#[non_exhaustive]`-struct без поломки. Это развилка Q2.

## 7. Рыночный эталон

См. [§9](#9-рыночный-эталон-подробно). Норма, которую берём: один контрактный крейт +
бэкенды; headless/test-бэкенд как полноправный бэкенд; unsupported — значение, и «нет на
платформе» / «не зарегистрировано» / «запрещено пользователем» различимы; разрешения —
состояние на handle, запрос асинхронный; версия плагина — свой minor при общем major SDK.

## 8. Инвентаризация (этап 1)

Сырые отчёты по пунктам 1–5 остаются вне репозитория (рабочие заметки сессии). Здесь —
выводы, которые двигают дизайн; каждая ссылка file:line проверена по `main` @ `d56188c14`.

### 8.1 `flui-platform-api`

- Зависимые: flui-app, -interaction, -platform, -widgets, -view, -sdk, -runtime, -testing, фасад
  (комментарий в `Cargo.toml:64-66` перечисляет 4 — устарел). [R]
- Upstream в сигнатурах: `ui-events` (через `input.rs:29-38`, `PlatformInput`,
  `PlatformWindow::on_input`), `keyboard-types` (`Key`, `Modifiers`, `PlatformWindow::modifiers`),
  `dpi` (позиция указателя). Разрешённые: `raw-window-handle` 0.6 traits, `cursor-icon`, `serde`. [R]
- Нулевые потребители: `offset_from_coords`, `delta_offset_from_coords`, `TransferImage`,
  `LockKind`, `DEFERRED_LOCK_CAPACITY`, `utf16_range`. [R]
- Один потребитель: `LockArbiter`, `CompositionLedger`, `EditGeneration` (widgets);
  `project_ime_event` (interaction); `TextStoreHost` (Win32); `WindowMode` и пиксельные хелперы
  (Win32); `PlatformHaptics` (fake); `Storage` (testing). [R]
- Нет ADR: `Storage` (только спека persistence / ADR-0133 на ветке), `Locale`, `Brightness`,
  `TargetPlatform`. [R]
- Owner-local: `TextStore`, `TextStoreHost`, `TextStoreObserver`, `CommitGate`, `LockArbiter`,
  `LockGrant`, `OwnerCalls`, `InMemoryTextStore` — `!Send`, закреплено `static_assertions`. [R]

### 8.2 `flui-platform`

- Бэкенды: Win32 9 927 строк (real), AppKit 8 246 (real), winit 6 066 (real, Linux production и
  единственный реальный DnD), iOS 2 489 (real), Android 1 391 (MVP: mock clipboard, inline
  executor, выдуманный дисплей), web 1 379 (partial), `linux/` 978 (`LinuxPlatform` —
  `unimplemented!()`; AT-SPI-адаптер реален и используется winit), headless 1 752. [R]
- Исполняются в CI: winit (Linux, Xvfb) и headless. Остальное — cross-typecheck. [R]
- Матрица возможностей и способ отказа — §1 и ADR-0151. [R]
- Упоминаний `flui-platform`/`flui_platform` (без `-api`): 243 файла; вне самого крейта 198
  (rs 72, toml 9, md 111, прочее 6). [R] Команда:
  `rg -l -P --hidden -g '!.git' 'flui[-_]platform(?![-_]api)'`.

### 8.3 Понятия не на своём месте

Сведено в §4. Отдельно: `ExecutionServices` (flui-runtime) — исполнитель фреймворка, не
платформа; остаётся. `BackgroundExecutor` и `Task` в бэкенде — остаток ADR-0047, удаляются.

### 8.4 Путь возможности

Сейчас — 7 именованных полей через flui-runtime и flui-view, `LifecycleContext` закрыт для
пакетов. ADR-0084 предлагает один обобщённый вход и per-realm реестр; не реализован. В
`LifecycleContext` 16 методов (ADR-0084 говорит 11): `storage`, `close_guard`, `lifecycle_handle`
добавлены позже и не классифицированы — ADR-0152 их классифицирует: `storage` → шов,
`close_guard`/`lifecycle_handle` — framework-возможности, остаются методами.

## 9. Рыночный эталон (подробно)

Прочитано по первичным источникам (upstream main через `gh api`, официальные доки) 2026-10-06.

| Проект | Контракт | Бэкенды | Unsupported | Тестовый бэкенд | Возможности / плагины |
|---|---|---|---|---|---|
| winit 0.31 | `winit-core` (`#![warn(clippy::exhaustive_enums)]`, dyn-safe трейты) | `winit-<os>` + фасад `winit` с `winit::platform::*`, все на одной версии | `RequestError::NotSupported`, `None`; ОС-специфика — `Option<&mut dyn …ExtMacOS>` | нет | — |
| raw-window-handle 0.6 | `no_std`, без зависимостей | — | `HandleError::{NotSupported, Unavailable}`, `#[non_exhaustive]` | — | interop-крейт почти не выпускается |
| Bevy | `bevy_window` (без winit, `no_std`) | `bevy_winit`; a11y — `bevy_a11y`, не реэкспортирует `accesskit` | — | `ScheduleRunnerPlugin`, `primary_window: None` | `Plugin` с `build/ready/finish`; один поезд версий |
| Masonry | `masonry_core` (без winit) | `masonry_winit` | ядро шлёт `RenderRootSignal`, «some platforms may ignore» | `TestHarness` | — |
| GPUI | `gpui` (`Platform`, `PlatformWindow: HasWindowHandle`, `PlatformDispatcher`) | `gpui_<os>`, селектор `gpui_platform` (`application()`/`headless()`) | default-методы: `false`/`None`/`Err(anyhow)`/no-op | `TestPlatform` (детерминированный dispatcher; местами `unimplemented!()`) | — |
| Slint | `Platform` (обязателен только `create_window_adapter`) | `i-slint-backend-*` + selector (feature или `SLINT_BACKEND`) | `PlatformError::{NoPlatform, Unsupported, …}`, `#[non_exhaustive]` | `TestingBackend { mock_time }` | upstream-типы только за `unstable-winit-030`-фичами |
| Tauri v2 | `tauri-runtime` | `tauri-runtime-wry` | плагины объявляют `support level`; геолокация на десктопе отдаёт нулевую позицию — антипаттерн | `MockRuntime` | плагин = desktop.rs + mobile.rs; разрешения `plugin:allow-cmd`, capability-файлы на окна (build time); `PermissionState {Granted, Denied, Prompt, PromptWithRationale}` |
| Flutter | `<x>_platform_interface` | `<x>_<os>`, `implements:`, `default_package` | `MissingPluginException`, `UnimplementedError` | mock через `MockPlatformInterfaceMixin` | интерфейс только растёт методами с default; `permission_handler`: `denied, granted, restricted, limited, permanentlyDenied, provisional` + отдельно `ServiceStatus` |
| Compose MP | `expect` | `actual` на каждый target, проверка компилятором | — | — | сами Kotlin-доки советуют интерфейс + фабрику ради фейков |
| SwiftUI | `EnvironmentValues` | система | — | подмена `openURL` и пр. в окружении | `accessibilityReduceMotion { get }`, `dynamicTypeSize`, `scenePhase` — система пишет, view читает |

ОС: Android после двух отказов (API 30+) больше не показывает диалог; «только сейчас» —
одноразово; отзыв в настройках **убивает процесс**; проверка статуса не отличает «не спрашивали»
от «запрещено навсегда» — это видно только по запросу. iOS: `notDetermined/restricted/denied/
authorizedWhenInUse/authorizedAlways`; смена Camera/Photos/Contacts в настройках — SIGKILL
(Apple developer forums, thread 64740; в API-доках не описано) [не проверено запуском].

**Что из этого норма и что берём:**

- **Разрешения:** состояние на возможность, запрос асинхронный, глобального гранта нет. Ядро
  состояний — Granted / Denied / NotDetermined (Prompt); сверху — Restricted (ОС/родительский
  контроль), DeniedPermanently (известно только после запроса), частичные гранты (`Limited`).
  «Сервис включён» — отдельная ось. Отзыв на мобильных обычно убивает процесс, поэтому событие
  отзыва на handle — best-effort (десктоп, геолокация), а не гарантия. → R4 и §5 п.5;
  `#[non_exhaustive]`.
- **Unsupported — значение**, и три случая различимы: «нет на этой платформе», «не
  зарегистрировано», «пользователь запретил». Фейковые данные, `unimplemented!()` и исключения —
  антипаттерны (Tauri geolocation, GPUI `TestPlatform`, Flutter). → `UnsupportedReason` +
  отдельный `PermissionState`.
- **Тестовый бэкенд — полноправный**, с управляемым временем и скриптуемыми ответами (GPUI,
  Slint, Masonry, Tauri). → headless-провайдеры в `flui-testing`.
- **Версии:** ядро и бэкенды — один поезд; плагин — свой minor при общем major; интерфейс
  растёт методами с default; upstream-churn — за фичами с номером версии. → совпадает с
  ADR-0088 (SDK `0.N`, train guard) и ADR-0089 §5.
- **Имена:** контракт чаще всего `-core` (winit, masonry, slint) или голое имя (`gpui`,
  `bevy_window`); бэкенды — по ОС/технологии. `-api`, `-host`, `-shell`, `-native`, `-port`
  для этого слоя не использует никто; `-sys` по конвенции Rust — сырой FFI. `core` запрещён N1
  спеки naming, поэтому для FLUI ближайшее к норме — голое предметное имя контракта
  (`flui-platform`, как `bevy_window`) и имя бэкендов по роли.

## 10. Имена

Требование владельца: `flui-<слово>`. `flui-platform-api` ему не отвечает, значит
переименование контракта неизбежно. Варианты:

| Вариант | Контракт | Бэкенды | Код пользователя и пакета | Плюсы | Минусы |
|---|---|---|---|---|---|
| **A (рекомендую)** | `flui-platform` | `flui-native` | `flui::platform::Clipboard` (как сейчас в фасаде); пакет: `use flui_platform::{Capability, Clipboard};` | имя крейта = путь фасада `flui::platform`, который пользователь уже пишет; `flui_platform::Capability` без заикания; «native» говорит «код ОС»; только `flui-app` видит `flui_native` | имя `flui-platform` меняет смысл (бэкенд → контракт): открытые ветки и 198 файлов истории читаются иначе; web и headless — не совсем «native» |
| B | `flui-platform` | `flui-os` | то же | короче | «os» для web/headless ещё хуже; `flui_os::platforms::windows` — шум; слово в два символа плохо ищется |
| C | `flui-platform` | `flui-backend` | то же | точное слово для роли | «backend» в FLUI уже значит GPU-бэкенд wgpu и `TextInputBackend` — коллизия терминов |
| D | `flui-port` / `flui-contract` | `flui-platform` | `flui::platform::…` ≠ `flui_port::…` | бэкенд не переименовывается, меньше churn | путь фасада и крейта расходятся; «port» читается как «портирование» |
| E | `flui-platform` | `flui-shell` | то же | так называет это Flutter (embedder/shell) | «shell» в FLUI нигде не используется, а у читателя — командная оболочка |

crates.io (проверено 2026-10-06, API `crates.io/api/v1/crates/<name>`): свободны `flui`,
`flui-platform`, `flui-platform-api`, `flui-native`, `flui-os`, `flui-backend`, `flui-shell`,
`flui-port`, `flui-host`, `flui-system` и остальные проверенные; `flui-cli` 0.1.0 уже
опубликован владельцем (`vanyastaff`). Чужих `flui-*` нет. docs.rs: имя крейта = заголовок
страницы и корень путей; у A путь в доках `flui_platform::Clipboard` совпадает с тем, что
пользователь пишет через фасад. Совет: после утверждения занять оба имени публикацией
placeholder `0.0.0` (ваше решение, outward-facing).

Механика A (один PR, скрипт): сначала `flui_platform` → `flui_native` и `crates/flui-platform/` →
`crates/flui-native/`, затем `flui_platform_api` → `flui_platform` и `crates/flui-platform-api/`
→ `crates/flui-platform/`. Порядок важен: обратный склеит оба крейта. Плюс имена в xtask
(`globals.rs` `PLATFORM`/`BACKENDS`, `tiers.rs` `SDK_SURFACE`), `allowed-dependents`,
allowlists send-flip (thread-boundary, unsafe-impl ключуются именем крейта), `deny.toml`,
`docs/crates.md`, пути в живых доках и ADR (только пути; решения старых ADR не правятся,
карта имён — в ADR-0151). Архивные корни (`docs/research`, `docs/plans`) не переписываются.
Окно — вместе с массовым переименованием спеки naming (11-10…11-14), до publish-конвейера
(12-01). Инструкция для открытых веток — в tasks.md.

## 11. Вопросы к владельцу

По одному на развилку; работа идёт дальше по рекомендации.

- **Q1. Имя.** A (`flui-platform` + `flui-native`) — рекомендую; или B/C/D/E.
- **Q2. Системные настройки.** Один `SystemPreferences` в контракте, которым пользуются и
  animation (вместо `SystemMotion`/ADR-0146), и interaction (вместо `GestureSettingsSource`) —
  рекомендую; или три независимых шва.
- **Q3. `AppLifecycleState`.** Перенести тип в контракт (scheduler реэкспортирует), окно — после
  слияния ядра send-flip — рекомендую; или оставить в scheduler, а контракт отдаёт только
  per-window факты.
- **Q4. ADR-0084.** Заменить новым ADR-0152 (Supersedes) с изменениями §5 — рекомендую; или
  принять ADR-0084 как есть и поправить его отдельным ADR позже.
- **Q5. Машинерия text store** (`LockArbiter`, `OwnerCalls`, `CompositionLedger`, …) после
  text-ime T6: вынести из Stable в `flui-interaction` — рекомендую; или оставить в контракте за
  `#[doc(hidden)]`.
- **Q6. Номера ADR.** 0151–0153 — записать резерв в реестр `docs/plans/specs/release/tasks.md`
  на `plans/specs-next` (чужая активная ветка — нужно ваше «да»).
