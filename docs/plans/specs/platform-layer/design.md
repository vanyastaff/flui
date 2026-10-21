# Платформенный слой — дизайн

- **Статус:** редакция 3 (после ревью), на утверждение владельцу; слияние изменения, которое
  её принесло, и есть утверждение — ADR при нём помечены принятыми. Заменяет редакцию 2 целиком:
  горизонтов 0.2/0.3 больше нет, серия идёт до последней задачи [tasks.md](tasks.md).
- **Дата:** 2026-10-20
- **База:** `main` @ `deb9faea2`
- **Требования:** [requirements.md](requirements.md)
- **ADR:** [ADR-0151](../../../adr/ADR-0151-platform-layer-boundary-and-names.md) (крейты, имена,
  закрытый список, содержимое, устройство ядра), [ADR-0152](../../../adr/ADR-0152-capability-seam-revised.md)
  (шов возможностей), [ADR-0153](../../../adr/ADR-0153-release-train-and-capability-crates.md)
  (версии), [ADR-0154](../../../adr/ADR-0154-capability-crates.md) (крейты возможностей),
  [ADR-0254](../../../adr/ADR-0254-window-contract.md) (контракт окна),
  [ADR-0255](../../../adr/ADR-0255-mobile-and-web-hosts.md) (хосты Android, iOS и Web)
- **Сверка с другими проектами:** решения проверены по winit 0.31, raw-window-handle 0.6, GPUI,
  Slint, Masonry, SDL3, embedder'у и плагинам Flutter, Tauri v2, Compose Multiplatform и
  AccessKit; источники приведены в ADR. Что поддерживает каждая ОС —
  [platform-parity.md](../../../platform-parity.md)

Каждое решение ниже сравнено с альтернативами, включая то, что уже стоит в коде. «Так уже
сделано» основанием не считается: где текущая форма остаётся, сказано, чем она лучше.

## 1. Что есть сейчас

| Факт | Источник |
|---|---|
| Контракт `flui-platform-api`: 16 481 строка в `src/`; фасад реэкспортирует его целиком как `flui::platform` | `crates/flui-platform-api/src/`, `src/lib.rs:203-229` |
| Бэкенд `flui-platform`: 97 363 строки (`platforms/` 72 528, `shared/` 20 320, `traits/` 1 709); зависит от него только `flui-app` | `crates/flui-platform/src/`, `allowed-dependents` в `crates/flui-platform/Cargo.toml` |
| Тройка `WindowState` / `WindowCommand` / `WindowCapabilities` (ADR-0237) лучше всех семи образцов, но `PlatformWindow` по-прежнему отдаёт те же факты геттерами и 14 регистрациями `on_*`; 21 из 39 методов по умолчанию ничего не делает (`let _ = callback;`) | `crates/flui-platform-api/src/platform_window.rs`, `window_state.rs` |
| `PlatformWindow: Send + Sync` ради одного потребителя — потока растеризации; из-за `Send`-колбэков `flui-app` держит runtime в `thread_local!` | `crates/flui-platform-api/src/platform_window.rs:205`, `crates/flui-app/src/app/runner/host.rs:431-441` |
| Девять перечислений по-своему говорят «не поддерживается»; 26 из 82 публичных перечислений закрыты без решения | `window_state.rs:455`, `window.rs:283`, `:342`, `drag.rs:48`, `haptics/request.rs:125`, `input_capabilities/mod.rs:24`, `storage.rs:300`, `preferences/geometry.rs:361`, `text_store/host.rs:81` в `crates/flui-platform-api/src/` |
| `shared/` на 45 % состоит из кода одной ОС; правила ОС бывают `pub #[doc(hidden)]` ради `tests/`; шесть разных «аренд» хуков; пользовательский код под локом в `android/mod.rs:1010` и headless | `crates/flui-platform/src/shared/`, `platforms/android/mod.rs:1010`, `platforms/headless/platform.rs:748-750` |
| Haptics — настоящие на пяти бэкендах; машинерия text store имеет production-потребителей; `PlatformDisplay`, геттеры окна, `window.rs`, `PlatformCapabilities` уже заменены или удалены | `crates/flui-platform/src/platforms/*/haptics.rs`, `shared/haptic_degradation.rs`, `crates/flui-interaction/src/haptics.rs` |
| Шва возможностей в коде нет; ADR-0152/0153/0154 приняты этим изменением, реализация не начата | ADR-0152, ADR-0153, ADR-0154 |
| Android: `NativeActivity` (`hasCode="false"`) плюс внедрённый `FluiView` из готового dex (ADR-0247); нет a11y, «назад», живых insets, результатов activity, разрешений, тёплых deep links; `targetSdk` берётся «последний установленный» | `crates/flui-cli/templates/platforms/android/AndroidManifest.xml:5-22`, `build.gradle.kts:7-23`, `crates/flui-cli/src/build/android_package.rs:70-97` |
| iOS: чистый Rust на objc2 и UIScene; нет `UITextInput`, a11y, URL-колбэков сцены | `crates/flui-platform/src/platforms/ios/platform.rs:1192-1219` |
| Web: скрытый `<textarea>` и `EditContext`; нет ARIA DOM и маршрутизации | `crates/flui-platform/src/platforms/web/text_input.rs:559-590` |
| Нижние границы версий ОС и типизированные гейты уже решены ADR-0244 (`platform-support.toml`, `Since<F>`) | `platform-support.toml` |

