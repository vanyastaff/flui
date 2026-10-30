# flui-animation / catalog-components — задачи

- **Статус:** в работе. Влиты T1, T2, T3, T7, T8, T9, T10, T11, T13; T4 сделан (`SymbolEffect`,
  `PathEffect`); дальше переключаемый `IconButton`, T6 после T33 полноты ввода, затем T12.
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

### T1 — геометрия путей (#1692) — влито

- **Что:** `Path::length`, `trim`, `transform`, `lerp` в `flui-painting` на kurbo, с политикой
  домена из G1/G2. Потребители — T2, T3, T4.
- **Требования:** G1, G2.

### T2 — скруглённые многоугольники и морф (#1693) — влито

- **Что:** собственные `RoundedPolygon`, `Morph`, `MaterialShapes` в `flui-painting`
  (Rust-дизайн FLUI); значения androidx.graphics.shapes — оракул в таблице, код не
  переносится. Потребитель — T9.
- **Требования:** G3. **После:** T1.

### T3 — `AnimatedIcon` (#1694) — влито

- **Что:** `AnimatedIconData`, `AnimatedIcon` в `flui-widgets::icon` (RTL, `IconTheme`,
  семантика); `cargo xtask icon-import` (AVD XML, пары SVG); четырнадцать Material-пар в
  `flui-material`, сгенерированные конвертером. API приложения (Z2): `AnimatedIcon` — из
  `flui-widgets` через фасад и `flui-sdk`, пары — из `flui-material`.
- **Требования:** I1, I2, I3, I5, строка M1. **После:** T1.
- **Проверка:** `animation_and_visibility` в `flui-widgets` (`animated_icon::icon_data_is_checked`,
  `…::the_frame_follows_the_progress`, `…::theme_and_direction_shape_the_icon`,
  `…::reduced_motion_shows_the_target_frame`, `…::the_label_lives_on_the_icon`);
  `material_animated_icons` в `flui-material`; `cargo xtask icon-import material` в `xtask checks`.
  Данные пар — из сгенерированных данных Flutter (BSD-3), ревизия в
  `packages/flui-material/assets/animated_icons/README.md`.

### T4 — `SymbolEffect` (#1695)

- **Что:** дискретные, бесконечные, переходные эффекты и `replace` на любой иконке;
  `draw_on`/`draw_off` через `Path::trim` для векторных; потребитель — `NavigationBar`
  (bounce выбранной иконки) и `IconButton` с переключением.
- **Требования:** I4, I5, строка M1. **После:** T3.
- **Сделано.** `SymbolEffect` (`bounce`, `wiggle`, `rotate`, `pulse`, `breathe`, `appear`,
  `disappear`, `replace`; `repeat`, `speed(Speed)`, `animate_on_appear`) на `Icon` и
  `AnimatedIcon`, `PathEffect` (`draw_on`/`draw_off`) только на `AnimatedIcon`. Отступления,
  одобренные координатором: `Speed::try_new` вместо `Speed::new`; триггер `on_change(u64)`;
  длительности — ассоциированные константы (`SymbolEffect::BOUNCE_DURATION` …); эффект
  наследуется через `IconThemeData::symbol_effect`, а `IconThemeData` стал `#[non_exhaustive]`
  с методом на каждое поле. Потребитель — `NavigationBar`: новая выбранная иконка
  подпрыгивает, покинутая — нет. Переключаемый `IconButton` (`is_selected`/`selected_icon`,
  `replace`) — отдельная задача: у `IconButton` ещё нет варианта-переключателя.
  Тесты: `symbol_effect::*` в `widgets_it` (`each_effect_peaks_where_its_timing_says`,
  `a_one_shot_effect_plays_per_change_and_repeat`,
  `a_repeating_effect_finishes_its_cycle_when_turned_off`,
  `a_replace_cross_fades_and_only_the_new_label_is_heard`, `transitions_play_when_first_applied`,
  `reduced_motion_keeps_the_icon_still_or_at_its_end`,
  `a_speed_is_positive_and_finite_and_scales_the_timing`,
  `a_path_effect_draws_the_paths_on_and_off`); доктест `compile_fail` у `PathEffect` (у `Icon`
  нет `path_effect`); `navigation_bar::a_newly_selected_icon_bounces` в `material_it`.

### T6 — движение перестановки (#1697)

- **Что:** поверх DnD-сессии `ReorderableList`/`ReorderableGrid` (T33 полноты ввода):
  `proxy_decorator` подъёма, FLIP-сдвиг соседей из `AnimatedList`, посадка и возврат при
  отмене, то же движение для клавиатуры и действий A8; контракт колбэков R1 закреплён
  тестами.
- **Требования:** R1–R4, строка M1. **После:** T33 полноты ввода.

### T7 — карусель (#1698) — влито

