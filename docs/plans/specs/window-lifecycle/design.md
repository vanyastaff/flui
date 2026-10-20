# Window lifecycle — design

- **Статус:** черновик, решения владельца приняты 2026-10-18
- **Дата:** 2026-10-18
- **База:** `main` @ `eb0f4903c`
- **Требования:** [requirements.md](requirements.md)
- **Задачи:** [tasks.md](tasks.md)
- **ADR:** [ADR-0237](../../../adr/ADR-0237-window-state-and-host-notices.md) (Proposed)
- **Связанные:** ADR-0222 (форма наблюдения окна), ADR-0236 (снимок дисплеев, `DisplaySource`),
  ADR-0078 (ёмкости на `LifecycleContext`), ADR-0091 (один поток владельца), ADR-0035 (жизненный
  цикл), ADR-0123 (исключительное закрытие), ADR-0039 (main-thread операции)

## Текущее состояние

Факты с `file:line` — в разделе «Текущее состояние» требований. Коротко: команды окну частично есть
на `PlatformWindow`, но разбросаны по десятку методов с заглушками-умолчаниями, приложению
недоступны и на половине бэкендов молча ничего не делают; чтение состояния — умолчания, которые
выглядят как ответ (`false`, `Bounds::default()`, пустая строка); рядом лежит второй trait окна.
Системные события — один объявленный, но нигде не подключённый хук `on_session_end`; остального
нет в контракте.

## Варианты

**A. Дописать недостающие методы.** Добавить `set_position`, `set_min_size`, … в `PlatformWindow`,
выставить их через ручку как есть, события режима — через существующие `WindowEvent::Minimized` и
т. п. Минимум кода, но сохраняет три дефекта: умолчание неотличимо от ответа, «не поддерживается»
неотличимо от «сделано», состояние собирается из шести событий, которые расходятся с чтением.

**B. Снимок + команды + ёмкости.** Окно публикует неизменяемый `WindowState` с ревизией и
payload-free уведомление «состояние изменилось» (как appearance, ADR-0222); все изменения идут одной
командой `WindowCommand` через один метод `apply`, которая сначала сверяется с `WindowCapabilities`.
Приложение получает `WindowHandle` с очередью команд на поток владельца. Системные события —
отдельный тип `HostNotice`, доставляемый через `LifecycleHandle`.

**C. Состояние как сигнал реактивного графа.** `WindowState` пишется прямо в `Signal`
презентации, команды — запись в «желаемый» сигнал, бэкенд сходится к нему. Красиво для чтения в
`build`, но «желаемое состояние» спорит с ОС (пользователь развернул окно, а желаемое — обычное:
кто прав?) и требует разрешения конфликтов, которого у Flutter, Compose и GPUI нет по причине.

**Выбор: B.** Снимок и уведомление уже проверены на дисплеях (ADR-0236) и appearance (ADR-0222);
одна команда даёт одну точку проверки ёмкостей и одну точку очереди; приложение может само
положить снимок в сигнал, если хочет читать его в `build` (R1–R3 не требуют реактивности).

**Ответы на открытые вопросы.**

1. Снимок хранит бэкенд в общем `WindowStateCell` (D-3); ревизию выдаёт ячейка.
2. Команда ставится в очередь на `WindowHandle` и применяется `flui-app` в начале следующего хода
   владельца, до кадра (D-6).
3. Постоянный ключ — новое поле `DisplayInfo::persistent_key` (D-10).
4. Бюджеты — таблица D-13.
5. Уровни — таблица D-15.
6. Порядок — `tasks.md`.

## Решения

- **D-1. Один trait окна.** `PlatformWindow` — единственный trait окна. Удаляются
  `flui_platform::window::{Window, WindowBuilder, WindowManager}`, macOS `window_manager.rs`, старые
  `WindowMode` и `WindowBounds` (подложки Mica/Blur из `WindowMode` переезжают в уже существующий
  `set_background_appearance`, пробел 20 их не трогает), методы `set_title`, `get_title`, `minimize`,
  `maximize`, `restore`, `toggle_fullscreen`, `resize`, `bounds`, `window_bounds`, `is_maximized`,
  `is_fullscreen`, `on_moved` и варианты `WindowEvent::{Moved, Minimized, Maximized, Restored,
  Fullscreen, ExitFullscreen}`. Их место занимают `state()`, `capabilities()`, `apply()`,
  `on_state_changed()`. Код, читавший старые методы (раннер, `first_reveal.rs`, примеры), переходит
  на снимок в той же задаче.

