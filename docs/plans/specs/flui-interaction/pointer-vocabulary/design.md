# pointer-vocabulary — дизайн

- **Статус:** черновик; P1 — в ветке `interaction/pointer-vocabulary-types`
- **Дата:** 2026-10-06, база `main` @ `d56188c14`
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **ADR:** [ADR-0143](../../../../adr/ADR-0143-flui-owned-input-event-vocabulary.md) (Proposed)

## Текущий код

- **Словарь — чужой.** `flui-platform-api` реэкспортирует `ui_events::{ScrollDelta,
  keyboard::{KeyboardEvent, Key, Modifiers}, pointer::{PointerButton, PointerButtons,
  PointerEvent, PointerId, PointerType, PointerUpdate}}` (`crates/flui-platform-api/src/input.rs:28-38`),
  и `PlatformInput::{Pointer, Keyboard}` несут их (`input.rs:107-124`). Это Stable-крейт:
  ADR-0089 §4 требует своих типов.
- **Единицы врут.** `PointerState::position` — `dpi::PhysicalPosition`, но все бэкенды кладут
  туда логические пиксели (`input.rs:94-106`); `ScrollDelta::PixelDelta` — тоже
  `PhysicalPosition` с логическими значениями (`crates/flui-platform/src/shared/scroll.rs:46`).
- **Датчики подменяются.** Мышь Win32: давление 0.5 при нажатой кнопке, 0.0 без
  (`crates/flui-platform/src/platforms/windows/events.rs:182`, `:209-215`); касание winit
  без силы — тот же 0.5 (`platforms/winit/events.rs:201`). Ориентация по умолчанию
  (π/2, π/2) и контакт 1×1 неотличимы от реальных показаний (`ui-events` `pointer/mod.rs`).
- **Нет полей.** Twist, ластик как инструмент, `persistent_device_id` с доступом (у
  `ui_events::PersistentDeviceId` нет геттера), время у клавиатуры и у `Cancel`/`Enter`/`Leave`,
  фазы и инерция прокрутки, Start/End жеста трекпада, причина отмены, подключение и отключение
  устройств.
- **Время.** Общая эпоха `PROCESS_START` (`crates/flui-platform/src/shared/events.rs:15-28`),
  Win32/macOS/winit ставят время трансляции, а не время ОС.
- **Прокрутка теряет единицу.** Нормализация знака и единиц — одна функция на платформу
  (`shared/scroll.rs:34-112`), но `ScrollEventData::delta_to_offset` переводит строку в 53 px и
  страницу в 400 px до любого виджета (`crates/flui-interaction/src/events.rs:530-548`).
- **Жест трекпада — тики.** `shared/gestures.rs:24-37` нормализует `Pinch`/`Rotate`;
  `flui_interaction::pan_zoom::convert_gesture` (`crates/flui-interaction/src/pan_zoom.rs:261`)
  делает из каждого тика `Update` с `scale = 1 + d`, называя его накопленным; Start/End не
  производятся никем.
- **Второй словарь в interaction.** `PointerDeviceKind` (`crates/flui-interaction/src/device_kind.rs:7-26`)
  и `PointerPanZoomEvent` (`pan_zoom.rs:86`) дублируют части словаря; facade реэкспортирует
  upstream-имена (`src/interaction.rs:15`, `:44`).
- **Потребители.** `PlatformInput` разбирают `flui-runtime` (`crates/flui-runtime/src/ui_realm/input.rs:268`,
  `:286`, `presentation_lifecycle.rs:527`, `held_input.rs`) и `flui-interaction` (binding,
  routing, recognizers, processing — около 20 файлов), а также `flui-widgets` (listener,
  gesture_detector), `flui-testing` (harness, replay), `flui-app` (тесты dispatch) и
  `examples/web_demo`. Производители — семь бэкендов `flui-platform` (`platforms/{windows,
  winit, macos, ios, android, web}`) и `shared/{events,scroll,gestures,keys}.rs`.

## Варианты

1. **Обёртки над `ui-events`.** Newtype-структуры с геттерами поверх upstream-типов.
   Прячут имена, но не семантику: «нет датчика» не выразить, единицы и фазы прокрутки не
   добавить без своих полей; каждая версия `ui-events` — правка обёрток. Отклонён.
