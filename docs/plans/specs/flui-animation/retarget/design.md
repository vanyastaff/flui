# retarget — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Требования:** [requirements.md](requirements.md) (R1–R21)
- **ADR:** да — «Retarget сохраняет значение и скорость» (черновик ниже)
- **Границы:** построение и вычисление пружины (`SpringSimulation::new`, `x`, `dx`, `is_done`,
  конечность при больших t/ζ) — [physics](../physics/design.md); раздача статуса и порядок
  реентерабельных переходов — [listener-delivery](../listener-delivery/design.md);
  `DrivenController` и якорь Vsync при переносе — [ownership](../ownership/design.md); контракт
  `Curve` (концы, валидация) — [curves](../curves/design.md). Здесь — только шов. Ответ на
  вопрос владельцу 2 спеки physics: `AnimatedValue` остаётся — это движок implicit-режимов ниже;
  компоненты строятся через `SpringSimulation::new(.., tolerance) -> Result` из physics.

## Текущее состояние (чтением)

- Implicit: `ImplicitAnimation::retarget` (`crates/flui-widgets/src/animated/implicitly_animated.rs:242-254`)
  → `Tween::new(current, target)` + `restart_from_zero` (`:149-153`) = `forward_from(Some(0.0))`.
  Скалярный прогресс 0→1 через `CurvedAnimation`; векторной скорости нет. `OptTween`
  (`:268-309`) — то же для `AnimatedContainer`.
- `velocity()` (`crates/flui-animation/src/controller.rs:1766-1787`): simulation → `dx(cycle)`;
  time-based → `(target − start)/duration`. `cycle` — elapsed последнего сэмпла
  (`cycle_elapsed_secs`, `:2520`).
- Time-based тик: `start + (target − start)·curve(t)` (`:1927-1939`), кривая — `Arc<dyn Curve>`.
  У `Curve` только `transform` (`curve.rs:32-36`); у `Cubic` есть аналитические `x(s)`, `x'(s)`
  (`curve.rs:251-268`) и Newton-solver (`:281-312`).
- Vsync анкерит новый run на следующем тике (`vsync.rs:484-489`) — кадр «стоит» в точке шва.
- `fling` строит `SpringSimulation::new(spring, value, bound ± 0.01, velocity)` (`controller.rs:1626-1627`),
  скорость — в единицах контроллера в секунду.
- `AnimatedValue<T>` (`spring.rs:100-181`): покомпонентные пружины, retarget с x/dx
  (`:133-143`), внешний `advance(dt)`; production-пользователя нет, тест одного шага (`:191`).

## Варианты для implicit

| | A. Curve + CSS reversal | B. Spring (векторная) | C. `MotionSpec` = Curve(Hermite) \| Spring (выбран) |
|---|---|---|---|
| C⁰ | да | да | да |
| C¹ | нет: скорость сбрасывается (CSS/Flutter) | да, точно (аналитический `dx`) | да в обоих режимах для `TwoWayConverter`-свойств |
| Контроль дизайнера | длительность и кривая | только физика | оба |
| Разворот | сокращение §3.1 | естественный | §3.1 в Curve, естественный в Spring |
| Цена | минимальная | меняет смысл `.duration().curve()` | новый enum, сегмент Hermite |

**Hermite-сегмент кривой** (собственное решение, не заимствовано): по компонентам
`x(τ) = a + (b − a)·c(τ) + r·D·τ(1 − τ)² + s·τ²(1 − τ)`, `τ = t/D`,
`r = v₀ − (b − a)·c'(0)/D`, `s = (b − a)·c'(1)`. Тогда `x(0) = a`,
`x'(0) = v₀` (C¹), `x(1) = b`, `x'(1) = 0`,
`|x − x_curve| ≤ (|r|·D + |s|)·4/27`. При согласованной стартовой скорости
и нулевом конечном наклоне сегмент совпадает с обычным curved-run.
Приход в покой — явное уточнение первоначального черновика: ненулевая конечная
скорость кривой всё равно обрывалась бы при завершении контроллера. Две Hermite-поправки
сохраняют непрерывность и на шве прерывания, и на границе покоя. Почему не аддитивность
(SwiftUI `shouldMerge`, CA `isAdditive`): владелец вынес аддитивность из scope при условии, что
retarget с x и v покрывает прерывание; Hermite делает это без стека анимаций.