- **D-2. `WindowState` — значение.** Поля (все `pub`, структура `#[non_exhaustive]`):
  - `revision: WindowRevision`;
  - `outer: WindowGeometry { position: Option<Point<f64>>, size: Size<f64> }` и
    `content_size: Size<f64>` — логические пиксели; `position` `None`, где ОС глобальную позицию не
    сообщает (Wayland, Android, iOS);
  - `mode: WindowMode { Normal, Minimized, Maximized, Fullscreen }` (новый, без подложек);
  - `normal_geometry: Option<WindowGeometry>` — границы, в которые окно вернётся из
    развёрнутого или полноэкранного режима, если ОС их сообщает;
  - `visible`, `focused: bool`, `title: Option<String>` (`None` — ОС заголовка не имеет);
  - `constraints: SizeConstraints { min: Option<Size<f64>>, max: Option<Size<f64>> }`, `resizable`,
    `decorated: bool`;
  - `display: Option<DisplayId>` — дисплей окна из снимка ADR-0236.

  Свёрнутость на Wayland не наблюдаема: там `Minimized` не публикуется никогда, и это записано в
  `WindowCapabilities::observes_minimized`.

- **D-3. `WindowStateCell`.** Общая ячейка в `crates/flui-platform/src/shared/window_state.rs`
  по образцу `DisplaySource`: `Mutex<WindowState>` плюс флаг «устарело». Бэкенд на событие ОС
  (`WM_SIZE`, `WM_MOVE`, `WM_WINDOWPOSCHANGED`, `windowDidResize`, winit `Resized`/`Moved`,
  `fullscreenchange`) зовёт `invalidate()` и будит владельца; `state()` при устаревании заново читает
  ОС без удержания блокировки на время вызова ОС и публикует через `publish(sample)`, который
  сравнивает с прошлым значением без ревизии: равное значение ревизию не трогает (R2). Уведомление
  `on_state_changed` вызывается после публикации, вне блокировки, только если ревизия выросла.
  Исчерпание ревизии — `WindowRevisionExhausted`, после которого ячейка больше не публикует и одна
  диагностика уходит в лог (R4).

- **D-4. `WindowCapabilities`.** Значение-набор (`#[non_exhaustive]` struct с `bool`-полями, не
  битовая маска наружу): `sets_title`, `sets_position`, `sets_size`, `sets_constraints`,
  `sets_resizable`, `sets_decorated`, `minimizes`, `maximizes`, `enters_fullscreen`, `hides`,
  `shows`, `activates`, `observes_position`, `observes_minimized`, `places_at_open`; наборы
  `WindowCapabilities::NONE` и `ALL` — константы. Набор известен до создания окна
  (`Platform::window_capabilities`), чтобы восстановление подогнало размещение под то, что окно
  умеет (`places_at_open`). Бэкенд отдаёт его один раз на окно
  (может уточнить после создания — тогда ревизия состояния растёт). `apply` обязан отказать
  `Unsupported` ровно тогда, когда соответствующее поле `false`; conformance-набор (R39) проверяет
  это для каждой команды.

- **D-5. `WindowCommand`.** `#[non_exhaustive]` enum: `SetTitle(String)`, `SetPosition(Point<f64>)`,
  `SetSize(Size<f64>)` (внутренний размер), `SetConstraints(SizeConstraints)`, `SetResizable(bool)`,
  `SetDecorated(bool)`, `SetMode(WindowMode)` (все переходы: `Normal` из любого — restore;
  `Fullscreen` — без рамки, R12), `Show`, `Hide`, `Activate`. Закрытие — не команда окна, а
  существующий путь `request_close` с вето (ADR-0123), ручка его только вызывает.
  `PlatformWindow::apply(&self, WindowCommand) -> Result<(), WindowCommandError>` синхронен; ошибки:
  `Unsupported`, `InvalidGeometry` (неконечное, ≤ 0, `min > max` — проверяется в
  `WindowCommand::validate` до ОС, R10), `Closed`, `Backend { message }`.

