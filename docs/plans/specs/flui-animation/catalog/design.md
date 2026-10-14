# flui-animation / catalog — design

- **Статус:** утверждён (см. [requirements.md](requirements.md))
- **Дата:** 2026-10-13
- **База:** `main` 4628368ff.

## Решения

| # | Решение | Почему |
|---|---|---|
| C1 | Один спек на всё: маршруты, рябь, неявные и явные переходы, списки, эффекты, скелетоны, Material-переходы контейнеров | Каталог закрывает самые частые сценарии одним набором работ |
| C2 | Общее движение — модули `flui-widgets`; Material — модуль `motion` в `flui-material`; новых крейтов нет | «Layers, not micro-crates» |
| C3/Q8 | Тонкие публичные структуры, общий `MotionBuilder` и приватный помощник `ImplicitMotion<T>` для жизненного цикла одного значения | Rustdoc и ошибки показывают `AnimatedOpacity`, а не `Implicit<Opacity>`; так устроены `animate*AsState` в Compose |
| C4/Q9 | Общий примитив `Effect`, две композиции: обратимый `Transition` (слоты) и `Effects` (шкала) | Появлению/исчезновению нужна обратимость, хореографии — `then`; одна алгебра не даёт обоих |
| C5/Q11 | Хук переходов страниц в `flui-widgets`, карта по платформе в `flui-material` | Механизм ниже, политика выше; `flui-cupertino` подключает свой переход через тот же хук |
| C6/Q14 | Рябь принадлежит `InkWell`, без реестра InkFeature | Модель `Indication` в Compose; сохраняет «нет утечки между виджетами» |
| C7/Q15 | Контролы анимируются на ядре C3; `TabController::position()` выводится, без обратной записи | Одна истина вместо `offset`/`indexIsChanging` Flutter |
| Q10 | `AnimatedList` только декларативный, призраки, FLIP | Императивный `insertItem` даёт класс ошибок рассинхронизации индексов |
| Q12 | `OpenContainer` отдельно от `HeroFlight`; общий только `pub(crate)` помощник замера и места-заглушки | У морфа контейнера нет сопоставления тегов и divert |
| Q13 | `Skeleton` красит свой градиент в координатах `Shimmer`; эффект поверх содержимого — `ShaderMask` | Одна полоса на все скелетоны без offscreen-группы; движок уже применяет `ShaderMask` |

Первичные источники каждого решения (Compose, SwiftUI, Flutter `animations`, flutter_animate,
токены M3) перечислены в issue спека; значения длительностей и кривых ниже взяты
из них и закрепляются тестами, а не ссылкой.

## Публичный API

### Неявное ядро (`flui-animation`, `flui-widgets`)

```rust
impl MotionSpec {
    pub fn with_duration(self, duration: Duration) -> Self; // пружина → кривая по умолчанию
    pub fn with_curve(self, curve: impl Curve + 'static) -> Self;
}

pub trait MotionBuilder: Sized {          // prelude flui-widgets
    fn motion_mut(&mut self) -> &mut MotionSpec;
    fn duration(self, d: Duration) -> Self { .. }
    fn curve(self, c: impl Curve + 'static) -> Self { .. }
    fn spring(self, s: SpringDescription) -> Self { .. }
    fn motion(self, m: MotionSpec) -> Self { .. }
}

pub struct Animated<T, B> { .. }          // Animated::new(target, |value, ctx| view)
```

`ImplicitMotion<T>` остаётся `pub(crate)`: публичность заморозила бы поверхность SDK.
`AnimatedAlign` и `AnimatedContainer` сохраняют многосвойственное `PropertyMotion`, получая только
`MotionBuilder`. Владелец `AnimatedValue<Size>` у `AnimatedSize` — состояние view: оно привязывает
его к `VsyncScope` в `did_change_dependencies` и отдаёт render object не владеющий ретаргет-хэндл,
который тот вызывает из `perform_layout` (форма ADR-0013 D2/D3; `attach` не получает часов).

### Эффекты (`flui-widgets::motion`)