**Выбор C.** Скалярный прогресс сохранить C¹ не может: старая векторная скорость не параллельна
новому пути `current → target`. Поэтому implicit-свойства с `TwoWayConverter` идут покомпонентно
через `AnimatedValue<T>`; свойства без вектора (`BoxDecoration`) — скалярный прогресс, C⁰ +
сокращение §3.1 (R20). Default-режим implicit-виджетов — решение владельца (ниже).

## Варианты точной скорости кривой (D-36)

| | Аналитика у `Cubic`, численно у прочих (выбран) | Только центральная разность | Только аналитика |
|---|---|---|---|
| Точность | `Cubic`: `y'(s)/x'(s)` до точности solver; прочие: O(h²), h = 1e-4 → ~1e-8·|c'''| | ~1e-8, но на концах односторонняя O(h) | неполно: `Arc<dyn Curve>` пользователя |
| Концы | `x'(s) = 0` (ease-out, x1 = 0): предел `y''(s)/x''(s)`; если и он 0/0 — численно | — | — |

Новый provided-метод `Curve::slope(&self, t) -> f64` (контракт: `d transform/dt`, конечен;
default — центральная разность с шагом `h = 1e-4`, у концов — односторонняя второго порядка
`(−3f(t) + 4f(t±h) − f(t±2h))/(2h)`, неконечный результат → 0). Переопределяют: `Linear` (1),
`Cubic` (аналитика), `Steps` (0), стёртая кривая `ArcCurve` (делегирует),
`FlippedCurve`/`Interval` (цепное правило). Метод вводит тема curves (владелец `curve.rs`) первой
(решение X13); composition (касательные keyframes) и retarget его только потребляют.

## Решение

### Публичный API (дельта)

```rust
// flui-animation
/// How a value moves toward a new target. `#[non_exhaustive]`: keyframes may follow.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum MotionSpec {
    /// Over `duration` along `curve`; a segment that starts with velocity bends by a Hermite
    /// term that vanishes at both ends. A reversal shortens `duration` (CSS Transitions §3.1).
    Curve { duration: Duration, curve: ArcCurve },  // PartialEq: встроенные кривые — по значению,
                                                   // пользовательские — по идентичности (X4)
    /// A damped spring (physics spec); inherited velocity is kept, time-defined or not.
    Spring(SpringDescription),
}

impl AnimationController {
    /// Starts a segment toward `target` from the value and velocity of the last sample;
    /// the segment's `t = 0` is that sample's instant. Cancels the displaced run's future
    /// (`RunCanceled`) without an intermediate settled status.
    /// # Errors `Disposed`; `NonFiniteInput { Target | Velocity }`, `SpanOverflow` before any mutation.
    pub fn retarget(&self, target: f64, motion: &MotionSpec) -> Result<RunFuture, AnimationError>;
    /// `fling` with a gesture velocity in px/s over an extent of `extent` px:
    /// `velocity · (upper − lower) / extent` controller units per second.
    /// # Errors `InvalidExtent` (extent ≤ 0 or non-finite), plus `fling`'s.
    pub fn fling_across(&self, velocity: f64, extent: f64) -> Result<RunFuture, AnimationError>;
}
// velocity(): curved runs return (target − start)·curve.slope(τ)/D (D-36).

/// A value of any `TwoWayConverter` type moved per component by a `MotionSpec`,
/// ticked by its own `DrivenController`. Replaces the `advance(dt)` engine primitive.
pub struct AnimatedValue<T: TwoWayConverter> { /* driven: DrivenController, state: Rc<SegmentCell<T>> */ }
impl<T: TwoWayConverter> AnimatedValue<T> {
    pub fn new(initial: T, motion: MotionSpec, vsync: Option<&Vsync>) -> Result<Self, AnimationError>;
    pub fn animate_to(&mut self, target: T) -> Result<RunFuture, AnimationError>; // C⁰ + C¹
    pub fn set_motion(&mut self, motion: MotionSpec);   // R21: next segment, from (x, v)
    pub fn snap_to(&mut self, value: T);               // velocity 0, run cancelled
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError>;
    #[must_use] pub fn value(&self) -> T;
    #[must_use] pub fn velocity(&self) -> T::Vector;
    #[must_use] pub fn target(&self) -> &T;
    #[must_use] pub fn is_settled(&self) -> bool;
    #[must_use] pub fn animation(&self) -> AnimatedValueView<T>; // impl Animation<T> + Listenable
}
```

`AnimationError::InvalidExtent` добавляется в `error.rs` через controller-robustness (владелец
файла; варианты — структурные, как остальные в её `AnimationError`). `TwoWayConverter` получает impl для `EdgeInsets`,
`Alignment` (в `spring.rs`, orphan-правило: трейт локален); внутренний `ContainerGeometry`
`AnimatedContainer` выводится `#[derive(TwoWayConverter)]` (переименование derive и вывод `Lerp` — R9, карточка derive/composition; production-пользователь derive; имя derive —
спека derive).

### Механика

1. **Сегмент — это run-simulation.** `retarget` строит crate-private `Segment` (enum `Spring` |
   `Curve{from, to, duration, curve, excess}`), реализующий `Simulation` (`x`, `dx`, `is_done`;
   `Curve` завершается при `t ≥ D` ровно в `to`). Значит `velocity()` сегмента — аналитический
   `dx`, а тик идёт существующим путём simulation (user-кривая вызывается без lock, сэмпл
   коммитится только при совпадении `SampleIdentity`, `controller.rs:1906-1922`).
2. **Захват шва.** Под одним lock: `value`, `cycle` последнего сэмпла, источник run (кривая или
   simulation) клонируется в `Opaque`; lock отпущен; вычисляются `v = slope/dx` (user-код);
   lock снова, проверка, что run не сменился (`run_generation`), иначе — повтор с новым швом
   (не более одного раза, дальше `AnimationError::ReentrantMotion` и `warn` без замены
   последнего установленного run). Отказ вместо первоначального `Fresh` с v = 0 сохраняет
   контракт C¹: повторная reentry не превращается в молчаливую потерю скорости. Устаревший
   результат источника, включая неконечную производную, отбрасывается до проверки результата
   и вызова новой кривой. Затем
   установка сегмента с `RunAnchor::Continue`.
3. **Якорь.** `walk_probe` отдаёт `anchor: RunAnchor { Fresh, Continue }`. Vsync хранит номер
   кадра (`frame: u64`, `checked_add`, насыщение) и для регистрации — кадр и `now` последнего
   тика. `Continue` при новом поколении: если регистрация тикалась в текущем или предыдущем
   кадре — `run_start = now последнего тика` (момент шва), иначе `run_start = now` (R12).
   Время — `AnimationTime` таймлайна поведения запуска (motion-clock, reduce-motion), неконечным
   быть не может; `RunAnchor` — общий для ownership (`Resume`) и retarget (`Continue`), его форму
   задаёт motion-clock T3 (см. `../review.md`). Manual-часы — вызывающий сам анкерит (документировано).
4. **Сокращение §3.1** живёт в `AnimatedValue` (Curve-режим): хранит `reversing_adjusted_start`
   и `shortening`; условие разворота — `target == reversing_adjusted_start` по `PartialEq` на
   векторе (точное равенство, как в CSS).
5. **`AnimatedValue`.** `DrivenController` (unbounded, `.behavior(Normal)` явно — X2) ведёт run-«часы»: simulation
   `x(t) = t`, `is_done(t)` = все компоненты в покое (`|Δ| ≤ ε`, `|v| ≤ ε·ω` — физика). Компоненты
   (`SmallVec<[Segment; 4]>`) — в `SegmentCell` поверх `share::{Shared, StateCell}` frame-path-state
   (до F3 — `Arc`/`Mutex`, после — `Rc`/`RefCell`; guard не держится через `from_vector`). Вектор
   копируется из ячейки, borrow отпущен, затем `T::from_vector` (user-код). Аллокации — только на
   retarget (снимок сегментов, стёртый один раз: `Rc<dyn Simulation>` после F3, R8), не на тик.
6. **Implicit.** `ImplicitAnimation<T>`: для `T: TwoWayConverter` — `AnimatedValue<T>`; иначе —
   скалярный прогресс + `Tween` + §3.1. `AnimatedOpacity` отдаёт `AnimatedValueView<f64>` в
   `ProxyAnimation` (рендер-объект тот же). Билдер виджета: `.duration(d)`, `.curve(c)` →
   `MotionSpec::Curve`; новый `.spring(s)` → `MotionSpec::Spring`.
7. **Жесты.** `Dismissible`: `fling_across(v, overall_extent)` со знаком направления вместо
   `|v|·1/300` (удалить `FLING_VELOCITY_SCALE`, `dismissible.rs:107`); Drawer: `fling_across(v·dir,
   width)` вместо ручного деления (`drawer.rs:617-619`); back gesture — по решению владельца.

### Инварианты

1. На шве значение и скорость нового сегмента — это значение и скорость последнего сэмпла
   старого; `t = 0` сегмента — момент этого сэмпла (кроме простоя, R12).
2. Retarget не публикует промежуточный settled-статус; future старого run — `RunCanceled`
   после того, как статус нового наблюдаем (порядок ADR-0064, `controller.rs:2411-2421`).
3. Публикуемые значения конечны: неконечные цель/скорость/span отвергаются до мутации; сегмент,
   давший неконечный сэмпл, завершается на последнем конечном (путь `tick_simulation`, `controller.rs:1956-1962`).
4. Ни lock контроллера, ни borrow ячейки не держатся при вызове `Curve::slope/transform`,
   `Simulation::x/dx/is_done`, `TwoWayConverter::from_vector`, слушателей.
5. Новых `static` нет. Время сегмента — `Duration` в API и на границе Vsync/контроллера
   (motion-clock); `f64` секунд — только внутри `Simulation::x/dx/is_done` (physics).

## Миграция

| Где | Что |
|---|---|
| `flui-widgets/src/animated/implicitly_animated.rs` | `ImplicitController`/`restart_from_zero` → `AnimatedValue` / прогресс с §3.1; `OptTween` — поверх тех же сегментов |
| `animated_opacity.rs`, `animated_padding.rs`, `animated_align.rs`, `animated_container.rs` | `.spring()`; `ContainerGeometry` с derive |
| `interaction/dismissible.rs:107,1175,1180` | `fling_across` |
| `packages/flui-material/src/drawer.rs:617-619` | `fling_across` |
| `navigator/back_gesture.rs:148,154-184` | решение владельца |
| `scroll/scroll_controller.rs:463-516` | `set_value` + `animate_to_curved` → `retarget(.., Curve)`: прерванная прокрутка сохраняет скорость |
| `flui-animation/src/spring.rs` | `advance(dt)` удаляется; бенч `animated_value_color_frame` (`benches/animation_bench.rs:121`) — через `Vsync::tick_all` |
| `flui-sdk/tests/surface.rs` | `MotionSpec`, `AnimatedValue` (новая форма) — строки pinned-списка (ADR-0088 §4) |

## Adversarial review

- **Reentry слушателя в контроллер и реестр.** `retarget` из status/value-слушателя этого же
  контроллера: шов берётся вне lock (механика 2), установка — под lock, `finish` раздаёт после
  unlock; реентерабельный порядок статусов — listener-delivery. Retarget из слушателя другого
  контроллера в середине `tick_all`: курсор Vsync уже прошёл/не прошёл — якорь `Continue` всё
  равно ставит `t = 0` на момент шва; если запись ещё впереди в этом кадре, она тикается с
  `elapsed = now − T` (ноль, если шов в этом же кадре). Тест R16.
- **Снятие/добавление слушателей во время уведомления.** Не меняется этой спекой.
- **Последний владелец из колбэка.** `AnimatedValue` дропается из слушателя своего контроллера →
  `DrivenController` (ownership R6); `SegmentCell` жив, пока жив хоть один `AnimatedValueView`.
- **Два контроллера на одном Vsync.** Якоря независимы (поле регистрации); номер кадра общий.
- **Realm остановлен.** Сегменты — чистые функции времени; drop handle отменяет future (ownership).
- **Retarget в последнем кадре / в t = 0 / после простоя.** R11, R10, R12 — строки таблиц.
- **dt = 0, огромный dt, время назад.** `elapsed = max(0, now − T)`; пружина — аналитика
  (physics гарантирует конечность при больших t), Hermite-сегмент при `t ≥ D` возвращает `to`.
- **Переполнение и NaN.** `r = v₀ − Δ·c'(0)/D`: `D → 0` → `r·D·h` конечно, но `r` огромно —
  сегмент с `D < 1 ms` заменяется settle в цель (как zero-duration сейчас). `Δ` переполняет →
  `SpanOverflow`. `slope` неконечен → 0 по контракту. Пружина с `4mk` overflow — physics D-39.
- **Panic в пользовательском коде.** `slope`/`dx` на шве (R15): старый run установлен, panic
  распространяется без lock. `from_vector` в `value()`: borrow отпущен. Слушатели —
  listener-delivery.
- **Остаточные риски.** (1) Hermite-перелёт `( |r|·D + |s| )·4/27` может вывести значение за пределы
  ограниченного контроллера — `retarget` на bounded контроллере клампит (C¹ рвётся на границе,
  документировано). (2) Покомпонентный C¹ для `Color` — в пространстве `to_vector`, который
  interpolation переводит в premultiplied Oklab `(L·α, a·α, b·α, α)` с точными концами (решение
  X5); до её слияния — sRGB u8. (3) Наследование скорости time-defined пружиной
  может давать заметный перелёт на малом диапазоне (Motion поэтому отказался, market-B I6) —
  осознанно ради C¹ (R9).

## Решения владельца (orchestration, 2026-10-06)

1. Default implicit-виджетов — `Curve(EaseInOut, 200 ms)` с непрерывной скоростью (Hermite).
2. Back gesture — settle по скорости пальца и оставшемуся пути (`fling_across`) вместо
   фиксированных 350 ms; вариант `PopPacing` со скоростью (`navigator/binding.rs:277`).

## Владение

| Объект | Сильные | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `AnimatedValue<T>` | поле state implicit-виджета | — | drop → `DrivenController` (ownership): unregister, dispose, future `Err(RunCanceled)` |
| `DrivenController` внутри | `AnimatedValue` | — | `rebind` из `did_change_dependencies` |
| `SegmentCell<T>` (`Shared<StateCell<_>>`) | `AnimatedValue`, каждый `AnimatedValueView`, run-симуляция | — | последний из них |
| `AnimatedValueView<T>` | `ProxyAnimation` render object'а | — | `detach`/`set_parent` |

Циклов не вводится: симуляция сегментов держит `SegmentCell`, но не контроллер и не view;
слушатель, захвативший `AnimatedValue`, — C1-форма, разрывается `dispose` handle (ownership,
controller-robustness R6.3); «Drop == 1» — `animated_value_drop_releases_segments` (Weak-сентинел
`SegmentCell` после drop виджета и view). Порядок замков до T6c: контроллер → `SegmentCell`;
`from_vector` и кривые — после отпускания обоих (инв. 4).

## Паттерн

- **Закрытый `#[non_exhaustive]` enum** — `MotionSpec { Curve, Spring }`; трейт режима не вводится.
- **Сегмент как `Simulation`** — crate-private enum `Segment` реализует открытый трейт, тик идёт
  существующим путём симуляции (стирание один раз, R8).
- **Ассоциированный тип** — `T::Vector` из `TwoWayConverter` для скорости.
- **RAII handle** — `DrivenController` внутри `AnimatedValue`.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `restart_from_zero` + новый `Tween` (Flutter `ImplicitlyAnimatedWidget`) | сброс скорости | сегмент из (x, v), Hermite-член |
| `AnimatedValue::advance(dt)` | Unity/игровой `Update(dt)` | ведомый `Vsync`, без внешнего `dt` |
| аддитивные анимации (SwiftUI `shouldMerge`, CA `isAdditive`) | стек анимаций | retarget с x и v (scope-решение) |
| `|v|·1/300` в Dismissible | магическая константа Flutter | `fling_across(v, extent)` |

## Черновик ADR

**ADR-NNNN: Retarget сохраняет значение и скорость.** Решение: прерывание движения новой целью
начинает сегмент из значения и скорости последнего сэмпла; `t = 0` сегмента — момент этого
сэмпла (Vsync-якорь `Continue`), кроме простоя дольше кадра. `MotionSpec::{Curve, Spring}` —
оба режима C¹: пружина наследует скорость (и time-defined тоже), кривая получает Hermite-член
`r·D·τ(1−τ)²`, исчезающий на концах; разворот к исходной точке в Curve-режиме сокращает
длительность по CSS Transitions §3.1. `velocity()` curved-run — производная кривой
(`Curve::slope`). Скорость жеста передаётся в settle нормированной на протяжённость
(`fling_across`). Аддитивные анимации не вводятся (scope-решение владельца). Следствия:
implicit-виджеты, Dismissible, Drawer, `ScrollController::animate_to` непрерывны по скорости;
`AnimatedValue` становится ведомым Vsync и теряет `advance(dt)`.

## Changelog

```markdown
### Added
- `MotionSpec` and `AnimationController::retarget`: interrupting an animation keeps its value
  and velocity; `fling_across` hands a gesture's px/s velocity to the settle animation.
- Implicitly animated widgets accept `.spring(..)`.
### Changed
- `AnimatedValue` is ticked by a `Vsync` (`new(initial, motion, vsync)`); `advance(dt)` is removed.
- Implicit animations retarget with continuous velocity; a reversal shortens the run.
### Fixed
- `AnimationController::velocity` follows the run's curve.
- A dismissed card leaves at the finger's speed on any width.
```

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| `PartialEq` спеки — по значению (X4) | нарушала | design.md:79-83 (`ArcCurve` = `ptr_eq`) | значение для встроенных, идентичность для пользовательских |
| Одна реализация производной (X13) | нарушала | design.md:64-69; tasks T2 | `Curve::slope` вводит curves T9 |
| Цвет в Oklab premultiplied (X5) | нарушала | design.md:210-211 | `TwoWayConverter for Color` — interpolation |
| `behavior` явно (X2) | нарушала | design.md:146 | `AnimatedValue` — `Normal` |
| Имена после удаления `Ticker` | нарушала | design.md:91-106, R-тексты | `RunFuture`/`RunCanceled` |
| Simulation стирается один раз (R8) | нарушала | design.md:151 `Arc<dyn Simulation>` | `Rc<dyn Simulation>` после F3 |
| Derive по имени трейта (R9) | нарушала | design.md:121 `#[derive(Animatable)]` | `#[derive(TwoWayConverter)]` |
| Решения владельца применены | нарушала | design.md:215-222 | раздел «Решения владельца» |
| Закрытый enum, `#[non_exhaustive]` | соответствует | `MotionSpec` | — |
| Ошибки структурные до мутации | соответствует | `retarget`, `fling_across` | — |
| Ни lock, ни borrow при user code | соответствует | инв. 4 | — |
| Таблица владения | нарушала | — | раздел «Владение» |