- **D-6. Путь команды и `WindowHandle`.** `WindowHandle` живёт в `flui-view` (`!Send`, слабая ссылка
  на ячейку презентации), выдаётся `LifecycleContext::window()` (R5; в `BuildContext` метода нет,
  doctest `compile_fail` это закрепляет). Ручка вызывает `WindowControl` — новый объектный trait в
  `flui-platform-api`, который реализует `flui-app` над `HostWindow`:
  `state()`, `capabilities()`, `submit(WindowCommand) -> Result<(), WindowCommandError>`.
  `submit` сразу проверяет `validate` и ёмкость (R7, R10) и ставит команду в owner-local очередь;
  очередь вычерпывается в начале следующего хода владельца, до кадра, по порядку (R8), и
  `submit` будит владельца. Почему не сразу: Win32 `SetWindowPos` синхронно шлёт `WM_SIZE`, Cocoa —
  `windowDidResize`; вызов ОС из обработчика жеста вернул бы в runtime событие посреди
  пользовательского кода (повторный вход). Ошибка `Backend` при вычерпывании не теряет остаток
  очереди; она уходит в диагностику с `WindowId`, следующая команда применяется. Ручка также даёт
  `state()` и `subscribe(FnMut(&WindowState))` с той же семантикой, что `LifecycleHandle::subscribe`
  (FIFO, без синхронного вызова, сдерживание паники). Отклонение от первоначального замысла:
  `WindowRequest` возвращает только `PresentationAddress`, а ручку выдаёт отдельный
  `flui_app::window_handle(address) -> Option<WindowHandle>` на потоке владельца. Причина:
  `WindowHandle` — `!Send`, а `WindowRequest` уезжает через `Send`-future, так что ручку нельзя
  положить в ответ. Цена — второй вызов после `await`, и окно между ними могло закрыться: поиск
  возвращает `None` для закрытого окна (и для другого потока, и для неизвестного адреса), поэтому
  вызывающий обязан обрабатывать `None`, как закрытое окно.

- **D-7. Идентичность ручки.** Ручка хранит `WindowId` и поколение презентации; после закрытия
  окна (терминальное уведомление ADR-0123) ячейка закрыта, команды — `Closed`, подписки сняты. Новое
  окно с тем же заголовком — другая ручка (R9).

- **D-8. Опции создания.** `WindowOptions` получает `position: Option<Point<f64>>`,
  `mode: WindowMode`, `normal_geometry: Option<WindowGeometry>` (для восстановления развёрнутого
  окна с правильными границами «до») и прежние поля. `From<&AppConfig> for WindowOptions` больше не
  перезаписывает `visible`/`reveal`; ни одно поле `AppConfig` не теряется (R13, таблица
  `config_to_options_keeps_every_field`). Бэкенд, не умеющий поле, отражает это в снимке
  (позиция `None`, режим `Normal`) и в ёмкостях.

- **D-9. Показ после первого кадра.** Раннер всегда открывает окна с
  `WindowReveal::AfterFirstFrame` (уже так в `runner/desktop.rs:56`); задача — чтобы бэкенды это
  соблюдали:
  - **Windows:** создать без `WS_VISIBLE`, применить режим и позицию, показать в
    `reveal_after_first_frame` через `ShowWindow` с командой, соответствующей режиму
    (`SW_SHOWMAXIMIZED` и т. п., R15); тёмный фон класса окна не нужен.
  - **X11 (winit):** `with_visible(false)` и `set_visible(true)` после первого кадра.
  - **Wayland (winit):** окно не получает буфер до первого кадра — compositor не показывает
    surface без буфера, `show` больше не `Unsupported` для первого показа; явный `Hide` на Wayland —
    `Unsupported` (протокол не умеет).
  - Запасной показ: существующий ограниченный по времени таймер `first_reveal.rs` (R14).

- **D-10. Постоянный ключ дисплея.** `DisplayInfo` получает `persistent_key: Option<String>` —
  строку, которая переживает перезагрузку: Windows — `monitorDevicePath` из
  `DISPLAYCONFIG_TARGET_DEVICE_NAME` (не LUID: он меняется при перезагрузке); macOS — UUID дисплея;
  X11 — имя выхода RandR плюс EDID-идентификатор (производитель, модель, серийный номер), если есть;
  Wayland — имя коннектора; headless — внедрённое; Android, iOS, Web — `None` (позиции нет). Ключ
  сравнивается только на равенство (идентичность, не метка), а при `None` или промахе —
  запасной вариант D-11.