```rust
#[non_exhaustive]
pub enum Effect { Fade { from: f64, to: f64 }, Slide { from: Offset }, Scale { from: f64, origin: Alignment },
                  Rotate { turns: f64 }, Shimmer(ShimmerSpec) }

pub struct Transition { .. }              // fade() + slide_in(..); asymmetric(enter, exit)
impl Add for Transition { .. }            // правый операнд выигрывает слот

pub struct Effects { .. }                 // fade_in().slide_y(0.2).then().scale(..)
pub enum Trigger { OnMount, OnChange(u64), Target(f64), Controller(AnimationHandle) }
pub struct Animate<V> { .. }              // Animate::new(effects, child).trigger(..)
pub trait AnimateExt: IntoView { fn animate(self, effects: Effects) -> Animate<Self>; }
```

Порядок применения фиксирован: прозрачность и масштаб, затем размер, затем сдвиг. Blur
появляется только вместе с `RenderImageFiltered` и GPU-readback.

### Списки (`flui-widgets::animated`)

```rust
AnimatedList::new(items, |item| item.id, |item, ctx| row(item))
    .transition(Transition::fade() + Transition::expand_vertically())
    .placement(Some(MotionSpec::spring_default()));
AnimatedGrid::new(..); SliverAnimatedList::new(..);   // та же разница ключей
```

Разница ключей — один модуль, общий для трёх виджетов. Призрак хранит последний `BoxedView`
и свой прогресс; возврат ключа ретаргетит тот же `AnimatedValue` к 1. Размещение — сдвиг
`AnimatedValue<Offset>` от прежнего смещения к новому. Ленивый вариант резервирует под
призраков индексы в менеджере детей и анимирует размещение только для элементов, выложенных в
обоих кадрах.

### Маршруты (`flui-widgets::navigator`, `flui-material::motion`)

```rust
pub trait PageTransitionBuilder {         // flui-widgets
    fn duration(&self) -> Duration;
    fn build(&self, ctx: &BuildContext, primary: &RouteAnimation, secondary: &RouteAnimation,
             child: BoxedView) -> BoxedView;
}
pub struct PageTransitions(Rc<dyn PageTransitionBuilder>);  // ambient InheritedView

#[non_exhaustive]                         // flui-material
pub enum PageTransition { FadeForwards, Zoom, Cupertino, SharedAxis(SharedAxis), FadeThrough, None,
                          Custom(Rc<dyn PageTransitionBuilder>) }
pub struct PageTransitionsTheme { .. }    // TargetPlatform -> PageTransition, в ThemeData
pub struct OpenContainer<T> { .. }        // closed/open builders, ContainerTransition::{Fade, FadeThrough}
```

| Переход | Длительность | Кривая |
|---|---|---|
| FadeForwards (Android) | 450 мс | `EASE_IN_OUT_CUBIC_EMPHASIZED` |
| Cupertino (iOS/macOS) | 500 мс | общий сдвиг из `flui-widgets` |
| Zoom (Windows/Linux/web) | 300 мс | две cubic-фазы |
| SharedAxis, FadeThrough | 300 мс | интервалы 0.3/0.7 и 6/20–14/20 |
| Диалог | 150 мс | fade |
| OpenContainer | 300 мс | `FAST_OUT_SLOW_IN` |

Параллакс-сдвиг iOS переезжает из `flui-cupertino` в `flui-widgets` (ADR-0221 D4): его
используют и маршрут `flui-cupertino`, и вариант `Cupertino` карты Material — одна копия, без
зависимости между пакетами.

Помощник места-заглушки (`navigator/flight_seat.rs`, `pub(crate)`): замер прямоугольника в
координатах навигатора и замороженное место источника. Сначала на него переходит `Hero` (его
тесты не меняются), потом его использует `OpenContainer`. Пакет получает помощник через
`flui-sdk` только если `OpenContainer` в `flui-material` без него не обойдётся — иначе
`OpenContainer` строится на публичных `Overlay` и `RenderId`-замере.

