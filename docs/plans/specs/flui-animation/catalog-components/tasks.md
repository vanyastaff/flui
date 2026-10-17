# flui-animation / catalog-components — задачи

- **Статус:** готов к работе
- **Дата:** 2026-10-17
- **Design:** [design.md](design.md), [requirements.md](requirements.md).
- **Тикеты:** родитель vanyastaff/flui#1691; T1 #1692 · T2 #1693 · T3 #1694 · T4 #1695 ·
  T6 #1697 · T7 #1698 · T8 #1699 · T9 #1701 · T10 #1700 · T11 #1702 · T12 #1704 ·
  T13 #1703. T5 (#1696) снят: его работа — T29/T30 спека полноты ввода (K12).
- **Порядок:** ветка и worktree на задачу; PR на задачу; весь набор строится целиком, порядок
  задаёт только граф. Каждая задача закрывает свои требования тестом, падающим без неё, и
  несёт фрагмент `changelog.d/`. Задача, после которой пакет впервые использует элемент
  `flui-painting`/`flui-widgets`, сама добавляет его в `flui-sdk` и строку в
  `tests/surface.rs` (Z1); задача, которая вводит render object, сама добавляет строку
  `RENDER_OBJECT_TYPES` и `harness_*` (Z3); задача называет production-потребителя своего
  публичного API или записывает в PR, что это API приложения (Z2); каждая задача следует
  числовой политике N1 и тестирует свои экстремальные конечные входы. T12 только проверяет, что
  ничего не пропущено.
- **Внешние предпосылки:** задачи спека
  [полноты ввода](../../flui-interaction/completeness/tasks.md) T21 (канал прокрутки),
  T23 (`SnapPolicy`), T29/T30 (DnD на сессиях, автопрокрутка), T33 (`ReorderableList` и
  `ReorderableGrid` как DnD-сессия).

## Граф

```text
T1 геометрия путей ──┬─► T3 AnimatedIcon + данные ─► T4 SymbolEffect
                     └─► T2 RoundedPolygon/Morph ─► T9 LoadingIndicator
[ввод T33] ─► T6 движение перестановки
[ввод T21, T23] ─► T7 Carousel (общий, sliver) ─► T8 CarouselView (Material)
T10 индикаторы прогресса (круговой, зазор, волна) ─► T9
T7 ─► T11 PageIndicator + CupertinoPageControl
T13 частицы (независима)
T1..T13 ─► T12 поверхность SDK, ARCHITECTURE, motion_lab, таблица reduce motion
```

## Задачи

### T1 — геометрия путей (#1692)

- **Что:** `Path::length`, `trim`, `transform`, `lerp` в `flui-painting` на kurbo, с политикой
  домена из G1/G2. Потребители — T2, T3, T4.
- **Требования:** G1, G2.

### T2 — скруглённые многоугольники и морф (#1693)

- **Что:** собственные `RoundedPolygon`, `Morph`, `MaterialShapes` в `flui-painting`
  (Rust-дизайн FLUI); значения androidx.graphics.shapes — оракул в таблице, код не
  переносится. Потребитель — T9.
- **Требования:** G3. **После:** T1.

### T3 — `AnimatedIcon` (#1694)

- **Что:** `AnimatedIconData`, `AnimatedIcon` в `flui-widgets::icon` (RTL, `IconTheme`,
  семантика); `cargo xtask icon-import` (AVD XML, пары SVG); четырнадцать Material-пар в
  `flui-material`, сгенерированные конвертером. API приложения (Z2): `AnimatedIcon` — из
  `flui-widgets` через фасад и `flui-sdk`, пары — из `flui-material`.
- **Требования:** I1, I2, I3, I5, строка M1. **После:** T1.

### T4 — `SymbolEffect` (#1695)

- **Что:** дискретные, бесконечные, переходные эффекты и `replace` на любой иконке;
  `draw_on`/`draw_off` через `Path::trim` для векторных; потребитель — `NavigationBar`
  (bounce выбранной иконки) и `IconButton` с переключением.
- **Требования:** I4, I5, строка M1. **После:** T3.

### T6 — движение перестановки (#1697)

- **Что:** поверх DnD-сессии `ReorderableList`/`ReorderableGrid` (T33 полноты ввода):
  `proxy_decorator` подъёма, FLIP-сдвиг соседей из `AnimatedList`, посадка и возврат при
  отмене, то же движение для клавиатуры и действий A8; контракт колбэков R1 закреплён
  тестами.
- **Требования:** R1–R4, строка M1. **После:** T33 полноты ввода.

### T7 — карусель (#1698)

- **Что:** модель ключевых линий, `RenderSliverCarousel` (ленивый, маска; строка
  `RENDER_OBJECT_TYPES` и `harness_*`), фаллибельный `CarouselLayout` с
  `CarouselLayoutError`, `CarouselItemScope`, `KeylineSnap` как `SnapPolicy`,
  `CarouselController` и автопрокрутка через `NestedScrollDispatcher` с причиной
  `Programmatic`, гарантии остановки.
- **Требования:** C1–C6, строка M1. **После:** T21, T23 полноты ввода.

### T8 — `CarouselView` (#1699)

- **Что:** Material-карточки элементов, токены M3, рябь и тап; потребитель общего
  `Carousel`. Сам `CarouselView` — API приложения (Z2): экспорт из `flui-material` и его
  prelude (не через `flui-sdk`) и сцена в `motion_lab`.
- **Требования:** C7. **После:** T7.

### T9 — `LoadingIndicator` (#1701)

- **Что:** `LoadingIndicator`, `ContainedLoadingIndicator` (неопределённый и определённый)
  на `Morph`; потребитель — Expressive-стиль `RefreshIndicator` через шов P6 из T10.
- **Требования:** P3, P5, P6 (Expressive-стиль), строка M1. **После:** T2, T10.

### T10 — индикаторы прогресса (#1700)

- **Что:** `CircularProgressIndicator`; зазор трека и stop-точка у линейного и кругового;
  волнистые варианты; шов `RefreshIndicatorStyle` в `flui-widgets` (по умолчанию
  `ActivityIndicator`) и Material-стиль, который ставит `MaterialApp`. Потребитель
  `CircularProgressIndicator` — этот стиль; волнистые варианты — API приложения (Z2),
  экспорт из `flui-material`.
- **Требования:** P1, P2, P5, P6 (шов и Material-стиль), строка M1.

### T11 — индикатор страниц (#1702)

- **Что:** `PageIndicator::controlled` и `PageIndicator::observed` (дробная позиция, тап и
  действия только у управляемого, окно, семантика); `CupertinoPageControl` — его
  потребитель.
- **Требования:** P4, строка M1. **После:** T7.

### T13 — частицы (#1703)

- **Что:** `ParticleEmitter`, `ParticleConfig`, `ParticleController`, пресеты; атлас
  спрайтов из `Image::from_rgba8`; `fastrand` с сидом; остановка тика и перезапуск по
  всплеску. API приложения (Z2): экспорт через фасад и сцена в `motion_lab`.
- **Требования:** X1–X8, N1, строка M1.

### T12 — закрытие (#1704)

- **Что:** проверка, что каждая строка `flui-sdk` `tests/surface.rs`, строка
  `RENDER_OBJECT_TYPES` и запись Z2 добавлены своей задачей; сводная таблица reduce motion в
  `ARCHITECTURE.md` затронутых крейтов и проверка, что каждую строку уже закрыл тест её
  задачи (M1); сцены в
  `examples/motion_lab.rs`.
- **Требования:** M1, Z1–Z4. **После:** T1–T4, T6–T11, T13.