## 2. Правила

1. **Ядро делает с ОС только закрытый список (§4, D2).** Остальное — крейты возможностей.
2. **В контракте — словарь и трейты, которыми пользуется кто-то выше бэкенда** (включая
   приложения через фасад), и типы, которыми бэкенд передаёт факт фреймворку. Элемент без такого
   потребителя — в ядро как `pub(crate)` или удалить. «Вернётся с первым потребителем» не бывает:
   либо строим потребителя в этой серии, либо элемента нет.
3. **Один источник истины на факт.** Состояние окна — снимок; изменения — команды; события —
   один обработчик. Геттер рядом со снимком и колбэк рядом с обработчиком — вторые истины.
4. **Нет инертных умолчаний.** Метод, который молча выбрасывает переданное, — дефект «accepted
   work dropped when a hook is missing».
5. **Отсутствие — значение.** Один тип `Unsupported` с причиной во всех ошибках; версия ОС в
   контракт не попадает (ADR-0244).
6. **Owner-local по умолчанию.** `Send` — только у действительно межпоточного: цель поверхности
   для растеризации, poster на owner-поток, `SharedPlatform`.
7. **OS-типы не публичны.** Глубокий контроль снаружи — через крейты возможностей и мост к хосту.
8. **Общий код — в самом нижнем крейте, которому он нужен; в ядре — в модуле по смыслу.**
   Код одной ОС живёт в этой ОС, код одного семейства — в семействе.
9. **Пользовательский код никогда не вызывается под локом или заёмом.**

## 3. Целевая карта крейтов

| Крейт | Класс | Содержит | Не содержит |
|---|---|---|---|
| **`flui-platform`** (сейчас `flui-platform-api`) | контракт, tier C, layer 1, `stable` | словарь окна, ввода, текста, дисплеев, drag and drop, предпочтений, host notices, haptics, system chrome; трейты окна и граней; шов возможностей (`Capability`, провайдер, `Plugin`, регистратор, `Completion<T>`, `Unsupported`) | OS-код, `unsafe`, тестовые двойники, backend-таблицы, `tracing`, синхронный `Clipboard` |
| **`flui-native`** (сейчас `flui-platform`) | ядро-хост, tier H, layer 3, `internal`; зависит только `flui-app` | `Platform`, `AppHandler`, `OwnerPlatform`, `HostWindow`; нативные бэкенды Windows, macOS, iOS, Android, Web, Linux (Wayland и X11); headless; winit до прихода нативного Linux; OS-объекты core-сервисов (clipboard, `FileStore`); Java-часть Android | сервисы вне закрытого списка, публичные OS-типы, заглушки, провайдеры шва (маркеры возможностей лежат выше ядра, поэтому провайдеры собирает `flui-app`) |
| `flui-app` | корень композиции | единственный зависимый от `flui-native`; встроенные провайдеры шва для clipboard и storage поверх OS-объектов ядра; `Application::plugin`/`capability` | — |
| **`flui-system`**, затем `flui-notify`, `flui-device`, `flui-vault`, `flui-location`, … | крейты возможностей, `pkg`, `capability` | один набор разрешений ОС: маркер, handle, провайдеры ОС, симулирующий провайдер, conformance-таблица, декларации | зависимостей друг от друга |

Крейта на каждую ОС нет: winit 0.31 и GPUI разделились поверх открытого шва бэкенда и ради
внешних потребителей; у FLUI шов запечатан и второго потребителя нет, а компиляцию на одной
цели разделение не ускоряет (доводы и ссылки на winit и GPUI — в ADR-0151 §1). Триггеры
пересмотра перечислены там же.

## 4. Решения

