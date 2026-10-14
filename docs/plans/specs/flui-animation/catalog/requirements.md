# flui-animation / catalog — требования

- **Статус:** утверждена владельцем (решения C1–C7 — владелец; Q8–Q15 делегированы и закрыты
  исследованием, см. design «Решения»)
- **Дата:** 2026-10-13
- **База:** `main` 4628368ff.
- **Связанные:** [design.md](design.md), [tasks.md](tasks.md); ADR-0088 (поверхность SDK),
  ADR-0195/ADR-0218 (политика движения, `MotionOverride`), ADR-0220 (диагностика).

## Пользовательский сценарий

Автор приложения получает готовое движение без ручной сборки контроллеров: переходы страниц и
диалогов, рябь нажатия, анимированные переключатели и вкладки, появление и удаление элементов
списка, скелетоны загрузки, цепочку эффектов на любом view и Material-переходы контейнеров.
Каждый готовый элемент уважает reduce motion, переживает прерывание без скачка и проверяется
тестом через публичный API.

## Текущее состояние (чтением)

- Пять неявных виджетов (`AnimatedOpacity`, `Align`, `Padding`, `Rotation`, `Container`)
  копируют билдеры `duration`/`curve`/`spring` и жизненный цикл `AnimatedValue`;
  `AnimatedSize` на старом `AnimationController`, без пружины.
- Нет `AnimatedCrossFade`, `AnimatedPositioned`, `AnimatedScale`, `AnimatedSlide`,
  `AnimatedDefaultTextStyle`, `AnimatedTheme`; нет `SizeTransition`, `AlignTransition`,
  `PositionedTransition`, `DecoratedBoxTransition`.
- Нет `AnimatedList`/`AnimatedGrid`, цепочки эффектов, shimmer/skeleton.
- `PageRoute`/`PopupRoute` по умолчанию — 300 мс jump cut без темы; `show_dialog` без fade.
- `InkWell` красит один цвет состояния: нет ряби и затухания.
- `Switch`, индикатор вкладок и `NavigationBar` прыгают; `TabController::animate_to` —
  псевдоним без анимации.

## Требования

ID — только в этом каталоге. «Таблица» — строка существующего runner'а крейта
(`run_table`/`run_cases`); виджетные тесты — `flui_testing::widgets::lay_out_animated` +
`pump_for` на виртуальных часах, кадр читается из записанного display list. Каждое требование
закрывается тестом, падающим без реализации.

### Неявное движение (W)

- **W1** Все неявные виджеты делят одно ядро: `duration()` сохраняет кривую, `curve()` —
  длительность, `spring()` переключает на пружину; правило слияния живёт один раз на
  `MotionSpec`.
- **W2** Новые неявные виджеты: `AnimatedPositioned`, `AnimatedScale`, `AnimatedSlide`,
  `AnimatedDefaultTextStyle`, `AnimatedTheme` (Material), `AnimatedCrossFade`.
- **W3** `Animated::new(target, builder)` анимирует любое `T: TwoWayConverter` (выход для
  пользовательских значений).
- **W4** `AnimatedSize` на `MotionSpec`, пружина по умолчанию, ретаргет при смене размера
  ребёнка без скачка.
- **W5** Прерывание в середине сохраняет позицию и скорость под пружиной (строка на виджет).
- **W6** Под `MotionOverride::Reduce` каждый виджет показывает целевое значение в первом кадре.

### Явные переходы (X)

- **X1** `SizeTransition`, `AlignTransition`, `PositionedTransition`,
  `DecoratedBoxTransition` на `Animation`; каждый новый render object — строка
  `RENDER_OBJECT_TYPES` и `harness_*`.

### Эффекты и переходы появления (E)

- **E1** `Effect` — закрытый `#[non_exhaustive]` набор: fade, slide, scale, rotate, shimmer;
  blur — только вместе с render object и GPU-readback.
- **E2** `Transition` (появление/исчезновение): слоты сливаются, правый операнд `+`
  выигрывает; `Transition::asymmetric(enter, exit)`; обратим — прерванное исчезновение
  проигрывается назад от текущего прогресса.
- **E3** `Effects` (хореография): упорядоченная временная шкала; задержка, длительность и
  движение наследуются, `.then()` переносит базу к концу предыдущего; полная длительность —
  максимум концов.
- **E4** `Animate::new(effects, child)` и `AnimateExt::animate(effects)`; триггеры `OnMount`
  (по умолчанию), `OnChange(value)` (повтор только при новом значении), `Target(f64)`
  (обратимо к позиции), `Controller(handle)`.
- **E5** Stagger: одинаковые `Effects` на детях получают задержку `Stagger::delay(i, n)`.
- **E6** `AnimatedSwitcher` принимает `Transition`.
- **E7** Reduce motion: эффект на mount показывает конечный кадр, повтор останавливается.

