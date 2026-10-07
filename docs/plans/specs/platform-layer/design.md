# Платформенный слой — дизайн

- **Статус:** черновик, редакция 2 (после ревью), на утверждение владельцу
- **Дата:** 2026-10-06
- **База:** `main` @ `d56188c14`
- **Требования:** [requirements.md](requirements.md)
- **ADR (Proposed):**
  [ADR-0151](../../../adr/ADR-0151-platform-layer-boundary-and-names.md) — граница, имена,
  lifecycle, системные настройки;
  [ADR-0152](../../../adr/ADR-0152-capability-seam-revised.md) — шов возможностей (вместо
  ADR-0084);
  [ADR-0153](../../../adr/ADR-0153-release-train-and-capability-crates.md) — поезд релизов и
  крейты возможностей: открытый вопрос к 0.3 с опытами;
  [ADR-0154](../../../adr/ADR-0154-capability-crates.md) — крейты возможностей

Обозначения: **[R]** прочитано в коде, **[C]** собрано, **[X]** запущено, **[I]** вывод,
**[—]** недоступно на этом хосте.

## 1. Что есть сейчас

| Факт | Источник |
|---|---|
| `flui-platform-api`: C/1, layer 1, `stable`, `reach-forbid = [accesskit, tokio]`; фасад делает `pub use flui_platform_api as platform` | [R] `crates/flui-platform-api/Cargo.toml:69-74`, корневой `src/lib.rs:183` |
| Контракт зависит от `flui-foundation = "=0.2.0-dev"`, а у foundation train guard `links = "flui_train"` | [R] `crates/flui-platform-api/Cargo.toml:27`; ADR-0088 |
| В Stable-сигнатурах — типы `ui-events` 0.3 (`PlatformInput::Pointer(PointerEvent)`, `PlatformWindow::modifiers`), а через них `keyboard-types` и `dpi`. ADR-0089 (Proposed) это запрещает; манифест называет это «ADR-0089 debt» | [R] `flui-platform-api/src/input.rs:29-38,108-110`, `platform_window.rs:288` |
| ~2 000 строк политики text store в контракте (`LockArbiter`, `OwnerCalls`, `CompositionLedger`, `EditGeneration`, `project_ime_event`); тестовые двойники `InMemoryClipboard`, `InMemoryTextStore` (730 строк, 637 без тестов); backend-слаб `OfferTable` | [R] `text_store/*`, `clipboard.rs:23`, `data_transfer.rs` |
| Без потребителя выше бэкенда: `PlatformDisplay`, `WindowBounds`, `WindowMode`, `WindowEvent`, словарь data transfer, кроме `DragDropEvent` и `DataTransferId` (их видит runtime через `PlatformInput`), методы `display`, `window_bounds`, `set_background_appearance`, `mouse_position`, `is_hovered`. `PlatformHaptics` реализован только headless-фейком; `Storage` — только `MemoryStorage` в `flui-testing` | [R] |
| `flui-platform`: H/1, layer 3, `allowed-dependents = [flui-app]`, 97 файлов / 40 717 строк src; в production его называет только `flui-app` (19 файлов src) | [R] |
| В бэкенде: словарь без потребителя выше (`PlatformCapabilities`, `PathPromptOptions`, `SessionEndPhase/Answer`); `LinuxPlatform` — `unimplemented!()` во всех методах, кроме `name` и `data_transfer`; наследный `window.rs` с сырым `RawWindowHandle`; публичные OS-типы (`win32::HWND`, `NSApplication`, `AndroidApp`, `HtmlCanvasElement`, `tokio::runtime::Handle`, `accesskit::TreeUpdate`) | [R] |
| `BackgroundExecutor` и `Task` живые: их используют Win32, macOS, winit; файловые диалоги Win32 возвращают `Task` (ADR-0039 §2) | [R] `windows/platform.rs:660,831,2444-2538`, `macos/platform.rs:49,135`, `winit/platform.rs:228,353` |
| «Нет возможности» сообщается по-разному: `Option::None`, `CursorError::Unsupported`, `Ok(None)` у диалога по умолчанию (читается как отмена), no-op у `open_url`, выдуманный дисплей на Android, паника `LinuxPlatform` | [R] |
| `AppLifecycleState` — в `flui-scheduler` (ADR-0035); производитель — `flui-app`. Пробелы ADR-0035: видимость native-Windows, minimize Windows, web `visibilitychange`, транспорт pause/resume Android, `onExitRequested` | [R] `flui-scheduler/src/frame.rs:230`; ADR-0035:102-108 |
| `AccessibilityFeatures` никто не пишет и не читает; `text_scale_factor` всегда 1.0; локаль системы не доставляется; `GestureSettings` — константы, `for_platform`/`native()` не вызываются | [R] `flui-app/src/app/runtime.rs:92-97`, `flui-widgets/src/app/media_query.rs:60`, `flui-interaction/src/settings.rs` |
| Возможность до виджета: `RealmHostServices` → `RealmServices` → `PresentationState` → `BuildOwner` → `BuildCapabilities` → `LifecycleContext`, производитель в `flui-app`; у `LifecycleContext` 16 методов (15 публичных); пакет добавить возможность не может; ADR-0084 — 0 строк кода | [R] `build_context.rs:369-631` |
| `ClipboardHandle` уже owner-local (`PhantomData<Rc<()>>`) и читает через колбэк (под асинхронный транспорт, ADR-0038 §6); единственный production-потребитель — `EditableText` | [R] `flui-interaction/src/clipboard.rs:1-24`, `editable_text.rs:1384` |
| В CI исполняются только Linux/headless; Win32, AppKit, iOS, Android — clippy cross-typecheck; Windows-job из комментария `ci.yml:405` не существует | [R] |

