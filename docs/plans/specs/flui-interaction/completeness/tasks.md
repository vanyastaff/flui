# Полнота ввода: ядро без пакетов-заплаток — задачи

- **Статус:** в работе: строки со статусом `merged` слиты в интеграционную ветку `interaction/completeness`
- **Дата:** 2026-10-13
- **Требования:** [requirements.md](requirements.md); design — [design.md](design.md)
- **Правила:** задача = ветка = worktree = PR. Каждое поведение — тест через публичный API, красный с
  откатом изменения (вывод в PR); `cargo xtask check-changed` зелёный; нативный путь, не исполненный на
  хосте, помечен в PR. ID задач действуют только здесь. Задачи одного яруса идут параллельно, когда их
  зависимости слиты. Ничего не откладывается (S3). Каждая задача добавляет свои строки `surface.rs`,
  таблицы размещения, проверки соответствия и матрицы доступности (E1, R6, A6); чеклист N1–N5 — в каждом PR.
  Задача, меняющая поддержку на платформе, обновляет свою строку в `docs/platform-parity.md` (✅ — только
  после прогона на этой платформе) и пункт в `docs/gaps.md` в той же ветке.
- **Статусы:** `todo`, `in-progress`, `merged`, `blocked-external` (ждёт внешнего релиза, E4).

## Ярус 0 — документы

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T01 | ADR из design §16 (11 шт.), перепроверка `../matrix.md`, вывод M2-R3 и M3-M3 из OUT; docs-only PR со спекой | §16, S3 | — | merged |

## Ярус 1 — основание

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T02 | Слои модулей `flui-interaction` | M1, ADR-0217 | T01 | merged |
| T03 | Сборка указателя, чистые функции разбора, часы, `ClickCounter`, исходное нативное событие на контакт; бюджет без аллокаций | P2, P3, P4, Q3, ADR-0217 | T01; Win32 QPC — ветка `platform/win32-pointer-qpc-time` | merged |
| T05 | `SystemPreferences` и пресеты `preset(platform, kind)` с цепочкой отката (только числа) | S13, H4, ADR-0212 | T01 | merged |

## Ярус 2

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T04 | Словарь: `Scroll` с фазами для прокрутки двумя пальцами, `PanZoom` только масштаб/поворот, `Revise`, поля пера, `PointerKind::Virtual`, `InputCapabilities`/`Availability` за `unstable-input`, удаление `PlatformCapabilities`; аудит всех `_`-ветвей потребителей; `bitflags` для `PointerKindSet` и вместо самописного `TransferActions(u8)`; workspace-записи для `android-activity`, `slab`, `smallvec` | P1, P5, P6, B5, S25, L, ADR-0213, ADR-0217 | T02 | merged |
| T06 | Сервисы `GestureBinding` с ключом `InputSurface`, `drain(&self)`; статическая проверка `!Send` для `GestureBinding`, `compile_pass` с `Rc`-захватами | M2, Q2, Z1 | T02 | merged |

## Ярус 3 — позвоночник

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T07 | `HostInput` (во `flui-runtime`), шкала времени (дедлайны на `EventTime`), `InputSequenceId`, очередь реентерабельных нативных событий, детерминизм двойного прогона на рантайм; снятие `+ Send` с `on_input` после регистрации в потоке владельца (ADR-0082); вид указателя доходит до hit-test (`min_target` у `HitSlop` и пресет `preset(platform, kind)` выбираются по виду контакта, а не одним профилем презентации) | B1–B4, Z5, Z6, ADR-0207 | T04, T06 | merged |
| T08 | Запись и скрипты: `InputTap`, DTO, фрейминг, круговой gate вариантов, builder'ы устройств, текстовые скрипты, поиски по semantics, строки кадр/события | R1, R2, R4, R5, R7, R9, R10, Q1, Q4, ADR-0215 | T07, T05 | merged |
| T09 | `InputRegions` (протокол, render-объект с harness, сбор на окно, метод окна), `SystemGestureExclusion` | P7, ADR-0217 | T04 | merged |
| T10 | `HitSlop` (визуальное → край → z, без пересечения слоёв), hit-test по фигуре (render-объект с harness) | W9, W10 | T05 | merged |
| T11 | `OneEuro` после распознавания | S17 | T05 | merged |

