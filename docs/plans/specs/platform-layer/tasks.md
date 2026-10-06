# Платформенный слой — задачи

- **Статус:** черновик; исполнение — после утверждения design.md, ADR-0151 и ADR-0152 владельцем
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **База:** `main` @ `d56188c14`
- **Итог:** 16 задач, 7 из них `[P]` (параллельны внутри своего окна); ≈ 30 инженеро-дней

## Правила исполнения

Задача = worktree (`cargo xtask worktree new platform-layer/<slug>`) = PR; `cargo xtask
check-changed` зелёный; fix-тест падает с откатом (изолированный checkout); имена тестов и кода
без номеров задач и требований. Где: **L** — Linux CI (исполняется); **W** — Windows нативно
(локальный запуск); **T** — только cross-typecheck (Win32/AppKit/Android/iOS без запуска — так и
пишется в PR). Каждый PR, меняющий поверхность для потребителя, добавляет
`changelog.d/<branch-slug>.md`.

Ни одна задача не трогает файлы активной ветки без согласия её владельца; столбец «Окно»
говорит, после чего задача может начаться.

## Окна по веткам

| Окно | Когда | Почему |
|---|---|---|
| **W0 — сейчас** | после утверждения design/ADR | только новые файлы и файлы, которых не касаются активные ветки |
| **W1 — после ядра send-flip** | ~11-09 (слияние ядра), не раньше | send-flip владеет `flui-scheduler`, `flui-runtime/src/{realm_services,presentation}.rs`, `ui_realm/*`, `flui-view/src/owner/*` |
| **W2 — переименование** | 11-10…11-14, вместе с массовым переименованием спеки naming | окно, в котором и так меньше всего живых веток; до publish-конвейера 12-01 |
| **W3 — после text-ime T6 и teardown T5** | ~11-17…11-28 | text-ime держит `text_store/*` и Win32 `text_services`; teardown и text-ime держат Win32 `platform.rs` |
| **W4 — после persistence** | по её последней задаче | persistence держит доставку `Storage` |

## Граф

```mermaid
graph LR
  LY0 --> LY1 & LY2 & LY3 & LY4 & LY5
  LY1 & LY2 & LY3 --> LY6
  SF["send-flip ядро"] --> LY6 & LY8 & LY9
  LY6 --> LY7
  LY1 --> LY9
  LY5 --> LY9 --> LY10
  NM["naming: cargo xtask rename"] --> LY11 --> LY12
  LY6 & LY8 --> LY12
  LY12 --> LY13 & LY14 & LY15
  TI["text-ime T6"] --> LY13 & LY14
  PS["persistence"] --> LY16
  LY12 --> LY16
  PV["interaction pointer-vocabulary P1/P2"] -.R1.5.-> DONE["R1.5 закрыт"]
```

## Задачи