| № | Решение | Против чего | Основание |
|---|---|---|---|
| D1 | Имена: контракт `flui-platform`, ядро `flui-native`; один крейт для всех нативных бэкендов | крейт на ОС (winit, GPUI); `flui-port`/`flui-os`/`flui-backend`; оставить `-api` | ADR-0151 §1, §10 |
| D2 | **Правило членства:** сервис входит в ядро, если его доставка требует объекта, которым владеет только хост (app delegate, Activity, оконная процедура, цикл событий), или если его потребляют виджеты самого фреймворка без проводки приложением. Всё остальное — крейт возможности. **Закрытый список по этому правилу:** запуск и активация (доставка URL и файлов при холодном и тёплом старте, reopen, передача аргументов единственному экземпляру), session end, host notices (memory pressure, сон и пробуждение), окна (состояние, команды, декорации и свой заголовок, материал фона, дочерние и всплывающие окна), дисплеи и темп кадров, ввод (указатель, клавиатура, IME и text store, drag and drop), доступность, clipboard, haptics, system chrome, системные предпочтения, меню (модель меню, строка меню, контекстные меню, меню Dock — его отдаёт app delegate), поверхности, каталоги данных. Jump list Windows (`ICustomDestinationList`, хосту не нужен), трей, уведомления, диалоги, открытие URL наружу — крейты возможностей | всё в ядре (GPUI: 76 методов `Platform`); меню и haptics плагинами (виджеты фреймворка не могут ими пользоваться); список без правила (растёт по вкусу) | ADR-0151 §3; ADR-0154 §4 |
| D3 | Правило содержимого контракта (§2 п. 2); конкретная судьба каждого модуля — §5 | «предусмотрительный» контракт | ADR-0151 §2 |
| D4 | Окно: снимок `WindowState` (с возможностями внутри) — единственная истина; команды только через `AdmittedCommand` (`admit` — единственный конструктор; `AdmissionError { Unsupported, InvalidGeometry }` для вызывающего, `ApplyError { Closed, Superseded, Backend }` для бэкенда, где `Superseded` означает, что возможности сменили ревизию между допуском и применением на следующем owner-ходе, и команду надо допустить заново); геттеры, дубли команд (`show`, `activate`, `close`) и `WindowEvent` удаляются; один `WindowHandler` при открытии вместо 14 `on_*`; `PlatformWindow` становится `!Send`, поток растеризации получает отдельную `Send + Sync` цель поверхности; покадровые глаголы возвращают `Result<(), Unsupported>` | геттеры «для удобства»; события с данными (winit); маршалинг между потоками (winit); одно перечисление в одно замыкание (Slint); очередь опроса (SDL); токен без `Superseded` (гонка «допустили на ревизии N, применили на N+1» уходит в `Backend` и неотличима от сбоя ОС) | ADR-0254 |
| D5 | Один словарь отсутствия: `Unsupported { reason }` с причинами `NotOnThisPlatform`, `NotOnThisDevice` (ОС или железо этого устройства не умеет; номер версии не называется), `NotDeclared { what }`, `Unavailable` (временно, повторить), `NotRegistered`, `NoWindow`; встраивается во все ошибки вместо девяти unit-вариантов; `Availability` и в `WindowCapabilities` | девять написаний; номер версии ОС в контракте; причина `NotOnThisVersion` (провоцирует вопрос «какой?» и ветвление по версии в приложении) | ADR-0152 §2 (п. 7); ADR-0244 |
| D6 | Стабильность: `clippy::exhaustive_enums` в контракте (закрытое перечисление — `#[allow]` с причиной); гейт API-замыкания (ADR-0089 §6) до публикации; `Platform` и `HostWindow` запечатаны явно; `PlatformWindow` открыт для тестовых двойников, дополнительные грани — отдельные трейты через `Option`; версия контракта едет с поездом, пока `cargo-semver-checks` не покажет его стабильным два поезда подряд, затем своя (ADR-0153 вариант 1) | своя версия сразу (контракт зависит от `flui-foundation` с `links = "flui_train"`; без поезда живут только rwh без зависимостей и C API); скрытый второй уровень для своих бэкендов (Slint) | ADR-0151 §5; ADR-0153 |
| D7 | Text store: в контракте остаются трейты, значения, `CommitGate`, `OwnerCalls`, `commit_composition_in_place`, `project_ime_event` (их вызывают бэкенды); `LockArbiter`, `CompositionLedger`, `EditGeneration`, `byte_range`, `committed_text` (их вызывает только `flui-widgets`; граница перепроверяется `rg` в самом PR переезда, `lock.rs` при этом делится на `CommitGate` и `LockArbiter`) переезжают в `flui-interaction::text_input` и реэкспортируются через `flui-sdk` для авторов текстовых виджетов | оставить 1,6 тыс. строк политики одного потребителя в Stable; перенести во `flui-widgets` (пакеты читают через `flui-sdk` в обоих случаях, а `flui-interaction` ниже и уже ведёт ввод текста) | ADR-0151 §6; заменяет закрепление ADR-0142 |
| D8 | Сервисы уровня приложения (clipboard, storage) доходят до виджетов только через шов возможностей; маркеры и handle живут у потребителей (`ClipboardCapability` во `flui-interaction`), OS-объекты — в ядре (owner-local clipboard с чтением через колбэк, `FileStore`), провайдеры — во `flui-app`; синхронный `Clipboard` удаляется из контракта (`read_text` нельзя честно реализовать на Web и Wayland); грани окна (text input, haptics, system chrome, drag source) остаются гранями окна и через шов не ходят | синхронный трейт с асинхронным близнецом (GPUI); провайдеры в ядре (ядру пришлось бы зависеть от `flui-interaction` ради маркера) | ADR-0152 §2 (п. 10); ADR-0151 §7 |
| D9 | Тестовые двойники уходят из контракта: `InMemoryTextStore` — во `flui-testing`; `InMemoryClipboard` и `FakeHaptics` делятся на две роли — headless-реализация в ядре (`pub(crate)`, headless-бэкенд не может зависеть от `flui-testing`) и сценарный двойник во `flui-testing` | оставить в контракте ради DRY (роли разные: буфер headless-хоста против сценария ответов теста) | ADR-0243 |
| D10 | Только для бэкенда — в ядро: `OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferUri`, `TransferImage`, `device_to_logical`, `logical_to_device`, `offset_from_coords`, `delta_offset_from_coords`, `DeviceInputRegion`, `ProjectedInputRegions`. Удаляются: `WindowEvent`, `Platform::on_window_event`, `KeyText`, `WindowBackgroundAppearance` (её заменяет команда материала фона с возможностью). `WindowOptions::input_latency` сегодня никто выше бэкенда не задаёт: опция проводится из конфигурации окна приложения (это её потребитель) в этой же серии. Контракт без `tracing` (логирует вызывающий) и на `std::sync::Mutex` вместо `parking_lot` | оставить мёртвую опцию «на будущее» | ADR-0151 §7 |
| D11 | Escape hatch: `as_any` переезжает в `HostWindow`; модули расширений ОС публикуются фасадом как `flui::native::<os>` за `cfg` и фичей `native` (Evolving); их методы принимают и возвращают только типы FLUI (прогресс на панели задач, материал, вкладки macOS) и отвечают `Option` или `Unsupported`, не паникуют; сырые handle окна и дисплея — только через трейты `raw-window-handle` (ADR-0089 §2); опции создания для одной ОС — типизированное значение этой ОС в `WindowOptions`; все OS-типы в путях ядра становятся `pub(crate)` | публичные `HWND`/`NSApplication`/`AndroidApp`; `unwrap` при downcast (winit) | ADR-0151 §8; ADR-0254 §8 |
| D12 | `Platform` худеет: удаляются `background_executor`, `BackgroundExecutor`, `Task`, `active_window`, `write_to_clipboard`, `read_from_clipboard`, `on_window_event`, `compositor_name`; в `flui-system` уходят `open_url`, `reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path`, `app_path`. `keyboard_layout` и его событие выше бэкенда никто не читает — они становятся `pub(crate)` ядра (их используют раскладка клавиш и TSF); публичными их сделает фича меню, когда появится потребитель (подписи сочетаний). Все регистрации уровня приложения (`on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`, `on_preferences_changed`) заменяет один `AppHandler`, переданный в `run`, по той же причине, что `WindowHandler`. `AppHandler` — трейт ядра (`flui-native`), не контракта: его реализует только `flui-app`, поэтому обязательство `opened(urls, files)` добавится к нему вместе с фичей активации, а не как инертное умолчание сейчас. Ядро перестаёт реэкспортировать имена контракта: у каждого элемента один путь | оставить `keyboard_layout` публичным без потребителя; `opened` в трейте сейчас с пустой реализацией в `flui-app` (инертное умолчание, правило 4) | ADR-0151 §9; ADR-0254 §4 |
| D13 | Устройство `flui-native` (§6): `platforms/<os>/{rules,ffi}`, семейство `platforms/apple/`, общий код двух и более семейств — модули верхнего уровня, названные по содержимому: `owner/` (owner-поток), `translate/` (перевод фактов ОС в словарь контракта), `version/` (пороги и гейты версий), `displays/` (лента реестра дисплеев); `shared/` и `traits/` исчезают; тесты правил — рядом с правилами; `module-dag` проверяет направления; одна точка вызова пользовательского кода вместо шести аренд | крейт на ОС; общий `shared/`; `backend/` (слово уже занято GPU-бэкендом и самими `platforms/<os>`); `shell/` для FFI-половины (слово занято «OS shell» — сервисами Explorer и Finder в `flui-system`); один `mapping/` для всего общего (grab-bag: гейты версий и лента дисплеев ничего не отображают) | ADR-0151 §10 |
| D14 | Android: `FluiActivity extends NativeActivity` в APK (готовый dex, `hasCode="true"`) с очередью Java → Rust; `FluiView` остаётся; a11y через низкоуровневый `accesskit_android::Adapter`; «назад» через `OnBackInvokedCallback`; insets и анимация IME через слушатели; dex грузится загрузчиком Activity; рукопожатие протокола; `targetSdk` закреплён в `platform-support.toml`; манифест генерируется | `hasCode="false"` и устаревший `Fragment`; GameActivity (Gradle, слабее IME); своя Activity со своей склейкой (заменяет android-activity без нужды) | ADR-0255 |
| D15 | iOS: чистый Rust на objc2, без Swift и Objective-C; `UITextInput` в отдельном невидимом responder-view; `accesskit_ios::Adapter`; колбэки сцены для URL, user activity и shortcut; клавиатура через уведомления (граница iOS 13); «назад» — краевой свайп по тому же контракту «можно ли вернуться», что Android | Swift-прослойка (второй тулчейн); iOS-бэкенд winit (нет UIScene) | ADR-0255 |
| D16 | Web: Rust владеет DOM (корень: canvas, хост ввода текста, хост семантики); `<textarea>` — контракт, `EditContext` — улучшение для Chrome; ARIA DOM через `accesskit_consumer`, включение лениво; маршруты через History API; страницу генерирует `flui-cli` | только `EditContext` (нет Firefox и iOS); ждать web-адаптер AccessKit (только в планах) | ADR-0255 |
| D17 | Шов возможностей: маркер и handle у владельца handle; провайдер на UI runtime; получение только в lifecycle-хуках; `Plugin` и регистратор — в контракте; один тип результата `Completion<T>`; `ProviderContext` даёт `owner()` (Send-poster) и `window()` (`Option<WindowRef>`); причины `Unsupported` из D5; `PermissionState { Granted(Grant), NotDetermined, Denied(Denial) }`, `Grant { Full, Reduced, Provisional }`, `Scope` как ассоциированный тип; проверка на Android не возвращает `Permanently`; без процесс-глобального состояния; интерфейс там, где потребители: маркер и handle возможности, которую называют виджеты фреймворка или пакетов (clipboard, ссылки), живут в самом нижнем крейте, который их называет, и только провайдер ОС — в листе (haptics сюда не относится: это грань окна, D8) | синглтон процесса (Flutter, Capacitor); handle как ключ (Tauri); двойные sync/async API (Tauri) | ADR-0152 |
| D18 | Крейты возможностей: один крейт на набор разрешений; нативный код по уровням — только Rust, затем встроенный dex, затем Gradle-модуль или SPM-пакет (никогда `javac`/`swiftc` из `build.rs`); набор крейтов приложения — по `cargo metadata`; декларации — строгие `[package.metadata.flui.capability]` с фичами, `flui-cli` генерирует манифесты и умеет проверять; приложение переопределяет usage-строки; `flui add <capability>`; сценарный примитив в фиче `testing` контракта, симулирующий провайдер в каждом крейте, одна conformance-таблица; поддержка — только исполненный прогон. Первый крейт — `flui-system`: диалоги, URL, файлы, reveal, trash, share, message box, автозапуск, глобальные сочетания, прогресс и значок на панели задач, jump list, пути и сведения об ОС. Трей входит в `flui-system` вместе с фичей меню: без модели меню трей — только значок | патчинг манифестов в `build.rs` (Tauri); только документация (Flutter iOS, Electron); трей сейчас без меню (половина сервиса) | ADR-0154 |
| D19 | Мост к хосту: сначала `owner()` и `WindowRef`; Android — очередь `FluiActivity`: `launch_for_result` и `request_permissions` возвращают `Completion`, `JavaVM` и Activity — сырые указатели в контексте, не `static`; Apple — события в типах FLUI (URL, reopen, сцена); Win32 — ничего, пока нет потребителя; Linux — порталы по raw handle | регистрация объектов протоколов objc2 (утечка типов и версий) | ADR-0154 §9; ADR-0255 |
| D20 | Нативный Linux (Wayland и X11) — часть серии, после архитектуры; строится на живом слое `platforms/linux` (дисплеи, logind, AT-SPI); потом winit удаляется вместе с переводом CI на нативный Linux | оставить winit навсегда | ADR-0151 §11 |
| D21 | Переименование — первые два PR серии (ядро, затем контракт); без проверки «отставного имени» (между шагами крейта `flui-platform` нет, компилятор ловит любое использование); `git mv` и `sd` по целому токену; ловушки — §9 | один PR (ломает `git blame` у совпадающих путей); `cargo xtask rename` ради одного переименования | ADR-0151 §12 |
| D22 | `AppLifecycleState` остаётся во `flui-scheduler`: это агрегат runtime (ADR-0035 §4); бэкенды публикуют факты presentation (`WindowExecutionState`, видимость) | перенос в контракт (политика фреймворка в Stable) | ADR-0151 §13 |
| D23 | Версии ОС: решение ADR-0244 верно и остаётся; добавляется закреплённый `targetSdk` Android в `platform-support.toml` | `cfg` по версии; «последний установленный SDK» | ADR-0151 §14; ADR-0255 |
| D24 | Доступность не опциональна: фича `a11y` удаляется из ядра, `flui-app` и фасада; адаптеры AccessKit компилируются всегда на каждой ОС. Довод «на Linux это тянет D-Bus» снимается нативным Linux-бэкендом, которому D-Bus нужен и так (logind, порталы) | фича, выключенная по умолчанию (продукт, который «готов», выходит без доступности, если автор не знал про фичу) | ADR-0151 §15; `crates/flui-app/Cargo.toml:50-56` |
| D25 | **Планка качества — требования с гейтами, а не пожелания** (раздел «Quality bar» в platform-parity.md): ноль CPU в простое (каждый сигнал ОС — уведомление; опрос по таймеру — дефект, а не fallback); паника пользовательского кода никогда не разматывается в C — гейт проверяет, что каждая `extern "C"` проходит через `panic_boundary`; один монотонный таймлайн для ввода, кадров и показа; обратная связь о времени показа кадра (DXGI statistics, `addPresentedHandler`, `wp_presentation_time`, frame timelines, rAF) питает frame clock; HDR, широкий охват и цветовой профиль дисплея — словарь контракта (`SurfaceColorSpace`, headroom, смена профиля); sandbox-aware доступ к файлам (порталы, security-scoped bookmarks, SAF, AppContainer) — слой никогда не отдаёт путь, которым процесс не может пользоваться; защищённый ввод пароля и скрытые записи буфера; активация по правилам ОС (`xdg-activation`, передача права на передний план); conformance каждой ОС исполняется с настоящей ассистивной технологией (Narrator, VoiceOver, Orca, TalkBack) через `cargo heli xtask device`; записи нативного ввода воспроизводятся на headless детерминированно и покрывают IME, lifecycle, предпочтения и дисплеи, а не только указатель. Эти пункты входят в закрытый список D2 как свойства существующих областей (дисплеи, ввод, текст, lifecycle, каталоги данных) | «качество потом» (слой, который компилируется на шести ОС и опрашивает ОС по таймеру, — это и есть «полуходячий») | ADR-0151 §16; platform-parity.md «Quality bar» |
| D26 | Контракт `flui-platform` группируется по областям вместо 28 плоских модулей: `window/` (опции, состояние, команды, возможности, размещение, трейт окна, обработчик, яркость), `input/` (`PlatformInput`, указатель, клавиатура, возможности и регионы ввода, `EventTime`), `text/` (`PlatformTextInput`, `ImeEvent`, `text_store`), `display/`, `preferences/` (предпочтения, локаль), `transfer/` (data transfer, drag), `haptics/`, `capability/`, `menu/`, `unsupported`, `notice` (host notices), `system_chrome`, `storage`, `target_platform`. Корневые реэкспорты имён, которыми пользуются фасад и `flui-sdk`, остаются: путь `flui::platform::WindowOptions` не меняется; меняются пути модулей, что до публикации бесплатно | оставить плоский корень (дерево модулей Stable-крейта — публичный API, и 32-строчные модули верхнего уровня его замусоривают) | опись §A1; `crates/flui-platform-api/src/lib.rs` (24 корневых `pub use`) |
| D27 | **Драйвер — единственный способ гонять приложение.** Conformance-таблица бэкенда выражается сценариями `flui-driver` (ADR-0238): один и тот же сценарий исполняется на headless в CI (in-process lane, без транспорта) и через `os`-lane на устройстве (`cargo xtask device <check>` — сценарий на драйвере, как решила спека драйвера). Зелёная ячейка parity — сценарий, прошедший на этой ОС через `os`-lane, или ручной прогон; headless-lane доказывает путь фреймворка, не бэкенд. Headless-бэкенд — эталонный бэкенд драйвера: его `inject_event` и `simulate_*` остаются API набора conformance, а синтетический ввод драйвера входит через путь хоста в `flui-runtime` (ADR-0239 п. 4), то есть через тот же `WindowHandler::input` | четыре способа гонять приложение (headless `inject_event`, replay записей, xtask device по ОС, desktop-mcp) — каждый со своей проверкой | ADR-0238, ADR-0239; спека flui-driver §«Device scenarios» |
| D28 | **Журнал нативного ввода — модуль ядра.** Кодек нативных записей ADR-0215 §4 лежит в `tests/native_recording/`, а ADR-0241 производит записи во время работы (`record.start{native}`); он переезжает в `flui-native/src/owner/journal/` (захват того, что произвёл бэкенд до перевода, и кодек), фича `native-capture` сливается с `driver`, схема записи объявляется во `flui-protocol` (у него нет зависимостей, его читает `flui-driver`), а ядро её реализует. Один журнал на границе хоста плюс журнал `HostInput` в runtime — это и есть «наблюдаемость платформенной границы» из планки качества, и оба встраиваются в трассу драйвера | кодек в тестах при записи во время работы; вторая схема у драйвера | ADR-0215 §4, ADR-0241 |
| D29 | **Сценарный примитив — фича `scripted` контракта,** не `testing`: упорядоченный сценарий ответов на запросы, переключения служб, отзывы и инъекции ошибок, разрешаемые на границе owner-хода (ADR-0154 п. 10). Его включают `flui-testing` и `flui-app` под `driver`, чтобы драйвер подменял диалог или разрешение в debug-сборке без OS-диалога. В той же серии 15 двойников `PlatformWindow` по крейтам сводятся к одному в `flui-testing` (ADR-0243) | примитив только под `testing` (драйвер не может им пользоваться в приложении) | ADR-0154 п. 10, ADR-0243 |