## Ярус 4

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T12 | Каркас `assert_backend_conforms` (указатель, часы, грамматика `Scroll`/`PanZoom`) | R6, ADR-0215 | T03, T08 | merged |
| T13 | Нативный кодек в тестах `flui-platform`, проигрывание в Linux CI | R3, ADR-0215 | T03, T08 | merged |
| T14 | Арена: таблица владения, `PanClaim`, `allowed_devices`, накопленное смещение, `Sequenced`, вложенная композиция, `max_resolution_delay`, отмена проигравшим и `Listener`, `assert_ownership_table` | G1–G6, ADR-0208 | T07, T05 | merged |
| T15 | `GestureObserver` (очередь, паники, без аллокаций) | S19, Q3 | T07 | merged |
| T16 | Haptics: уровни 1–2, `HapticRequest`, `Queued`, очередь с дедлайном, устаревший источник; строки проверки | H1–H5, ADR-0210 | T07, T05, T12 | merged |

## Ярус 5 — фокус для T19 (задачи спеки focus-keyboard, делаются здесь)

Спека [focus-keyboard](../../focus-keyboard/tasks.md) — черновик; T19 нужны её модальность ввода, Back/Dismiss и правило
клавиш во время перехода маршрута. Её задачи ниже делаются в этой интеграционной ветке целиком, со статусом и в её
`tasks.md`; остальное (T2, T4 → T33, T5 уже сделана, T7, T9, T11 → T33, T12–T16) остаётся в её плане.

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| FK3 | focus-keyboard T3 целиком (ядро `flui-interaction`: `Stale`, `NotificationInFlight`, `FocusCause`, модальность, журнал клавиш, вход обхода, маркер сирот) с нужной ей частью контракта T1 | focus-keyboard T3 | T02 | merged |
| FK6 | focus-keyboard T6 и T7 целиком (рантайм: `note_pointer_input`, X1 → `BrowserBack`, `forget_pressed_keys`; `settle_orphaned_focus` в кадре после fixpoint — без него FK3 оставляет фокус на удалённом узле до следующего ввода) | focus-keyboard T6, T7 | FK3, T07 | merged |
| FK8 | focus-keyboard T8 целиком (`on_release`, Activation/Back/Dismiss, таблица `DefaultFocusTraversal`, `FocusRing`, `RawButton`, `Focus` `Assistive`) | focus-keyboard T8 | FK3 | merged |
| FK10r | focus-keyboard T10 целиком: R1 (первый Tab входит в текущий маршрут), R7 (первый фокус push предварителен, `autofocus` его перебивает до хода пользователя), R8 (restore pop окончателен), R9 (Back/Dismiss), R11 (клавиши только верхнему маршруту, push, отменённый Back посреди перехода, возвращает фокус) | focus-keyboard T10 | FK8 | merged |

## Ярус 5

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T17 | `RawGestureDetector` (фабрика, `update(&config)`), поверхность SDK, нестандартный распознаватель с пересборкой посреди жеста, шаблон-чеклист | W2, E1–E3, E5 | T14 | merged |
| T18 | `AxisLock`, `TransformLock`, `SwipeGestureRecognizer`, поворот как Scale, hold на Down как правило арены, замирание hover | G7–G10, ADR-0208 | T14 | merged |
| T19 | `GestureAlternatives`, `assert_gesture_alternatives`, клавиши как `Shortcuts` платформы, локализация с параметрами, диагностика без `Intent` | A1–A5, A9, ADR-0216 | T17; semantics `semantics/custom-action-ids` (слит); FK3, FK6, FK8, FK10r | todo |

## Ярус 6

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T20 | `GestureDetector` на слотах поверх `RawGestureDetector` | W1, S17 | T17, T18, T19 | todo |
| T21 | Канал прокрутки: один примитив смещения, лист без контакта, latch и таймауты, fling с наследованием, `reveal`, `ScrollEndReason`, замена endpoint ADR-0169; `Scrollable` на осевом drag; строка C1 | C1, C5–C9, Q3, ADR-0209 | T18, T08 | merged |
| T22 | `GestureInspector` | S19 | T15, T17 | merged |

## Ярус 7

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T23 | `SnapPolicy`; `PageView` = `Scrollable` + `SnapPolicy` | C10, W11, ADR-0209 | T21 | todo |
| T24 | Pull-to-refresh, строки C3 и матрицы | C3 | T21, T19 | todo |
| T25 | Сворачивающийся заголовок, строки C4 (включая Tab и AT) | C4 | T21, T19 | todo |
| T26 | `InteractiveViewer` как лист канала, строки C2 | C2, W6 | T21, T18, T19 | todo |
| T27 | `AnchoredDrag` (одномерный, `SnapPolicy`), haptics на якорях | W3 | T23, T16 | todo |
| T28 | DnD: ядро сессий, идентичность, эскалация в ОС, доставка в runtime | D1, D2, D6, ADR-0211 | T21, T07 | todo |