| ID | Задача | Окно | Крейты и файлы | Метаданные / гейты | Тесты | Где |
|---|---|---|---|---|---|---|
| **LY0** | Утверждение design.md, ADR-0151, ADR-0152; ответы на Q1–Q6; резерв ADR-0151…0153 в реестре `release/tasks.md` (ветка `plans/specs-next`) | — | docs | `cargo xtask checks` | — | — |
| **LY1** [P] | Контракт: модуль `capability` (`Capability`, `CapabilityProvider`, `Unsupported`, `UnsupportedReason`, `impl Capability for dyn Clipboard`) | W0 | `flui-platform-api/src/{capability.rs (новый), lib.rs}` | без новых зависимостей | dyn-compatibility пин; `Unsupported` форматируется с причиной; `#[non_exhaustive]` | L |
| **LY2** [P] | Runtime: `CapabilityRegistry`, `CapabilityRegistrar`, `Plugin` — новый модуль, ещё не подключён (PR называет LY6 как подключение) | W0 | `flui-runtime/src/capability/` (новый), `lib.rs` | `cargo xtask globals` (реестр не static) | приоритет app > built-in > plugin; конфликт двух плагинов → ошибка в детерминированном порядке; поиск неизвестного → `NotRegistered` | L |
| **LY3** [P] | `flui-testing`: headless-провайдер clipboard поверх `InMemoryClipboard` | W0 | `flui-testing/src/capability.rs` (новый) | — | подмена провайдера в тесте | L |
| **LY4** [P] | Бэкенд: удалить мёртвое — `LinuxPlatform`-заглушку, `window.rs`, `PlatformCapabilities` (+ `Platform::capabilities`), `BackgroundExecutor`, `Task`, `PlatformExecutor`, `background_executor`; ребро `tokio` из бэкенда | W0, с согласия владельца teardown (`traits/platform.rs`) | `flui-platform/src/{lib.rs, window.rs, executor.rs, task.rs, traits/{platform,capabilities}.rs, platforms/linux/mod.rs}` | `cargo xtask workspace`, `reach`, `deps` (tokio уходит), `globals` | существующие; `current_platform()` на Linux по-прежнему winit | L + T |
| **LY5** [P] | Контракт: тип `SystemPreferences` (+ `Contrast`, `TextScale`, `Motion`, `GestureTimings`) и default-методы `PlatformWindow::preferences`/`on_preferences_changed`; headless отдаёт и меняет значение. Зависит от Q2 | W0 | `flui-platform-api/src/{preferences.rs (новый), platform_window.rs, lib.rs}`, `flui-platform/src/platforms/headless/platform.rs` | — | `TextScale` отвергает не-конечное и ≤ 0; headless: смена → колбэк один раз | L |
| **LY6** | Подключить шов: параметр конструктора realm (в `RealmHostServices` по плану persistence), `BuildOwner`, скрытый `capability_erased`, `LifecycleContextExt::capability`; встроенный clipboard-провайдер в `flui-app`; `EditableText` через шов; `clipboard_handle` и `ClipboardHandle` удалены | W1 | `flui-runtime/src/{realm_services,presentation}.rs`, `flui-view/src/{owner/build_owner.rs, context/*}`, `flui-widgets/src/text/editable_text.rs`, `flui-interaction/src/clipboard.rs`, `flui-app/src/app/runner/host.rs` | `cargo xtask workspace`, `reach`; changelog | ADR-0152 §4: copy/paste через headless; `NotRegistered` без провайдера; `NotOnThisPlatform` доходит до виджета; trybuild: `capability` в `build` → E0599 | L |
| **LY7** | `Application::plugin`/`capability`, `AppRunError::CapabilityConflict` до окна; fixture-пакет вне workspace на `flui-sdk` + контракт | W1 | `flui-app/src/app/{application,error}.rs`, `flui-sdk/src/lib.rs` (+ `tests/surface.rs`), `tests/fixtures/` | `cargo xtask reach` (fixture не видит бэкенд) | конфликт до первого окна; fixture регистрирует и получает свою возможность | L |
| **LY8** | `AppLifecycleState` → контракт `lifecycle`; `flui-scheduler` реэкспортирует по прежнему пути; ребро scheduler → контракт. Зависит от Q3 | W1 | `flui-scheduler/src/frame.rs`, `Cargo.toml`; `flui-platform-api/src/lifecycle.rs` (новый) | `workspace` (S→C, layer 2→1 — разрешено); changelog | существующие тесты lifecycle без правок (путь сохранён) | L |
| **LY9** | Доставка `SystemPreferences`: `MediaQuerySource` (text scale, контраст, motion, bold, локали), `GestureBinding` строит `GestureSettings` из `gestures`; анимации читают `motion`; `AccessibilityFeatures` удалён. Согласовать с animation (ADR-0146) и interaction X1 по Q2 | W1 | `flui-runtime/src/media_query_root.rs`, `flui-widgets/src/app/media_query.rs`, `flui-interaction/src/{settings,binding}.rs`, `flui-semantics/src/accessibility.rs`, `flui-app/src/app/runtime.rs` | `cargo xtask workspace`; changelog | headless: смена `text_scale` → перестройка виджета, читающего `MediaQuery`; смена `gestures.long_press` → распознаватель ждёт новое время; тест падает без доставки | L |
| **LY10** [P] | Производители `SystemPreferences` по бэкендам: winit/Linux; web (`matchMedia`); Win32 (`SystemParametersInfo`, `WM_SETTINGCHANGE`); AppKit; iOS | winit, web — W1; Win32 — W3 (после teardown T5 и text-ime T6); AppKit, iOS — W1 | `flui-platform/src/platforms/{winit,web,windows,macos,ios}/*` | `cross-typecheck` | winit — L; web — `wasm-check`; Win32 — W (локальный запуск обязателен, иначе T); AppKit/iOS — T | L/W/T |
| **LY11** | Скрипт переименования: `cargo xtask rename` спеки naming с картой `flui_platform → flui_native`, затем `flui_platform_api → flui_platform` (пути каталогов, `Cargo.toml`, xtask-константы, allowlists, `deny.toml`, живые доки и пути в ADR; архивные корни не трогает); сухой прогон на `main`, повторный прогон — пустой diff | W2 (до 11-10) | `tools/xtask/src/rename*` (у спеки naming) | `cargo xtask checks` | самотест скрипта на временной копии | L |
| **LY12** | **Переименование крейтов** (только с подтверждения владельца): один PR, один коммит, произведённый LY11, без ручных правок; описание PR — инструкция для открытых веток (ниже) | W2 (11-10…11-14) | весь workspace (~198 файлов + сам крейт) | `workspace`, `reach`, `globals`, `checks`, `check-changed`, `cross-typecheck`; changelog | весь набор | L + T |
| **LY13** | Бэкенд-модули по смыслу вместо `shared`/`traits` (naming PM4; согласовать с владельцем naming, чтобы не переименовывать дважды); `LinuxWindowExt` (naming PL6) — удалить, нет реализации | W2–W3 | `flui-native/src/{shared,traits}/*` | `cargo xtask names` | существующие | L + T |
| **LY14** | Диета Stable-крейта: `InMemoryClipboard`, `InMemoryTextStore` → `flui-testing`; `OfferTable`, `WindowMode`, `WindowEvent`, `WindowBounds`, `PlatformDisplay`, пиксельные хелперы, `Mica*`/`Vibrant*` → бэкенд; `display`/`window_bounds`/`set_background_appearance`/`mouse_position`/`is_hovered` → `HostWindow`; удалить `offset_from_coords`, `delta_offset_from_coords`, `utf16_range`, `TransferImage`; решение Q5 по машинерии text store | W3 | `flui-platform/src/*`, `flui-native/src/*`, `flui-testing` | `workspace`, `reach`, `deps` (`parking_lot` уходит из контракта, если ничего не держит); changelog | существующие + `text_store_kit::assert_conforms` | L + T |
| **LY15** | OS-типы бэкенда → `pub(crate)` или `#[doc(hidden)]`-модуль для examples (R1.6); тест, что публичная поверхность `flui-native` не называет `windows::`/`objc2::`/`android_activity::`/`web_sys::`/`winit::`/`tokio::` | W3 | `flui-native/src/platforms/*/mod.rs`, `examples/*` | — | тест на `cargo public-api` вывод или rustdoc JSON | L + T |
| **LY16** | `Storage` через шов: встроенный провайдер (`FileStore`), headless `MemoryStorage`; `LifecycleContext::storage` удалён; `Persisted` через `cx.capability::<dyn Storage>()` | W4 | `flui-view/src/persist/*`, `flui-app/src/app/storage_host.rs`, `flui-testing/src/storage.rs` | changelog | persist-тесты persistence без смены поведения; `Unsupported` на wasm | L |