### Рябь и контролы (`flui-material`)

- `InkWell` владеет слоем состояния (`AnimatedValue<f64>` альфы) и короткой очередью волн;
  красит под ребёнком, обрезая своей формой (bounded) или радиусом (unbounded).
- `RippleConfiguration { color: Option<Color>, alpha: StateLayerAlpha }` — ambient;
  `StateLayerAlpha` хранит токены M3.
- Волна: стартовый радиус `0.3 × max(w, h)`, конечный — до углов + 10 dp; 75/225/150 мс.
- `Switch`, `NavigationBar` — ядро C3. `TabController`:

```rust
impl TabController {
    pub fn index(&self) -> usize;
    pub fn position(&self) -> impl Animation<f64>;   // дробная, непрерывная
}
enum PositionSource { Own(AnimatedValue<f64>), Pager(PageController) }
pub enum TabIndicatorAnimation { Linear, Elastic }
```

### Скелетоны (`flui-widgets::motion`)

```rust
Shimmer::new(child).period(Duration::from_millis(1500)).direction(AxisDirection::LeftToRight);
Skeleton::rect(size); Skeleton::line(width_fraction); Skeleton::circle(diameter); Skeleton::text(lines);
```

`Shimmer` публикует фазу ambient-значением; `Skeleton` при paint получает смещение до
ближайшего `Shimmer` и сдвигает градиент на него. Если смещения до предка при paint получить
нельзя, `Shimmer` кладёт слой начала градиента — проверка первой задачей группы S.

## Тестовые швы

Один стол на группу, через публичный API:

- **W:** `flui-widgets` `tests/` — строка на виджет: прерывание под пружиной без скачка
  скорости, слияние `duration`/`curve`, reduce motion в первом кадре; `AnimatedSize` — размер
  строго между на половине.
- **E:** арифметика шкалы — чистый стол в `flui-animation`; слияние `Transition`; виджетные
  строки `OnChange`/`Target`/reduce motion.
- **L:** удаление (призрак виден на половине, исчез после), возврат ключа (тот же элемент —
  счётчик `dispose`, прозрачность растёт от ~0.5), перестановка (смещения строго между),
  дубликат, ленивый вход из-за экрана без размещения.
- **R:** под темой с FadeForwards на Android маршрут ещё в переходе через 300 мс (отличает 450 мс
  от прежних 300), значение на 225 мс совпадает с `EASE_IN_OUT_CUBIC_EMPHASIZED(0.5)` в пределах
  допуска (отличает кривую), через 450 мс переход завершён; так же для каждого умолчания и
  варианта — граница длительности и значение кривой; без темы — jump cut; строка на умолчание платформы; `Custom`; `OpenContainer` — прямоугольник
  между, место держит размер, результат pop, перемер после прокрутки; тесты `Hero` без
  изменений.
- **M:** записанный display list `InkWell` — альфа hover, радиус между, отпускание на 30 мс
  видно на 75 мс, reduce motion без круга, два `InkWell` не пересекаются; `Switch` — на
  половине длительности центр бегунка строго между крайними положениями и зеркален в RTL,
  цвет дорожки строго между; `NavigationBar` — индикатор нового выбора на промежуточном кадре
  частично раскрыт, прежний частично свёрнут; `TabController` — перетаскивание на 0.25,
  эластичная ширина в середине больше концов, RTL. Каждая строка падает при снапе.
- **S:** два `Skeleton` в одном `Shimmer` — сдвиги градиента отличаются ровно на их x; reduce
  motion — сплошной цвет.

Каждый новый `RenderBox` — строка `RENDER_OBJECT_TYPES` и `harness_*`.

## Риски

- Движение из layout (`AnimatedSize`, FLIP) идёт по ADR-0013: владелец в состоянии view,
  render object только слушает и ретаргетит; ни один не освобождает владельца в layout.
- Морф формы `OpenContainer` ограничен радиусами `BorderRadius::lerp`.
- Хук переходов — межкрейтовый контракт: ADR и строки `flui-sdk` `tests/surface.rs`.