## Ярус 8

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T29 | DnD: `Draggable`/`DragTarget` на сессиях, `DragTrigger` через `Sequenced`, Esc и Cut/Copy/Paste, курсор | D3, D4 | T28, T19, T16 | todo |
| T30 | DnD: spring-loading, автопрокрутка, несколько сессий, drop последней принявшей цели | D3, D4 | T28 | todo |
| T31 | `Dismissible` на `AnchoredDrag`, `SwipeActions`, системные края | W4 | T27, T09, T19, T32 | todo |
| T32 | `ContextMenuArea` на `Sequenced` | W7 | T14, T19 | todo |
| T33 | `ReorderableList`, `ReorderableGrid` как DnD-сессия, keep-alive, A8 | W5, A8 | T29, T30; focus-keyboard T4, T11 | todo |
| T34 | `SelectionArea` (указатель, клавиатура, AT, ручки на `Sequenced`, `TapAndDrag`) | W8, S17, ADR-0214 | T17, T19, T14; поправка ADR-0204 (T01); спека text-ime | todo |
| T35 | Несколько окон и рантаймов: DnD между окнами, захват при drag над другим окном | Q2, D2 | T28 | todo |

## Ярус 9 — Windows

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T36 | DirectManipulation: `Scroll` с фазами и `PanZoom`, прокачка, свой запрос кадра, модальный цикл размера | P8, S6 | T03, T13, T21 | merged |
| T37 | OLE DnD: источник и цель, отложенный старт, кадры всем рантаймам, перерисовка второго окна | P8, D5 | T28 | todo |
| T38 | Haptics Windows: перо; `InputHapticsManager` — `Unavailable` | P8, H1 | T16 | todo |
| T38a | Подъём `windows` с `InputHapticsManager` | P8 | внешний релиз | blocked-external |
| T39 | Golden-записи и `xtask device windows-input` | R8 | T36, T37 | todo |

## Ярус 10 — остальные ОС (на каждую ОС: ввод, DnD, haptics, IME, инъекция и записи)

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T40a–e | macOS: ввод · DnD · haptics · IME-проверка · `CGEvent` и записи | P9, P14, R8 | T12 и ядро функции (T21, T28, T16) | todo |
| T41a–e | iOS: ввод · DnD · haptics · `UITextInput` · XCUITest и записи | P10, P14, R8 | T12, T09 и ядро функции | todo |
| T42a–e | Android: ввод · DnD · haptics · `InputConnection` · instrumentation и записи | P11, P14, R8 | T12, T09 и ядро функции | todo |
| T43a–c | Linux: `composing` и `Unavailable`-пути · uinput/libei и записи · после T43x — перо, планшет, DnD | P12, R8 | T12 | todo |
| T43x | Подъём winit до 0.31 | P12 | внешний релиз | blocked-external |
| T44a–e | Web: ввод и синхронный `preventDefault` · HTML DnD · вибрация · IME (`EditContext`/`<textarea>`) · BiDi и записи | P13, P14, P15, R8, Q4 | T12 и ядро функции; спека web-canvas-input-ownership | todo |

## Ярус 11 — завершение

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T45 | Material/Cupertino: обёртки стиля | W12 | каждый виджет | todo |
| T46 | Вклад в AccessKit (custom actions, `ShowContextMenu`) | A7 | — (в любой момент) | todo |
| T47 | Сборка матрицы A6 и проверка версии адаптеров | A6 | T24–T26, T29–T34; focus-keyboard T15 (нативный тур) | todo |
| T48 | Аудит достижимости `pub`, имён и конвенций | M3, N1–N5 | все | todo |

## Внешние связи

- semantics-model: `semantics/custom-action-ids` (PR #1606 слит: `Semantics::custom_action(label, handler)`, `CustomActionId`), W7 (announcer), строки W5 жестовых виджетов
  (design.md спеки semantics-model, раздел (e), ветка `semantics/architecture-decisions`).
- focus-keyboard: T3, T4 (pinned keep-alive slot), T6, T10, T11, T15.
- text-ime, web-canvas-input-ownership — для T34, T44.
- Win32 QPC — ветка `platform/win32-pointer-qpc-time`.