## 2. Правила решения

1. **Контракт — словарь и трейты, которыми пользуется кто-то выше бэкенда.** Без OS-кода,
   `unsafe`, upstream-типов (ADR-0089 §1–§2, принят для контракта ADR-0151), тестовых двойников.
   Элемент без потребителя выше бэкенда в контракт не входит; он возвращается с первым
   потребителем.
2. **Геометрия — значения `flui_foundation::geometry`** (ADR-0098); контракт зависит от
   `flui-foundation`, как сейчас. Как крейт возможности со своей версией переживает релизы
   FLUI — вопрос ADR-0153 к 0.3; разделение, если понадобится, — узкой поправкой к ADR-0098, не
   отдельным крейтом типов.
3. **OS-код фреймворка — в ядре-хосте, его список закрыт** (ADR-0151 §3). OS-код
   необязательного сервиса — в крейте возможности (ADR-0154).
4. **Источник системной настройки — хост, представление — у потребителя.** Один производитель
   на хост; потребитель строит своё и не тащит снимок ОС в свою логику; политика приложения —
   во фреймворке.
5. **Общий тип — в самом нижнем нужном крейте**; экземпляры — выше (ADR-0083).
6. **Owner-local по send-flip:** handle owner-потока — `!Send`; `Send` — только у
   действительно межпоточного (колбэки `Platform` и окна до ADR-0082 §4 шаг 2, `Storage`,
   backend-трейт `Clipboard`).
7. **Отсутствие — значение.** `Unsupported { reason }`, не паника, не `None` без причины, не
   `Ok(None)`, не выдуманные данные.
8. **На `main` нет поверхности без потребителя.** Новая поверхность сливается вместе с первым
   production-потребителем.

## 3. Целевая карта крейтов (1.0)

| Крейт | Класс / tier / kind | Layer | Содержит | Не содержит |
|---|---|---|---|---|
| **`flui-platform`** (сейчас `flui-platform-api`) | контракт, C/1, `stable` | 1 | `window`, `input` (свой словарь указателя/клавиатуры), `ime`, `text_store` (трейты и значения), `clipboard`, `storage`, `locale`, `target_platform`, `lifecycle`, `preferences`, `capability`; с 0.3 — мост к хосту и разрешения | OS-код, двойники, backend-таблицы, haptics, data transfer (кроме `DragDropEvent`, `DataTransferId`), upstream-типы, `parking_lot`/`tracing` после ухода нуждающихся |
| **`flui-native`** (сейчас `flui-platform`) | ядро-хост, H/1, `internal` | 3 | `Platform`, `OwnerPlatform`, `SharedPlatform`, `PlatformProxy`, `HostWindow`, бэкенды `windows`, `macos`, `ios`, `android`, `winit`, `web`, `headless`; файловое хранилище; AT-SPI-адаптер; правила маппинга по смыслу | сервисы вне закрытого списка; заглушки; публичные OS-типы |
| **`flui-location`, `flui-sensors`, `flui-media`, `flui-notify`, `flui-vault`, `flui-device`, `flui-system`…** | возможности, pkg, `capability` (ADR-0154) | 7 | один набор разрешений ОС: тип возможности, handle, OS-бэкенды под `cfg`, симулирующий провайдер, conformance-таблица, декларации | зависимостей друг от друга |
| `flui-semantics` | S/5 | 3 | без изменений, кроме удаления `AccessibilityFeatures` | — |
| `flui-scheduler` | S/2 | 2 | реэкспорт `AppLifecycleState` из контракта; политика кадров (`should_render`, `should_animate`) — своим extension-трейтом | определение типа |
| `flui-interaction` | S/4 | 2 | `GestureSettings` строится из `SystemPreferences`; `ClipboardCapability` + `ClipboardHandle` | константы как «системные» значения |
| `flui-animation` | S/6 | 3 | своя политика движения, построенная из `SystemPreferences::motion` и политики приложения | производитель системного сигнала |
| `flui-runtime` | K/4 | 6 | `CapabilityRegistry`, `CapabilityRegistrar`, `Plugin`; доставка `SystemPreferences` в realm; политика приложения поверх ОС | — |
| `flui-view` | K/1 | 5 | `LifecycleContextExt::capability::<C>()` + скрытый `capability_erased` | `clipboard_handle`, позже `storage` |
| `flui-testing` | K/6 | 6 | headless-провайдеры встроенных возможностей, `MemoryStorage`, `InMemoryClipboard`, `InMemoryTextStore` | — |
| `flui-app` | H/2 | 9 | единственный, кто называет `flui-native`; встроенные провайдеры; `Application::plugin`/`capability`; рассылка `SystemPreferences` в realm | — |
| `flui-cli` | H/3 | 9 | с 0.3 — генерация манифестов ОС из деклараций крейтов возможностей | — |

