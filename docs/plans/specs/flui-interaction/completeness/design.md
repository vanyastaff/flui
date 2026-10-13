# Полнота ввода: ядро без пакетов-заплаток — design

- **Статус:** на утверждение владельцем после второго раунда атаки; имена — §0 (N5), размещение — §0.2 (E3)
- **Дата:** 2026-10-12, база `main` @ `4da87df0b`
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **Исследования:** [best-practices](../../../../research/2026-10-12-interaction-input-best-practices.md)
  («BP»), [followups](../../../../research/2026-10-12-interaction-input-followups.md) («FU»)
- Пути — от корня репозитория.

## 0. Имена

Правило (N2): устоявшийся термин экосистемы, записанный по конвенциям Rust; колонка «Где принят»
обязательна. Существующие семейства (`TapGestureRecognizer` и т.п.) переименовывает общий проход
[naming](../../naming/renames.md); имена semantics — из ADR-0203 без изменений.

### 0.1 Публичные имена

| Имя | Что это | Где принят |
|---|---|---|
| `GestureComposition::{Exclusive, RequireFailure}` | отношение двух ветвей арены, вкладывается; заменяет `GestureCompetition` | SwiftUI `ExclusiveGesture`; UIKit `require(toFail:)` |
| `AxisLock::{Free, Horizontal, Vertical, Directional, Rails { breakout: Angle }}` | блокировка оси pan/scale | UIKit `isDirectionalLockEnabled`; DirectManipulation `RAILS_X/Y` |
| `TransformLock` (`lock_rotation`, `lock_scale` при pan) | блокировка трансформа в scale | Compose `lockRotationOnZoomPan` |
| `SwipeGestureRecognizer`, `SwipeStart::{Anywhere, FromEdge(PhysicalEdge)}` | свайп; край — физический, виджет разрешает leading/trailing | UIKit `UISwipeGestureRecognizer`, `UIScreenEdgePanGestureRecognizer.edges` |
| `RawGestureDetector` | публичный детектор стороннего распознавателя | Flutter `RawGestureDetector` (имя совпадает с назначением) |
| `NestedScrollConnection`: `pre_scroll`, `post_scroll`, `pre_fling`, `post_fling`, `sequence_started`, `sequence_ended` | участник канала прокрутки | Compose `NestedScrollConnection` (`onPreScroll`…), Android `onNestedPreScroll`; `on_` снят |
| `NestedScrollDispatcher` | диспетчер канала | Compose `NestedScrollDispatcher` |
| `ScrollStep` (`#[non_exhaustive]`: `sequence`, `cause`, `device`, `available`) | аргумент участника | вместо позиционных аргументов, которые трижды ломали Android (`NestedScrollingParent` 1→2→3) |
| `ScrollCause::{UserInput, Momentum, SideEffect}` | причина дельты | Compose `NestedScrollSource.UserInput`/`SideEffect`; `Momentum` — AppKit `momentumPhase` |
| `FlingHandoff::{Declined, Accepted}` | ответ `post_fling` | Compose `onPostFling` |
| `OverscrollBehavior::{Auto, Contain}` | политика передачи остатка | CSS `overscroll-behavior` |
| `AnchoredDrag`, `DraggableAnchors` | примитив перетаскивания к якорям | Compose `anchoredDraggable`, `DraggableAnchors` |
| `SwipeActions`, `Dismissible` (существующий), `ReorderableList`, `ReorderableGrid`, `ContextMenuArea`, `SelectionArea`, `HitSlop` | поведенческие виджеты | SwiftUI `.swipeActions`; Flutter `ReorderableListView`, `SelectionArea`; Compose Desktop `ContextMenuArea`; React Native `hitSlop` |
| `GestureDetector` слоты: `tap`, `double_tap`, `long_press`, `drag(DragSlot::{Pan, Horizontal, Vertical})`, `transform(TransformSlot::{Scale, Pan})`, `swipe`, `force_press`, `multi_tap` | типизированные слоты детектора | — (конфликт невыразим: вместо assert Flutter `gesture_detector.dart:321`) |
| `InputSupport { pen, precise_scroll, trackpad_gestures, system_gesture_exclusion }` из `Support::{Native, Emulated, Unsupported}` | возможности без собственного handle | W3C feature detection |
| `system_gesture_exclusion` | lifecycle-возможность областей исключения системных жестов | Android `setSystemGestureExclusionRects`; iOS `preferredScreenEdgesDeferringSystemGestures` |
| `HapticFeedback` (существующий) + `Alignment`, `LevelChange`, `DragStart`, `Drop`, `Confirm`, `Reject` | уровень 1 | AppKit `.alignment`/`.levelChange`; Android `DRAG_START`, `CONFIRM`/`REJECT` |
| `HapticPattern`, `HapticEvent::{Transient, Continuous}`, `Intensity`, `Sharpness`, `ParameterCurve`, `ControlPoint`, `HapticCapabilities` | уровень 2 | Core Haptics `CHHapticPattern`, `CHHapticEvent`, `hapticIntensity`/`hapticSharpness`, `CHHapticParameterCurve.ControlPoint` |
| `HapticRequest { source: Option<DeviceId>, timing: HapticTiming::{Immediate, FramePresented} }` | откуда и когда | UIKit `UIFeedbackGenerator(view:)` + `occurred(at:)` (iOS 17.5); AppKit `NSHapticFeedbackPerformanceTime.drawCompleted` |
| `HapticOutcome::{Submitted, Approximated, FellBack, Suppressed, Unsupported}` (`#[must_use]`) | результат | — («played» не обещается: `CHHapticEngine.start` асинхронен) |
| `DragSource`, `DropTarget`, `DragItem`, `DragSessionId`, `DragPhase` | DnD поверх ADR-0038 | UIKit `UIDragItem`, `UIDragSession`; Win32 `IDropTarget`/`IDropSource`; HTML «drop target» |
| `AllowedActions { within_app, outside_app }` (`TransferActions`) | разрешённые операции по контексту | AppKit `sourceOperationMaskForDraggingContext:` |
| `SpringLoading` (опция `DropTarget`) | активация цели удержанием | UIKit `UISpringLoadedInteraction` |
| `Intent` (существующий в `actions.rs`), `GestureAlternatives` | тройка альтернатив жеста | Flutter/FLUI Shortcuts–Actions–Intents (ADR-0023) |
| `PointerEvent::Revise { sample, properties }` | поправка свойств задним числом | UIKit `touchesEstimatedPropertiesUpdated`, `estimationUpdateIndex` |
| `PanZoomPhase::{…, MomentumBegan, MomentumChanged, MomentumEnded}` | инерция pan/zoom | зеркало существующего `ScrollPhase`; DirectManipulation `INERTIA` |
| `InputRecording`, `RecordingReader`, `RecordingWriter`, `RecordingError` (`flui-testing`) | запись и проигрывание | `libinput record` |
| `PointerScript` (существующий, расширяется) | скрипт синтетического ввода | — (существует в `flui-testing::replay`) |
| `assert_backend_conforms`, `assert_gesture_alternatives` (`flui-testing`) | проверки соответствия | «conformance test suite»: W3C WPT, Khronos CTS |
| `GestureInspector` | оверлей в `flui-devtools` | Flutter Widget Inspector |