### Списки (L)

- **L1** `AnimatedList` декларативен: элементы, функция ключа, билдер; новый ключ входит,
  пропавший становится «призраком» из последнего `BoxedView` и играет выход, затем удаляется.
  Вызывающий ничего не перестраивает.
- **L2** Ключ, вернувшийся во время выхода, забирает тот же элемент (без `dispose`) и
  разворачивается от текущего прогресса.
- **L3** Перестановка анимирует размещение (FLIP), прерывание ретаргетит без скачка.
- **L4** Дубликат ключа — паника `BUG:` с именем ключа.
- **L5** `AnimatedGrid` и `SliverAnimatedList` на той же разнице; ленивый список анимирует
  размещение только для элементов, выложенных в обоих кадрах; поиск индекса по ключу учитывает
  призраков.
- **L6** Ступенчатое появление списка (`stagger`) на `Effects` из E5.

### Маршруты и диалоги (R)

- **R1** Ambient-хук переходов страниц в `flui-widgets`: `PageRoute` без `.transitions()`
  спрашивает его; без хука — прежний jump cut.
- **R2** Material `PageTransitionsTheme` по `TargetPlatform`; `PageTransition` —
  `#[non_exhaustive]` enum `{FadeForwards, Zoom, Cupertino, SharedAxis(_), FadeThrough, None,
  Custom(_)}`; `Custom` сравнивается по `Rc::ptr_eq`.
- **R3** Умолчания: Android — FadeForwards 450 мс emphasized; iOS/macOS — Cupertino 500 мс
  (общий параллакс-сдвиг, перенесённый в `flui-widgets`, не копия); Windows/Linux/web — Zoom 300 мс;
  SharedAxis и FadeThrough — 300 мс, по выбору.
- **R4** `show_dialog` — fade 150 мс через `PopupRoute`.
- **R5** Reduce motion: переход завершается в первом кадре.
- **R6** `OpenContainer<T>`: закрытый и открытый билдеры, морф цвета, высоты и радиуса,
  `Fade`/`FadeThrough`, 300 мс fast-out-slow-in, типизированный результат, источник
  перемеряется при pop; закрытый контейнер держит размер (соседи не сдвигаются).

### Обратная связь Material (M)

- **M1** `InkWell` владеет своей рябью: геометрическая волна 75 мс появление, 225 мс радиус,
  150 мс затухание; отпущенное нажатие сначала завершает появление.
- **M2** Слой состояния по токенам M3: hover 0.08, focus 0.10, pressed 0.10, dragged 0.16,
  определены один раз в `flui-material`; переходы hover 15 мс, focus 45 мс.
- **M3** `RippleConfiguration` ambient; `None` отключает рябь; рябь не выходит за форму
  своего `InkWell`.
- **M4** Reduce motion: слой состояния есть, расширяющегося радиуса нет.
- **M5** `Switch`: бегунок и дорожка анимируются на ядре W; RTL зеркалится.
- **M6** `NavigationBar`: индикатор выбора анимируется на ядре W.
- **M7** `TabController::position()` — дробная позиция без обратной записи: собственное
  `AnimatedValue<f64>` без `TabBarView`, `PageController::page()` с ним; `index` фиксируется
  при остановке страницы.
- **M8** Индикатор вкладок: линейный для полноширинных, эластичный для по-ширине-метки;
  RTL зеркалится; во время перетаскивания `index` не меняется.

### Скелетоны (S)

- **S1** `Shimmer` — одна общая фаза для поддерева; `Skeleton::rect/line/circle/text` красят
  свой `LinearGradient` в координатах `Shimmer`, поэтому все скелетоны делят одну полосу.
- **S2** Shimmer-эффект поверх произвольного содержимого (E1) — через `ShaderMask`.
- **S3** Период по умолчанию 1500 мс; reduce motion и `enabled(false)` — плоский базовый цвет.

### Сквозные (Z)

- **Z1** ADR: хук переходов страниц (межкрейтовый контракт). Движение из layout
  (`AnimatedSize`, FLIP) уже допускает ADR-0013: владелец — состояние view, render object
  слушает и запускает прогон из `perform_layout`, тик — отложенная пометка следующего кадра.
- **Z2** Новые публичные элементы, нужные пакетам, — в `flui-sdk` по ADR-0088 §4 со строкой в
  `tests/surface.rs`.
- **Z3** У каждого нового публичного элемента есть production-потребитель в этом наборе
  работ (не только тест или пример).
- **Z4** Фрагменты `changelog.d/`, обновлённые `ARCHITECTURE.md` затронутых крейтов.

## Вне объёма

Lottie/Rive, частицы и конфетти, карусели и индикаторы страниц, `ReorderableList`,
`AnimatedIcon`, `InkSparkle`, predictive back на Android, общий морф `ShapeBorder` (кроме
радиусов скругления).
