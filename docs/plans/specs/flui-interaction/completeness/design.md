# Полнота ввода: ядро без пакетов-заплаток — design

- **Статус:** третья редакция после трёх раундов атаки дизайна; Q35–Q36 — на владельце; имена — §0,
  размещение — §0.2
- **Дата:** 2026-10-13, база `main` @ `4da87df0b`
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **Исследования:** [best-practices](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-12-interaction-input-best-practices.md)
  («BP»), [followups](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/2026-10-12-interaction-input-followups.md) («FU»)
- Пути — от корня репозитория.

## 0. Имена

Правило (N2): устоявшийся термин экосистемы в конвенциях Rust; колонка «Где принят» обязательна.
Существующие семейства (`TapGestureRecognizer` и т.п.) переименовывает общий проход
[naming](../../naming/tasks.md); имена semantics — из ADR-0203.

### 0.1 Публичные имена

| Имя | Что это | Где принят |
|---|---|---|
| `HostInput::{Platform, Window, Frame, Accessibility}` | граница ввода хоста | — (объединение существующих `PlatformInput`, `WindowEvent`, кадра, `ActionRequest`) |
| `InputSequenceId` | идентичность последовательности ввода | W3C pointer «pointerId» + последовательность; `NonZeroU64` (ADR-0027) |
| `InputSurface` | корень маршрутизации: окно или встроенная поверхность | Bevy `NormalizedRenderTarget` |
| `PointerKind::Virtual` (без полей; устройство — `DeviceId` события) | виртуальный указатель | Bevy `PointerId::Custom` |
| `GestureComposition::{Exclusive, RequireFailure}`, `arena.sequenced() -> SequencedBranch`, `CompositionKind::{Exclusive, RequireFailure, Sequenced}` (для наблюдателя) | композиция арены | SwiftUI `ExclusiveGesture`, `SequenceGesture`; UIKit `require(toFail:)` |
| `PanClaim::{Auto, PanX, PanY, None}` | декларативная заявка оси поддерева при Down | CSS `touch-action` |
| `allowed_devices` (`PointerKindSet`) | допуск распознавателя по виду устройства | UIKit `allowedTouchTypes`; PencilKit `drawingPolicy` |
| `AxisLock`, `TransformLock` | блокировка оси и трансформа | UIKit `isDirectionalLockEnabled`; Compose `lockRotationOnZoomPan` |
| `SwipeGestureRecognizer`, `SwipeStart::{Anywhere(Axis), FromEdge(PhysicalEdge)}`, `HoldGestureRecognizer` | свайп; hold на Down | UIKit `UISwipeGestureRecognizer`, `UIScreenEdgePanGestureRecognizer` |
| `RawGestureDetector` (фабрика + `update(&config)`) | публичный детектор | Flutter `RawGestureDetector` (фабрики `GestureRecognizerFactory`) |
| `NestedScrollConnection` (`accepts_programmatic`, `sequence_started`, `pre_scroll`, `post_scroll`, `released`, `post_fling`, `sequence_ended`), `NestedScroll` (виджет, ставящий участника в цепочку) | канал прокрутки; диспетчер — внутренний (`ScrollSequence`), публичного типа нет: участник входит в цепочку только через `NestedScroll` | Compose `NestedScrollConnection`, `Modifier.nestedScroll` |
| `ScrollStep { sequence, cause, device, contact, available, focal_point, device_pixel_ratio }`, `FlingStep { sequence, device, available, focal_point, device_pixel_ratio }` | аргументы участника, `#[non_exhaustive]`, в плоскости участника (ADR-0209 §1) | против позиционных аргументов Android `NestedScrollingParent` 1→2→3 |
| `ScrollCause::{UserInput, Momentum, SideEffect, Programmatic}` | причина дельты | Compose `NestedScrollSource`; AppKit `momentumPhase`; Blitz источник прокрутки |
| `ScrollDevice::{Pointer { kind, device }, Keyboard, Assistive, Programmatic}` | устройство последовательности; действие прокрутки AT — `UserInput` с `Assistive`, `reveal` — `Programmatic` | переиспользует `PointerKind`, `DeviceId` |
| `ScrollEndReason::{Released, Cancelled, Preempted, ArenaLost}` | причина конца последовательности | — |
| `FlingHandoff::{Declined, Accepted}` | ответ `post_fling` | Compose `onPostFling` |
| `OverscrollBehavior::{Auto, Contain}` | политика остатка | CSS `overscroll-behavior` |
| `SnapPolicy`, `PageSnap` (`ViewportFraction`), `AnchorSnap` (смещения, не ключи), `SharedSnapPolicy` | привязка к якорям и страницам | Compose snap fling behavior; Flutter `PageController.viewportFraction` |
| `AnchoredDrag`, `DraggableAnchors` | одномерное перетаскивание к якорям | Compose `anchoredDraggable`, `DraggableAnchors` |
| `SwipeActions`, `ReorderableList`, `ReorderableGrid`, `ContextMenuArea`, `SelectionArea`, `HitSlop` | поведенческие виджеты | SwiftUI `.swipeActions`; Flutter `ReorderableListView`, `SelectionArea`; Compose Desktop `ContextMenuArea`; React Native `hitSlop` |
| `GestureDetector` слоты | типизированные слоты | — (конфликт невыразим, вместо assert Flutter `gesture_detector.dart:321`) |
| `InputCapabilities`, `Availability::{Native, Emulated, Unavailable}` | возможности без handle | W3C feature detection |
| `InputRegions`, `SystemGestureExclusion` | области ввода, собранные с layout | Android `setSystemGestureExclusionRects`; iOS `preferredScreenEdgesDeferringSystemGestures` |
| `HapticFeedback` (+ `Alignment`, `LevelChange`, `DragLift`, `Drop`, `Confirm`, `Reject`) | уровень 1 | AppKit, Android `HapticFeedbackConstants` |
| `HapticPattern`, `HapticEvent`, `Intensity`, `Sharpness`, `ParameterCurve`, `ControlPoint`, `HapticCapabilities` | уровень 2 | Core Haptics |
| `HapticRequest { source, timing, sequence: Option<InputSequenceId> }`, `HapticTiming::{Immediate, FramePresented}`, `HapticTicket` (порядковый номер на презентацию) | откуда, когда и в какой последовательности (вне диспетчеризации — `None`; корреляция итога — по билету, а не по последовательности) | UIKit `UIFeedbackGenerator(view:)`, `impactOccurred(at:)`; AppKit `drawCompleted` |
| `HapticOutcome::{Submitted, Queued(HapticTicket), Approximated, FellBack, Suppressed, Unavailable}` | результат; `HapticTicket` — порядковый номер (`NonZeroU64`) в презентации, без последовательности | — |
| `Draggable` (источник), `DropTarget` (цель; сегодняшний `DragTarget`, переименование — проход naming с `#[doc(alias)]`), `DragItem`, `DragSessionId`, `DragPhase::{Armed, InWindow, EscalationPending, System, Concluded}`, `AllowedActions`, `DragReach::{Window, Runtime, System}` (докуда может дойти сессия; reorder — `Window`), `SpringLoading`, `DragTrigger::{Immediate, LongPress}` | DnD | UIKit `UIDragItem`, `UIDragSession`, `UISpringLoadedInteraction`; Win32 `IDropTarget`; AppKit `sourceOperationMaskForDraggingContext:` |
| `Intent` (существующий), `GestureAlternatives`, `MenuSpec` | тройка альтернатив; `MenuSpec` — пункты меню `ContextMenu` | ADR-0023 Shortcuts–Actions–Intents |
| `GestureCommand` (метка, `Intent`, клавиши), `CommandHandle`, `ControlSpec`, `ControlPlacement::{Leading, Trailing}`, `MenuRequest`, `MenuItem`, `MenuCause`, `ShowContextMenuIntent` | команда жеста и её пути; запрос меню презентеру | UIKit `UICommand`/`UIKeyCommand`; CSS logical `inline-start`/`inline-end` |
| `PointerEvent::Revise` | поправка свойств | UIKit `touchesEstimatedPropertiesUpdated` |
| `InputTap`, `InputRecord` | наблюдатель упорядоченных записей `InputRecord::{Input, Decision, Deadline, Preferences, Gap, End}` | «tap» потока событий |
| `InputRecording`, `RecordingReader`, `RecordingWriter`, `RecordingError`, `PointerScript` | запись и скрипты | `libinput record`; `.ice`-скрипты Iced |
| `assert_backend_conforms`, `assert_gesture_alternatives`, `assert_ownership_table` | проверки соответствия | «conformance test suite» |
| `GestureObserver`, `GestureInspector` | наблюдатель, оверлей | Flutter Widget Inspector |