### 0.2 Размещение (E3)

| Имя | Крейт | Уровень | `surface.rs` | Первый продакшн-вызывающий |
|---|---|---|---|---|
| `GestureComposition`, `AxisLock`, `TransformLock`, `SwipeGestureRecognizer`, трейт распознавателя, dispatch | `flui-interaction` | через SDK — Evolving | да | `GestureDetector`, `Scrollable`, `PageView` |
| `RawGestureDetector` | `flui-widgets` | Evolving | да | `GestureDetector` (строится на нём) |
| `NestedScrollConnection`, `NestedScrollDispatcher`, `ScrollStep`, `ScrollCause`, `FlingHandoff`, `OverscrollBehavior` | `flui-widgets` (`scroll::nested`) | Evolving | да | `Scrollable`, `PageView`, `RefreshIndicator`, `InteractiveViewer`, `AnchoredDrag` |
| `AnchoredDrag`, `DraggableAnchors` | `flui-widgets` | Evolving | да | `SwipeActions`, `Dismissible`, `PageView` snap, шторки, reorder |
| поведенческие виджеты | `flui-widgets` | Evolving | да | `flui-material`, `flui-cupertino` |
| `HapticFeedback` (+варианты), `HapticPattern` и части, `HapticRequest`, `HapticOutcome`, `HapticCapabilities`, `PlatformHaptics::play` | `flui-platform-api` | Stable | реэкспорт | виджеты через lifecycle-возможность |
| `InputSupport`, `Support`, `PanZoomPhase::Momentum*`, `PointerEvent::Revise`, поля пера | `flui-platform-api` | Stable | реэкспорт | бэкенды, `GestureBinding` |
| `DragSource`, `DropTarget`, `DragItem`, `DragSessionId`, `DragPhase`, `AllowedActions`, `SpringLoading` | `flui-interaction` (сессии) поверх `flui-platform-api` (ADR-0038) | Evolving | да | `Draggable`, `DragTarget`, `ReorderableList` |
| `system_gesture_exclusion` | `flui-view` `LifecycleContext` (ADR-0078) | Evolving | да | `SwipeActions`, edge-swipe, back-gesture |
| запись, скрипты, проверки соответствия | `flui-testing` | тестовый | нет | тесты всех слоёв; подключение записи — `flui-app` |
| `GestureInspector`, наблюдатель | `flui-devtools` / `flui-interaction` | официальный пакет / Evolving | наблюдатель — да | `flui-devtools` |

