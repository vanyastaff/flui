# Полнота ввода: ядро без пакетов-заплаток — design

- **Статус:** черновик на утверждение владельцем; имена — таблица §0 (требование N5)
- **Дата:** 2026-10-12
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **Исследования:** [best-practices](../../../../research/2026-10-12-interaction-input-best-practices.md)
  (далее «BP»), [followups](../../../../research/2026-10-12-interaction-input-followups.md) (далее «FU»)
- **База кода:** `main` @ `749b74466`. Пути — от корня репозитория.

## 0. Имена на утверждение

Каждое имя проходит N1–N4. Существующие семейства (`TapGestureRecognizer` и т.п.) не переименовываются
здесь: это делает общий проход спеки [naming](../../naming/renames.md); новые типы получают имена,
которые переживут этот проход.

| Имя | Что это | Почему так |
|---|---|---|
| модуль `close` | закрытие владельца: `CloseMode`, `ClosePanic`, `CloseTombstone` (вынос из `__runtime`) | типы уже называются `Close*`; модуль называет предмет, `__runtime` — нет |
| `PointerSequences` | сервис: последовательности контактов, кэш маршрутов, отказанные контакты | расширяет существующий `PointerSequence` |
| `MoveCoalescer` | сервис: склейка `Move` и ресэмплинг до кадра | событие называется `Move`; «coalesce» — термин W3C |
| `ScrollSequences`, `PanZoomSequences` | два сервиса допуска и завершения последовательностей scroll и pan/zoom | вместо собирательного `SignalSequences`, который не говорит, каких сигналов |
| `ArenaDriver` | сервис: владеет ареной, снимком настроек и тиками дедлайнов; закрывает на Down, подметает на Up | «drive» — то, что он делает с ареной; `Host` не говорит ничего |
| `ContactLifecycle` | трейт, которым владеет `pointer`: `fn routed(..)`, `fn ended(..)`; его реализует `ArenaDriver` | `pointer` не называет типы `gesture` (M1) |
| `CloseCoordinator` | сервис: флаг закрытия, режим, tombstones, рассылка закрытия сервисам | координирует закрытие нескольких владельцев |
| `pointer_assembly` (в `flui-platform/src/shared/`) | общая сборка указателя из нативных показаний | говорит, что собирает; `shared/` — существующий каталог |
| `ContactRegistry`, `DeviceRegistry`, `HeldButtons`, `ClickCounter`, `PanZoomAccumulator`, `MoveBatch` | части `pointer_assembly` | каждое — один предмет |
| `GestureRelation::{Exclusive, AfterFailureOf, Simultaneous}` | отношение двух ветвей арены | заменяет `GestureCompetition`: `Simultaneous` — не соревнование; `AfterFailureOf` читается как фраза «вторая — после провала первой» |
| `AxisLock::{Free, Horizontal, Vertical, DominantAfterSlop, Rails { breakout_angle }}` | блокировка оси распознавателя | варианты — состояния; у порога есть имя и единица (`Angle`) |
| `NestedScrollChain`, трейт `NestedScrollParticipant` | канал дельт scroll/fling между прокручиваемыми предками | Compose называет это connection; «participant» говорит, что тип делает в цепочке |
| `before_child_scroll`, `after_child_scroll`, `before_child_fling`, `after_child_fling` | методы участника | порядок относительно ребёнка в имени; `pre_`/`post_` требуют знать Compose |
| `ScrollSource::{Drag, Wheel, Fling, Keyboard, Programmatic}` | откуда пришла дельта | — |
| `RotateGestureRecognizer`, `SwipeGestureRecognizer` | новые распознаватели | семейство существующих имён до общего прохода |
| `SwipeStart::{Anywhere, FromEdge(ScreenEdge)}`, `ScreenEdge::{Leading, Trailing, Top, Bottom}` | конфигурация свайпа | edge-swipe — конфигурация, не отдельный тип; `Leading`/`Trailing` учитывают RTL; типа края в workspace нет |
| `InputCapabilities`, `Support::{Native, Emulated, Unsupported}` | типизированный ответ бэкенда о поддержке | заменяет TODO в `capabilities()` |
| `HapticIntent`, `HapticPattern`, `HapticEvent`, `Envelope`, `ControlPoint`, `UnitInterval`, `HapticCapabilities`, `Fidelity`, `PlayOutcome` | модель haptics (§7) | `UnitInterval` вместо `Unit`: `Unit` уже занят параметром геометрии (ADR-0098) |
| `DragItem`, `DragRepresentation`, `DragOperation`, `DragSession`, `DropTarget` | модель DnD (§8) | — |
| `InputRecording`, `RecordingReader`, `RecordingWriter`, `RecordingError`, `ScriptedInput` | запись и проигрывание ввода (§9) | фича `recording`; `trace` путается с `tracing` |
| модуль `backend_conformance`, `assert_backend_conforms` | проверка соответствия бэкенда (§10) | вместо `*_kit` |
| `cargo xtask input-smoke` | нативная инъекция для сертификации (§11) | говорит, что делает |
| `SwipeActions`, `ReorderableList`, `ReorderableGrid`, `ZoomableView`, `ContextMenuRegion`, `SelectionArea`, `HitArea` | поведенческие виджеты (§13) | описывают поведение, не стиль |
| `DragStart::{Immediate, LongPress}` | конфигурация `Draggable` | вместо отдельного `LongPressDraggable` |
| `GestureInspector` | оверлей в `flui-devtools` (§15) | — |

