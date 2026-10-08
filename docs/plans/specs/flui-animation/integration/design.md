# integration — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md) (R1–R22)
- **ADR:** не нужен — межкрейтовый контракт не меняется (render-объект в каталоге flui-objects,
  виджеты сохраняют публичные сигнатуры). Решение о слое для чистого переноса — запись
  `## Mapping decisions` в `crates/flui-objects/ARCHITECTURE.md` и правка
  `crates/flui-rendering/ARCHITECTURE.md:381-386`.

## Текущее состояние (чтение HEAD)

| Где | Что происходит на тике |
|-----|------------------------|
| `crates/flui-widgets/src/transitions/slide_transition.rs:111-117` | `AnimatedView` перестраивает элемент, создаёт новый `FractionalTranslation`; чистый перенос рисуется смещением — перерисовка поддерева |
| `transitions/scale_transition.rs:50-55`, `rotation_transition.rs:50-54` | rebuild → новый `Transform` → `RenderTransform::set_transform` (обновление слоя, `crates/flui-objects/src/layout/transform.rs:193-216`) |
| `transitions/fade_transition.rs` | образец без rebuild: `ProxyAnimation` в state, `RenderAnimatedOpacity` слушает её (`crates/flui-objects/src/proxy/animated_opacity.rs:189-262`: кэш в атомике, пометка через `RenderInvalidationHandle`, сбой отправки — кэш не двигается) |
| `packages/flui-material/src/drawer.rs:734-743`, `:860-883` | value-слушатель → rebuild всего `DrawerController`; панель — `Align::width_factor(value)` (layout), scrim — цвет с alpha `v·a` |
| `crates/flui-widgets/src/interaction/dismissible.rs:667-671`, `:1397-1415` | value-слушатель → rebuild `DismissibleState::build` (жесты, замыкания); содержимое — `FractionalTranslation` |
| `crates/flui-widgets/src/scroll/refresh_indicator.rs:449-450` | внешний `AnimatedBuilder` над позицией прокрутки: на каждый пиксель — `SingleChildScrollView::offset(pixels)`, `Stack`, `GestureDetector` и 11 замыканий; внутренний (`:460`) — на каждое изменение дистанции оттягивания |
| `refresh_indicator.rs:558-566`, `crates/flui-widgets/src/scroll/scrollable.rs:658-675` | литерал `clamp(±8000)` + NaN → 0 мимо `GestureSettings::max_fling_velocity` (`crates/flui-interaction/src/settings.rs:166`) — задача I3 |

Уже есть: `SingleChildScrollView::position(ScrollPosition)` (`single_child_scroll_view.rs:93`) —
viewport слушает позицию сам, как у `Scrollable`; обновление слоя трансформации без перерисовки
(`flui-rendering/ARCHITECTURE.md:361-390`, тест
`the_transform_update_path_and_a_repaint_produce_the_same_pixels`); публичный счётчик
`FrameReport` (`crates/flui-testing/src/lib.rs:1160-1187`).

## Варианты

| | Суть | За | Против |
|---|------|----|--------|
| A | Свой render-объект на переход (`RenderSlideTransition`, `…Scale…`, `…Rotation…`) | типы точные | три копии логики слушателя/пометок/кэша, три семейства harness |
| B | Один `RenderAnimatedTransform` с закрытым `enum TransformMotion` | одна логика; набор движений закрыт и типизирован; paint не вызывает чужой код | `match` на варианты; новое движение — новый вариант |
| C | Один render-объект с `Box<dyn Fn(f64, Size) -> Matrix4>` | открыт для любого движения | пользовательский код в paint/hit-test (R15), куча, нет `Debug`; разные типы значений (`TranslationFraction` vs f64) не выражаются одним `Fn` |
| D | `RenderTransform` получает необязательный источник-анимацию | без нового типа | статичный объект обрастает жизненным циклом слушателя; две ответственности |
| E | Оставить rebuild | ничего не делать | 60–120 перестроек в секунду на каждый переход и drawer |

**Выбран B.** Закрытый набор движений — ровно три существующих перехода; enum
`#[non_exhaustive]`. Paint/hit-test читают кэш, а не анимацию.

## Устройство `RenderAnimatedTransform`