## 1. Модули `flui-interaction` (M1)

Факты: распознаватели импортируют `routing::{PointerDispatch, PanZoomDispatch, RoutePanic}`
(`recognizers/tap.rs:14`, `scale.rs:41` и др.); `__runtime.rs` (399 строк) — узел цикла; `settings ↔
processing` и `details ↔ recognizers` — циклы внутри будущего `gesture`; фокус в `routing/`; `Velocity`
публичен по двум путям (`velocity.rs` и `processing/velocity.rs`).

| Слой | Модули |
|---|---|
| 0 | `events`, `ids`, `close` (из `__runtime.rs` 17–24, 217–399), `containment` (`RoutePanic` и сдерживание), `retain` |
| 1 | `pointer`: `hit_test`, `interaction_lane`, `pointer_router`, `pointer_capture`, `mouse_tracker`, `resampler`, `sampling_clock`, `wheel_preferences` |
| 1 | `focus`: `focus`, `focus_scope`, `traversal`, диспетчеризация клавиш |
| 1 | `text_input`: `text_input`, `clipboard` |
| 2 | `gesture`: `arena`, `recognizers`, `velocity` (один путь), `lsq_solver`, `one_euro`, `settings`, `details`, `observability`, `traits` (`DragAxis`), `drag` (сессии DnD) |
| 3 | `binding`: `GestureBinding` и сервисы §2; фасад `__runtime.rs` 25–215 |
| — | `testing` (фича), `prelude` — `transparent` |

`[package.metadata.flui.modules]` в `crates/flui-interaction/Cargo.toml` по образцу `flui-widgets`
(`Cargo.toml:238-298`), `exceptions = []`. Перенос — механические коммиты без изменения поведения;
`globals`-записи (`Cargo.toml` 129–135) обновляются в том же коммите.

## 2. `GestureBinding` (M2)

| Сервис | Слой | Владеет |
|---|---|---|
| `PointerSequences` | pointer | `hit_tests`, счётчик последовательностей, `refused_contacts` |
| `MoveCoalescer` | pointer | ожидающие `Move`, поколения, ресэмплер, `SamplingClock` |
| `ScrollSequences`, `PanZoomSequences` | pointer | последовательности scroll и pan/zoom, latch по `(окно, устройство)` |
| `MouseTracker` | pointer | (существующий) |
| `ArenaDriver` | gesture | арена, настройки, дедлайны; close на Down, sweep на Up |
| `CloseCoordinator` | binding | флаг закрытия, режим, `capture_wake`, tombstones |