## 1. Модули `flui-interaction` (M1)

Факты (сверка кода): распознаватели импортируют `routing::{PointerDispatch, PanZoomDispatch, RoutePanic}`
(`recognizers/tap.rs:10`, `scale.rs:36` и др.); `__runtime.rs` — узел цикла: импортирует указатель, фокус,
IME и `GestureBinding` и при этом поставляет `CloseMode`/`ClosePanic`/`CloseTombstone` всем модулям;
`settings ↔ processing` и `details ↔ recognizers` — циклы внутри будущего `gesture`; фокус лежит в
`routing/`.

Слои снизу вверх (`[package.metadata.flui.modules]` в `crates/flui-interaction/Cargo.toml`, формат как у
`flui-widgets` `Cargo.toml:238-298`):

| Слой | Модули | Содержимое |
|---|---|---|
| 0 | `events`, `ids`, `close`, `containment` | реэкспорт словаря, ID, закрытие владельца (`__runtime.rs` 17–24, 217–399), `RoutePanic` и сдерживание (из `interaction_lane.rs`) |
| 1 | `pointer` | `hit_test`, `interaction_lane`, `pointer_router`, `pointer_capture`, `mouse_tracker`, `resampler`, `sampling_clock`, `wheel_preferences` |
| 1 | `focus` | `focus`, `focus_scope`, `traversal`, диспетчеризация клавиш |
| 1 | `text_input` | `text_input`, `clipboard` |
| 2 | `gesture` | `arena`, `recognizers`, `velocity`, `lsq_solver`, `one_euro`, `settings`, `details`, `observability` |
| 3 | `binding` | `GestureBinding` и сервисы §2; фасадные функции `__runtime.rs` 25–215 |
| — | `testing` (фича), `prelude` | `transparent` |

`exceptions = []`. Перенос — механический коммит без изменения поведения; `globals`-записи
(`Cargo.toml` ~131–135) обновляются в том же коммите.

## 2. `GestureBinding` как корень композиции (M2)

Сейчас 3727 строк, из них тесты 2801–3727. Поля 469–531 делятся так:

| Сервис | Слой | Владеет | Строки сейчас |
|---|---|---|---|
| `PointerSequences` | pointer | `hit_tests`, счётчик последовательностей, `refused_contacts`, guards реентрантности | 137–172, 265–325, 369–440, 2265–2337, 2474–2695 |
| `MoveCoalescer` | pointer | ожидающие `Move`, поколения, ресэмплер, `SamplingClock` | 173–246, 629–705, 807–1101, 2338–2473 |
| `ScrollSequences`, `PanZoomSequences` | pointer | последовательности и плечи ядра | 328–368, 1384–1626, 1978–2187 |
| `MouseTracker` | pointer | уже сервис | — |
| `ArenaDriver` | gesture | арена, настройки по умолчанию, дедлайны; реализует `ContactLifecycle` | 603–628, 1325–1383 |
| `CloseCoordinator` | binding | `closed`, режим, `capture_wake`, tombstones | 532–602, 1142–1324 |