2. **Свой словарь целиком, мост из `ui-events` у бэкендов (выбран).** Типы в
   `flui-platform-api` (`pointer`, `keyboard`, `EventTime`), генерация `NamedKey`/`Code` из
   `keyboard-types` командой xtask с проверкой в `checks`, мост `ui-events` → словарь в
   `flui_platform::shared::input_vocabulary`. Переход — тремя шагами без «большого взрыва».
3. **Свой словарь и сразу прямые производители во всех бэкендах.** Одна большая правка
   семи бэкендов, `flui-interaction`, runtime и facade; не проверяема на одном хосте
   (macOS/iOS/Android — только `cross-typecheck`) и конфликтует с параллельными ветками I1–I9.
   Отклонён как порядок работ, не как цель: это P2 + P3.

## Выбранный дизайн

### Типы (P1) и Rust-паттерн каждого

Toolchain 1.99.0, edition 2024. Используемые возможности сверены с `RELEASES.md` rust-lang/rust:
`NonZero<T>` — 1.79; `Option::expect` в `const fn` — 1.83; let-chains — 1.88 (edition 2024).

| Тип | Паттерн | Почему |
|---|---|---|
| `PointerId`, `DeviceId` | newtype над `NonZeroU64`; `new(NonZeroU64)`, `TryFrom<u64>` (`TryFromIntError`), `From` в обе стороны с `NonZeroU64` | ноль — не идентификатор, и это видно в типе; `Option<PointerId>` без накладных расходов (niche) |
| `EventTime` | newtype над `u64` наносекунд; `saturating_duration_since -> Duration` | монотонная метка, а не `f64` секунд; разность — `Duration`, без отрицательного времени |
| `PointerPosition` | проверенный newtype над `Point<f64>`, `try_new`/`TryFrom` → `InputValueError` | конечность доказывается один раз; конструкторы событий, принимающие `PointerPosition`, — инфаллибельные `const fn` |
| `Pressure`, `TangentialPressure` | проверенные newtype над `f32`: `try_new` (строго: NaN/inf и вне диапазона — ошибка) и `saturating` (прижимает конечное, NaN/inf — ошибка) | диапазон — свойство типа; платформы округляют за 1.0, поэтому есть явная насыщающая форма, а не молчаливый clamp |
| `PenOrientation`, `Twist` | проверенные значения; altitude — строгий диапазон, azimuth и twist — периодические, сворачиваются в `[0, 2π)` | у периодического угла нет «вне диапазона» |
| `ContactSize` | проверенный newtype над `Size<f64>` | отрицательная или бесконечная сторона не выражается |
| `ScrollDelta` | значение с приватными полями `ScrollUnit` + `x`, `y`; `try_new`, `zero(unit)` | единица и конечность в одном типе; `zero` — явная политика «фаза доставляется, дельта нет» |
| `PanZoomTransform` | проверенное значение: `try_new`, `IDENTITY`, `Default` | масштаб > 0 и конечность — инвариант типа |
| `InputValueError`, `Quantity` | `thiserror`-enum, `#[non_exhaustive]`; `Quantity` — что именно отвергнуто | вызывающий код сопоставляет по варианту, а не по строке |
| `PointerButton` | newtype над номером бита; константы `PRIMARY`…`FORWARD`, `TryFrom<u8>` (`InvalidButtonNumber`), `number() -> NonZeroU8` | 0 и 6 (ластик W3C) не выражаются; enum на 32 варианта не нужен |
| `PointerButtons`, `Modifiers` | битовый набор-newtype с `const` операциями, `FromIterator`/`BitOr`, свой `Debug` | без upstream `bitflags` в Stable-сигнатурах (его трейт `Flags` стал бы частью API) |
| `PointerKind`, `PenTool`, `CancelReason`, `ScrollPhase`, `ScrollPrecision`, `ScrollUnit`, `PanZoomPhase`, `Key` | `#[non_exhaustive]` enum | множества растут (устройства, причины); `PointerKind::Pen { tool }` несёт инструмент данными варианта |
| `PointerButtonEvent<D>` (`PointerPress`, `PointerRelease`), `ButtonChange` | typestate: sealed `ButtonDirection` с маркерами `Press`/`Release`; `PointerEvent::Down(PointerPress)`, `Up(PointerRelease)`, `ButtonChange::{Pressed, Released}` | отпускание не может начать последовательность, нажатие — закончить: ошибка компиляции (`compile_fail`-доктест), а не проверка |
| `PointerRole`, `KeyRepeat`, `ImeComposition`, `KeyState`, `Location` | enum вместо `bool` | `with_role(PointerRole::Primary)` читается без документации; закрытые множества W3C — без `non_exhaustive` |
| `PointerSample`, `PointerInfo`, `PointerButtonEvent`, `PointerMove`, `PointerSignal`, `PointerCancel`, `PointerDeviceChange`, `ScrollEvent`, `PanZoomEvent`, `KeyEvent` | `#[non_exhaustive]` struct с публичными полями проверенных типов, конструктор `new` и `#[must_use]` by-value builder-методы `with_*` (в основном `const fn`) | поля читаются и сопоставляются; вне крейта не создаются в обход конструктора, а инварианты держат типы полей, поэтому запись в поле их не ломает |
| `PointerEvent` | `#[non_exhaustive]` enum, вариант на событие | диспетчеризация — `match` |
| `NamedKey`, `Code` | сгенерированные enum + `ALL`, `as_str`, `from_w3c`, `Display` | ADR-0143 §4 |

