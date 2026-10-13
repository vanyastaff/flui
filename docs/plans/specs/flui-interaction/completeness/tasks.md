# Полнота ввода: ядро без пакетов-заплаток — задачи

- **Статус:** план задач; реализация не начата
- **Дата:** 2026-10-12
- **Требования:** [requirements.md](requirements.md); design — [design.md](design.md)
- **Правила:** задача = ветка = worktree = PR. Каждое поведение — тест через публичный API, красный с
  откатом изменения (вывод в PR); `cargo xtask check-changed` зелёный; нативный путь, не исполненный на
  хосте, помечен в PR. ID задач действуют только здесь. Задачи одного яруса идут параллельно, когда их
  зависимости слиты. Ничего не откладывается (S3): порядок есть, пропусков нет.
- **Статусы:** `todo`, `in-progress`, `merged`, `blocked-external` (ждёт внешнего релиза, E4).

## Ярус 0 — документы

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T01 | ADR из design §15 (9 шт.), перепроверка `../matrix.md`, вывод M2-R3 и M3-M3 из OUT в `../scope-closure.md`; docs-only PR вместе со спекой | §15, S3 | — | todo |

## Ярус 1 — основание

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T02 | Слои модулей `flui-interaction`: `close`, `containment`, `pointer`, `focus`, `text_input`, `gesture`, `binding`; `[package.metadata.flui.modules]`; `globals` | M1 | T01 | todo |
| T03 | Сборка указателя в `flui-platform/src/shared/`, чистые функции разбора, часы (QPC на устройство/последовательность), `ClickCounter` | P2, P3, P4 | T01; Win32 QPC — отдельный PR ветки `platform/win32-pointer-qpc-time` (сессия «Fix Win32 pointer timestamps to use QPC») | todo |
| T04 | Словарь: `PanZoomPhase::Momentum*`, `PointerEvent::Revise`, поля пера (hover, высота, roll, squeeze), `InputSupport`/`Support`, удаление `PlatformCapabilities` | P1, P5, P6 | T01 | todo |
| T05 | `SystemPreferences` (haptics, значения на дисплей, нижние границы доступности) и пресеты `(платформа, вид указателя)`: `min_target`, spring-loading, край свайпа, передача остатка, таймаут колеса, таблица клавиш | S13, A2, H4 | T01 | todo |

## Ярус 2

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T06 | Сервисы `GestureBinding`, `drain(&self)`, закрытие флагом | M2 | T02 | todo |
| T07 | Запись (`flui-testing`), `InputTap` (`flui-app`), `PointerScript` → `PlatformInput`, окна/кадры/AT в потоке | R1, R2, R4, R5, R9, Q1, Q4 | T04 | todo |
| T08 | Haptics: уровни 1–2, `HapticRequest`, `HapticOutcome`, очередь кадра, headless | H1–H5 | T04, T05 | todo |
| T09 | `SystemGestureExclusion` (render-свойство, сбор на окно) | P7 | T04 | todo |
| T10 | `HitSlop` (`min_target`, ближайший центр), hit-test по фигуре | W9, W10 | T05 | todo |
| T11 | `OneEuro` после распознавания | S17 | T05 | todo |

## Ярус 3

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T12 | `assert_backend_conforms` (указатель, часы, `PanZoom`, DnD, haptics) | R6 | T03, T07 | todo |
| T13 | Нативное проигрывание разобранных структур в Linux CI | R3 | T03, T07 | todo |
| T14 | Вложенная `GestureComposition`, удаление `AlreadyComposed`, задержка дерева | G1 | T06 | todo |
| T15 | `GestureObserver`: очередь, изоляция паник | S19 | T06 | todo |

## Ярус 4

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T16 | Осевое разрешение, `AxisLock`, `TransformLock`, правило свободной оси; `Scrollable`/`PageView` на осевом drag | G2, G3 | T14, T05 | todo |
| T17 | Поверхность `flui-sdk`, `RawGestureDetector`, сдерживание в SDK, нестандартный распознаватель в `flui-sdk/tests`, таблица размещения | W2, E1–E3 | T14 | todo |

## Ярус 5

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T18 | `SwipeGestureRecognizer`, поворот как конфигурация Scale | G4 | T16 | todo |
| T19 | Канал вложенной прокрутки, замена endpoint ADR-0169, hold на Down, политика, wheel latch | C1, C5–C8, G6, Q3 | T16, T07 | todo |
| T20 | `GestureAlternatives`, `assert_gesture_alternatives`, слот видимого элемента, локализация с параметрами | A1–A5 | T17; semantics `semantics/custom-action-ids`; focus-keyboard T3, T11 | todo |
| T21 | `GestureInspector` в `flui-devtools` | S19 | T15, T17 | todo |