`GestureBinding` держит сервисы и делегирует; ядро диспетчеризации (1627–2264) превращается в
сопоставление `PointerEvent` → вызов сервиса. Каждый сервис получает таблицу тестов через публичный API
binding (тесты не трогают поля сервисов — правило «Writing tests»).

## 3. Координация жестов: арена (G4, G7)

`GestureCompetition` → `GestureRelation`, ADR-0167 заменяется.

- `arena.compose(relation) -> GestureBranches`; ветвь может снова вызвать `compose`. Отношения образуют
  дерево, владеемое владельцем (`Rc`, без глобальных счётчиков, как в ADR-0167).
- `Exclusive` — один победитель на поддерево (текущее правило).
- `AfterFailureOf` — текущий `RequireFirstFailure` без изменений семантики.
- `Simultaneous` — каждое поддерево разрешается само; контакт может иметь по победителю в каждом
  поддереве. Это не возвращение «команд»: множественная победа объявляется родителем явно и ограничена
  этим узлом; вне него правило одного победителя остаётся.
- Отмена и дедлайны обходят дерево; первая ошибка остаётся авторитетной (ADR-0127). Тест —
  таблица: одиночный провал, конкурирующие провалы, следующая операция после сдерживания; плюс
  proptest «каждый член урегулирован ровно один раз» на случайных деревьях глубины ≤ 4.

`AxisLock` (G6) — поле конфигурации drag/pan/scale/swipe. `DominantAfterSlop` решает ось при первом
пересечении slop; именно так арена выбирает между ортогональными drag в `Exclusive`. `Rails` следует
DirectManipulation `RAILS_X/Y` (BP §1): ось держится, пока угол к ней меньше `breakout_angle`.

Новые распознаватели (G5): `RotateGestureRecognizer` (угол двух контактов, без масштаба; реализован на
том же разборе контактов, что `scale.rs`, без копии), `SwipeGestureRecognizer` (направление и скорость
при отпускании, порог скорости из пресета, `SwipeStart::FromEdge` — старт в полосе у края с шириной из
пресета). Оба используют только публичный трейт (M3).

## 4. Канал вложенной прокрутки (G1–G3)

Живёт в `flui-widgets/src/scroll/nested.rs`: владельцы прокрутки (`Scrollable`, `PageView`,
`InteractiveViewer`, `RefreshIndicator`) — там, и цепочка обобщает уже существующую inherited endpoint
из ADR-0169, а не добавляет второй механизм. Публичен через `flui-sdk` для сторонних участников.

```rust
#[non_exhaustive]
pub enum ScrollSource { Drag, Wheel, Fling, Keyboard, Programmatic }

pub trait NestedScrollParticipant {
    /// Takes part of `available` before the child scrolls; returns what it consumed.
    fn before_child_scroll(&self, available: Vec2, source: ScrollSource) -> Vec2 { Vec2::ZERO }
    /// Takes part of what the child left; returns what it consumed.
    fn after_child_scroll(&self, consumed: Vec2, available: Vec2, source: ScrollSource) -> Vec2 { Vec2::ZERO }
    fn before_child_fling(&self, available: Velocity) -> Velocity { Velocity::ZERO }
    /// Admission only: an accepting participant starts its own ballistic run (ADR-0169).
    fn after_child_fling(&self, consumed: Velocity, available: Velocity) -> Velocity { Velocity::ZERO }
}
```

- Порядок: `before_*` — от внешнего к внутреннему, `after_*` — от внутреннего к внешнему (Compose
  `NestedScrollNode.kt` L81–85, FU §5.1).
- `NestedScrollChain::dispatch` снимает `Weak`-звенья в локальный `SmallVec` до вызовов; звено, закрытое
  во время прохода, пропускается; идентичность — `Rc` + поколение.
- Ни одного заимствования через вызов участника. Каждый результат проверяется: конечен, тот же знак по
  оси, не больше доступного; иначе обрезается с диагностикой.