## 5. Контракт `flui-platform`: судьба модулей

| Модуль | Судьба |
|---|---|
| `brightness`, `locale`, `target_platform`, `event_time`, `keyboard/`, `pointer/`, `ime`, `text_input`, `input_capabilities/`, `display`, `placement`, `window_state`, `host_notice`, `preferences/`, `system_chrome`, `haptic_feedback`, `haptics/` (без `FakeHaptics`), `drag`, `storage` | остаются; `keyboard` без `KeyText`; `display` на `std::sync::Mutex` |
| `window`, `platform_window` | переделываются по ADR-0254: снимок, `AdmittedCommand`, `WindowHandler`, `!Send` окно и `SurfaceTarget`; удаляются `WindowEvent`, `WindowBackgroundAppearance`, геттеры и дубли команд |
| `input` | остаются `PlatformInput`, `DispatchEventResult`, `DragDropEvent`; координатные помощники — в ядро |
| `input_regions` | остаются `InputRegion`, `InputRegionKind`; `DeviceInputRegion`, `ProjectedInputRegions` — в ядро |
| `data_transfer` | словарь остаётся; `OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferUri`, `TransferImage` — в ядро |
| `clipboard` | удаляется; clipboard — возможность ядра через шов (`ClipboardHandle` во `flui-interaction`) |
| `text_store/` | остаются трейты, значения, `CommitGate`, `OwnerCalls`, `commit_composition_in_place`, `project_ime_event`; машинерия виджетов — во `flui-interaction`; `InMemoryTextStore` — во `flui-testing` |
| новый `unsupported` | `Unsupported`, `UnsupportedReason`, `Availability` |
| новый `capability` | `Capability`, `CapabilityProvider`, `ProviderContext`, `WindowRef`, `Plugin`, `CapabilityRegistrar`, `Completion<T>`, `PermissionState` и родня |
| новый `menu` | модель меню (строка, контекстное, меню Dock; jump list — `flui-system`, D2) — вместе с первой реализацией и потребителем |