**Почему это не «проблема множества крейтов».** Ядро не дробится: платформа фреймворка остаётся
двумя крейтами, геометрия остаётся в `flui-foundation` (ADR-0098). Крейты возможностей —
листья: от них никто не зависит, друг о друге они не знают, пользователь компилирует только
добавленные, а CI при изменении пересобирает один лист. Боль множества крейтов — это сцепка
версий: опыты ADR-0153 показывают, что крейт возможности ломает несовместимая смена версии
контракта, а не геометрия; решение — до первого крейта возможности. Гранулярность — один крейт на набор разрешений и темп релизов (к 1.0
около 8–10), не на API.

## 4. Таблица ответственности

«C» = контракт, «N» = ядро-хост, «K» = крейт возможности.

### 4.1 Окно и дисплей

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| `PlatformWindow` | контракт окна | ядро | C | C; `display`, `window_bounds`, `set_background_appearance`, `mouse_position`, `is_hovered` → `HostWindow` (N) | нет потребителя выше бэкенда |
| `HostWindow` | host-подтрейт | ядро | N | N | держит `accessibility()` (accesskit) и `text_store_host` |
| Мониторы `PlatformDisplay`, `DisplayId` | словарь | ядро (host) | C, потребитель только N | N; в C — с первым потребителем | правило 1 |
| DPR, размер, refresh rate | значение окна | ядро | C, проведено | C | работает |
| Safe area / insets | значение окна | ядро | C, проведено только iOS | C; провести Android, web; `view_insets` — с производителем | `MediaQueryData.padding/view_insets` без производителя |
| Курсор | команда окна | ядро | C `set_cursor(CursorIcon)` | C | ADR-0089 §2 разрешает `cursor-icon` |
| Заголовок, полноэкранный режим | команда окна | ядро | C | C | десктоп-база |
| `WindowAppearance` (тема окна, `Vibrant*`) | значение окна | ядро | C | C | окно может переопределять тему системы |
| `WindowBackgroundAppearance` (`Mica*`), `WindowMode`, `WindowEvent`, `WindowBounds` | backend | — | C | N | только бэкенд; Windows-only варианты |
| Мобильный chrome (status bar), liquid glass | стиль ОС | возможность | N `window_ext` | K | вне закрытого списка |

### 4.2 Ввод

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| Указатель (`PointerEvent`, `PointerId`, `PointerKind`, `ScrollDelta`, фазы) | словарь | ядро | C, реэкспорт `ui-events` | C, свой тип | ADR-0089 §4; владелец — interaction pointer-vocabulary |
| Клавиатура (`KeyEvent`, `Key`, `NamedKey`, `Code`, `Modifiers`) | словарь | ядро | C, через `ui-events`/`keyboard-types` | C, свой тип | то же; focus-keyboard переходит после |
| `ImeEvent`, `PlatformTextInput` | контракт | ядро | C | C | ADR-0030/0090 |
| `TextStore`, `TextStoreHost`, `TextStoreObserver`, значения | контракт | ядро | C | C | ADR-0090/0135/0142 |
| Машинерия text store | реализация | ядро | C | после text-ime T6 (Q5): `flui-interaction` (заменяет часть ADR-0142) или C за `#[doc(hidden)]` | политика, не словарь |
| `InMemoryTextStore` | двойник | — | C | `flui-testing` | двойник не Stable |
| Drag-and-drop: `DragDropEvent`, `DataTransferId` | словарь ввода | ядро | C | C | runtime получает их через `PlatformInput` |
| Drag-and-drop: offers, форматы, запросы, `DataTransferSource`, `OfferTable`, `ClaimSlot` | словарь + backend | ядро | C | N; словарь — в C с первым DnD-виджетом | правило 1 |
| Пиксельные хелперы, `offset_from_coords`, `delta_offset_from_coords` | хелперы | — | C | N или удалить | backend-код / мёртвые |