Общие трейты (C-COMMON-TRAITS): все `Debug` и `Clone`; `Copy` у всего без `String`/`Vec`;
`PartialEq` везде; `Eq`/`Hash` только у типов без float; `Default` там, где есть естественное
значение (`Modifiers`, `PointerButtons`, `Location`, `PenTool`, `PointerRole`, `KeyRepeat`,
`ImeComposition`, `ScrollPrecision`, `PanZoomTransform`); `PartialOrd` у скаляров-датчиков.
Упорядочивание — по `EventTime` (целое), `total_cmp` не требуется. Ни одного lossy `as`:
`f32 → f64` через `f64::from`, индексы — `u8::try_from`.

Политика NaN/inf: NaN и бесконечность — всегда `InputValueError::NonFinite`; конечное вне
ограниченного диапазона — `OutOfRange` в `try_new`; насыщение — только явным `saturating`;
периодические углы сворачиваются. `debug_assert!` нигде не служит проверкой.

### Где форма заимствована и что с этим сделано

| Форма | Откуда | Решение |
|---|---|---|
| Имена `Down`/`Move`/`Up`/`Cancel`/`Enter`/`Leave` | W3C Pointer Events | оставлено как словарь предметной области; форма — enum с данными, не иерархия классов `PointerDownEvent extends PointerEvent` (Flutter) |
| `buttons` как битовый набор, нумерация кнопок | W3C `buttons` | оставлено ради взаимно-однозначного перевода на всех платформах; ластик вынесен в `PenTool` (у W3C — бит 32) |
| `isPrimary: bool` | W3C | заменено на `PointerRole` |
| `repeat`/`isComposing: bool` | UI Events `KeyboardEvent` | заменены на `KeyRepeat`, `ImeComposition` |
| давление 0.5 по умолчанию | W3C | отвергнуто: `Option<Pressure>` |
| `altitude`/`azimuth` вместо `tiltX/Y` | W3C PE3 | оставлено (PE3 сам выбрал эту пару), типизировано `PenOrientation` |
| `PointerPanZoomStart/Update/End` | Flutter | взята только семантика накопленной трансформации; вместо трёх классов — `PanZoomPhase` в одном событии, `panDelta` не дублируется |
| `PointerScrollInertiaCancelEvent` | Flutter | вариант `ScrollInertiaCancel(PointerSignal)` |
| `PointerKind`; `PinchGesture`/`PanGesture`/`RotationGesture` | winit | `PointerKind` с данными варианта — образец; три потока жестов отвергнуты в пользу одного |
| setter-стиль (`event.pressure = …`) | Flutter/Dart | не используется: `try_new` и by-value `with_*` |
| `KeyboardEvent::key_down(...)` | keyboard-types | не перенесено: `KeyEvent::new(state, …)` |

### Инварианты

1. Ни одно значение словаря не содержит NaN/inf: числа входят только через проверенные типы
   (`PointerPosition`, `Pressure`, …, `try_new` → `InputValueError`); датчик — в своём
   диапазоне или `None`.
2. «Нет датчика» — `None`; стандартные значения W3C (0.5, π/2, 1×1) не подставляются.
3. Последовательность контакта завершается `Up` или `Cancel` всегда; непригодный `Up` —
   `Cancel { InvalidInput }`; отключение устройства отменяет его контакты до `DeviceRemoved`.