```rust
// crates/flui-objects/src/proxy/animated_transform.rs
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TransformMotion {
    /// Перенос на долю собственного размера; dx зеркалится при `TextDirection::Rtl`.
    Slide { offset: ProxyAnimation<TranslationFraction>, text_direction: TextDirection },
    /// Равномерный масштаб вокруг центра.
    Scale { scale: ProxyAnimation<f64> },
    /// Поворот на `turns` полных оборотов вокруг центра.
    Rotation { turns: ProxyAnimation<f64> },
}

pub struct RenderAnimatedTransform {
    motion: TransformMotion,
    sample: Rc<Cell<MotionSample>>, // приватный: `MotionSample { value_bits, size }` — `Copy`
    transform_hit_tests: bool,
    has_child: bool,
    listener_id: Option<ListenerId>,
}
impl RenderAnimatedTransform {
    pub fn new(motion: TransformMotion) -> Self;
    pub fn set_transform_hit_tests(&mut self, value: bool) -> RenderUpdateImpact; // NONE
    pub fn set_text_direction(&mut self, dir: TextDirection) -> RenderUpdateImpact; // PAINT | SEMANTICS для Slide, иначе NONE
}
```

`transform_hit_tests: bool` повторяет соглашение `RenderTransform`/`RenderFractionalTranslation`
и `SlideTransition::transform_hit_tests`; отдельный enum ради одного флага, совпадающего с
соседями, не вводится.

Инварианты:
1. **Один кэш.** `paint_effects`, `paint`, `hit_test`, `hit_test_transform`,
   `apply_paint_transform` и semantics вычисляют матрицу из `MotionSample` и размера;
   `Animation::value()` вызывается только в `attach` и в слушателе (R15).
2. **Пометка до коммита.** Слушатель: читает значение; неконечное — игнорирует (кэш остаётся,
   R12); совпадает с кэшем побитово — ничего (R14); иначе классифицирует матрицу по сэмплу и
   последнему размеру (`Identity` / `Layered` / `Degenerate`: масштаб 0 или неконечная
   матрица). Смена класса → `mark_needs_paint`; иначе `mark_needs_composited_layer_update`;
   затем `mark_needs_semantics`. Кэш записывается только после успешных отправок (R11) —
   правило `RenderAnimatedOpacity`.
3. **Слой.** Любая `Layered`-матрица, **включая чистый перенос**, сообщается через
   `paint_effects().transform` — в отличие от `RenderTransform`, который рисует перенос смещением
   без слоя. Движущемуся узлу дешевле патч слоя, чем перерисовка поддерева на тик; в покое
   (`Identity`) слоя нет. `Degenerate` — `skip_paint` и отказ hit-test (R5).
4. **Hit-test** — как `RenderTransform::hit_test` (`transform.rs:376-400`): обратная матрица из
   кэша; `transform_hit_tests = false` — по позиции раскладки (R4).
5. **Жизненный цикл.** Слушатель регистрируется в `attach` (с немедленным пересчётом, как у
   opacity), снимается в `detach` (R10). Анимацией владеет `ProxyAnimation` из state виджета;
   смена анимации — `set_parent(new)` в `did_update_view` (R8; конструкторы принимают
   `impl Animation<_> + 'static`, R5), render-объект не пересоздаётся. Без блокировок на пути
   paint: кэш — `Rc<Cell<MotionSample>>` (owner-local, как кэш opacity по frame-path-state F4).
   Временных атомиков нет (orchestration, «Граница с send-flip» п. 3): тема идёт после ядра
   send-flip, когда render object уже `!Send` (T4/T6d). Слушатель захватывает только кэш, handle
   инвалидации и **слабый** proxy (`ProxyAnimation::downgrade`): сильный захват дал бы цикл C5
   (proxy → его слушатель → proxy), как сейчас у `RenderAnimatedOpacity::attach`.

## Виджеты

- `SlideTransition`, `ScaleTransition`, `RotationTransition` — по образцу `FadeTransition`:
  state держит `ProxyAnimation` и текущий источник, `build` отдаёт приватный `RenderView` с
  `RenderAnimatedTransform`; `update_render_object` передаёт только `transform_hit_tests` и
  направление текста. Публичные сигнатуры прежние; `impl_animated_view!` снимается.
- **Drawer** (`open_panel`): `Align::width_factor(value)` заменяется на `SlideTransition` с
  `Tween(TranslationFraction(−dir, 0) → ZERO).animate(controller)` — край панели в `−(1−v)·w`
  совпадает с прежним раскрытием от края экрана; scrim — `FadeTransition(controller)` над
  `ColoredBox(scrim_color)` (alpha = `a·v`, расхождение округления ≤ 1/255). Value-слушатель
  `drawer.rs:735-737` удаляется; статус-слушатель (монтаж/снятие панели) остаётся.