- Паника участника: звено закрывается, лист не применяется дважды; таблица из трёх строк (одна, две
  паники, следующая диспетчеризация).
- Готовые участники вместо декларативного режима (FU §5.3): `ScrollOrder::{SelfFirst, ParentFirst,
  EdgeOnly}` реализуют трейт.
- Три композиции из FU §5.5 — три строки одной табличной проверки в `flui-widgets/tests` через
  записанный ввод (§9): список в pager, фото в pager, pull-to-refresh над списком. `Wheel` и `Fling`
  не вытягивают индикатор — отдельная строка.

## 5. Платформенный слой: общее (P1, P2, P2a)

- `PlatformWindow::input_capabilities() -> InputCapabilities`: поля по возможностям (pen, touch,
  trackpad gestures, precise scroll, drag-and-drop, haptics, IME), каждое — `Support`. Заменяет TODO в
  `platforms/windows/platform.rs:2367`. Виджеты и тесты читают его; headless задаёт любой набор.
- `flui-platform/src/shared/pointer_assembly.rs` забирает дублированное (сверка кода):

| Часть | Сейчас в бэкендах |
|---|---|
| `ContactRegistry` | windows `events.rs:469-531`, winit `events.rs:341-427`, ios `events.rs:38-147`, macos `events.rs:33-47` |
| `DeviceRegistry` | windows `events.rs:634-672`, winit `events.rs:780-880`, android `input.rs:144-260` |
| `HeldButtons` | windows `events.rs:1165-1192`, winit `events.rs:218-297`, macos `events.rs:118-151`, web `events.rs:471-621`, android `input.rs:479-497` |
| `ClickCounter` | только macOS отдаёт `clickCount`; Win32 жёстко 1 (`events.rs:1320`) |
| `PanZoomAccumulator` | winit `events.rs:18-216`, macos `events.rs:274-355` |
| `MoveBatch` | windows `events.rs:320-443`, android `input.rs:367-381`, web `events.rs:583-627` |

  Модификаторы и `native_time` (одинаковые в ios `events.rs:276` и macos `events.rs:393`) —
  `shared/input_clock.rs`. Нативный разбор остаётся в бэкенде.

## 6. Платформенный слой: по ОС (P3–P8)

| ОС | Что добавить | Заметки |
|---|---|---|
| Windows | DirectManipulation → `PanZoom` (тачпад и touch pan/zoom, инерция, rails); OLE `IDropTarget`/`DoDragDrop` + `IDragSourceHelper`; haptics: `InputHapticsManager` (тачпад, Windows 11 SDK 2026-03) и `SimpleHapticsController` (перо) | обе модальные петли реентерабельны; `windows` 0.62.2 не содержит `InputHapticsManager` — поднять крейт или узкий биндинг (FU §1) |
| macOS | `tabletPoint`/`tabletProximity` → перо; `smartMagnify`, `swipe`; `NSDraggingDestination/Source`, `NSFilePromiseProvider`; `NSHapticFeedbackManager` | magnify/rotate не инъектируются публично — сертификация записью и железом |
| iOS | `UIPinch`/`UIRotation` → `PanZoom`; трекпад/мышь scroll; coalesced/predicted touches; `UITextInput`; `UIDrag/DropInteraction`; `UIFeedbackGenerator` + Core Haptics; `UIPencilInteraction` (squeeze, double-tap), hover | — |
| Android | `InputConnection` IME; `OnReceiveContentListener` + DnD; `VibrationEffect` (envelope API 36+, primitives 30–35); pinch тачпада; рукописный ввод стилусом — `Support::Unsupported` до отдельного решения | — |
| Linux | winit 0.31 после stable: перо/планшет, двусторонний DnD; честный `composing`; жесты тачпада X11 — `Unsupported` | Wayland pointer-gestures — вместе с winit, не своим бэкендом |
| Web | IME: `EditContext` в Chromium, скрытый зеркальный `<textarea>` в Firefox/Safari, один адаптер к `TextStore`; ~15 своих `wasm-bindgen` объявлений; ctrl+wheel и Safari `gesture*` → `PanZoom`; HTML DnD; Vibration API (Chromium, только с user activation) | `web-sys` 0.3.106 не биндит EditContext (FU §2) |