- **Что:** модель ключевых линий, `RenderSliverCarousel` (ленивый, маска; строка
  `RENDER_OBJECT_TYPES` и `harness_*`), фаллибельный `CarouselLayout` с
  `CarouselLayoutError`, `CarouselItemScope`, `KeylineSnap` как `SnapPolicy`,
  `CarouselController` и автопрокрутка через `NestedScrollDispatcher` с причиной
  `Programmatic`, гарантии остановки.
- **Требования:** C1–C6, строка M1. **После:** T21, T23 полноты ввода.
- **Проверка:** модуль `carousel` в `flui-widgets` (16 тестов: маски между линиями, тап только по видимой
  части, `SingleAdvance` на ±1 от начала жеста, контроллер и `on_index_changed`, автопрокрутка с
  паузой и переходом с последнего на первый, reduced motion, семантика и кнопка паузы, RTL,
  свойства `CarouselGeometry` для всех раскладок); `harness_sliver_carousel_geometry_and_frames`.
  Пауза по фокусу вспомогательной технологии невозможна: сигнала нет (`docs/gaps.md`, пункт 36).

### T8 — `CarouselView` (#1699) — влито

- **Что:** Material-карточки элементов, токены M3, рябь и тап; потребитель общего
  `Carousel`. Сам `CarouselView` — API приложения (Z2): экспорт из `flui-material` и его
  prelude (не через `flui-sdk`) и сцена в `motion_lab`.
- **Требования:** C7. **После:** T7.
- **Проверка:** `carousel_view_contracts` в `flui-material` (`every layout implies its snap`,
  `the implied snap settles a release and none scrolls freely`, `cards cover the shown part of each
  item`, `the widget beats the theme which beats the tokens`, `a tap reports its card and cards are
  buttons only with on_tap`). Prelude у `flui-material` нет, экспорт из корня крейта; сцена в
  `motion_lab` переходит в T12.

### T9 — `LoadingIndicator` (#1701) — влито

- **Что:** `LoadingIndicator`, `ContainedLoadingIndicator` (неопределённый и определённый)
  на `Morph`; потребитель — Expressive-стиль `RefreshIndicator` через шов P6 из T10.
- **Требования:** P3, P5, P6 (Expressive-стиль), строка M1. **После:** T2, T10.
- **Проверка:** `progress_indicator_variants` в `flui-material` (`loading indicator morphs on a
  spring`, `… stands still`, `… follows its value`, `contained loading indicator has its
  container`, `expressive material app refresh shows the loading indicator`).

### T10 — индикаторы прогресса (#1700) — влито

- **Что:** `CircularProgressIndicator`; зазор трека и stop-точка у линейного и кругового;
  волнистые варианты; шов `RefreshIndicatorStyle` в `flui-widgets` и ADR на него (по умолчанию
  `ActivityIndicator`) и Material-стиль, который ставит `MaterialApp`. Потребитель
  `CircularProgressIndicator` — этот стиль; волнистые варианты — API приложения (Z2),
  экспорт из `flui-material`.
- **Требования:** P1, P2, P5, P6 (шов и Material-стиль), строка M1.

### T11 — индикатор страниц (#1702) — влито (#2048)

- **Что:** `PageIndicator::controlled` и `PageIndicator::observed` (дробная позиция, тап и
  действия только у управляемого, окно, семантика); `CupertinoPageControl` — его
  потребитель.
- **Требования:** P4, строка M1. **После:** T7.
- **Отклонение:** модуль `scroll`, не `controls`: `controls` ниже `scroll` в графе модулей и
  не видит `PageController`.
- **Проверка:** `page_indicator::*` в `widgets_it` (`flui-widgets`), строка M1 —
  `page_indicator::reduced_motion_turns_the_page_at_once`; `page_control::*` в `cupertino_it`.

### T13 — частицы (#1703) — влито

- **Что:** `ParticleEmitter`, `ParticleConfig`, `ParticleController`, пресеты; атлас
  спрайтов из `Image::from_rgba8`; `fastrand` с сидом; остановка тика и перезапуск по
  всплеску. API приложения (Z2): экспорт через фасад и сцена в `motion_lab`.
- **Требования:** X1–X8, N1, строка M1.
- **Проверка:** случаи `particles::*` в `animation_and_visibility` (`flui-widgets`). Сцена
  `motion_lab` — в T12: примера ещё нет.

### T12 — закрытие (#1704)

- **Что:** проверка, что каждая строка `flui-sdk` `tests/surface.rs`, строка
  `RENDER_OBJECT_TYPES` и запись Z2 добавлены своей задачей; сводная таблица reduce motion в
  `ARCHITECTURE.md` затронутых крейтов и проверка, что каждую строку уже закрыл тест её
  задачи (M1); сцены в
  `examples/motion_lab.rs`.
- **Требования:** M1, Z1–Z4. **После:** T1–T4, T6–T11, T13.