4. `PointerButtonEvent::buttons` — набор после изменения; ластик — инструмент, не кнопка.
5. `PanZoomTransform` накопленный с `Start`; масштаб положителен.
6. Единица прокрутки доходит до прокручиваемого виджета; знак — W3C.
7. `NamedKey`/`Code` равны таблицам закреплённого `keyboard-types` (`checks`).
8. Ни одна публичная сигнатура `flui-platform-api`, кроме уходящих в P2 реэкспортов, не
   называет `ui-events`/`keyboard-types`/`dpi`.

### Владение и границы

- Словарь — `flui-platform-api` (уровень C, Stable). Модули `pointer` и `keyboard` публичны,
  потому что корневые имена пока заняты реэкспортами `ui-events`; P2 переносит словарь в корень.
- Мост — `flui_platform::shared::input_vocabulary` (`flui-platform`, internal): он называет
  upstream-типы, поэтому в Stable-крейте ему не место; `From` между upstream- и Stable-типом
  невозможен (orphan rule) и не нужен. Модуль `pub`, как соседние `shared::*`: бэкенды
  вызывают его каждый на своей цели, тесты идут на любом хосте.
- Значения `Copy`/`Clone` и не держат ссылок; владение — у события. Потоки: словарь — данные,
  `Send + Sync` автоматически, на пути кадра ничего не блокирует.

### Что остаётся внутри бэкендов

Перевод нативных кодов (VK/`NSEvent`/`MotionEvent`/DOM), перевод device → logical,
накопление шагов жеста трекпада в `PanZoomTransform`, ребейз времени ОС на `EventTime`,
выдача идентификаторов указателей и устройств, `SetCapture`/`ReleaseCapture` и производство
`CaptureLost`, синтез `Cancel` перед `DeviceRemoved`, `Leave` после отмены активного контакта.

### Отказы (adversarial review)

- **Отмена посреди последовательности:** любая `CancelReason` завершает последовательность
  одинаково; причина — для диагностики и политики.
- **Потеря захвата:** `CaptureLost` (P3 Win32 — `WM_CAPTURECHANGED`).
- **Отключение устройства:** двойная гарантия — `Cancel { DeviceRemoved }` каждого контакта,
  затем `DeviceRemoved`, по которому потребитель закрывает остатки.
- **Неподдерживаемое устройство или поле:** `PointerKind::Unknown`, `NamedKey::Unidentified`,
  `Code::Unidentified`, `None` у датчика.
- **NaN/inf:** проверенные типы (инвариант 1); мост отбрасывает `Down`/`Move`/прокрутку/жест
  с непригодной позицией, превращает `Up` в `Cancel` и доставляет прокрутку с непригодной
  дельтой как `ScrollDelta::zero` той же единицы.
- **Гонка двух указателей:** идентичность — `PointerId` + флаг первичности, не значение id;
  повторное использование id после конца последовательности документировано.
- **Реентерабельность и паника:** словарь — данные без колбэков; содержит паники только
  `expect("BUG: …")` в мосте на заведомо истинных инвариантах (B7…B32 → кнопки 7…32).

## План миграции

- **P1 (этот PR).** Типы, ADR-0143, генератор `cargo xtask key-vocabulary` и его проверка в
  `checks`, мост с таблицей тестов. Поведение не меняется; новые типы не подключены — их
  подключает P2.
- **P2.** `PlatformInput::{Pointer, Keyboard}` несут словарь; корневые реэкспорты
  `flui-platform-api` и facade указывают на него; бэкенды вызывают мост на своей границе;
  `flui-interaction` переходит на словарь (binding, routing, recognizers, processing,
  testing-builders), удаляя `PointerDeviceKind`, свой `PointerPanZoomEvent` и разрешение
  прокрутки фиксированными 53/400 px (оно уходит в `Scrollable`). Файлы согласуются с I1–I5
  (`tasks.md` волны 1): P2 стартует после их слияния или ребейзится на них.
- **P3.** Каждый бэкенд производит словарь сам и заполняет то, что мост не может: время ОС,
  фазы и инерцию, `ScrollPrecision`, реальное «нет датчика», ластик, twist, размер контакта,
  `DeviceAdded`/`DeviceRemoved`, `CaptureLost`, `ButtonChange`. После последнего бэкенда мост
  и зависимости `ui-events`/`keyboard-types`/`dpi` уходят из `flui-platform-api`.