### 0.2 Размещение

| Имя | Крейт | Уровень | `surface.rs` | Первый продакшн-вызывающий |
|---|---|---|---|---|
| `HostInput`, `InputSequenceId`, `InputSurface` | `flui-runtime` (`HostInput`), `flui-foundation` (тип `InputSequenceId`, рядом с `DataTransferId`, чтобы контрактные крейты могли его называть), `flui-interaction` (выдаёт `GestureBinding`), `flui-platform-api` (`PointerKind::Virtual`) | Evolving / Stable | `InputSequenceId` — да (`foundation`); `HostInput`, `InputSurface` — нет: граница хоста (`flui::app::HostInput`, `GestureBinding::embed_surface`), пакет на `flui-sdk` хоста не видит (ADR-0088 §4) | `flui-app` runner, `GestureBinding` |
| арена, композиция, `PanClaim`, `allowed_devices`, `AxisLock`, `TransformLock`, свайп, трейт распознавателя, сдерживание | `flui-interaction` | Evolving через SDK | да | `GestureDetector`, `Scrollable` |
| `RawGestureDetector`, слоты `GestureDetector` | `flui-widgets` | Evolving | да | все детекторы |
| канал прокрутки, `SnapPolicy`, `OverscrollBehavior` | `flui-widgets` (`scroll::nested`) | Evolving | да | `Scrollable`, `PageView`, `RefreshIndicator`, `InteractiveViewer`, `AnchoredDrag` |
| `AnchoredDrag` и поведенческие виджеты | `flui-widgets` | Evolving | да | `flui-material`, `flui-cupertino` |
| `InputRegions` | `flui-rendering` (протокол), `flui-objects` (render-объект), `flui-widgets` (`SystemGestureExclusion`) | Evolving | да | `SwipeActions`, edge-swipe, back-gesture |
| `HapticFeedback` (+варианты), `PanZoomPhase`, `PointerEvent::Revise`, поля пера | `flui-platform-api` | Stable | реэкспорт | бэкенды, binding |
| `HapticPattern` и части, `HapticRequest`, `HapticOutcome`, `HapticCapabilities`, `InputCapabilities`, `Availability` | `flui-platform-api` за фичей `unstable-input` | Unstable до таблиц деградации и соответствия | реэкспорт за фичей | виджеты через lifecycle-возможность |
| DnD-сессии | `flui-interaction` поверх ADR-0038 | Evolving | да | `Draggable`, `ReorderableList` |
| `InputTap` | `flui-runtime` (реестр у рантайма), реэкспорт и установка — `flui-app` | Evolving | нет: установка хостом (`flui::app::InputTap`, `AppConfig::with_input_tap`), пакету не нужна | запись (dev/tool сборка) |
| запись, скрипты, проверки | `flui-testing` (бестиповый фрейминг, DTO); нативный кодек — тесты `flui-platform` | тестовый | нет | тесты всех слоёв |
| `GestureObserver` / `GestureInspector` | `flui-interaction` / `flui-devtools` | Evolving / пакет | наблюдатель — да | `flui-devtools` |
| `GestureAlternatives`, `Alternative`, `ControlSpec`, `MenuSpec`, `GestureCommand`, `CommandHandle`, `MenuRequest`, `ShowContextMenuIntent` | `flui-widgets` (`interaction::gesture_alternatives`; ключи меню — `DefaultFocusTraversal`) | Evolving | да | `Dismissible` |
| `assert_gesture_alternatives`, `GestureAlternativesCheck`, `AlternativesSubject` | `flui-testing` | тестовый | нет | матрица A6 в тестах `flui-widgets`, `flui-sdk/tests/surface` |

## 1. Позвоночник: граница ввода, время, идентичность (B1–B5)

Единственный вход рантайма — `HostInput`, определённый во `flui-runtime` (граница UI-рантайма; `flui-interaction` не зависит от accesskit): рантайм передаёт `Platform`/`Window`/`Frame` в `GestureBinding`, а `Accessibility` — владельцу semantics; вариант `Accessibility` связывает `HostInput` с accesskit 0.x — это сказано явно. `Accessibility` несёт `accesskit::ActionRequest` — тот, что
мост доступности уже передаёт (`flui-semantics/src/platform.rs:61`); декодирует его владелец semantics;
`Frame { vsync: EventTime }` — кадры на той же шкале. Runner `flui-app` сводит в него платформенный ввод, оконные
события, кадры и запросы AT (сейчас идут отдельными путями). `InputTap` наблюдает `InputRecord` (ввод `HostInput` и строки решений, настроек и пропусков в одном порядке), поэтому
запись содержит весь ввод, влияющий на решения: потерю фокуса, смену DPI, кадры, действия AT; изменения состояния приложения из невходных источников (результаты воркеров, таймеры приложения) в запись не входят.

