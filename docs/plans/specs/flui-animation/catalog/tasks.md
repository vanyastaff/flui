# flui-animation / catalog — задачи

- **Статус:** готов к работе
- **Дата:** 2026-10-13
- **Design:** [design.md](design.md), [requirements.md](requirements.md).
- **Порядок:** ветка и worktree на задачу; PR на задачу; весь набор строится целиком, порядок
  задаёт только граф. Каждая задача закрывает свои требования тестом, падающим без неё, и
  несёт фрагмент `changelog.d/`. Задача, после которой пакет впервые использует элемент
  `flui-widgets`/`flui-animation`, сама добавляет его строку в `flui-sdk` `tests/surface.rs`
  (Z2); T14 только проверяет, что ничего не пропущено.

## Граф

```text
T1 ADR (хук переходов)
T2 неявное ядро ─┬─► T3 новые неявные + Animated<T> + AnimatedSize
                 ├─► T9 Switch + NavigationBar
                 └─► T10 TabController + индикатор
T4 явные переходы (независима)
T5 Effect/Transition/Effects/Animate ─┬─► T6 AnimatedSwitcher на Transition
                                      ├─► T7 AnimatedList/Grid/Sliver + stagger
                                      └─► T13 Shimmer/Skeleton
T1 ─► T8 хук + PageTransitionsTheme + диалог
T8 ─► T11 flight_seat + OpenContainer
T12 рябь InkWell (независима)
T3..T13 ─► T14 поверхность SDK, ARCHITECTURE, motion_lab
```

## Задачи

### T1 — ADR

- **Что:** ADR «хук переходов страниц» (механизм в `flui-widgets`, карта в пакетах, общий
  параллакс-сдвиг в `flui-widgets`) — ADR-0221. Движение из layout покрыто ADR-0013 (D2/D3); `AnimatedValue` вместо
  контроллера сохраняет ту же форму.
- **Требования:** Z1.

### T2 — неявное ядро

- **Что:** `MotionSpec::with_duration/with_curve`, `MotionBuilder`, `ImplicitMotion<T>`;
  перевод пяти существующих виджетов; стол W.
- **Требования:** W1, W5, W6.

### T3 — новые неявные виджеты

- **Что:** `AnimatedPositioned`, `AnimatedScale`, `AnimatedSlide`,
  `AnimatedDefaultTextStyle`, `AnimatedCrossFade`, `Animated<T>`; `AnimatedTheme` в
  `flui-material` (потребитель — смена темы в `MaterialApp`); `AnimatedSize` на `MotionSpec`.
- **Требования:** W2, W3, W4. **После:** T1, T2.

### T4 — явные переходы

- **Что:** `SizeTransition`, `AlignTransition`, `PositionedTransition`,
  `DecoratedBoxTransition`; тень края маршрута Cupertino на `DecoratedBoxTransition`
  (потребитель).
- **Требования:** X1.

### T5 — эффекты

- **Что:** `Effect`, `Transition` (+, asymmetric), `Effects` (шкала, `then`), `Animate`,
  `AnimateExt`, `Trigger`, stagger; столы E.
- **Требования:** E1–E5, E7.

### T6 — `AnimatedSwitcher` на `Transition`

- **Требования:** E6. **После:** T5.

### T7 — списки

- **Что:** модуль разницы ключей; `AnimatedList`, `AnimatedGrid`, `SliverAnimatedList`;
  ступенчатое появление; стол L.
- **Требования:** L1–L6. **После:** T1, T5.

### T8 — переходы страниц и диалогов

- **Что:** `PageTransitionBuilder`, `PageTransitions`; `PageTransition`,
  `PageTransitionsTheme` в `ThemeData`; FadeForwards, Zoom, SharedAxis, FadeThrough;
  перенос параллакс-сдвига в `flui-widgets` и перевод на него `flui-cupertino`; fade
  диалога.
- **Требования:** R1–R5, Z2. **После:** T1.

### T9 — `Switch` и `NavigationBar`

- **Требования:** M5, M6. **После:** T2.

### T10 — `TabController` и индикатор

- **Требования:** M7, M8. **После:** T2.

### T11 — `OpenContainer`

- **Что:** `flight_seat.rs` (сначала `Hero`), `OpenContainer<T>` в `flui-material`.
- **Требования:** R6. **После:** T8.

### T12 — рябь

- **Что:** `RippleConfiguration`, `StateLayerAlpha`, волна и слой состояния в `InkWell`;
  стол M.
- **Требования:** M1–M4.

### T13 — скелетоны

- **Что:** проверка смещения до предка при paint; `Shimmer`, `Skeleton`; эффект shimmer на
  `ShaderMask`.
- **Требования:** S1–S3. **После:** T5.

### T14 — закрытие

- **Что:** проверка, что каждая строка `flui-sdk` `tests/surface.rs` добавлена своей задачей
  и ни одна не пропущена; `ARCHITECTURE.md` затронутых крейтов; сцены в
  `examples/motion_lab.rs`; проверка Z3 (у каждого публичного элемента есть
  production-потребитель).
- **Требования:** Z2–Z4. **После:** T3–T13.