Сервисы — `pub(crate)` поля без обратных ссылок. `pointer` возвращает итог маршрутизации, binding передаёт
его `ArenaDriver` (трейта-шва с одной реализацией нет). Закрытие ставит флаг; каждый сервис отдаёт
`drain(&mut self) -> Vec<Retired>` через `mem::take`; освобождение и пользовательский код — вне
заимствований. Guards реентрантности живут в `CloseCoordinator`.

## 3. Арена (G1–G6)

- `GestureComposition::{Exclusive, RequireFailure}` вкладывается. Узел держит родителя сильно, родитель
  детей — слабо (нет циклов на контакт). `AlreadyComposed` удаляется. Один победитель на контакт; захват
  (ADR-0164) и причина завершения (ADR-0112) — как сейчас.
- Наихудшая задержка: сумма дедлайнов `RequireFailure` по пути от корня; ADR фиксирует формулу и тест с
  деревом глубины 3.
- Ортогональные заявки: на сэмпле пересечения slop каждый член с `AxisLock` сравнивает проекцию на свою
  ось; побеждает член с большей проекцией, ничья — по оси, объявленной родителем; член, проигравший по
  оси, отклоняет, а не держит контакт. ADR явно заменяет фразу ADR-0167 «Horizontal drag acquires no
  invented orthogonal-rejection behavior».
- `AxisLock` — только у pan и scale (у осевых распознавателей ось уже в типе). `Directional` всегда
  фиксирует (в отличие от UIKit, где диагональный старт не фиксируется — сказано в ADR).
- Поворот — `ScaleGestureRecognizer` с `TransformLock` и колбэками поворота (ADR-0166); отдельного
  распознавателя нет.
- Свайп: `SwipeGestureRecognizer`, порог скорости из значений поведения, `FromEdge(PhysicalEdge)`.
- Меню и перетаскивание: `ContextMenuArea` и `Draggable` с `DragStart::LongPress` на одном узле
  объединяются композицией `RequireFailure`, где меню ждёт провала перетаскивания: удержание → меню;
  движение за slop → меню закрывается, начинается перетаскивание.
- Hold на Down у `Scrollable`: `pre_scroll`-цепочка получает `sequence_started`, баллистика
  останавливается, дети игнорируют указатель, пока активность — drag или ballistic (кроме `Momentum`).
- Пассивное наблюдение (ripple, аналитика) — `Listener`; в арену не входит.

## 4. Канал вложенной прокрутки (C1–C8)

`flui-widgets/src/scroll/nested.rs`; заменяет приватную endpoint ADR-0169 (новый ADR — Supersedes
ADR-0169; его тесты `nested_fling_*` остаются строками). Формулировка ADR-0169 о `Send`-продолжении
ошибочна: `when_complete_or_cancel` принимает `FnMut + 'static` без `Send`
(`flui-animation/src/run_future.rs:298`); endpoint переводится с `Arc`/`AtomicBool` на `Rc`/`Cell`.

```rust
#[non_exhaustive]
pub struct ScrollStep { pub sequence: ScrollSequenceId, pub cause: ScrollCause,
                        pub device: ScrollDevice, pub available: Vec2 }

pub trait NestedScrollConnection {
    fn sequence_started(&self, sequence: ScrollSequenceId) {}
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 { Vec2::ZERO }
    fn post_scroll(&self, step: &ScrollStep, consumed_by_child: Vec2) -> Vec2 { Vec2::ZERO }
    fn pre_fling(&self, sequence: ScrollSequenceId, available: Velocity) -> Velocity { Velocity::ZERO }
    fn post_fling(&self, sequence: ScrollSequenceId, available: Velocity) -> FlingHandoff { FlingHandoff::Declined }
    fn sequence_ended(&self, sequence: ScrollSequenceId, reason: GestureEndReason) {}
}
```