### 4.3 Состояние системы

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| `AppLifecycleState` (тип) | словарь | ядро | `flui-scheduler` | C `lifecycle`, `#[non_exhaustive]`; scheduler реэкспортирует и держит политику своим трейтом | бэкенды будут производить недостающие сигналы ADR-0035 |
| Lifecycle: факты окна и агрегат | событие / вычисление | ядро | N → `flui-app` → `flui-runtime` | без изменений | агрегат — решение фреймворка |
| Session end | событие | ядро | N | N (teardown: internal к хосту) | спека teardown |
| `SystemPreferences` | настройки ОС | ядро | нет | C тип; N производитель на хост; realm — доставка | §6 |
| Тема (`Brightness`) | настройка | ядро | C + `appearance()` окна | `WindowAppearance` окна | окно переопределяет систему |
| Контраст, bold text, масштаб текста, локали | настройки | ядро | мёртвые поля / нет | поля `SystemPreferences` | один источник |
| Reduce motion, масштаб длительностей | настройка | ядро | мёртвые поля `AccessibilityFeatures` | `SystemPreferences::motion` (`NoPreference`, `Reduce`, `Scaled(DurationScale)`) | заменяет per-window `SystemMotion` |
| Системные параметры жестов | настройка | ядро | константы | `SystemPreferences::gestures` | заменяет `GestureSettingsSource` |
| Политика приложения поверх ОС | политика | ядро | нет | realm (`flui-runtime`), например motion «как в системе / всегда / никогда» | не дело контракта |
| Memory pressure | событие | ядро | нет производителя | C колбэк хоста с первым потребителем (кэш изображений) | после 0.2 |
| Энергосбережение, батарея | настройка / сервис | возможность | нет | K `flui-device` | вне закрытого списка |
| `PlatformAccessibility` | a11y-мост | ядро | `flui-semantics` | без изменений (вне объёма) | называет `accesskit::TreeUpdate` |

### 4.4 Данные

| Понятие | Категория | Ядро / пакет | Сейчас | Цель | Почему |
|---|---|---|---|---|---|
| Clipboard (backend-трейт) | контракт | ядро | C `Clipboard`, `Send + Sync` | C | ADR-0039 |
| Clipboard (доставка) | транспорт | ядро | именованные поля → `clipboard_handle` | шов: `cx.capability::<ClipboardCapability>()` → `ClipboardHandle` | первый потребитель шва |
| `InMemoryClipboard` | двойник | — | C (headless, runtime `test-support`) | `flui-testing`; runtime `test_clipboard` — своя замена под `test-support` | двойник не Stable |
| Storage (трейт) | контракт | ядро | C (persistence) | C, форма не меняется | спека persistence |
| Storage (доставка) | транспорт | ядро | `LifecycleContext::storage()`; `host_storage()` всегда `None` | после persistence — встроенная возможность без окна | один вход |
| URL наружу, share, файловые диалоги, пути | сервисы | возможность | N `Platform::open_url`, `prompt_for_paths` (Win32 real) | K `flui-system`; Win32-диалог переезжает, executor/`Task` удаляются тем же ADR (заменяет ADR-0039 §2) | вне закрытого списка |
| Deep links внутрь | событие | ядро | N `on_open_urls` без потребителя | C колбэк хоста с router-потребителем | правило 8 |
| Защищённое хранилище, биометрия | сервисы | возможность | нет | K `flui-vault` | — |

### 4.5 Необязательные сервисы (к 1.0)

| Крейт | Что | Разрешения / декларации |
|---|---|---|
| `flui-location` (пилот 0.3) | геолокация, геофенсинг | Android `ACCESS_*_LOCATION`, iOS `NSLocation*UsageDescription`, Windows `location`, фоновый режим |
| `flui-sensors` | акселерометр, гироскоп, магнитометр, барометр | iOS `NSMotionUsageDescription`; Android — нет |
| `flui-media` | камера, выбор изображений и видео | `CAMERA`, `NSCameraUsageDescription`, `webcam` |
| `flui-notify` | локальные и push-уведомления, бейджи | `POST_NOTIFICATIONS`, push entitlements |
| `flui-vault` | Keychain / DPAPI / Credential Manager / Keystore, биометрия | `NSFaceIDUsageDescription`, `USE_BIOMETRIC` |
| `flui-device` | батарея, энергосбережение, connectivity, сведения об устройстве, haptics | `ACCESS_NETWORK_STATE`, `VIBRATE` |
| `flui-system` | launcher/URL, share, файловые диалоги, пути, трей и меню | — |

## 5. Шов возможностей (ADR-0152)