После 0.2 (не в объёме этой спеки, но модель задана): haptics как первый пакет через шов;
файловые диалоги; launcher (`open_url`); app-level lifecycle-сигнал ОС; memory pressure;
`view_insets`; модель разрешений с первым потребителем.

## Инструкция для открытых веток после LY12

1. `git fetch origin && git merge origin/main` — каталоги переехали; git отследит переименования
   файлов. Конфликты — только там, где ваша ветка правила те же строки.
2. `cargo xtask rename --apply platform-layer` на своей ветке — перепишет новые упоминания
   `flui_platform` → `flui_native` и `flui_platform_api` → `flui_platform`, которые вы добавили
   после ветвления. Порядок внутри скрипта фиксирован; руками не менять.
3. `cargo xtask check-changed`.

Если ветка открыта во время W2 и не может слиться до 11-14 — сообщите владельцу спеки
platform-layer до 11-09, окно сдвигается целиком, а не по частям.

## Проверка требований

| Требование | Задачи |
|---|---|
| R1.1, R1.2 | LY12 |
| R1.3 | уже верно; LY12 сохраняет `allowed-dependents` |
| R1.4 | LY4 (tokio), LY14 |
| R1.5 | interaction pointer-vocabulary P1/P2 (вне этой спеки), LY14 |
| R1.6 | LY15 |
| R1.7 | LY13 |
| R1.8 | LY4 |
| R2.1–R2.4 | LY5, LY8, LY9, LY10 |
| R3.1–R3.9 | LY1, LY2, LY3, LY6, LY7, LY16 |
| R4.1, R4.2 | design §5 п.5; тип — с первым потребителем |
| R5.1–R5.5 | все; LY11/LY12 для R5.3 |