- `&self`: участник — владельца-локальный, состояние в `Cell`/`RefCell`, ни одного заимствования через
  вызов `ScrollController`. Трейт не запечатан, объектно-безопасен, `!Send`.
- Снимок цепочки — при `sequence_started`, привязан к id; закрытые звенья пропускаются, не заменяются.
- Порядок: `pre_*` от внешнего к внутреннему, `post_*` от внутреннего к внешнему (Compose
  `NestedScrollNode.kt` L81–85).
- Fling: баллистика ребёнка идёт дельтами `cause = SideEffect` через диспетчер; на тике, где цепочка
  недопоглотила, остаток скорости берётся из `dx(t)` симуляции и предлагается `post_fling`; принявший
  запускает свой бег и сам передаёт свой остаток выше. `Momentum` (инерция ОС) идёт через
  `pre/post_scroll` владельцу latch и никогда не вызывает `*_fling`. Новый `UserInput` отменяет инерцию,
  переданную предкам этой цепочки.
- Политика: `OverscrollBehavior::Contain` удерживает остаток; `Auto` передаёт по правилу пресета
  платформы (Android — посреди drag; iOS/macOS — следующей последовательностью).
- Реентрантность: диспетчеризация той же последовательности изнутри участника отклоняется с диагностикой.
- Паника: участник считается поглотившим всю `available`; звено закрывается, получает
  `sequence_ended(Cancelled)` вне заимствований; лист не применяется.
- Проверка результатов: конечен, тот же знак по оси, не больше доступного; иначе обрезка с диагностикой,
  ограниченной по частоте.
- Строки таблицы (`flui-widgets/tests`, записанный ввод): C1 (в т.ч. 44/45/46°), C2 (край в начале,
  второй палец, отмена), C3 (колесо и инерция не вытягивают, отмена сбрасывает), C4 (свернуть, развернуть,
  float/pin/snap), C5 (касание после передачи, `Momentum` без двойного применения), реентрантность,
  паника ×3, без аллокаций.

`AnchoredDrag` (W3) реализует `NestedScrollConnection`: потребляет `post_scroll`-остаток и `pre_scroll`,
когда уже смещён. `SwipeActions`, `Dismissible` (переписывается), привязка `PageView`, шторки и reorder
стоят на нём — внутри прокрутки они больше не соперники в арене.

## 5. Платформенный слой: общее (P1–P7)

- **Поддержка.** Haptics и IME — по существующим `haptics()`/`text_input()`; `InputSupport` — только
  перо, точная прокрутка, жесты тачпада, исключение системных жестов. Присутствие устройств — из
  `DeviceRegistry` (`DeviceAdded/Removed`). Изменение — событием по тому же пути, что изменения
  `SystemPreferences`. `PlatformCapabilities` удаляется (ADR-0082 §5).
- **Сборка указателя** — `flui-platform/src/shared/`, расширяя существующий `shared/events.rs` (эпоха,
  идентичность мыши): `ContactRegistry`, `DeviceRegistry`, `HeldButtons`, `ClickCounter` (настройка ОС),
  `PanZoomAccumulator` (из winit `events.rs:18-216` и macOS `events.rs:274-355`), `MoveBatch`. Бэкенд
  подаёт нормализованное `NativeReading`; разбор — чистые функции над обычными структурами (без
  `windows::*`), компилируются на всех целях.
- **Часы.** Самые точные часы ОС: Win32 `POINTER_INFO.PerformanceCount` (QPC), иначе `dwTime`;
  одинаковый `native_time` iOS/macOS — в `shared/events.rs`.
- **Поправки пера**, фазы инерции `PanZoom`, фокус pinch без известного курсора — в словаре до заморозки
  формата записи.

## 6. Платформенный слой: по ОС (P8–P14)