## 6. `flui-native`: устройство

```text
crates/flui-native/
  android/                 Java-исходники, готовый classes.dex, запись хэша (ADR-0247, ADR-0255)
  src/
    lib.rs                 current_platform(): единственная лестница выбора бэкенда
    platform.rs            Platform (запечатан), AppHandler
    host_window.rs         HostWindow (запечатан), as_any
    owner/                 OwnerPlatform, SharedPlatform, PlatformProxy, сигнал и слот owner-потока,
                           граница паники, единая точка вызова пользовательского кода,
                           journal/ — журнал нативного ввода и его кодек (ADR-0215, фича driver)
    translate/             перевод фактов ОС в словарь контракта, общий для двух и более семейств:
                           pointer_assembly, keys, window_state (режим из флагов), system_notice,
                           haptic_degradation, input_availability
    version/               пороги версий ОС и гейты Since<F> (ADR-0244)
    displays/              лента реестра дисплеев (display_source)
    storage/               FileStore, страничное хранилище Web
    platforms/
      mod.rs               один гейт на бэкенд
      windows/{rules,ffi}    rules: cfg(any(windows, test)), forbid(unsafe_code); ffi: cfg(windows), unsafe
      apple/{rules,ffi}      общее для macOS и iOS
      macos/ ios/ android/ web/ linux/   та же пара
      winit/               заменитель Linux до нативного бэкенда
      headless/            компилируется всегда, выбирается во время работы
```