Изменения против ADR-0084: маркер-тип у владельца handle (`ClipboardCapability` в
`flui-interaction`, `Handle = ClipboardHandle`, `!Send`, чтение через колбэк); провайдер
получает `ProviderContext` с `Option` окна; без кэша — подписка заканчивается с последним клоном
handle; `Unsupported` — `#[non_exhaustive]` с конструктором `of::<C>()` и написанным `Display`;
разрешения — на handle, с `Denial`, чтобы ошибка не могла нести `Granted`; повтор `NAME` —
конфликт до окна; `storage` → шов после persistence; `close_guard`, `lifecycle_handle`,
`flush_registry_in_crate` остаются методами. Форма — в ADR-0152 §3.

**Правило посадки:** шов сливается вместе с clipboard (R4.10). Если окно W1 не откроется до
11-24, шов целиком переходит в 0.3; `clipboard_handle` живёт до него.

## 6. Системные настройки (решение Q2)

Один источник, свои представления.

```rust
// flui-platform::preferences — собрано в scratch-крейте (rustc 1.99.0), вывод — в PR
#[non_exhaustive]
pub struct SystemPreferences { /* приватные поля */ }
impl SystemPreferences {
    pub fn builder() -> SystemPreferencesBuilder;
    pub fn text_scale(&self) -> TextScale;
    pub const fn motion(&self) -> Motion;
    pub const fn gestures(&self) -> &GesturePreferences;
    // contrast(), bold_text(), locales() — так же
}
#[non_exhaustive]
pub enum Motion { NoPreference, Reduce, Scaled(DurationScale) }
impl Motion {
    /// 0 (и -0.0) → Reduce, ровно 1 → NoPreference, иное конечное > 0 (субнормальные тоже) →
    /// Scaled; отрицательное и неконечное → InvalidPreference::DurationScale.
    pub fn from_duration_scale(v: f64) -> Result<Self, InvalidPreference>;
}
/// Конечное и строго положительное: `Scaled(0)` непредставимо.
pub struct DurationScale(f64);
/// Каждое поле — Option: None, где у ОС нет значения; потребитель держит своё умолчание.
#[non_exhaustive]
pub struct GesturePreferences { /* double_click_interval: Duration, double_click_area: Size,
                                    drag_area: Size, long_press_timeout: Duration,
                                    touch_slop: Distance, fling: FlingSpeeds */ }
pub struct Distance(f64);          // логические px, конечное ≥ 0
pub struct Speed(f64);             // логические px/с, конечное > 0
pub struct FlingSpeeds { /* min ≤ max, проверяется */ }
#[non_exhaustive]
pub struct WheelPreferences { /* vertical: Option<WheelStep>, horizontal_chars: Option<u32> */ }
#[non_exhaustive]
pub enum WheelStep { Lines(u32), Page }
#[non_exhaustive]
pub enum InvalidPreference { TextScale, DurationScale, GestureArea, Distance, Speed, FlingRange }
```

- **Производитель:** `Platform::preferences()` и одна подписка `on_preferences_changed` в ядре-
  хосте (колбэк `Platform`, `+ Send`, как остальные хуки `Platform`; в храповике send-flip —
  класс «platform hook»). Каждый бэкенд подписывается на ОС один раз: Win32 `WM_SETTINGCHANGE`
  и `SystemParametersInfo` (в том числе `SPI_GETWHEELSCROLLLINES`, `SPI_GETWHEELSCROLLCHARS`)/
  `GetDoubleClickTime`/`SM_CXDOUBLECLK`/`SM_CXDRAG`; AppKit
  `NSWorkspace`/`NSEvent.doubleClickInterval`; iOS `UIContentSizeCategory`/
  `UIAccessibility`; Android `Settings.Global`/`ViewConfiguration` (`getScaledTouchSlop`,
  `getScaledMinimumFlingVelocity`, `getScaledMaximumFlingVelocity`); web `matchMedia`; winit/Linux
  — значения по умолчанию, пока нет источника (порталы XDG — позже). Где у ОС нет значения,
  поле жестов или колеса — `None`, и потребитель держит своё умолчание (контракт «системных
  умолчаний» не выдумывает); масштаб текста по умолчанию — 1, motion — `NoPreference`.
  `Distance` и `Speed` — проверенные значения настроек (f64 логических пикселей и пикселей в
  секунду по ADR-0098, конечные), а не единицы измерения: геометрия остаётся f64-значениями
  `flui_foundation::geometry`, без `px()`.
- **Доставка:** `flui-app` кладёт текущее значение в каждый realm при создании (значение есть до
  первого окна) и рассылает изменение одной типизированной операцией хоста.