Время: все дедлайны (долгое нажатие, `RequireFailure`, latch колеса, spring-loading, End pinch в вебе) —
на шкале `EventTime`, которую задают события; арена больше не читает `web_time::Instant`
(`binding.rs:1376`, `recognizers/contact.rs:333`). В headless один виртуальный источник ведёт часы
взаимодействия по времени прихода и `FrameClock` по vsync; строка: два прогона одной записи при фиксированном состоянии приложения дают
одинаковые решения арены.

Идентичность: `InputSequenceId` выдаёт `GestureBinding` — на контакт при Down, на последовательность
колеса, клавиатуры, AT и автопрокрутки. Его несут события, детали, канал (без собственного выпуска),
DnD-сессия, `HapticRequest`, запись и наблюдатель. Latch закрепляется по `(InputSurface, DeviceId)`.

Реентерабельность платформы: нативное событие, пришедшее во время колбэка, ставится в очередь бэкенда, а
не теряется (у GPUI теряется).

Новые устройства: `PointerKind` `#[non_exhaustive]` с `Virtual` без полей (устройство — `DeviceId` события); hit-test сегодня — точка, а
расширение (луч XR) идёт через тип запроса hit-test, не через `Vec2` в записи.

## 2. Модули `flui-interaction` (M1)

| Слой | Модули |
|---|---|
| 0 | `events`, `ids`, `retain` |
| 1 | `close`, `containment` (импортируют `retain`; модули одного слоя друг друга не называют) |
| 2 | `pointer`: `hit_test`, `interaction_lane`, `pointer_router`, `pointer_capture`, `mouse_tracker`, `resampler` и `sampling_clock` (из `processing`), `wheel_preferences`, выдача `InputSequenceId` |
| 2 | `focus` (выносится из `routing/`: `focus`, `focus_scope`, `traversal`); `text_input` (`text_input`, `clipboard`) |
| 3 | `gesture`: `arena`, `recognizers`, `velocity`, `lsq_solver`, `one_euro` (из `processing`; `Velocity` — один путь), `settings`, `details`, `traits` (`DragAxis`), `observability`, DnD-сессии |
| 4 | `binding` |
| 5 | `__runtime` (шов завершающей очистки рантайма), `testing` (фича; builder'ы, управляющие binding, — содержит код, поэтому не `transparent`) |
| — | `prelude` — `transparent` (только `use`) |

`[package.metadata.flui.modules]`, `exceptions = []`; перенос — механические коммиты; `globals` в том же
коммите. Факты: распознаватели импортируют `routing::{PointerDispatch, …}` (`tap.rs:14`, `scale.rs:41`);
`__runtime.rs` — узел цикла; `Velocity` публичен по двум путям.

## 3. `GestureBinding` (M2)

Сервисы `pub(crate)` без обратных ссылок: `PointerSequences`, `MoveCoalescer`, `ScrollSequences`,
`PanZoomSequences`, `MouseTracker`, `ArenaDriver`, `CloseCoordinator`. Каждый ключуется по `InputSurface`
с первого PR. Закрытие ставит флаг; сервисы отдают состояние `drain(&self)` через `RefCell::take` при выходе
из диспетчеризации; пользовательский код — вне заимствований.

## 4. Арена: модель владения (G1–G10)

Одна нормативная таблица в ADR, заменяющем ADR-0167; каждая клетка — строка `assert_ownership_table` в
`flui-testing`. Все разделы ссылаются на неё, а не пересказывают правила.

| Член \ топология | ортогональный предок-скролл | тот же оси предок-скролл | без конкурента |
|---|---|---|---|
| осевой (drag, `Scrollable`) | решение по накопленному смещению на slop; проигравший отклоняет | внутренний владеет; предок — только канал | держит до slop своей оси или отпускания |
| свободный (pan) | уступает осевому при ≥ cos 45° на его ось, если `PanClaim` поддерева не `None` | то же | владеет после slop |
| scale/`InteractiveViewer` | лист канала: не уступает, предок берёт остаток через канал | то же | владеет |
| `AnchoredDrag` лист (строка `SwipeActions`) | осевой член, поглощает первым, остаток — предку | то же | владеет |
| `AnchoredDrag` предок (dismiss фото) | только участник канала | то же | — |
| `Sequenced` (меню → drag, DnD, reorder, ручки) | удержание — член арены; после передачи — осевой | то же | владеет |

- `PanClaim` поддерева при Down (`None` — карта, подпись) и `allowed_devices` (перо рисует, палец
  прокручивает) — решают до правил угла; `OverscrollBehavior` в арене не участвует (он — политика канала).
- Ось — по накопленному смещению; строки 44°/45°/46° с шумом первого сэмпла. Два ортогональных осевых члена на одном сэмпле: больше проекция — побеждает, точная ничья — внутренний; `PanClaim` вдоль пути hit-test сочетается как CSS `touch-action` (пересечение).
- `Sequenced`: один член арены с явной передачей удержание → преемник; меню видно до отпускания, движение
  за slop закрывает его и начинает перетаскивание.
- `max_resolution_delay` из пресета: превышение — `CompositionError::DelayCeilingExceeded` при композиции, детектор показывает его диагностикой; нижние границы доступности ОС в проверке не участвуют.
- Отдача контакта рассылает `Cancel { reason: CancelReason::Claimed }` (новый вариант `#[non_exhaustive]` `CancelReason`) проигравшим и колбэкам `Listener` на пути, кроме потока, питающего победителя; наблюдение дальше — `GestureObserver` (как `pointercancel`).
- Касание движущегося прокручиваемого: член hold побеждает tap и long-press на контакте, не drag и scroll;
  hit-test детей не выключается.
- `Revise` не пересматривает решения; hover замирает, пока последовательность прокрутки закреплена.
- Поворот — конфигурация Scale (ADR-0166); свайп — `SwipeGestureRecognizer`; `AxisLock` с проекцией скорости.

## 5. Канал вложенной прокрутки (C1–C10)

`flui-widgets/src/scroll/nested/`; ADR заменяет ADR-0169 (ошибочное утверждение о `Send`-продолжении:
`when_complete_or_cancel` — `FnMut + 'static`, `run_future.rs:298`; endpoint переходит на `Rc`/`Cell`).
Форма ниже — текущая, с поправками ADR-0209 (см. его строку `Amended`).

```rust
#[non_exhaustive]
pub struct ScrollStep { pub sequence: InputSequenceId, pub cause: ScrollCause,
                        pub device: ScrollDevice, pub contact: bool, pub available: Vec2,
                        pub focal_point: Point, pub device_pixel_ratio: f64 }
#[non_exhaustive]
pub struct FlingStep { pub sequence: InputSequenceId, pub device: ScrollDevice, pub available: Velocity,
                       pub focal_point: Point, pub device_pixel_ratio: f64 }

pub trait NestedScrollConnection {
    /// Asked once when a `ScrollDevice::Programmatic` sequence starts; a decliner hears none of it.
    fn accepts_programmatic(&self) -> bool { false }
    fn sequence_started(&self, sequence: InputSequenceId) {}
    /// The last user contact lifted, before any fling run; release decisions (refresh, snap) go here.
    fn released(&self, step: &FlingStep) {}
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 { Vec2::ZERO }
    fn post_scroll(&self, step: &ScrollStep, consumed_by_child: Vec2) -> Vec2 { Vec2::ZERO }
    fn post_fling(&self, step: &FlingStep) -> FlingHandoff { FlingHandoff::Declined }
    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {}
}
```

- Один примитив смещения: всё, что меняет смещение (drag, колесо, клавиатура, AT, `reveal(rect)`,
  программная прокрутка, автопрокрутка DnD), идёт через диспетчер с причиной; пользовательский ввод
  прерывает программную и плавную прокрутку (Blitz). `Programmatic` участники обрабатывают по явному выбору: `accepts_programmatic` спрашивается один раз в начале программной последовательности, отказавшийся (по умолчанию) не слышит ни начала, ни шагов, ни конца. `reveal(rect)` двигает каждого предка-прокручиваемого его собственной геометрией, ближайшего первым, в отдельной программной последовательности через цепочку этого предка: участникам выше предлагается `pre_scroll` его шага, а остаток через `post_scroll` не поднимается — то, что предок должен сдвинуть, чтобы показать вложенный viewport, не равно тому, чего не смог сдвинуть вложенный (ADR-0209 §3). Участник, который хочет ответить на reveal (заголовок, раскрывающийся над строкой), делает это как область reveal. `ScrollController::set_pixels` — сырая запись позиции ниже канала (для обвязки и тестов, как коррекция якоря): участники её не видят. Коррекция якоря ADR-0051 — не прокрутка: применяется внутри layout без диспетчеризации, участники её не видят. `ScrollCause`, `ScrollDevice`, `ScrollEndReason` — `#[non_exhaustive]`.
- Лист последовательности с контактом — владелец арены (§4); без контакта — самый глубокий участник под
  точкой или фокусом, способный поглотить первую дельту; закреплён на последовательность. Пакет колеса
  или тачпада находит лист распространением события по пути hit-test (каждый `Scrollable`, начиная с
  листа, передаёт дальше пакет, которым не может сдвинуться), клавиша — фокусом; дальше latch тот же, что
  у контакта, в той же `ScrollSequence`. Это не код `AxisLock`: тот укладывает движение pan на ось внутри
  распознавателя, уже владеющего контактом, а у бесконтактного источника нет ни контакта, ни slop
  (ADR-0209 §5). Лист у края всё равно доносит пакет колеса до участников между ним и ближайшим
  объемлющим прокручиваемым.
- Fling дельтами `SideEffect`; остаток из `dx(t)`; передача наследует `InputSequenceId`; `sequence_ended`
  один раз на участника после последнего бега; `Momentum` не вызывает `post_fling`; новый `UserInput`
  отменяет переданную инерцию (`Preempted`).
- Latch: контактная последовательность закреплена до конца контакта; лист и цепочка закреплены на время жизни `InputSequenceId`, который выдал binding; таймаут простоя завершает только бесконтактные последовательности (колесо, клавиатура, AT, автопрокрутка) — это дедлайн binding на `EventTime` из пресета (`contactless_latch_idle` 1,5 с, `phased_latch_idle` 3 с — фазовый источник может начаться и не закончиться, Wayland); у канала своих таймеров нет: binding сообщает конец latch с каждым пакетом (`GestureArena::dispatched_latch_end` → `ScrollLatchEnd`), лист держит одну регистрацию дедлайна на часах арены (`GestureArena::register_deadline`). Эти два шва публичны в `flui-interaction`, потому что их читает `flui-widgets`. Цепочка закрепляется за самым глубоким участником, поглотившим часть первой ненулевой дельты (владелец у края не закрепляет); правило передачи остатка — пресет
  (Android посреди drag, iOS/macOS следующей последовательностью — проверяется на устройстве).
- Контракт: снимок цепочки на последовательность; пользовательский ввод (drag, колесо, клавиша, действие
  AT), посланный участником в свою же цепочку, отклоняется с диагностикой; программный запрос
  (`jump_to`, `animate_to`, reveal) из прохода не отклоняется, а ждёт его конца в слоте команды
  контроллера (ADR-0209 §3, §7); паника в любом вызове участника = поглощено всё, звено закрыто,
  `sequence_ended(Cancelled)`; результаты `pre_scroll` и `post_scroll` проверяются (неконечный или
  неверного знака — ничего, больше предложенного — всё); без аллокаций.
- `SnapPolicy` — одна: `PageView` = `Scrollable` + `SnapPolicy` (`PageSnap`); `AnchoredDrag` — ограниченный лист
  прокрутки с той же политикой. `AnchorSnap` не обобщён: цель — смещение, ключи держит `DraggableAnchors<K>`.
- Публичная поверхность сверх исходной формы — намеренная, каждый пункт из поправок ADR-0209 или из
  виджета, который его потребляет: `accepts_programmatic` (§2), поля шага `contact`, `focal_point`,
  `device_pixel_ratio` (§1; плоскости участников и долг оседания §6a — внутренние, публичного API нет),
  `NestedScroll` (единственный вход участника в цепочку), `ViewportFraction` и `InvalidViewportFraction`
  (`PageSnap`, `PageController`, `flui-material` `TabController`), семейство `CollapsingHeader`
  (`HeaderExtents`, `HeaderExtentsError`, `HeaderOrder`, C4), `RefreshController` и
  `RefreshIndicatorStyle` (C3; состояние обновления для индикаторов `flui-material`), `ScrollLatchEnd`,
  `GestureArena::dispatched_latch_end` и `register_deadline` (шов дедлайна выше).
- Строки (`flui-widgets/tests`, записи): C1, C2 (30°, 50°, край в начале, второй палец, отмена), C3, C4
  (включая Tab и AT под заголовком), C5 (три уровня передачи, касание после), C6–C9, по топологии §4.
  Каждая строка — в задаче, которая строит соответствующий виджет.

## 6. Платформенный слой: общее (P1–P7)

- `InputCapabilities` (приватные поля, `builder()` для бэкендов, методы доступа): перо, точная прокрутка, жесты тачпада, исходящий DnD (`outgoing_drag()`), области
  ввода; `Availability`; присутствие — из `DeviceRegistry`; начальный снимок `PlatformInput::Capabilities` до любого обычного ввода поверхности, затем изменения — тем же событием. `PlatformCapabilities`
  удаляется (ADR-0082 §5). Haptics и IME — по своим handle.
- Сборка указателя в `flui-platform/src/shared/` (расширяя `shared/events.rs`): `ContactRegistry`,
  `DeviceRegistry`, `HeldButtons`, `ClickCounter`, `PanZoomAccumulator`, `MoveBatch`; чистые функции
  разбора над обычными структурами; исходное нативное событие на контакт хранится для старта нативного DnD.
- Часы: QPC на Win32 (выбор на устройство и последовательность), частота и якорь в заголовке нативной
  записи.
- Прокрутка двумя пальцами — `Scroll` с фазами на всех ОС; `PanZoom` — только масштаб и поворот с
  определённым фокусом; инерция прокрутки — `ScrollPhase::Momentum*` (существующие).
- `InputRegions`: протокол в `flui-rendering`, render-объект в `flui-objects` (строка
  `RENDER_OBJECT_TYPES`, harness), сбор на окно при компоновке, метод `PlatformWindow`; первый вид —
  исключение системных жестов.

## 7. Платформенный слой: по ОС (P8–P15)

| ОС | Ввод | DnD | Haptics | IME | Инъекция и записи |
|---|---|---|---|---|---|
| Windows | DirectManipulation (тачпад): перевод в `Scroll` с фазами и `PanZoom`, прокачка, свой запрос кадра, модальный цикл размера; QPC | OLE источник и цель, отложенный старт, кадры всем рантаймам потока, перерисовка второго окна | `SimpleHapticsController` пера; `InputHapticsManager` — `Unavailable` (в опубликованных `windows` его нет; при нужде — узкий биндинг `windows-bindgen` 0.65) | TSF (существует) | `InjectSyntheticPointerInput`, `SendInput`; тачпад — записи с железа |
| macOS | перо (tablet, proximity), smart magnify, swipe | `NSDragging*`, promised files, маски по контексту | `NSHapticFeedbackManager` + `drawCompleted` | `NSTextInputClient` | `CGEvent` (мышь, scroll); жесты — записи |
| iOS | трекпадные pinch/rotate и прокрутка, coalesced/predicted, `Revise`, Pencil, системные края | `UIDrag/DropInteraction`, несколько сессий | `UIFeedbackGenerator(view:)`, Core Haptics с `resetHandler` | `UITextInput` | XCUITest |
| Android | pinch тачпада, стилус, рукописный ввод, системные края, `ViewConfiguration` | `OnReceiveContentListener`, DnD | `VibrationEffect` (envelope 36+, primitives 30–35, `USAGE_TOUCH`) | `InputConnection` | `adb shell input` и `uinput` (не instrumentation, ADR-0215 §8) |
| Linux | winit 0.31 (перо — только Wayland; X11 — `Unavailable`); жесты тачпада X11 — `Unavailable` | DnD winit 0.31 (исходящий на X11 — `Unavailable`) | `Unavailable` | честный `composing` | uinput/libei |
| Web | события с `preventDefault` — синхронно в колбэке; ctrl+wheel → pinch (дробная дельта, классификация по первому событию, End — дедлайн на шкале `EventTime`, `Emulated`, отказ в пользу браузерного zoom); Safari `gesture*` | HTML DnD | Vibration API с активацией | `EditContext` / скрытый `<textarea>` (свои `extern`: `EditContext`, наклон пера, predicted events) | WebDriver BiDi |

## 8. Haptics (H1–H5)

Уровни 1–2; оба принимают `HapticRequest`. Немедленный запрос возвращает итог; `FramePresented` —
`Queued(HapticTicket)`, итог с тем же билетом доставляется наблюдателю. Очередь запрашивает кадр, имеет дедлайн → `Suppressed`,
сливается при скрытии, перекрытии и закрытии. Устаревший источник — устройство окна по умолчанию.
Настройка haptics — `SystemPreferences`. Деградация — таблица FU §3.3 на бэкенд. Паттерны и итоги — за
`unstable-input`. ADR заменяет ADR-0031 §1–§4.

## 9. Drag-and-drop (D1–D6)

- Типы ADR-0038; ADR-0211 дополняет §2 (несколько одновременных предложений и сессий там, где платформа
  их доставляет), §5 (источник) и §8 (Web).
- Web не может начать HTML-перетаскивание посреди жеста и не даёт ленивых данных: на Web (на `dragstart`)
  и iOS/iPadOS (на подъёме) сессия стартует как сессия ОС по тому же правилу, что и эскалация: `outside_app`
  непусты **или** у рантайма-источника есть другая открытая презентация (тогда — только приватный формат
  идентичности, а ОС объявляются действия `within_app`); на Web данные материализуются. Сессия
  `DragReach::Window` (`ReorderableList`, `ReorderableGrid`) никогда не становится сессией ОС ни на одной
  платформе.
- Nonce процесса выпускается один раз на экземпляр платформы (не `static`).
- Внутри окна перетаскивание идёт на захвате указателя (touch, перо, второй палец сохраняются); в модальный
  цикл ОС — при выходе указателя из окна, если `outside_app` непусты или у рантайма-источника есть другое открытое окно (тогда предлагается только приватный формат идентичности, а ОС объявляются действия `within_app`). Исключения: Web (HTML DnD с `dragstart`) и iOS/iPadOS (`UIDragSession` с подъёма через `UIDragInteraction`, без эскалации посреди перетаскивания).
- Идентичность: (nonce процесса, `UiRuntimeId`, `DragSessionId`) в приватном формате предложения;
  Rust-значение — из таблицы сессий своего `UiRuntime` (любая его презентация). Между рантаймами — только сериализуемые представления (Q35); `WindowPolicy::Shared` — для обмена значениями между окнами.
- `AllowedActions`, несколько сессий, drop только цели, принявшей последнее движение, каждая другая вошедшая цель (в том числе отказавшаяся) получает ровно один leave; spring-loading;
  автопрокрутка `SideEffect`; keep-alive перетаскиваемого: `Draggable` держит аренду `KeepAliveHandle::hold`, пока жива его сессия, так что строка ленивого списка, уехавшая из полосы кэша, сохраняет состояние и сессию (`a_dragged_row_scrolled_out_of_a_lazy_list_keeps_its_drag`). Pinned-слот focus-keyboard T4 для этого не нужен: строку вне полосы не раскладывают, а отклик перетаскивания рисуется в оверлее.
- Модальный цикл: отложенный старт в самой внешней точке покоя; кадры всем рантаймам потока; строки
  реентрантности и перерисовки второго окна.
- Reorder — DnD-сессия с быстрым путём внутри процесса; `AnchoredDrag` — одномерный.

## 10. Запись, скрипты, проверки (R1–R10)

- Упорядоченный поток `InputRecord::{Input, Decision, Deadline, Preferences, Gap, End}`; у строки `Input` — `HostInput`, `InputSurface`/`WindowId`, `t`, время прихода, результат диспетчеризации и `InputSequenceId`; решения `GestureObserver` — строками `Decision` с последовательностью состязания и номером участника в записи (номер — идентичность, имя типа — только пометка; решения по дедлайнам воспроизводимы); `GestureObserver` сам получает производные события жестов, не `HostInput`. Заголовок — значения жестов каждого окна по видам указателя и ревизия `SystemPreferences` со значениями; воспроизведение восстанавливает их до первой строки.
- `flui-testing`: бестиповый фрейминг JSON Lines, заголовок, DTO типов `flui-platform-api` с закреплёнными
  `#[serde(rename)]`; gate — генерированный круговой прогон каждого варианта. Нативный кодек
  (`POINTER_INFO`, viewport DirectManipulation, аналоги) — тесты `flui-platform`.
- `PointerScript` выдаёт `HostInput`, читаемый текстовый вид рядом с JSONL; builder'ы мыши, touch, пера,
  тачпада и виртуального указателя наравне; каждая строка таблиц G и C — минимум для touch и мыши.
- Поиски по дереву semantics; запуск «до успокоения» с пределом шагов; строки «двойной клик за кадр»,
  «N событий за кадр», «событие между vsync».
- `InputTap` во `flui-runtime` (`UiRuntime::input_taps`), установка приложением — `AppConfig::with_input_tap` во `flui-app`; сериализатор — dev/tool сборка; приватность по умолчанию.
- `assert_backend_conforms`: указатель, часы, грамматика `Scroll`/`PanZoom`, DnD, haptics, IME-порядок; каждая
  функция добавляет свои строки.

## 11. Нативная инъекция

`cargo xtask device <os>-input` для всех ОС (§7); тачпад Windows и жесты macOS — записи с железа;
интерактивный рабочий стол; результат — в PR.

## 12. Значения поведения (S13)

`GestureSettings::preset(platform, pointer_kind)` — только числа, цепочка отката вид → coarse/fine →
платформа, ссылки на источники; `min_target`, spring-loading, край свайпа, правило передачи остатка,
таймауты latch, `max_resolution_delay`. Клавиши жестов — значения по умолчанию `Shortcuts` на платформу во
`flui-widgets` рядом с `Intent`. Headless выбирает пресет явно.

## 13. Виджеты (W1–W12)

- `GestureDetector` на слотах поверх `RawGestureDetector`; `RawGestureDetector` принимает фабрику и
  `update(&config)`, экземпляр — в состоянии элемента (как сегодня `gesture_detector.rs:1107-1187`).
- `AnchoredDrag` (одномерный, `SnapPolicy`) → `SwipeActions`, `Dismissible`, шторки.
- `PageView` = `Scrollable` + `SnapPolicy`, осевой drag.
- `ReorderableList`/`Grid` — DnD-сессия; `InteractiveViewer` — лист канала; `ContextMenuArea` и ручки
  `SelectionArea` — `Sequenced`.
- `HitSlop`: визуальное попадание → расстояние до края → z-порядок; не пересекает слой, барьер, оверлей.
- Hit-test по фигуре — render-объект с harness.
- `OneEuro` после распознавания.

Сверх перечня W, с причиной и потребителем:

- `Alternative::Companion` (`CompanionSpec`, `CompanionLink`, `CompanionRegistration`, `CompanionGap`) —
  третья одноуказательная альтернатива ADR-0216: виджет, уже рисующий элементы управления (вкладки
  `flui-material`), регистрирует их на ссылке вместо второго набора кнопок; `CompanionGap` — отказ
  `assert_gesture_alternatives`.
- `SwipeAlternative::PresentedMenu` — меню `SwipeActions`, показанное презентером пакета дизайна, а не
  бестемным `ContextMenuArea`; строка таблицы владения в `flui-testing` его проверяет.
- `Dismissible` сохраняет словарь направлений, порогов и колбэков обновления (`DismissDirection`,
  `DismissThreshold`, `DismissUpdateDetails`, `DismissDirectionCallback`): это настройка поверх
  `AnchoredDrag`, которую задают `flui-material` и приложения; порог — проверенный newtype, а не `f64`.
- `PanAxis` и `WheelScaleGate` у `InteractiveViewer` — ось панорамирования и выбор, масштабирует ли колесо
  без модификатора: без них просмотрщик внутри списка забирает его прокрутку колесом.
- `ForcePressSlot` у `GestureDetector` — слот распознавателя давления, как `DragSlot` и `TransformSlot`
  (порог старта и пика, только для устройств с измеренным давлением).
- `AnchoredDrag`: границы ключа, ошибки якорей и `set_anchors` — §18.

## 14. Доступность (A1–A9)

`GestureAlternatives` регистрирует каждый `Intent` как semantics-действие (ADR-0203), клавишу (`Shortcuts`
платформы) и одноуказательную альтернативу `Alternative::{VisibleControl, ContextMenu}` с условиями requirements A1 (Q36). Esc не деструктивен; при композиции IME Esc идёт в IME.
Действия ключуются ключом модели; «переместить в конец» в ленивом списке сохраняет фокус и объявляет;
бюджет стоимости semantics на строку. `SelectionArea` ждёт поправки ADR-0204 (текстовые прогоны для
текста под `SelectionArea`). Согласовано с semantics-model: `Semantics::custom_action(label, handler)` и
непрозрачный `CustomActionId` — место в списке узла; у каждого действия свой обработчик; при `MergeSemantics` действия ребёнка идут после действий родителя, устаревший запрос отклоняется; `CustomSemanticsAction` удалён (PR #1606, слит в main; одна метка — одно действие внутри `Semantics`, повтор заменяет обработчик; метки различимы по цели — «Delete Alice»; узел, блокирующий действия, их не предлагает); виджеты сразу используют `.custom_action(label, handler)`, без обёрток над `i32`; announcer W7 (до него — свой
live-region узел), строки W5 внесены у них. Строки матрицы — в PR каждого виджета; вклад в AccessKit —
задача этой спеки.

## 15. Наблюдаемость и расширение

`GestureObserver`: события ставятся в очередь и доставляются после диспетчеризации, изоляция паник, без
аллокаций без подписчиков. `GestureInspector` — через `flui-sdk`. Поверхность расширения — §0.2; каждая
задача добавляет свои строки `surface.rs`; доказательство — нестандартный распознаватель в
`flui-sdk/tests` с пересборкой посреди жеста. Шаблон-чеклист нового поведенческого виджета.

## 16. ADR

| ADR | Отношение | Решение | Задачи |
|---|---|---|---|
| [ADR-0207](../../../../adr/ADR-0207-host-input-boundary-and-timeline.md) | новый | граница `HostInput` (в `flui-runtime`), шкала времени, `InputSequenceId`, `InputSurface`, модель потоков | T07 |
| [ADR-0208](../../../../adr/ADR-0208-gesture-ownership-table-and-composition.md) | Supersedes ADR-0167 | таблица владения, `PanClaim`, `allowed_devices`, `Sequenced`, `CancelReason::Claimed`, потолок задержки | T14, T18 |
| [ADR-0209](../../../../adr/ADR-0209-nested-scroll-connection.md) | Supersedes ADR-0169 | канал, `released`, один примитив смещения, latch и таймауты binding, `SnapPolicy` (`Arc`) | T21, T23 |
| [ADR-0210](../../../../adr/ADR-0210-two-tier-haptics.md) | Supersedes ADR-0031 §1–§4 | haptics | T16, T38 |
| [ADR-0211](../../../../adr/ADR-0211-unified-drag-and-drop.md) | Amends ADR-0038 §2, §5, §8 | DnD: эскалация, iOS и Web с подъёма, идентичность, сессии | T28–T30, T35, T37 |
| [ADR-0212](../../../../adr/ADR-0212-system-preferences-for-input.md) | Amends ADR-0172 | haptics-настройка, значения на дисплей, нижние границы, порядок пресетов | T05 |
| [ADR-0213](../../../../adr/ADR-0213-input-capabilities.md) | Amends ADR-0082 §5 | `InputCapabilities` (builder), `PlatformInput::Capabilities` | T04 |
| [ADR-0214](../../../../adr/ADR-0214-selection-area-text-runs.md) | Amends ADR-0204 §2 | текстовые прогоны под `SelectionArea` | T34 |
| [ADR-0215](../../../../adr/ADR-0215-input-recording-and-backend-conformance.md) | новый | запись (со строками решений), проверки соответствия, инъекция, часы | T08, T12, T13, T39 |
| [ADR-0216](../../../../adr/ADR-0216-gesture-alternatives.md) | новый | `GestureAlternatives`, `ControlSpec`, `MenuSpec` | T19 |
| [ADR-0217](../../../../adr/ADR-0217-interaction-module-layers-and-pointer-assembly.md) | новый; Amends ADR-0143 | слои, сборка указателя, `InputRegions`, `PanZoom` без pan | T02, T03, T09 |

## 17. Открытые вопросы реализации

Настройка приватности для `CGEvent` на macOS; Core Haptics на встроенном трекпаде Mac; правило передачи
остатка на iOS (проверить на устройстве); различение ctrl+wheel и pinch в браузерах; выравнивание
`accesskit_ios` 0.2 и `accesskit_android` 0.9 по версиям `objc2` и `jni`; читает ли `windows-bindgen` 0.65
метаданные нового SDK.

## 18. Паттерны Rust по компонентам (requirements X)

| Компонент | Форма и диспетчеризация | Builder, newtype, ассоциированные типы | Владение | Атрибуты, ошибки |
|---|---|---|---|---|
| `HostInput` | перечисление во `flui-runtime`; `UiRuntime::dispatch(HostInput)` разворачивает его и передаёт `GestureBinding` только части `flui-interaction` (`PlatformInput`, `WindowEvent`, тик кадра), а `Accessibility` — владельцу semantics; ни одна сигнатура `flui-interaction` не называет `HostInput`; трейта источника нет — набор закрыт | — | значение внутрь, итог наружу | `#[non_exhaustive]`; `DispatchOutcome` `#[must_use]` |
| id (`InputSequenceId`, `DragSessionId`, …) | newtype `NonZeroU64`, приватное поле, `Copy + Eq + Ord + Hash`, без `Default`/`From<u64>`; межрантаймовый ключ — структура `{ProcessNonce, UiRuntimeId, DragSessionId}` | выпуск только `IdMint::next() -> Result<Id, IdExhausted>` | счётчик `Cell<u64>` | `IdExhausted` (`thiserror`) |
| арена, `GestureComposition` | конкретная структура арены; композиция — перечисление; члены — `Weak<dyn GestureArenaMember>` | `compose(&self, c) -> Result<GestureBranches, CompositionError>`; у `Sequenced` свой метод → `SequencedBranch` | ребёнок `Rc` на родителя, родитель `Weak` (`Rc::new_cyclic`) | `CompositionError::{DelayCeilingExceeded { sum, ceiling }, Closed}` — потолок задержки как `Err` при композиции |
| трейт распознавателя | открытый `GestureRecognizer: GestureArenaMember`, `&self` (ADR-0161), `dyn`, объектно-безопасный | без ассоциированного `Details` (сломал бы `dyn`) | `Rc<dyn …>`, upcasting к `Rc<dyn GestureArenaMember>` (1.86); `PointerDispatch<'_>` не конструируется извне | `on_unimplemented`: «реализуйте также `GestureArenaMember`» |
| фабрика `RawGestureDetector` | открытый `GestureRecognizerFactory`, стирается приватным blanket `ErasedFactory`, ключ — `TypeId` + явный `Key` | `type Recognizer: GestureRecognizer`; `create`, `update(&self, &Self::Recognizer)` | экземпляр в состоянии элемента | `do_not_recommend` на blanket-реализации |
| слоты `GestureDetector` | структура с `Option` на семейство конфликта (`DragSlot`, `TransformSlot`) | потребляющий builder без typestate | неизменяемая конфигурация, пересборка сравнивает | `#[must_use]` |
| `NestedScrollConnection`, диспетчер | открытый объектно-безопасный трейт на `&self`, методы по умолчанию, `!Send`; диспетчер — внутренняя `ScrollSequence`, участник подключается виджетом `NestedScroll` | `ScrollStep`/`FlingStep` строит только диспетчер (`for_test` за `testing`); RPITIT нет | снимок `SmallVec<[Weak<dyn …>; 4]>` на последовательность; состояние участника в `Cell`/`RefCell` | нарушение контракта — обрезка с диагностикой |
| `SnapPolicy` | открытый трейт по образцу `ScrollPhysics`: `target(&self, &ScrollMetrics, velocity) -> f64`; `PageSnap`, `AnchorSnap` (не обобщённый: цель — смещение, карту ключей держит `DraggableAnchors<K>`, ADR-0209 §9) | — | `SnapPolicy: Send + Sync + Debug`, `Arc<dyn SnapPolicy>` как `SharedScrollPhysics`, идентичность — `Arc::ptr_eq` (шов компоновщика ADR-0207) | неконечная цель — обрезка с диагностикой |
| `AnchoredDrag` | `AnchoredDrag<K: Copy + Eq + 'static>`, статически по перечислению якорей; `Hash` не требуется — якоря — отсортированный по смещению срез `Rc<[(K, f64)]>`, ключ ищется линейно; в канале участвует сам как прокручиваемый, публичного `connection()` нет | `DraggableAnchors::<K>::builder().at(k, x).build() -> Result<_, AnchorsError>`; `AnchoredDragController::set_anchors` — команда хэндла, как `jump_to`, а не `set_*` конфигурации: якоря `Dismissible` и `SwipeActions` зависят от протяжённости ряда, известной после layout | хэндл `Rc` с `Cell`, уведомления — существующий `ChangeNotifier` | `AnchorsError::{Empty, NonFinite, OutOfRange, Duplicate, Coincident, Missing}`: `OutOfRange` держит произведения скоростей конечными, `Coincident` — два ключа на одном смещении неразличимы при покое, `Missing` — ключ `jump_to`/`animate_to` вне набора |
| `GestureAlternatives` | `enum Alternative { VisibleControl(ControlSpec), ContextMenu(MenuSpec) }` — обязательный аргумент конструктора; `ContextMenu` строится только с условиями A1 | `I: Intent` типизированно | владеемая конфигурация | `on_unimplemented` на `Intent` |
| `HapticPattern` | проверяющий builder без typestate | `Intensity(f32)`, `Sharpness(f32)`: `TryFrom<f32>`, `const fn new -> Option`; готовый — `Arc<[HapticEvent]>` (неизменяемое значение через `Send + Sync` границу платформы) | значение | `HapticPatternError::{Unordered { index }, NonFinite, OutOfRange, EnvelopeNotClosed, TooLong { max }}`; `TryFrom<f32, Error = UnitRangeError>` |
| `HapticOutcome` | перечисление, не `Result` | `Queued(HapticTicket)`, билет — порядковый номер в презентации | значение | `#[must_use]`, `#[non_exhaustive]` |
| DnD-сессии | `pub(crate) DragSessions { Slab<Entry> }` на `UiRuntime`; `DragPhase` — перечисление с данными, `mem::replace` | `DragItem`: `InProcess(Rc<dyn Any>)` и `Serialized(TransferFormat, Lazy)`; `with_value<T>`, `value::<T>() -> Option<&T>` | `Rc<RefCell<DragSessions>>`, короткие заимствования; ленивый поставщик `Box<dyn FnOnce() -> Result<..>>` `!Send` | `TransferError`, `DragError::{IdExhausted, NoAllowedActions, NoOutsideActions, NoSerializedRepresentation, SessionClosed, Unavailable}` |
| `InputCapabilities`, `Availability` | `Copy`-структура с приватными полями, геттерами без `get_` и `InputCapabilities::builder()` для бэкендов; изменение — `PlatformInput::Capabilities`; `Availability` — закрытое перечисление | — | значение, изменения — событиями | шлюз `unstable-input` — на реэкспортах `flui-sdk` и фасада; `Availability` выводит `Clone, Copy, Debug, PartialEq, Eq, Hash` |
| DTO записи | приватные зеркальные перечисления в `flui-testing`, закреплённые `#[serde(rename)]`; неизвестные поля в пределах major-версии пропускаются, неизвестный вариант — ошибка; `From<&InputRecord>` и `TryFrom<Dto>` над всеми вариантами `InputRecord` (`Input` с вложенным DTO `HostInput`, `Decision`, `Preferences`, `Gap`, `End`); исчерпывающий `match` невозможен (`#[non_exhaustive]`) — gate кругового прогона по образцу каждого варианта записи и словаря обязателен | `RecordingReader<R: BufRead>: Iterator<Item = Result<Record, RecordingError>>`, `RecordingWriter<W: Write>` | владеемое | `RecordingError::{Io, Json { line, source }, Version { found, supported }, UnknownVariant { line, tag }, NonReplayable { line }, Incomplete { lost_rows }}` (`NonReplayable` — при проигрывании редактированная строка, маркер `Gap` или поток без строки `End`; проигрывание останавливается; `Incomplete` — завершение записи не доставило последний `Gap` или `End`) |
| `PointerScript` | отдельные потребляющие builder'ы на устройство (`TouchScript`, `MouseScript`, `PenScript`, `TrackpadScript`, `VirtualScript`), `Into<PointerScript>` | `build() -> Result<_, ScriptError>` проверяет пары down/up | владеемый `Vec<HostInput>` на виртуальном времени | `#[must_use]` |
| проверка соответствия бэкенда | `assert_backend_conforms<B: BackendUnderTest>(&mut B)`, статически | `type Reading`; RPITIT `decode(&mut self, r) -> impl Iterator<Item = HostInput> + use<'_, Self>` (трейт не `dyn`) | `&mut` | строки через `run_table` |
| `InputRegions` | `InputRegionKind` — перечисление; render-объекты пишут логические `Rect` в кадровый `&mut InputRegionSink`; `PlatformWindow::set_input_regions` каждый скомпонованный кадр с логическими областями; проекция и сравнение ключа (пиксели устройства, масштаб, `Option<DisplayId>`; переход `None`↔`Some` — изменение) с прошлым кадром — в бэкенде, ОС обновляется только при изменении | логические типы ADR-0098 | `&mut`, без `Rc` | `#[non_exhaustive]` |
| `GestureObserver` | открытый объектно-безопасный трейт на `&self` | события — `#[non_exhaustive]` перечисление по ссылке | `observe()` → `#[must_use] ObserverHandle`, `Drop` отписывает; очередь `RefCell<VecDeque>` после диспетчеризации | изоляция паник, первая ошибка авторитетна |
| сборка указателя | обычные структуры и чистые функции: `decode(&mut ContactRegistry, &NativeReading) -> Result<SmallVec<[PlatformInput; 2]>, DecodeError>`; трейта с одной реализацией нет | `NativeReading` без `windows::*`; `QpcTicks(u64)`, `NativeTime` | `&mut` состояние бэкенда | `DecodeError` отменяет открытый контакт (`CancelReason::InvalidInput`) с диагностикой; показание без открытого контакта отбрасывается |
| ошибки | одно `thiserror`-перечисление `#[non_exhaustive]` на домен | — | — | без `Box<dyn Error>`, без строковых вариантов, `anyhow` только в приложениях |