- Код одной ОС живёт в её каталоге; код одного семейства — в семействе; только код двух и более
  семейств попадает в `owner/`, `translate/`, `version/` или `displays/` — в тот, что называет его
  содержимое; нового общего каталога «для всего» нет.
- `rules` не называет `ffi`, крейты привязок и соседние ОС; это проверяет компилятор на хосте CI
  и `cargo xtask module-dag`, расширенный на `flui-native`: `owner`, `translate`, `version` и
  `displays` не зависят от `platforms`, `platforms/<a>` не зависит от `platforms/<b>` (кроме
  `macos`/`ios` → `apple`), `rules` не зависит от `ffi`.
- Тесты правил — модулями рядом с правилами (`pub(crate)`); `tests/` хранит conformance и записи
  нативных трасс. Определение готовности в AGENTS.md получает это исключение.
- Пользовательский код (обработчики приложения и окна) вызывается через одну точку, которая
  отпускает локи и заёмы до вызова; шесть нынешних аренд удаляются.

## 7. Шов и крейты возможностей

Решения D17–D19. Форма шва — [ADR-0152](../../../adr/ADR-0152-capability-seam-revised.md),
класс крейтов — [ADR-0154](../../../adr/ADR-0154-capability-crates.md). Шов сливается вместе с
первым потребителем: clipboard ядра (синхронный трейт удаляется в той же посадке), затем storage,
затем `flui-system`.

