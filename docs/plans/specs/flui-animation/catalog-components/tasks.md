# flui-animation / catalog-components — задачи

- **Статус:** на утверждении
- **Дата:** 2026-10-17
- **Design:** [design.md](design.md), [requirements.md](requirements.md).
- **Порядок:** ветка и worktree на задачу; PR на задачу; весь набор строится целиком, порядок
  задаёт только граф. Каждая задача закрывает свои требования тестом, падающим без неё, и
  несёт фрагмент `changelog.d/`. Задача, после которой пакет впервые использует элемент
  `flui-painting`/`flui-widgets`, сама добавляет его в `flui-sdk` и строку в
  `tests/surface.rs` (Z1); T12 только проверяет, что ничего не пропущено.

## Граф

```text
T1 геометрия путей ──┬─► T3 AnimatedIcon + данные ─► T4 SymbolEffect
                     └─► T2 RoundedPolygon/Morph ─► T9 LoadingIndicator
T5 отложенный drag + LongPressDraggable + EdgeAutoScroller ─► T6 Reorderable*
T7 Carousel (общий, sliver) ─► T8 CarouselView (Material)
T10 индикаторы прогресса (круговой, зазор, волна) ─► T9
T11 PageIndicator + CupertinoPageControl (после T7 — сцена карусели)
T13 частицы (независима)
T1..T13 ─► T12 поверхность SDK, ARCHITECTURE, motion_lab, таблица reduce motion
```

## Задачи

### T1 — геометрия путей

- **Что:** `Path::length`, `trim`, `transform`, `lerp` в `flui-painting` на kurbo; поверхность
  SDK для них.
- **Требования:** G1, G2.

### T2 — скруглённые многоугольники и морф

- **Что:** порт `RoundedPolygon`, `Morph`, `MaterialShapes` (androidx.graphics.shapes,
  Apache-2.0, с атрибуцией в заголовке модуля); таблица против значений исходника.
- **Требования:** G3. **После:** T1.

### T3 — `AnimatedIcon`

- **Что:** `AnimatedIconData`, `AnimatedIcon` в `flui-widgets::icon` (RTL, `IconTheme`,
  семантика); `cargo xtask icon-import` (AVD XML, пары SVG); четырнадцать Material-пар в
  `flui-material`, сгенерированные конвертером.
- **Требования:** I1, I2, I3, I5. **После:** T1.

### T4 — `SymbolEffect`

- **Что:** дискретные, бесконечные, переходные эффекты и `replace` на любой иконке;
  `draw_on`/`draw_off` через `Path::trim` для векторных; потребитель — `NavigationBar`
  (bounce выбранной иконки) и `IconButton` с переключением.
- **Требования:** I4, I5. **После:** T3.

### T5 — отложенное перетаскивание

- **Что:** отложенный multi-drag распознаватель в `flui-interaction`; `LongPressDraggable<T>`
  и `EdgeAutoScroller` в `flui-widgets` (автопрокрутка доступна и `Draggable`).
- **Требования:** R2 (общая часть), R4 (автопрокрутка).

### T6 — перетаскивание в списках и сетках

- **Что:** `ReorderableList`, `ReorderableGrid`, `SliverReorderableList`, `ReorderHandle`;
  посадка и сдвиг на FLIP из `AnimatedList`; клавиатура, действия семантики, объявления,
  отмена.
- **Требования:** R1–R7. **После:** T5.

### T7 — карусель

- **Что:** модель ключевых линий, `RenderSliverCarousel` (ленивый, маска), `CarouselLayout`,
  `CarouselItemScope`, `CarouselFling` (`SingleAdvance`, `MultiBrowse`, `Free`),
  `CarouselController`, автопрокрутка с гарантиями.
- **Требования:** C1–C6.

### T8 — `CarouselView`

- **Что:** Material-карточки элементов, токены M3, рябь и тап.
- **Требования:** C7. **После:** T7.

### T9 — `LoadingIndicator`

- **Что:** `LoadingIndicator`, `ContainedLoadingIndicator` (неопределённый и определённый)
  на `Morph`; потребитель — `RefreshIndicator` в Material-теме.
- **Требования:** P3, P5. **После:** T2, T10.

### T10 — индикаторы прогресса

- **Что:** `CircularProgressIndicator`; зазор трека и stop-точка у линейного и кругового;
  волнистые варианты.
- **Требования:** P1, P2, P5.

### T11 — индикатор страниц

- **Что:** `PageIndicator` (дробная позиция, тап, окно, семантика); `CupertinoPageControl`;
  потребитель — сцена карусели и `PageView` в `motion_lab`.
- **Требования:** P4. **После:** T7.

### T13 — частицы

- **Что:** `ParticleEmitter`, `ParticleConfig`, `ParticleController`, пресеты; атлас
  спрайтов из `Image::from_rgba8`; `fastrand` с сидом; остановка тика.
- **Требования:** X1–X6.

### T12 — закрытие

- **Что:** проверка, что каждая строка `flui-sdk` `tests/surface.rs` добавлена своей задачей;
  таблица reduce motion в `ARCHITECTURE.md` затронутых крейтов с тестом на строку (M1);
  сцены в `examples/motion_lab.rs`; проверка Z2 (у каждого публичного элемента есть
  production-потребитель).
- **Требования:** M1, Z1–Z4. **После:** T1–T11, T13.