- **D-11. `WindowPlacement` и модель восстановления.** Значение `WindowPlacement { display:
  Option<String>, offset: Option<Point<f64>>, size: Size<f64>, mode: WindowMode, scale: f64,
  maximized_before_minimize: bool }`, где `offset` — позиция внешних границ обычного окна
  относительно начала рабочей области своего дисплея в логических пикселях этого дисплея (так
  смешанный DPI не искажает позицию); `None`, если ОС позицию не сообщила, — тогда окно при
  восстановлении не позиционируется. `size` — размер содержимого. Сериализация — свой компактный
  формат с версией (`flui-window-placement/1`, без новой зависимости; `to_bytes`/`from_bytes`),
  испорченная, повторяющая поле или чужая версия — `PlacementError`, одна диагностика, окно
  открывается по `AppConfig` (R19). Чистая функция `resolve(placement, &DisplaySnapshot,
  &WindowCapabilities, frame) -> Option<ResolvedPlacement>` (модель, таблица): найден ли дисплей по
  ключу; иначе основной; дисплеи с нефинитным или неположительным масштабом и с пустой рабочей
  областью не кандидаты; подгоняется внешний размер (содержимое плюс рамка, которую даёт
  `Platform::window_frame_inset`, у headless — ноль) к рабочей области, позиция сдвигается так,
  чтобы окно целиком было на ней (R18); `Minimized` превращается в `Normal` или `Maximized` по
  `maximized_before_minimize` (R19); без `places_at_open` позиция не применяется (R20).
  `ResolvedPlacement { display, offset_in_display, size, mode }` остаётся в логических пикселях
  своего дисплея: бэкенд переводит в пиксели через масштаб этого дисплея. Геометрия — это обычные
  границы окна; открытое развёрнутым или полноэкранным окно возвращается к ним
  (`WindowOptions::normal_geometry`).

- **D-12. Кто сохраняет.** `AppConfig::restore_placement(name)` включает `PlacementRecorder` в
  `flui-app`: при открытии читает `storage` (до первого показа, R17 — чтение синхронно из
  локального файла на desktop, ограничено по размеру), применяет `resolve` в `WindowOptions`
  (возможности и рамка — от платформы до создания окна: `Platform::window_capabilities`,
  `Platform::window_frame_inset`); подписан на `WindowState` и пишет не чаще раза в 2 с после
  изменения (изменение внутри интервала пишется, когда он истёк: хвостовая запись по таймеру
  владельца), плюс сразу при закрытии окна и при `SessionEnded` (один общий бюджет на все окна) и
  при Android `SaveState`. Запись считается сохранённой, только когда `Storage::publish` ответил
  `Ok`. Имя записи — `StorageName` (`machine_local("window-main")`): имя `StorageName` — это
  `&'static str` из `[a-z0-9_-]`, поэтому не `window-placement/<key>`. Свёрнутое окно
  сохраняется по `normal_geometry`.

- **D-13. Конец сессии.** `SessionEndPhase`/`SessionEndAnswer` переезжают в `flui-platform-api`
  как часть `HostNotice`; хук становится `OwnerPlatform::on_session_end` (сейчас его нет — главная
  причина, почему `flui-app` не мог подписаться). Доставка синхронна на потоке владельца:
  - `HostNotice::SessionEnding(&SessionEndRequest)` — подписчик может вызвать
    `request.block(reason) -> SessionEndBlock`; пока хоть один `SessionEndBlock` жив, ответ ОС —
    `Block`; `drop`/`release()` последнего блока снимает его (R23). Агрегат по всем презентациям
    ведёт `flui-app`.
  - `SessionEndCancelled`, `SessionEnded` — уведомления. На `SessionEnded` `flui-app` сохраняет
    положение окон и ждёт записи принятых значений `storage` в пределах бюджета (R24).

  | ОС | Запрос | Block | Бюджет записи на `Ended` |
  |---|---|---|---|
  | Windows | `WM_QUERYENDSESSION` на скрытый top-level приёмник (`preferences/receiver.rs`) | вернуть `FALSE` + `ShutdownBlockReasonCreate(hwnd, reason)`; снятие — `ShutdownBlockReasonDestroy` | 4 с внутри `WM_ENDSESSION` (ОС ждёт ≈5 с) |
  | macOS | `applicationShouldTerminate:` + `NSWorkspaceWillPowerOffNotification` | `NSTerminateLater`, затем `replyToApplicationShouldTerminate:` при снятии | до ответа, не более 4 с |
  | Linux | logind `PrepareForShutdown(true)` через delay-inhibitor (`Inhibit("shutdown", …, "delay")`) | нет (только уведомление) | до `InhibitDelayMaxSec` (по умолчанию 5 с), не более 4 с |
  | iOS | `applicationWillTerminate` | нет | синхронно в вызове |
  | Android | `SaveState`/`Stop` (процесс могут убить без предупреждения) | нет | синхронно |
  | Web | `pagehide` | нет | синхронно (IndexedDB — best effort, документировано) |

  Паника подписчика сдерживается и не блокирует выход (R25); «не ответил» в синхронной доставке
  невозможен — ответ есть по возврату из подписчиков; бюджет относится к ожиданию записи.