| ОС | Что делается |
|---|---|
| Windows | DirectManipulation только для `PT_TOUCHPAD` (`SetContact` на `DM_POINTERHITTEST`), прокачка `Update` каждый кадр через scheduler, `ZoomToRect`-сброс, фильтр второго READY, фаза инерции; OLE `IDropTarget`/`IDropSource`/`DoDragDrop` + `IDragSourceHelper`, кадры внутри модального цикла; haptics: `InputHapticsManager` (подъём `windows` или узкий биндинг) и `SimpleHapticsController` пера; QPC; `WM_SETTINGCHANGE` и DPI на дисплей |
| macOS | tablet/proximity → перо; smart magnify, swipe, инерция; `NSDragging*`, `NSFilePromiseProvider`, маски операций по контексту; `NSHapticFeedbackManager` с `drawCompleted` |
| iOS | `UIPinch`/`UIRotation` только для трекпада (`allowedTouchTypes`); прокрутка трекпада и мыши; coalesced/predicted; поправки свойств; `UITextInput`; `UIDrag/DropInteraction`, несколько сессий; `UIFeedbackGenerator(view:)`, Core Haptics с `resetHandler`/`stoppedHandler`; Pencil squeeze/double-tap/hover, roll; `preferredScreenEdgesDeferringSystemGestures` |
| Android | `InputConnection`; `OnReceiveContentListener`, DnD; `VibrationEffect` (envelope API 36+, primitives 30–35, `USAGE_TOUCH`); pinch тачпада; рукописный ввод стилусом; `setSystemGestureExclusionRects`; `ViewConfiguration` как значения ОС |
| Linux | winit 0.31: перо, планшет, двусторонний DnD; честный `composing`; жесты тачпада X11 — `Unsupported` |
| Web | IME: `EditContext` (Chromium) / скрытый `<textarea>` (Firefox, Safari), ~15 своих `wasm-bindgen` объявлений; ctrl+wheel → pinch только для дробной `deltaMode==0`, синтез Start/End по таймауту, `Emulated`, отказ приложения в пользу браузерного zoom; Safari `gesture*`; HTML DnD; Vibration API с активацией |

## 7. Haptics (H1–H5)

Уровень 1 — пополненный `HapticFeedback`. Уровень 2 — `HapticPattern` (сборка валидирует порядок,
интенсивность и резкость `0.0..=1.0`, положительные рампы, огибающая заканчивается нулём, общий предел
длительности). `PlatformHaptics::play(&HapticRequest, &HapticPattern, fallback: HapticFeedback) ->
HapticOutcome`. Деградация — таблица FU §3.3, по таблице тестов на бэкенд. Настройка haptics пользователя —
новое поле `SystemPreferences` (ADR-0172); `Suppressed` при выключенной настройке и скрытом окне.

Виджетный путь: lifecycle-возможность; отклики виджетов с `FramePresented` ставятся в очередь и
сбрасываются при фиксации кадра; источник — устройство последовательности. `HapticsScope` выключает на
поддереве. Порог всегда сопровождается видимым изменением (A, H4). ADR заменяет ADR-0031 §1–§4.

## 8. Drag-and-drop (D1–D10)

- Типы ADR-0038: `DataTransferOffer`, `DataTransferId`, `TransferActions`, `TransferFormat`. ADR дополняет
  §5 (сторона источника, `IDropSource`/`NSDraggingSource`/`UIDragInteraction`) и §8 (Web).
- Таблица сессий — владельца-локальная, на `UiRuntime`: `DragSessionId` = `DataTransferId` + поколение;
  `DragPhase::{Pending, Active { .. }, Dropped { .. }, Cancelled { reason }}` — перечисление с данными
  состояния. Значение внутри процесса хранится в таблице и отдаётся при drop по id; между рантаймами
  или процессами — только сериализуемые представления.
- `AllowedActions { within_app, outside_app }`; несколько сессий; добавление элементов в идущую сессию.
- Модальный цикл ОС стартует после диспетчеризации (отложенный запуск), кадры идут внутри цикла; тест
  реентрантности через модальный цикл.
- Автопрокрутка над прокручиваемым — анимация по времени (`cause = SideEffect`), в том числе от ОС-DnD.
- Spring-loading с паузой из значений поведения; курсор операции; Esc отменяет.
- `Draggable`/`DragTarget` переписываются поверх; перетаскиваемый элемент списка держится живым
  (keep-alive) до конца сессии.