- **Dismissible:** содержимое — `SlideTransition` над `Tween(ZERO → (sign, cross)).animate(move_controller)`.
  Value-слушатель планирует rebuild только при смене `(value ≠ 0, sign)` (`Rc<Cell<_>>`,
  замыкание owner-local). Сжатие (`:1281-1312`, `:1426-1431`) — layout, rebuild остаётся.
- **RefreshIndicator:** внешний `AnimatedBuilder` удаляется; `SingleChildScrollView::position(
  scroll_controller.position())`. Внутренний builder заменяется слушателем
  `RefreshController`, который в `init_state` планирует rebuild только при смене
  `is_refreshing` (сравнение с `Cell<bool>`); при замене контроллера в `did_update_view` —
  переподписка по идентичности `inner` (`Rc::ptr_eq`, `pub(crate)`), снятие в `dispose`.
  `as_listenable` по-прежнему уведомляет внешних слушателей о каждом пикселе оттягивания (R20).
- **Fling (потребитель I3):** после I3 оба места берут `DragEndDetails::velocity` без литерала
  и запускают баллистику общей `pub(crate) fn start_ballistic(...)` в `scroll/` (R22). До I3
  литералы не трогаются.

## Миграция (rg на HEAD)

| Что | Файлы |
|-----|-------|
| `SlideTransition` в production | `packages/flui-cupertino/src/route.rs:171,177` — сигнатура прежняя, правок нет |
| Тесты, ищущие `RenderFractionalTranslation` по имени | `crates/flui-widgets/tests/slide_transition.rs:33-77`, `packages/flui-cupertino/tests/route.rs` (`find_all_by_render_type`) → `RenderAnimatedTransform`, проверка через `transform_to` |
| Хелперы `flui-testing` без вызовов | `crates/flui-testing/src/widgets.rs:1033-1060` (`transform_scale`, `transform_rotation` — вызовов в workspace нет) — удалить |
| `Scale`/`RotationTransition` в production | вызовов нет, кроме `AnimatedRotation` (тема interpolation) |
| Документы | `crates/flui-rendering/ARCHITECTURE.md:381-386` (SlideTransition больше не идёт быстрым путём `RenderTransform`); `crates/flui-objects/ARCHITECTURE.md` (Mapping decision: слой для переноса); таблица `render_object_harness.rs:20-40` |
| `flui-sdk` | `tests/surface.rs` переходы не закрепляет; новых `pub` в SDK не требуется |

## Changelog (`changelog.d/<branch-slug>.md`)

```markdown
### Added

- **`flui-objects`**: `RenderAnimatedTransform` with `TransformMotion::{Slide, Scale, Rotation}`:
  a transform that follows an animation without rebuilding the element tree.

### Changed

- **`flui-widgets`**: `SlideTransition`, `ScaleTransition` and `RotationTransition` update a
  transform layer per frame instead of rebuilding; `Dismissible` and `RefreshIndicator` no longer
  rebuild their subtree on every animation frame or scroll pixel.
- **`flui-material`**: the drawer slides and fades its scrim without rebuilding per frame.

### Removed

- **`flui-testing`**: `LaidOut::transform_scale` and `transform_rotation`; read a child's
  matrix with `PipelineOwner::transform_to`.
```

## Adversarial review

- **Reentry слушателя в контроллер и vsync-реестр.** Слушатель render-объекта не вызывает
  контроллер, только читает значение и шлёт пометки в канал конвейера; реестр vsync не
  трогает. Reentry из чужого слушателя (тот же контроллер останавливается/перезапускается) —
  наш слушатель увидит новое значение в той же раздаче или следующей; кэш монотонно отражает
  последнее прочитанное (R14 `reverse_mid_run`).
- **Добавление/снятие слушателей во время уведомления.** `detach` во время раздачи (узел
  удалён слушателем другого виджета) — семантика снятия у listener-delivery (D-07: снятый
  слушатель не должен вызываться). До её слияния вызов после `detach` возможен: слушатель
  держит `Rc<Cell<MotionSample>>`, handle и `Weak` proxy, пометка удалённого узла возвращает `SendError` или
  отбрасывается конвейером — без panic (R10 закрепляет итог, не порядок).
- **Освобождение последнего владельца из колбэка.** Render-объект держит `ProxyAnimation`,
  та — родителя; внешний drop не освобождает анимацию до `detach` (R13). Цикл
  «контроллер → слушатель → render-объект» отсутствует: замыкание держит кэш, `Weak` proxy
  и handle, не узел и не контроллер.