- **D-14. `HostNotice`.** `#[non_exhaustive]` enum в `flui-platform-api`: `SessionEnding(..)`,
  `SessionEndCancelled`, `SessionEnded`, `Sleep`, `Wake`, `SessionLocked`, `SessionUnlocked`,
  `MemoryPressure(MemoryPressureLevel)`. Источник — `OwnerPlatform::on_host_notice(FnMut(SystemNotice))`
  (`SystemNotice` — те же уведомления без `SessionEnding`, `Copy` и `Send`; у `SessionEnding` —
  отдельный хук с ответом, D-13). Общая диспетчеризация уведомлений в `flui-platform` на `Wake`
  сначала помечает устаревшими `DisplaySource` платформы и каждую ячейку состояния окна. `flui-app` доставляет каждое уведомление во
  все `UiRuntime` владельца; `flui-runtime` передаёт его в `LifecycleHandle` каждой презентации.
  `LifecycleHandle::subscribe_notices(FnMut(&HostNotice))` — та же реализация очереди и сдерживания,
  что `subscribe` (R22). `WidgetsBindingObserver::{did_have_memory_pressure, did_request_app_exit}` и
  `handle_memory_pressure` удаляются.

  | Уведомление | Windows | macOS | Linux | Android | iOS | Web |
  |---|---|---|---|---|---|---|
  | `Sleep`/`Wake` | `WM_POWERBROADCAST` `PBT_APMSUSPEND`/`PBT_APMRESUMEAUTOMATIC` | `NSWorkspaceWillSleep`/`DidWake` | logind `PrepareForSleep(true/false)` + delay-inhibitor | — | — | — |
  | `SessionLocked`/`Unlocked` | `WTSRegisterSessionNotification` → `WM_WTSSESSION_CHANGE` | `com.apple.screenIsLocked`/`Unlocked` (distributed) | logind сессия `Lock`/`Unlock` | — | — | — |
  | `MemoryPressure` | `CreateMemoryResourceNotification(Low)` + `RegisterWaitForSingleObject` → `Critical` | `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` WARN/CRITICAL | PSI `/proc/pressure/memory` триггеры some/full | D-15 | `didReceiveMemoryWarning` → `Critical` | — |

  После `Wake` `flui-app` инвалидирует `DisplaySource` и все `WindowStateCell` и берёт период
  кадров заново (R26).

- **D-15. Уровни памяти.** Два уровня — `Moderate` («освободи то, что легко восстановить») и
  `Critical` («освободи всё, что не нужно для текущего кадра»). Android без Java не получает
  `onTrimMemory` (нужен Java-объект `ComponentCallbacks2`, как отклонено в display-pacing D-9):
  источник — `MainEvent::LowMemory` → `Critical` и чтение
  `ActivityManager.getMyMemoryState(…).lastTrimLevel` через JNI на `LowMemory`, `Pause`, `Stop` и
  `Resume` (без опроса): `TRIM_MEMORY_RUNNING_MODERATE`/`RUNNING_LOW`/`BACKGROUND` → `Moderate`,
  `RUNNING_CRITICAL`/`COMPLETE` → `Critical`; повтор того же `lastTrimLevel` не доставляется.
  Linux PSI: `some 150ms/1s` → `Moderate`, `full 100ms/1s` → `Critical`; без PSI (старое ядро, нет
  прав) источник молчит, это документировано. Слияние: в одной обработке владельца доставляется
  максимум один `MemoryPressure` на уровень (R27).

- **D-16. Свои кэши первыми.** Трейт `ReclaimMemory { fn reclaim(&self, level) }` в
  `flui-foundation`. Общий для хоста `HostMemory` (`flui-app`) один раз на уведомление
  освобождает получателей приложения (`AppConfig::with_memory_reclaim`, например
  `AssetRegistry` — синхронный `reclaim`, оба уровня выбрасывают завершённые записи) и пулы
  рендерера — сообщением на raster-поток (`evict_over_budget(0)` для пула буферов на `Critical`,
  половина бюджета на `Moderate`; атлас глифов сбрасывается на `Critical`). Каждый `UiRuntime`
  освобождает только свой текстовый сервис (`flui-painting`: буферы раскладки и файлы шрифтов,
  которые никто не держит). Только после этого — подписчикам (R27). Первая паника собирается
  `FirstPanic` и поднимается после доставки.

- **D-17. Android видимость и выполнение.** `Start`/`Stop` → `dispatch_visibility_change(true/false)`,
  `Resume`/`Pause` → `dispatch_execution_state_change(Running/Suspended)`,
  `GainedFocus`/`LostFocus` → только фокус. Раннер `runner/android.rs:339`–`346` больше не выводит
  жизненный цикл из фокуса: состояние строит общий `derive_lifecycle_state` из трёх сигналов, как
  на iOS (R28).