## 9. Запись, скрипты, проверки соответствия (R1–R9)

- В `flui-platform-api` ничего нет. `flui-testing` держит wire-DTO с закреплёнными `#[serde(rename)]`,
  отвязанный от имён вариантов словаря; JSON Lines; заголовок `flui_input_recording: <major>`, окна
  (`WindowId`, начальный масштаб), устройства (вид, инструменты, оси), значения поведения.
- Поток: `Input(PlatformInput) | Window(WindowEvent) | Frame { vsync } | AccessibilityAction(ActionRequest)`;
  поля `t` (время события, мкс от начала) и `arrived`.
- Нативный уровень: разобранные `POINTER_INFO`/`POINTER_PEN_INFO`/viewport DirectManipulation (и аналоги
  ОС) проигрываются через чистые функции разбора — Linux CI исполняет трансляцию Win32.
- Приватность: текст, заголовки, имена устройств скрыты; `--keep-text` — явно.
- Подключение — конфигурация хоста в `flui-app`; глобального состояния и переменных окружения нет.
- `PointerScript` выдаёт `PlatformInput` (сейчас `PointerEvent`, `replay.rs:133`), получает окна, кадры и
  отмены: скрипты и записи — один путь проигрывания на виртуальных часах.
- `assert_backend_conforms`: указатель, часы (монотонность, строгий рост coalesced, переполнение,
  разрешение), `PanZoom` (грамматика с инерцией), DnD, haptics; `Unsupported` проверяет отказ.
- Golden-записи: `crates/flui-platform/tests/recordings/<os>/<device>-<gesture>.jsonl` и
  `crates/flui-interaction/tests/recordings/…`.

## 10. Нативная инъекция (R8)

`cargo xtask device <os>-input`, расширяя существующий `device windows-input`: Windows —
`InjectSyntheticPointerInput` (touch, pen) и `SendInput`; macOS — `CGEvent` (мышь, scroll); Linux —
uinput/libei; Web — WebDriver BiDi `input.performActions`; Android — instrumentation; iOS — XCUITest.
Тачпад Windows и magnify/rotate macOS не инъектируются — сертифицируются golden-записями с железа.
Инъекция требует интерактивного рабочего стола; результат прикладывается к PR.

## 11. Значения поведения (S13)

Порядок: `SystemPreferences` (значения ОС, перечитываются при изменении, на дисплей) → пресет
`GestureSettings::for(platform, pointer_kind)` со ссылкой на источник в комментарии к константе → явная
настройка поддерева. Тайминги доступности ОС (задержка удержания Android, Touch Accommodations) — нижняя
граница, поддерево их не сокращает. Пресет добавляет `min_target` (Android 48 dp, iOS 44 pt, WCAG 24 px),
паузу spring-loading, ширину края свайпа, правило передачи остатка. Headless-тесты выбирают пресет явно.

## 12. Виджеты (W1–W11)

- `GestureDetector` — слоты (§0.1), строится на `RawGestureDetector`; force press, multi-tap, свайп,
  поворот; конфликт невыразим. Файл не превышает предела строк — слоты выносятся модулями.
- `RawGestureDetector` принимает `Rc<dyn GestureRecognizer>`; встроенные детекторы — на нём.
- `AnchoredDrag` (§4) и на нём `SwipeActions`, `Dismissible`, привязка `PageView`, шторки, reorder.
- `ReorderableList`/`ReorderableGrid`: keep-alive перетаскиваемого, автопрокрутка по времени,
  Alt+стрелки, объявление позиции.
- `InteractiveViewer`: поворот, snap-back, double-tap zoom, участник канала (§4).
- `ContextMenuArea`, `SelectionArea` (указатель, клавиатура и AT вместе), `HitSlop`, hit-test по фигуре.
- `OneEuro`: `GestureSettings::with_smoothing(Smoothing::OneEuro { .. })`, применяется к позициям после
  распознавания, не к slop и не к скорости.
- Material/Cupertino — обёртки стиля, включая видимую точку доступа к действиям (A1).