- **Два контроллера на одном vsync / два перехода на одном контроллере.** Каждый узел —
  независимый слушатель со своим кэшем (R9).
- **Остановка realm посреди анимации.** Пометки возвращают `SendError`; кэш не двигается,
  панели нет; при возобновлении следующий тик повторяет (R11). Утечки нет: `detach` при
  разрушении дерева снимает слушатель.
- **Panic в пользовательском коде.** Единственная точка вызова — `Animation::value()` в
  слушателе и `attach`; изоляция panic в раздаче — listener-delivery (D-01). Если `value()`
  паникует в `attach`, это паника монтажа, как у `RenderAnimatedOpacity` сегодня; paint и
  hit-test пользовательский код не вызывают (R15). Drawer/Dismissible/RefreshIndicator:
  колбэки пользователя (`on_refresh`, `on_open_changed`) вызываются из жестов, как прежде.
- **Dispose во время тика / retarget на последнем кадре.** R10, R8. **dt = 0, огромный dt,
  время назад** — R14. **NaN и переполнение** — R12: неконечный сэмпл игнорируется,
  неконечная матрица — `Degenerate`.
- **Остаточные риски.** (1) Пометка semantics на каждом тике — паритет с
  `RenderTransform::set_transform`; стоимость (`semantics_nodes_updated`) записывается в perf-тест,
  но не ограничивается. (2) Перенос со слоем меняет решение, записанное в
  `flui-rendering/ARCHITECTURE.md:381-386`; выигрыш подтверждает бенч-сценарий в
  `crates/flui-widgets/tests/perf.rs` (до/после: `nodes_painted`, `layers_produced`). (3)
  Drawer: замена `width_factor` на перенос меняет раскладку панели (полная ширина вместо
  сжатой); виджеты, читающие ширину drawer через `LayoutBuilder` внутри панели, теперь видят
  полную ширину на всех кадрах — это и есть ширина панели, но поведение меняется. (4) Сжатие
  `Dismissible` по-прежнему перестраивается на тик.

## Владение

| Объект | Сильные | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `RenderAnimatedTransform` | дерево render | — | drop узла; слушатель снят в `detach` |
| `ProxyAnimation` перехода | state виджета, render object | слушатель (`Weak`) | drop state и узла; `set_parent` в `did_update_view` |
| `Rc<Cell<MotionSample>>` | render object, слушатель | — | с последним из них |
| слушатель | канал значений родителя proxy | — | `detach`; при drop дерева без `detach` — цикла нет (захват слабый) |

Цикл **C5** для нового объекта не возникает по построению; «Drop == 1» —
`animated_transform_releases_proxy_after_tree_drop` (drop `PipelineOwner` без `detach`, Weak-сентинел
proxy `None`), строка `harness_*` рядом с opacity. Замков нет.

## Паттерн

- **Закрытый `#[non_exhaustive]` enum** — `TransformMotion { Slide, Scale, Rotation }` вместо
  трёх типов (A) и `Box<dyn Fn>` (C): пользовательский код не попадает в paint/hit-test.
- **Push-кэш** — `Rc<Cell<MotionSample>>`, paint читает кэш.
- **Weak-захват в колбэке** — разрывает self-цикл proxy.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `SlideTransition` через `AnimatedWidget` rebuild | Flutter `AnimatedWidget` | render object со слушателем, без rebuild |
| три класса `RenderSlide/Scale/RotationTransition` | иерархия классов | один объект + enum |
| `set_transform_hit_tests`/`set_text_direction` | setter render-объекта | оставлено: конвенция `flui-objects` (сеттер возвращает `RenderUpdateImpact`), не пара getter/setter |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Owner-local кэш, без временных атомиков | нарушала | design.md:59, 94-97, 111-117, 165-169 | `Rc<Cell<_>>`, `Cell<bool>`, `Rc::ptr_eq` |
| Колбэк хранит минимум (`Weak`) | нарушала | design.md:76-84 (захват не назван) | слабый proxy; C5 |
| Конструкторы принимают `impl Animation + 'static` | нарушала | design.md:93, 97 (`Arc<dyn Animation>`) | R5 |
| Закрытый набор → enum | соответствует | design.md:39-55 | — |
| Нефинитное не публикуется | соответствует | инв. 2, R12 | — |
| Таблица владения + «Drop == 1» | нарушала | — | раздел «Владение» |