- **Потребители** строят своё: `MediaQuery` (масштаб текста, контраст, bold, локали, motion для
  виджетов); `flui-interaction` — `GestureSettings` через `GestureSettingsScope` (interaction X2),
  пересчитывая логические `Size` в пороги своего типа указателя; `flui-animation` — свою политику
  движения из `motion` и политики приложения.
- **Политика приложения** (motion «как в системе / всегда / никогда» и подобные) живёт в realm и
  задаётся конфигурацией приложения; контракт её не знает.
- **Почему не на окно:** настройки ОС общие на процесс; производитель на окно — N подписок и
  отсутствие значения до первого окна; один общий колбэк, который второе окно затирает
  (возражение спеки reduce-motion), не возникает, потому что подписчик один — `flui-app`.
- Текст для спек animation и interaction — §11.

## 7. Готовность к необязательным возможностям (R6, без кода в 0.2.0)

- **Подписки:** handle держит подписку ОС и завершает её с последним клоном; виджет получает
  значение, сведённое к кадру (signal), сырой поток — для не-UI потребителей; поведение в фоне —
  политика возможности.
- **Разрешения:** `PermissionState { Granted, NotDetermined, Denied(Denial) }`,
  `Denial { ByUser, Permanently, Restricted }`, оба `#[non_exhaustive]`; отдельно «служба ОС
  включена». «Запрещено навсегда» на Android видно только по результату запроса. Отзыв на
  Android и iOS обычно убивает процесс — событие отзыва best-effort.
- **Фон:** возможность объявляет, работает ли без realm; фоновая работа — сервисы ADR-0049, без
  UI-движка (у Flutter это отдельный isolate и headless-движок).
- **Декларации и манифесты:** крейт возможности объявляет разрешения, usage-строки,
  entitlements, privacy manifest и фоновые режимы в своих метаданных; `flui-cli` собирает их по
  графу зависимостей приложения и генерирует манифесты ОС. Схему ключей задаёт ADR манифестов
  (0.3); список ключей `[package.metadata.flui]` строгий, поэтому ключ появляется вместе с ним.
- **Мост к хосту:** activity Android и её результаты, события application delegate, хуки оконных
  сообщений Win32 — непрозрачные handle без upstream-типов; ADR моста — с пилотом.
- **Качество:** conformance-таблица и симулирующий провайдер на возможность; поддержка платформы
  заявляется только по исполненной таблице.

Пилот — `flui-location` на Windows (WinRT `Geolocator`) и Android: у него есть разрешение, поток,
фоновый режим и отдельное «служба выключена». Выдержит он — выдержат сенсоры и камера.

## 8. Инвентаризация (этап 1)

Сырые отчёты — вне репозитория. Ссылки проверены по `main` @ `d56188c14`.

### 8.1 `flui-platform-api`

- Зависимые: flui-app, -interaction, -platform, -widgets, -view, -sdk, -runtime, -testing и
  фасад; комментарий `Cargo.toml:64-66` перечисляет четыре — устарел. [R]
- Upstream: `ui-events` в `PlatformInput`, `PlatformWindow::on_input`, `PlatformWindow::modifiers`;
  `keyboard-types` и `dpi` — транзитивно. Разрешённые ADR-0089 §2: `raw-window-handle` 0.6
  traits, `serde`, `schemars`, `cursor-icon`. [R]
- Нулевые потребители: `offset_from_coords`, `delta_offset_from_coords`, `LockKind`,
  `DEFERRED_LOCK_CAPACITY`, `utf16_range`; `TransferImage` — только как полезная нагрузка
  варианта `Image`. [R]
- Один потребитель: `LockArbiter`, `CompositionLedger`, `EditGeneration` (widgets);
  `project_ime_event` (interaction). `TextStoreHost` — один production-реализатор (Win32), а
  потребители — interaction, runtime, хост; второй реализатор — `flui-testing`. [R]
- Без ADR: `Storage` (спека persistence), `Locale`, `Brightness`, `TargetPlatform`. [R]
- `!Send` закреплено `static_assertions` у `LockArbiter`, `CommitGate`, `InMemoryTextStore`;
  остальные owner-local по построению. [R]

### 8.2 `flui-platform`

- Бэкенды: Win32 9 927 строк (real), AppKit 8 246 (real), winit 6 066 (real; Linux production и
  единственный реальный DnD), iOS 2 489 (real), Android 1 391 (MVP: mock clipboard, inline
  executor, выдуманный дисплей), web 1 379 (частично), `linux/` 978 (`LinuxPlatform`-заглушка +
  реальный AT-SPI-адаптер, который использует winit), headless 1 752. [R]
- Исполняются в CI: winit (Linux, Xvfb) и headless; остальное — cross-typecheck. [R]
- Упоминаний `flui-platform`/`flui_platform` без `-api` на базе: 243 файла, вне крейта 198 (rs 72,
  toml 9, md 111, прочее 6); команда `rg -l -P --hidden -g '!.git' 'flui[-_]platform(?![-_]api)'`.
  [R]