## Ярус 6

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T22 | `GestureDetector` на слотах, force press, multi-tap, свайп, поворот; диагностика без `Intent` | W1, S17, A3 | T18, T17 | todo |
| T23 | Pull-to-refresh на канале | C3 | T19 | todo |
| T24 | Сворачивающийся заголовок над вкладками | C4 | T19 | todo |
| T25 | `InteractiveViewer`: поворот, snap-back, double-tap zoom, участник канала | C2, W6 | T19, T18 | todo |
| T26 | `AnchoredDrag` | W3 | T19, T20 | todo |
| T27 | Ядро DnD: сессии, `DragSessionId` источника, `AllowedActions`, несколько сессий, spring-loading, автопрокрутка, доставка в runtime | D1–D7, D9, D10 | T19, T04 | todo |
| T28 | `ContextMenuDragGestureRecognizer` | G5 | T14 | todo |

## Ярус 7

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T29 | `Dismissible` на `AnchoredDrag`, `SwipeActions` | W4 | T26, T08, T09 | todo |
| T30 | `ReorderableList`, `ReorderableGrid` | W5 | T26, T27 | todo |
| T31 | `ContextMenuArea` | W7 | T28, T20, T27 | todo |
| T32 | `SelectionArea` (указатель, клавиатура, AT), подключение `TapAndDrag` | W8, S17 | T17; спека text-ime | todo |
| T33 | Несколько окон: DnD между окнами, latch по `(окно, устройство)` | Q2 | T27, T19 | todo |

## Ярус 8 — Windows

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T34 | DirectManipulation для тачпада: прокачка, запрос кадра, модальный цикл размера, инерция | P8, S6 | T03, T13 | todo |
| T35 | OLE DnD источник и цель, кадры в модальном цикле | P8, D8 | T27 | todo |
| T36 | Haptics Windows: `SimpleHapticsController` пера; `InputHapticsManager` — `Unsupported` до подъёма `windows` | P8, H1 | T08 | todo |
| T36a | Подъём крейта `windows` с `InputHapticsManager` | P8 | внешний релиз | blocked-external |
| T37 | Golden-записи Windows, `cargo xtask device windows-input` с записями | R7, R8 | T34 | todo |

## Ярус 9 — остальные ОС (каждая — своя ветка задач, зависят от T12 и ядра функции)

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T38 | macOS: перо, smart magnify, swipe, инерция, DnD, haptics, `CGEvent`-инъекция | P9 | T12, T08, T27, T19 | todo |
| T39 | iOS: трекпад pinch/rotate, прокрутка, coalesced/predicted, `Revise`, `UITextInput`, DnD, haptics, Pencil, системные края | P10, P14 | T12, T08, T27, T19, T09 | todo |
| T40 | Android: IME, DnD, haptics, pinch тачпада, рукописный ввод, системные края, `ViewConfiguration` | P11, P14 | T12, T08, T27, T19, T09 | todo |
| T41 | Linux: честный `composing`; жесты тачпада X11 `Unsupported`; перо и двусторонний DnD — `Unsupported` до winit 0.31 | P12 | T12 | todo |
| T41a | Подъём winit до 0.31: перо, планшет, DnD | P12 | внешний релиз | blocked-external |
| T42 | Web: IME (`EditContext`/`<textarea>`), ctrl+wheel → pinch, Safari `gesture*`, HTML DnD, вибрация | P13, P14 | T12, T27, T19; спека web-canvas-input-ownership | todo |

## Ярус 10 — завершение

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T43 | Material/Cupertino: обёртки стиля каждого виджета | W11 | каждый виджет | todo |
| T44 | Вклад в AccessKit: custom actions и `ShowContextMenu` в адаптерах macOS, Windows, AT-SPI | A7 | — (в любой момент) | todo |
| T45 | Матрица доступности A6 (связана с semantics-model W5) | A6 | T29–T31, T22 | todo |
| T46 | Аудит достижимости `pub` и имён (§0) | M3, N5 | все | todo |

## Внешние связи

- semantics-model: ветка `semantics/custom-action-ids` (обработчик на id), W7 (announcer), строки W5
  жестовых виджетов — design.md спеки semantics-model, раздел (e), ветка `semantics/architecture-decisions`.
- focus-keyboard: T3, T6, T10, T11, T15 — зависимости T20.