## 8. Хосты

Решения D14–D16, полностью — [ADR-0255](../../../adr/ADR-0255-mobile-and-web-hosts.md).
Триггеры ухода с `NativeActivity` на Android: нативные view внутри UI или `InputLatency::Low`
(нужен Java-путь касаний).

## 9. Переименование: механика и ловушки

1. **PR 1:** `crates/flui-platform` → `crates/flui-native` (пакет, библиотека, пути, фичи вроде
   `flui-native/a11y`, `allowed-dependents`, `reach.tier.K.forbid`, `globals` и пути xtask,
   `.config/nextest.toml` `package(flui-native)`, `weekly.yml --exclude flui-native`, цели
   `tracing` и строки `RUST_LOG`, имя потока `flui-native-bg`).
2. **PR 2:** `crates/flui-platform-api` → `crates/flui-platform` (`flui_platform_api` →
   `flui_platform`, `SDK_SURFACE`, генератор `key_vocabulary`, `binary_id(flui-platform::compile_fail)`,
   фича `flui-platform/serde`).
3. **Инструмент:** `git mv` и `sd` по целому токену, длинное совпадение первым; не трогаются
   архивные `docs/plans` и `docs/research`, текст ADR-0151 и выпущенные записи `CHANGELOG.md`.
   Каждый PR приводит точные команды, чтобы любая ветка могла их повторить.
   **Щель между шагами:** после PR 2 имя `flui-platform` снова существует и означает контракт,
   поэтому строка вроде `--exclude flui-platform`, принесённая веткой, написанной до PR 1 и
   влитой после PR 2, молча нацелится на контракт. Поэтому PR 2 начинается с проверки, что в
   неархивных файлах не осталось токенов `flui-platform`/`flui_platform` (кроме ADR-0151), а
   ветка, вливающаяся после PR 2, применяет обе карты по порядку: сначала карту PR 1, затем
   карту PR 2.