### 8.3 Понятия не на своём месте

Сведено в §4. `ExecutionServices` (flui-runtime) — исполнитель фреймворка, остаётся.
`BackgroundExecutor`/`Task` в бэкенде — живые, уходят вместе с переездом диалогов в
`flui-system`.

### 8.4 Путь возможности

Сейчас — именованные поля через flui-runtime, flui-view и flui-app; `LifecycleContext` закрыт для
пакетов. ADR-0152 даёт один обобщённый вход и per-realm реестр.

## 9. Рыночный эталон

Прочитано по первичным источникам (upstream через `gh api`, официальные доки) 2026-10-06.

| Проект | Контракт | Бэкенды | Unsupported | Тестовый бэкенд | Возможности / плагины |
|---|---|---|---|---|---|
| winit 0.31 | `winit-core` (`#![warn(clippy::exhaustive_enums)]`, dyn-safe трейты) | `winit-<os>` + фасад, одна версия | `RequestError::NotSupported`, `None` | нет | — |
| raw-window-handle 0.6 | `no_std`, без зависимостей | — | `HandleError::{NotSupported, Unavailable}` | — | interop-крейт почти не выпускается |
| Bevy | `bevy_window` | `bevy_winit`; `bevy_a11y` | — | `ScheduleRunnerPlugin` | `Plugin`; один поезд — каждый релиз ломает плагины |
| Masonry | `masonry_core` | `masonry_winit` | `RenderRootSignal`, «some platforms may ignore» | `TestHarness` | — |
| GPUI | `gpui` (`Platform`, `PlatformWindow`, `PlatformDispatcher`) | `gpui_<os>`, селектор `gpui_platform` | default-методы, `anyhow` | `TestPlatform` (местами `unimplemented!()`) | — |
| Slint | `Platform` | `i-slint-backend-*` + selector | `PlatformError::{NoPlatform, Unsupported, …}` | `TestingBackend { mock_time }` | upstream за `unstable-winit-030`-фичами |
| Tauri v2 | `tauri-runtime` | `tauri-runtime-wry` | уровни поддержки; геолокация на десктопе отдаёт нули — антипаттерн | `MockRuntime` | плагин desktop.rs + mobile.rs; capability-файлы; `PermissionState {Granted, Denied, Prompt, PromptWithRationale}` |
| Flutter | `<x>_platform_interface` | `<x>_<os>`, `default_package` | `MissingPluginException`, `UnimplementedError` | `MockPlatformInterfaceMixin` | интерфейс растёт методами с default; `permission_handler`: 6 состояний + `ServiceStatus`; манифесты руками |
| Compose MP | `expect` | `actual` на каждый target | — | — | Kotlin советует интерфейс + фабрику ради фейков |
| SwiftUI | `EnvironmentValues` | система | — | подмена `openURL` | система пишет, view читает |

ОС: Android после двух отказов (API 30+) больше не показывает диалог; отзыв в настройках
убивает процесс; проверка статуса не отличает «не спрашивали» от «навсегда». iOS — смена
Camera/Photos/Contacts в настройках завершает процесс (Apple developer forums, thread 64740; в
API-доках нет) [не проверено запуском].

Норма, которую берём: один контракт + бэкенды; полноправный тестовый бэкенд с управляемым временем
и скриптом ответов; unsupported — значение, и «нет на платформе» / «не зарегистрировано» /
«запрещено» различимы; разрешения — состояние на возможность, запрос асинхронный; интерфейс
растёт методами с default; upstream-churn — за фичами с номером версии. Где обгоняем: прямые
вызовы ОС из Rust вместо каналов, декларации → манифесты, подписка-RAII, фон без UI-движка,
conformance-таблицы, крейты возможностей вне поезда.

## 10. Имена

Требование владельца: `flui-<слово>`. Варианты (crates.io, 2026-10-06: свободны все, включая
`flui-location`, `flui-sensors`, `flui-media`, `flui-notify`, `flui-vault`,
`flui-device`, `flui-system`; `flui-cli` 0.1.0 уже опубликован владельцем):

| Вариант | Контракт | Ядро-хост | Плюсы | Минусы |
|---|---|---|---|---|
| **A (рекомендую)** | `flui-platform` | `flui-native` | имя крейта = путь фасада `flui::platform`; `flui_platform::Capability` читается без повтора | имя `flui-platform` меняет смысл — переименование в два шага; web и headless — не совсем «native» |
| B | `flui-platform` | `flui-os` | короче | «os» для web/headless хуже; плохо ищется |
| C | `flui-platform` | `flui-backend` | точное слово роли | «backend» уже значит GPU-бэкенд и `TextInputBackend` |
| D | `flui-port` / `flui-contract` | `flui-platform` | один шаг переименования | путь фасада и крейта расходятся; «port» читается как «портирование» |
| E | `flui-platform` | `flui-shell` | так называет это Flutter | «shell» в FLUI нигде не используется |