## 7. Haptics (H1–H4)

Модель — FU §3.2 с именами §0. Уровень 1: `HapticIntent` (`Selection`, `Impact(ImpactWeight)`,
`Notification(Outcome)`, `Alignment`, `Step`, `Boundary`, `DragLift`, `DragDrop`, `Confirm`), fire-and-forget.
Уровень 2: `HapticPattern` из `HapticEvent::{Transient, Continuous}` с `UnitInterval` интенсивностью и
резкостью (общая модель Core Haptics и Android 16 envelope); `play(&pattern, fallback: HapticIntent) ->
PlayOutcome` (`#[must_use]`). Ошибка формы — `Err` при сборке паттерна, не при проигрывании.
Деградация — таблица FU §3.3, по одной табличной проверке на бэкенд. Системное «haptics выключены» и
скрытое окно гасят оба уровня.

Виджетный путь: lifecycle-возможность `haptics()` (ADR-0078); пресет платформы решает, какие события
виджетов звучат (long-press, порог dismiss/swipe-action, шаг reorder, захват drag); `HapticsScope`
выключает на поддереве. Первый продакшн-вызывающий у `UiRuntime::perform_haptic_feedback`.

ADR, заменяющий ADR-0031: проверка поддержки есть, десктоп — не «навсегда None».

## 8. Drag-and-drop (D1–D5)

- `DragItem` несёт список `DragRepresentation`: `InProcess(DragValue)` — типизированное Rust-значение,
  отдаётся при drop внутри процесса без сериализации; `Text`, `Uri`, `Files`, `PromisedFile`; каждое
  внешнее представление — готовое значение или ленивый поставщик.
- `DragOperation::{Copy, Move, Link}` — набор разрешённых и выбранная.
- `DragSession` — состояния `Pending → Active → Dropped | Cancelled` как типы (N4); изображение
  перетаскивания, глобальная позиция, стратегия якоря.
- Ограничения: `InputCapabilities::drag_and_drop` сообщает, когда нужны данные (`AtStart` для Web) и
  когда известна операция (`OnDrop` для X11); ленивый поставщик на таком бэкенде вызывается на старте.
- `Draggable`/`DragTarget` переписываются поверх сессии; `DragStart::{Immediate, LongPress}`.
- Runtime доставляет `PlatformInput::DragDrop` в hit-test цели (сейчас отбрасывается).

ADR, дополняющий ADR-0038.

## 9. Запись ввода (S10, T1–T3)

Формат — FU §6.2 с именами §0: JSON Lines, заголовок с версией `flui_input_recording`, окном, устройствами и
пресетом; поля `t` (время события, мкс от начала) и `arrived` (время прихода); текст клавиатуры и IME
скрыт по умолчанию; неизвестный вариант — `RecordingError::UnknownEvent { line }`, не пропуск.
Бинарная кодировка — только после замера размера на реальной записи.

`ScriptedInput` строит то же значение из скрипта (press, move с интерполяцией, release, pause), так что
синтетика и записи идут одним путём проигрывания. Headless проигрывает на виртуальных часах.
`RecordingWriter` подключается к бэкенду флагом окружения разработчика и в релизной сборке отсутствует
(фича).

Golden-записи: `crates/flui-platform/tests/recordings/<os>/<device>-<gesture>.jsonl`; первыми — Windows
touch, pen и точный тачпад.

## 10. Проверка соответствия бэкендов (S11, T4)

`flui_platform_api::backend_conformance::assert_backend_conforms(&mut impl BackendUnderTest)` — таблица
строк (переиспользование ID, cancel при потере захвата, coalesced раньше predicted, `PanZoom` всегда
завершается, DnD leave после enter, haptics принят или `Unsupported`). `BackendUnderTest` подаёт
нативные показания через `pointer_assembly` и отдаёт `PlatformInput`; бэкенд реализует его в своих
тестах. Строки, которые бэкенд объявил `Unsupported`, проверяют именно отказ.

## 11. Нативная инъекция (S12)