- **D-18. Web заморозка и контекст.** `document` `freeze` → состояние выполнения `Suspended`,
  `resume` → `Running` (R30); `pagehide` → `SessionEnded` (сохранение, D-13). Холст получает
  `webglcontextlost` (с `preventDefault`, иначе восстановления не будет) и `webglcontextrestored`;
  для WebGPU — `GPUDevice.lost`. Каждое ставит флаг потери в рендерер и будит владельца, чтобы
  `device_recovery` запустился сразу, а не после следующего кадра (R31). После восстановления
  презентация помечается на полную перерисовку.

- **D-19. Linux через winit плюс прямые запросы.** Команды — через `winit::window::Window`
  (`set_title`, `set_outer_position`, `request_inner_size`, `set_min/max_inner_size`,
  `set_resizable`, `set_decorations`, `set_minimized`, `set_maximized`, `set_fullscreen(Borderless)`,
  `set_visible`, `focus_window`). Чтение: `outer_position`, `inner_size`, `is_maximized`,
  `fullscreen`, `title` (X11), `is_visible`; свёрнутость на X11 — прямой запрос `_NET_WM_STATE` +
  `WM_STATE` через `x11rb` (уже в дереве через display source), на Wayland — не наблюдаема (D-2).
  Разбор свойств X11 в состояние — чистая функция, таблица (M). logind — через уже имеющийся `zbus`.

- **D-20. Headless.** `MockWindow` переходит на `WindowStateCell`; конструктор платформы принимает
  `WindowCapabilities` и `HeadlessWindowPolicy` (максимальный размер «экрана», отказ развернуть) для проверки R3;
  `simulate_*` для всех уведомлений D-14, `simulate_session_end(phase) -> SessionEndAnswer` реально
  доходит до хука (снимается `#[ignore]`), `simulate_page_freeze`, `simulate_context_loss`.

## Публичный контракт

```rust
// flui-platform-api
#[non_exhaustive]
pub struct WindowState {
    pub revision: WindowRevision,
    pub outer: WindowGeometry,
    pub content_size: Size<f64>,
    pub mode: WindowMode,
    pub normal_geometry: Option<WindowGeometry>,
    pub visible: bool,
    pub focused: bool,
    pub title: Option<String>,
    pub constraints: SizeConstraints,
    pub resizable: bool,
    pub decorated: bool,
    pub display: Option<DisplayId>,
}
#[non_exhaustive]
pub enum WindowMode { Normal, Minimized, Maximized, Fullscreen }
#[non_exhaustive]
pub enum WindowCommand {
    SetTitle(String), SetPosition(Point<f64>), SetSize(Size<f64>),
    SetConstraints(SizeConstraints), SetResizable(bool), SetDecorated(bool),
    SetMode(WindowMode), Show, Hide, Activate,
}
#[non_exhaustive]
#[derive(thiserror::Error)]
pub enum WindowCommandError { Unsupported, InvalidGeometry, Closed, Backend { message: String } }

pub trait PlatformWindow {
    fn state(&self) -> WindowState;
    fn capabilities(&self) -> WindowCapabilities;
    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError>;
    fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>);
    // … остальное без изменений
}
pub trait WindowControl {            // реализует flui-app, потребляет flui-view
    fn state(&self) -> WindowState;
    fn capabilities(&self) -> WindowCapabilities;
    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError>;
    fn request_close(&self);
}

#[non_exhaustive]
pub enum HostNotice {
    SessionEnding(SessionEndRequest), SessionEndCancelled, SessionEnded,
    Sleep, Wake, SessionLocked, SessionUnlocked, MemoryPressure(MemoryPressureLevel),
}
pub struct SessionEndRequest { /* … */ }
impl SessionEndRequest {
    pub fn block(&self, reason: impl Into<String>) -> Result<SessionEndBlock, SessionEndBlockError>;
    pub fn on_release(self, release: impl FnOnce() + 'static) -> Self;
    pub fn answer(&self) -> SessionEndAnswer;   // закрывает «книгу»: block() → Err(Answered)
}
// платформа сообщает хосту только SystemNotice (без SessionEnding):
pub enum SystemNotice { Sleep, Wake, SessionLocked, SessionUnlocked, MemoryPressure(MemoryPressureLevel) }
#[must_use] pub struct SessionEndBlock { /* drop = release */ }
#[non_exhaustive]
pub enum MemoryPressureLevel { Moderate, Critical }

pub struct WindowPlacement { /* … */ }
impl WindowPlacement {
    pub fn from_state(state: &WindowState, displays: &DisplaySnapshot, maximized_before_minimize: bool) -> Option<Self>;
    pub fn to_bytes(&self) -> Vec<u8>;
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PlacementError>;
    pub fn resolve(&self, displays: &DisplaySnapshot, caps: &WindowCapabilities, frame: Size<f64>)
        -> Option<ResolvedPlacement>;
}

// flui-view
impl dyn LifecycleContext { fn window(&self) -> Option<WindowHandle>; }
pub struct WindowHandle { /* !Send, weak */ }
impl WindowHandle {
    pub fn state(&self) -> Result<WindowState, WindowClosed>;
    pub fn capabilities(&self) -> Result<WindowCapabilities, WindowClosed>;
    pub fn set_title(&self, title: impl Into<String>) -> Result<(), WindowCommandError>;
    pub fn set_position(&self, at: Point<f64>) -> Result<(), WindowCommandError>;
    pub fn set_size(&self, size: Size<f64>) -> Result<(), WindowCommandError>;
    pub fn set_constraints(&self, c: SizeConstraints) -> Result<(), WindowCommandError>;
    pub fn set_resizable(&self, on: bool) -> Result<(), WindowCommandError>;
    pub fn set_decorated(&self, on: bool) -> Result<(), WindowCommandError>;
    pub fn set_mode(&self, mode: WindowMode) -> Result<(), WindowCommandError>;
    pub fn show(&self) -> Result<(), WindowCommandError>;
    pub fn hide(&self) -> Result<(), WindowCommandError>;
    pub fn activate(&self) -> Result<(), WindowCommandError>;
    pub fn request_close(&self) -> Result<(), WindowClosed>;
    pub fn subscribe(&self, f: impl FnMut(&WindowState) + 'static)
        -> Result<(WindowState, WindowSubscription), WindowClosed>;
}
impl LifecycleHandle {
    pub fn subscribe_notices(&self, f: impl FnMut(&HostNotice) + 'static)
        -> Result<HostNoticeSubscription, LifecycleClosed>;
}

// flui-app
impl AppConfig {
    pub fn with_position(self, at: Point<f64>) -> Self;
    pub fn with_mode(self, mode: WindowMode) -> Self;
    pub fn restore_placement(self, name: StorageName) -> Self;
    pub fn with_memory_reclaim(self, receiver: impl ReclaimMemory + Send + Sync + 'static) -> Self;
}
```