## 13. Доступность (A1–A7)

Есть на `main` (ADR-0203): `CustomSemanticsAction` публикуется в `accesskit::Node::custom_actions`,
`ShowContextMenu` в обе стороны, устаревший id отклоняется. Закреплённые адаптеры (`accesskit_windows`
0.35.1, `accesskit_macos` 0.27.1, `accesskit_atspi_common` 0.21.0) custom actions и `ShowContextMenu` не
передают; AT-SPI знает только `click`.

`GestureAlternatives` — конструктор поведенческого виджета регистрирует каждый `Intent` трижды:

| Жест | Semantics | Клавиша | Видимый элемент |
|---|---|---|---|
| swipe action | `CustomAction` на действие | Menu/Shift+F10 → меню действий | кнопка «Ещё» строки |
| reorder | `CustomAction` Move up/down/to start/to end (+left/right, зеркало в RTL) | Alt+стрелки | ручка; Click открывает меню перемещения |
| dismiss | `CustomAction` «Dismiss» | Delete/Esc | кнопка закрытия |
| zoom | `Increment`/`Decrement` над `NumericRange` | Ctrl+= / Ctrl+− / Ctrl+0 | кнопки масштаба (по стилю пакета) |
| refresh | `CustomAction` «Refresh» | F5 | кнопка в индикаторе |
| меню | `ShowContextMenu` | Menu/Shift+F10 | кнопка «Ещё» |
| перетаскивание | — | Esc отменяет | меню перемещения как альтернатива перетаскиванию (WCAG 2.5.7) |

`assert_gesture_alternatives` обходит все `Intent` виджета и проверяет три пути. Путевой колбэк
`GestureDetector` без `Intent` — диагностика в очереди проверки semantics-model W5. Метки —
`WidgetsLocalizations` с параметрами (сигнатура `&'static str` меняется на форматируемую); объявления —
announcer ADR-0204 §4 (semantics-model W7). Зависимость `CustomActionId` с обработчиком на id —
согласуется с semantics-model (у них это не W9; задача назначается в to-tickets). Матрица A6 расширяет
semantics-model R21. Вклад upstream в AccessKit — задача этой спеки.

## 14. Наблюдаемость (S19)

`GestureBinding` получает наблюдатель уровня владельца (`observe(Rc<dyn GestureObserver>)`): контакты,
решения арены с причиной (ADR-0112), скорости. `GestureInspector` в `flui-devtools` подписывается через
`flui-sdk` (строка `surface.rs`), рисует оверлей; глобального подписчика `tracing` нет.

## 15. ADR

| ADR | Решение |
|---|---|
| Supersedes ADR-0167 | вложенная `GestureComposition` без `Simultaneous`; осевое разрешение; `AxisLock`, `TransformLock`; задержка вложенного дерева |
| Supersedes ADR-0169 | `NestedScrollConnection`, модель fling, `Momentum`, политика, `Rc` |
| Supersedes ADR-0031 §1–§4 | двухуровневые haptics, источник и момент, `HapticOutcome` |
| Amends ADR-0038 §5, §8 | сторона источника, сессии, `AllowedActions`, Web |
| Amends ADR-0172 | настройка haptics, значения поведения на дисплей, нижние границы доступности |
| Amends ADR-0082 §5 | удаление `PlatformCapabilities`, `InputSupport` |
| новый | запись ввода, проверки соответствия, нативная инъекция, часы |
| новый | `GestureAlternatives`: доступность жестов как тип |
| новый | слои модулей `flui-interaction`, сборка указателя |

## 16. Открытые вопросы реализации

Не меняют scope; решаются в своей задаче с тестом:

- macOS: какая настройка приватности разрешает постинг `CGEvent`.
- Работает ли Core Haptics на встроенном трекпаде Mac — до проверки `Support::Unsupported`.
- Narrator и custom actions UIA — после вклада upstream.
- Различение ctrl+wheel и pinch в браузерах на реальных устройствах.
- Апстрим AccessKit iOS/Android-адаптеров — сроки зависят от проекта AccessKit.