`-core` (норма winit/masonry/slint) запрещён N1 спеки naming.

**Механика A — два шага** (ADR-0151 §1):

1. **Шаг 1** (окно naming 11-10…11-14): `flui-platform` → `flui-native`, каталог включительно;
   проверка «отставное имя»: `flui-platform`/`flui_platform` вне архивных корней — ошибка.
2. **Шаг 2** (11-24…11-28, после того как все открытые ветки влили шаг 1):
   `flui-platform-api` → `flui-platform`; проверка снимается.

Скрипт (`cargo xtask rename` спеки naming с картой platform-layer): совпадение по целому токену
(`flui-platform` не совпадает внутри `flui-platform-api`), `--changed-since <merge-base>` для
веток; файлы: манифесты, корневые `reach.tier.*.forbid`, `reach-forbid`, `reach-exceptions`,
`allowed-dependents`, имена в `tools/xtask` (`globals.rs` `PLATFORM`/`BACKENDS`, `tiers.rs`
`SDK_SURFACE`) и его фикстуры (`globals/fixture.rs`, `globals/tests.rs`), allowlists по именам
крейтов (thread-boundary, unsafe-impl send-flip; `docs-paths` со счётчиками), живые доки, ADR
(имена — механически, решения не меняются, карта — ADR-0151). Архивные корни не трогаются.
`deny.toml` имён крейтов не содержит.

## 11. Текст для спек animation и interaction (решение Q2)

Владельцы вставляют в свои design.md без правок.

> **Системные настройки: один источник, свои представления (решение владельца, 2026-10-06;
> ADR-0151 §4, спека platform-layer §6).**
> Системные настройки ОС приходят в FLUI из одного источника — `SystemPreferences` в контрактном
> крейте (`flui-platform-api`, после переименования `flui-platform`). Производитель один на
> хост: ядро-хост подписывается на ОС один раз, `flui-app` кладёт значение в каждый realm при
> создании (оно есть до первого окна) и рассылает изменения. Поля приватные, значения строятся
> builder'ом с проверкой; `Motion` — `NoPreference | Reduce | Scaled(DurationScale)`; жесты —
> интервал и прямоугольник double-click, прямоугольник drag (логические `Size`, ADR-0098) и
> таймаут long press.
> Потребитель не использует `SystemPreferences` в своей логике напрямую, а строит своё
> представление: **interaction** — `GestureSettings` (через `GestureSettingsScope`, X2);
> **animation** — свою политику движения. Политика приложения поверх ОС («как в системе /
> всегда / никогда» для reduce motion) — во фреймворке (realm), не в контракте.
> Это решение **заменяет** `GestureSettingsSource` (interaction X1) и `SystemMotion` с методами
> `PlatformWindow::system_motion`/`on_system_motion_changed` (animation reduce-motion P1): новых
> производителей и методов окна для настроек не добавлять. Тип `DurationScale` и его проверка
> (`InvalidPreference::DurationScale` для неконечного или отрицательного значения) берутся из
> контракта; `DurationScale` — только конечное s > 0, так что `Scaled(0)` непредставимо;
> производитель отображает масштаб ОС через `Motion::from_duration_scale`: 0 → `Reduce`, ровно
> 1 → `NoPreference`, иное конечное > 0 → `Scaled(s)`, отрицательное и неконечное — ошибка.
> Порядок: тип и производитель — задача platform-layer LY8 (окно W1); потребители —
> задачи animation и interaction после неё.

## 12. Вопросы к владельцу

Q2 решён (§6). Открыты; работа идёт по рекомендации:

- **Q1. Имя.** A (`flui-platform` + `flui-native`, переименование в два шага) — рекомендую; или
  D (один шаг, другое имя контракта).
- **Q3. `AppLifecycleState` в контракт** (`#[non_exhaustive]`, политика — в scheduler) —
  рекомендую; или оставить в scheduler.
- **Q4. ADR-0084 заменить ADR-0152** — рекомендую.
- **Q5. Машинерия text store** после text-ime T6 — в `flui-interaction` (заменяет часть ADR-0142)
  — рекомендую; или в контракте за `#[doc(hidden)]`.
- **Q6. Номера ADR 0151–0154** — записать резерв в реестр `release/tasks.md` на
  `plans/specs-next` (чужая активная ветка; нужно ваше «да»).
- **Q7 решён (2026-10-06): нового крейта геометрии нет.** ADR-0153 — вопрос к 0.3 с опытами;
  разделение, если понадобится, — узкой поправкой к ADR-0098.