`cargo xtask input-smoke --platform <os> --recording <file>`: Windows — `InjectSyntheticPointerInput`
(touch, pen); macOS — `CGEvent` (мышь, scroll; жесты — нет, FU §1); Linux — uinput/libei; Web — WebDriver
BiDi `input.performActions`; Android/iOS — instrumentation/XCUITest. Сравнивает полученный `PlatformInput`
с записью. Не-Windows — скелет, вывод «не запускалось на этом хосте».

## 12. Пресеты платформ (S13, T5)

`GestureSettings::{windows, macos, ios, android, linux, web}()` с константами, каждая с комментарием-
ссылкой на источник ОС (например, Android touch slop 8 dp, BP §4); `native()` выбирает по цели сборки.
Таблица тестов сравнивает пресет с источником; значения Flutter не используются как источник.

## 13. Виджеты (W1–W3)

- `GestureDetector`: недостающие колбэки W1; `ForcePress`, `MultiTap` подключаются здесь (S17).
- `SwipeActions` — строка с действиями слева/справа, порог полного свайпа, haptics на пороге.
- `ReorderableList`, `ReorderableGrid` — перенос с автопрокруткой через `NestedScrollChain`, клавиатура.
- `ZoomableView` — pinch/pan/rotate (опционально), snap-back, double-tap zoom, drag-to-dismiss,
  участник цепочки (§4).
- `ContextMenuRegion` — правый клик, long-press, клавиша Menu/Shift+F10; якорное popup.
- `SelectionArea` — выделение неизменяемого текста; ручки и тулбар; `TapAndDrag` — двойной/тройной клик,
  shift-click (S17).
- `HitArea` — расширение области попадания без изменения layout; hit-test по фигуре — колбэк
  `hit_test` у custom paint.
- `Draggable` — `DragStart`, глобальное позиционирование, якорь.
- `OneEuro` — `GestureSettings::with_smoothing(Smoothing::OneEuro { .. })`, по умолчанию выключено (S17).
- Material/Cupertino — тонкие обёртки стиля.

## 14. Доступность (A1, A2)

FU §4.2: FLUI публикует `custom_actions` в `accesskit::Node` (стабильные id на узел, подпись — метка) и
`ShowContextMenu` для long-press. Таблица соответствий FU §4.2 — контракт: тест на каждый виджет
проверяет созданный `accesskit::Node`. Адаптеры AccessKit не показывают custom actions ни на одной
платформе — вклад upstream; до него пробел записан в матрице semantics как известный, обхода через
платформенные типы нет (ADR-0082).

## 15. Наблюдаемость (O1)

`GestureInspector` в `packages/flui-devtools`: подписка на `observability` (`GestureEvent`, spans), рисует
контакты, траектории, вектор скорости, победителя арены и причину (терминальные причины ADR-0112). Только
через `flui-sdk`.

## 16. Фокус и клавиатура (F1)

Реализуются задачи T3–T16 [../../focus-keyboard/tasks.md](../../focus-keyboard/tasks.md) без изменения их
решений; эта спека лишь включает их в охват и располагает модуль `focus` по §1.

## 17. ADR

| ADR | Решение |
|---|---|
| новый, Supersedes ADR-0167 | `GestureRelation`, вложенная композиция, `Simultaneous`; `AxisLock` |
| новый | `NestedScrollChain` и обобщение ADR-0169 |
| новый, Supersedes ADR-0031 | двухуровневые haptics с проверкой поддержки |
| новый, дополняет ADR-0038 | единый DnD |
| новый | `InputRecording`, `ScriptedInput`, `backend_conformance`, `input-smoke` |
| новый | слои модулей `flui-interaction` и `pointer_assembly` |

## 18. Открытые вопросы реализации

Не меняют scope; решаются в своей задаче с тестом:

- macOS: какая настройка приватности разрешает постинг `CGEvent` (FU, unverified).
- Работает ли Core Haptics на встроенном трекпаде Mac (FU, unverified) — до проверки `Fidelity::None`.
- UIA не знает custom actions — дублируем действия в контекстном меню (FU §4.2).
- Отличить настоящий ctrl+wheel от pinch в браузере (FU, unverified) — до проверки оба идут в `PanZoom`
  с флагом источника.