Методы ручки — тонкие обёртки над `submit` с именами по Rust API Guidelines; отдельные методы
вместо `submit(WindowCommand)` наружу, чтобы автор не собирал enum. `flui-sdk` переэкспортирует
`WindowHandle`, `WindowState`, `WindowMode`, `HostNotice` (строки в `tests/surface.rs`).

## Инварианты, владение, lifecycle

- Снимок публикует только бэкенд; ручка и `flui-app` его не пишут. Команда не меняет снимок сама:
  только ответ ОС через `invalidate` (R3).
- Уведомление `on_state_changed` и подписчики `WindowHandle` вызываются вне блокировки ячейки и
  вне `RefCell`-заимствований очереди; подписчик, вызвавший команду или снявший подписку изнутри,
  обслуживается следующим ходом (повторный вход).
- Очередь команд вычерпывается на потоке владельца вне `build`/`layout`/`paint`; во время
  вычерпывания новые команды встают в хвост и выполняются следующим ходом (без бесконечного цикла).
- `SessionEndBlock` держит `Rc` агрегата; последний `drop` снимает блок ОС через тот же путь, даже
  при раскрутке после паники (`Drop` не паникует).
- Закрытие окна: ячейка закрывается до терминального уведомления; команды после — `Closed`.

## Ошибки и отказы

- `apply` на закрытом HWND/`NSWindow` — `Closed`; ошибка ОС — `Backend` с кодом; ни одна не
  паникует.
- `WindowRevisionExhausted` — ячейка замораживается, одна диагностика.
- Паника подписчика `HostNotice` или `WindowHandle` — сдерживается по правилам `LifecycleHandle`;
  на `SessionEnding` паникующий подписчик считается ответившим `Proceed`, а его `SessionEndBlock`,
  если успел, снимается при раскрутке.
- Испорченная запись положения — `PlacementError`, одна диагностика, запись перезаписывается при
  следующем сохранении.
- Источник уведомлений не поднялся (нет D-Bus, нет PSI, `WTSRegisterSessionNotification` отказал) —
  одна диагностика, остальные источники работают.

## Тестовая стратегия

Таблицы через публичный API (H), одна строка — одна функция:

- `window_state_contract` (`flui-platform/tests`, headless): каждая команда меняет снимок и растит
  ревизию на 1; повтор той же команды ревизию не трогает; урезанный `HeadlessWindowPolicy` размер виден в
  снимке; отказ ёмкости — `Unsupported` без изменения; неверная геометрия — `InvalidGeometry`.
- `window_handle_contract` (`flui-app` runner tests): `LifecycleContext::window` в `init_state`;
  команды применяются по порядку на следующем ходу; команда из подписчика — следующим ходом;
  закрытое окно — `Closed`; две ручки двух окон с одинаковым заголовком не путаются.
- `window_conformance` (`flui-testing`): набор R39 — для каждой команды ёмкость совпадает с
  `apply`, снимок сходится; запускается на headless в CI и из живой пробы.
- `placement_resolve` (M): дисплей найден/не найден/ключ `None`; окно шире рабочей области; окно
  за краем; смешанный масштаб; `Minimized` → `Normal`/`Maximized`; без `places_at_open`; чужая
  версия и обрезанные байты.
- `placement_recorder` (`flui-app`, headless + in-memory storage): закрыть → открыть
  восстанавливает; запись не чаще раза в 2 с; запись на `SessionEnded`.
- `host_notice_delivery` (`flui-app` + `flui-runtime`): каждое уведомление доходит до всех
  презентаций по FIFO; `MemoryPressure` сливается; кэши подрезаны до подписчика; `SessionEndBlock`
  держит `Block`, `drop` снимает; паника подписчика не блокирует; `Wake` инвалидирует снимки.
- `android_lifecycle_mapping` (M, `flui-platform` модель событий Android): Start/Stop/Pause/Resume/
  Focus → три сигнала; `lastTrimLevel` → уровень; повтор не доставляется.
- `x11_window_state_parse` (M): атомы `_NET_WM_STATE` и `WM_STATE` → режим.
- `web_page_lifecycle` (`cargo xtask wasm-test`): `freeze`/`resume` → выполнение; `pagehide` →
  `SessionEnded`; потеря контекста ставит флаг.
- `config_to_options_keeps_every_field` (M).
- `compile_fail`: `BuildContext` не имеет `window()`.

Каждая строка показывает откатом хунка, что падает по нужной причине.

**Живые прогоны (N).** Проба печатает снимок, ёмкости и каждое уведомление; владелец действует:

- **Windows 11:** без мигания при старте (видео/скриншот первого кадра), каждая команда с клавиш,
  тянуть край против минимума, развернуть мышью → снимок `Maximized`; закрыть на втором мониторе
  → открыть там же; отключить монитор → окно на живом; выход из системы → системный экран «приложение
  мешает выйти» с причиной → Отмена → `SessionEndCancelled`; сон и пробуждение → `Sleep`/`Wake`;
  Win+L → `SessionLocked`/`Unlocked`.
- **Chrome:** `chrome://discards` заморозить/разморозить → `Suspended`/`Running`; потеря контекста
  через `WEBGL_lose_context` → перерисовка; полный экран → снимок.
- **Galaxy S21:** Home и возврат → видимость/выполнение; `adb shell am send-trim-memory
  <pkg> RUNNING_CRITICAL` и уход в фон → `MemoryPressure(Critical)`.
- **WSL:** X11 (Xorg dummy) и Wayland (weston): команды, чтение режима, без мигания на X11;
  `busctl` эмуляция сигналов logind недоступна — logind проверяется на реальном `systemd` в WSL,
  если включён, иначе 🟡.

## ADR

ADR-0237 «Окно публикует снимок состояния и принимает команды; хост доставляет системные
уведомления»: D-1–D-7, D-13, D-14, D-16. Не superseding: дополняет ADR-0222 (та же форма
наблюдения) и ADR-0035 (уведомления — рядом с состоянием жизненного цикла, не в нём).

## Риски и открытые вопросы

- **Android `lastTrimLevel`** — может не обновляться от `am send-trim-memory` на всех версиях; если
  живой прогон это покажет, строка остаётся 🟡 с причиной, dex не добавляется.
- **WSL без systemd** — logind недоступен; Linux-уведомления тогда 🟡.
- **Wayland первый показ** — поведение «нет буфера — нет окна» зависит от compositor; проверяется на
  weston, остальные — документированы.
- **macOS `NSTerminateLater`** — ОС держит ответ без явного лимита; FLUI сам ограничивает 4 с,
  проверка только на Mac (issue `needs-mac`).
- **Удаление старых методов** затрагивает раннер, примеры и `flui-hot-reload`; объём проверяется
  `rg` в первой задаче.