4. **Ловушки:** trybuild `.stderr` перегенерируются (`TRYBUILD=overwrite`), а не заменяются
   текстом: в них номера колонок; `Cargo.lock` пересобирает cargo; строки `flui_native_recording`
   и `flui-native-library-*` уже существуют и к переименованию не относятся; реэкспорты контракта
   через ядро после PR 1 читаются как `flui_native::WindowOptions` и остаются до задачи, которая
   переводит `flui-app` на путь контракта (D12).

## 10. Что заменяется

| ADR | Что |
|---|---|
| ADR-0082 | §1 (состав контракта), §2 (имена), §4 шаг 2 (замена `Send`-колбэков регистраторами owner-потока), §5 строка Linux (winit как бэкенд Linux) — ADR-0151, ADR-0254 |
| ADR-0142 | место машинерии виджетов (`LockArbiter`, `CompositionLedger`, `EditGeneration`) — ADR-0151 |
| ADR-0237 | геттеры и колбэки фактов рядом со снимком — ADR-0254 (тройка остаётся) |
| ADR-0063 | `Send + Sync` окно как цель поверхности — ADR-0254 (цель поверхности становится отдельным объектом) |
| ADR-0247 | п. 1, 2, 9: доставка dex и загрузчик — ADR-0255 |
| ADR-0084 | целиком — ADR-0152 |
