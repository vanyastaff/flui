# flui-animation / physics — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)
- **ADR:** не нужен (см. «ADR»); фрагмент `changelog.d/` — ниже.

## Итог

Пружина хранится как валидированная пара `(ω, ζ)` с приватными полями и fallible-конструкторами
(`Result<_, SimulationError>`); вычисление — единая форма C/S-пропагатора без ветвей по режиму, с рядом
у ζ=1 и устойчивыми корнями overdamped. Покой — не мгновенная выборка, а момент `rest_time`, вычисленный
при построении по консервативной огибающей: `is_done` монотонна, `x(t ≥ rest_time) = end`, вывод конечен
на `[0, +inf]`. Допуск scroll — полпикселя устройства (dpr из pipeline owner через `ScrollMetrics`).
Fling контроллера завершается в момент пересечения границы. Новая `BouncingScrollSimulation` передаёт
трение пружине на краю. Неподключённая поверхность удаляется (список ниже).

## Текущее состояние (HEAD 9a4daa3ed, чтением)

- `SpringDescription` — `pub` поля `mass/stiffness/damping` (`simulation.rs:121-128`): литерал обходит
  проверки. `new`/`with_damping_ratio` паникуют `assert!` (`:137-176`); перцептивные —
  только `debug_assert!` (`:186`, `:215`), `bounce` и `damping_fraction` не проверяются (`:183-225`):
  `bounce=1` — незатухающая, `bounce>1`/`fraction<0` — экспоненциальный рост, `bounce=−2` —
  отрицательное затухание, классифицированное как Critical (`:298-312`) и принятое fling.
- Режим выбирается раз (`:430-448`); формы верны (сверено с RK4 в ledger зоны 3), но `4mk` переполняется
  (`:529`, `:564`), `x(+inf)` = NaN у critical/underdamped (`:497`, `:572`), overdamped вычисляет
  `c_i·e^{r_i t}` раздельно — при больших t безопасно, но неустойчиво у ζ→1⁺ (сокращение `c1≈−c2`).
- Покой: мгновенная выборка `|x|<distance ∧ |dx|<velocity` (`:411-414`), немонотонна; snap
  (`:396-409`) поэтому может мигать. `Tolerance::DEFAULT` 1e-3 (`:42-46`) на пикселях: scroll-хвосты
  ~190 кадров у fling, ~55 у page-snap. `Tolerance.time` и `Simulation::tolerance()` никем не читаются.
- Fling: `FLING_TOLERANCE` (`controller.rs:27-31`) влияет только на цель (`:1613-1617`); симуляция
  строится с `DEFAULT` (`:1627-1628`) → конец через ~0.26 с после границы.
- `BouncingScrollPhysics` внутри диапазона — `BoundedFrictionSimulation` (`scroll_physics.rs:349-355`):
  fling в край останавливается намертво. Пружина края — `ScrollSpringSimulation` (`:330`, `:338`;
  `page_view.rs:154`), сквозная обёртка над `SpringSimulation` (`simulation.rs:813-843`) с ложным
  обоснованием в doc.
- Трение: `drag` проверен `assert!` (`:616-619`); `through` (`:684-710`) паникует на underflow drag и с
  `end_velocity = 0` не завершается (`:705-708`, `:730`). `Bounded`/`Clamped` вызывают `f64::clamp` с
  непроверенными границами (`:889`, `:892`, `:934`) — panic на NaN или `min > max`.
- `smoothing.rs`: `max_speed < 0` → panic в `clamp` (`:193-196`), полином вместо `exp` (`:187`), `dt=inf`
  → вечный NaN. `AnimatedValue` (`spring.rs:100-181`): `advance(f64)` (`:156-158`) копит NaN навсегда.
- Production-пользователей нет у: `smoothing::*`, `AnimatedValue`, `GravitySimulation`,
  `ClampedSimulation`, `FrictionSimulation::through`, `SpringSimulation::{with_tolerance, end_position}`,
  `SpringDescription::{bounce, damping_ratio, smooth, snappy, bouncy}` (ledger зоны 3, §1; `rg` по
  `crates/`, `packages/`, `src/`).
- dpr: `ScrollPhysics` получает только `ScrollMetrics` (`scroll_physics.rs:44-53`). `MediaQuery`
  (`app/media_query.rs:173`) недоступен из `scroll`: `module-dag` ставит `app` выше `scroll`
  (`flui-widgets/Cargo.toml:236-283`). `PipelineOwner::device_pixel_ratio` (`flui-rendering
  pipeline/owner/accessors.rs:434`) всегда конечен и > 0 (`:1107-1117`); `PipelineCell::try_with`
  (`cell.rs:107`) и `downgrade` (`:91`) уже используются виджетами (`interaction/interactive_viewer.rs:463-468`).

## Варианты

### (a) Представление и вычисление пружины

1. **Оставить три режима, добавить проверки.** Минимальная правка; остаются ветка по классификации,
   `4mk`, NaN при `t=inf`, разрыв точности у ζ=1⁺.
2. **Единая форма C/S в `(ω, ζ)`** (market-C §2.1, так у GPUI): `x = e^{−at}[x0(C + aS) + v0 S]`,
   `v = e^{−at}[−ω²x0 S + v0(C − aS)]`, `a = ζω`, `q = ω²(1−ζ²)`; C, S — ряд при `|q t²| < 1e-2`, иначе
   cos/sin (q>0) или экспоненты с устойчивыми `λ_s = −ω/(ζ+√(ζ²−1))`, `λ_f = −ω(ζ+√(ζ²−1))` в
   слитой форме `½(e^{λ_s t} ± e^{λ_f t})` (q<0). Масса сокращается: движение определяется `(ω, ζ)`.
3. **Численный интегратор (RK4/semi-implicit).** Зависит от разбиения на кадры — противоречит R8.

**Выбор: 2.** Нет порога классификации, нет `4mk`/`c²`, конечно для `t → ∞`, непрерывно по ζ.
`ζ+√(ζ²−1)` считается как `ζ(1+√(1−ζ⁻²))` для ζ > 1 (нет `ζ²` overflow). `SpringType` сохраняется
(`ζ < 1`, `= 1`, `> 1` по хранимому ζ; `with_damping_ratio(…, 1.0)` хранит ζ = 1.0 точно).

### (b) Критерий покоя

1. **Мгновенная выборка** (сейчас, Flutter): немонотонна, snap мигает, хвосты зависят от единиц.
2. **`rest_time` по консервативной огибающей**, вычисленный при построении. Для всех ζ
   `e^{−at}|C + aS| ≤ (1 + a t)e^{−rt}`, `e^{−at}|S| ≤ t·e^{−rt}`, где `r = a` (ζ ≤ 1) или `−λ_s` (ζ > 1);
   отсюда `E_x(t) = e^{−rt}(|x0|(1 + at) + |v0|t)`, `E_v(t) = e^{−rt}(ω²|x0|t + |v0|(1 + at))`. Для ζ < 1
   дополнительно амплитудная оценка `A e^{−at}`; берётся меньший из двух валидных моментов.
   `rest_time = max(T_x, T_v)`; каждый — бисекция на хвосте огибающей (после максимума), ≤ 128 итераций.
3. **Выборка + защёлка в контроллере.** Состояние в `Simulation` противоречит «значение без
   блокировок» (frame-path-state) и не чинит прямых пользователей.

**Выбор: 2.** `is_done(t) ⇔ t ≥ rest_time` — монотонна, детерминирована, не зависит от кадров;
`x(t ≥ rest_time) = end`, `dx = 0` (скачок ≤ `distance` по построению — snap всегда, `with_snap_to_end`
не нужен). Цена — консервативность: по ledger оценка позже фактического конца на ≤ 11 % (ζ=0.5:
1.410 против 1.270 с); R7 ограничивает её сверху.

### (c) Масштаб допуска и dpr для scroll

1. **`ScrollMetrics.device_pixel_ratio`, dpr читается из pipeline owner в момент отпускания.**
   `ScrollableState` берёт `ctx.pipeline_owner()` в `init_state`/`did_change_dependencies`, хранит
   `WeakPipelineCell`; в `on_pan_end` — `upgrade()?.try_with(PipelineOwner::device_pixel_ratio)`,
   иначе 1.0. Borrow отпущен до вызова физики.
2. **`MediaQuery::device_pixel_ratio_of` в `build`.** Запрещено `module-dag` (`app` выше `scroll`);
   перенос `MediaQuery` вниз — отдельная задача.
3. **Параметр допуска в `ScrollPhysics`.** Каждая физика хранила бы dpr — один `Arc<dyn ScrollPhysics>`
   делится между окнами с разным dpr.

**Выбор: 1.** dpr — свойство места отрисовки, а не физики; снимок `ScrollMetrics` — уже вход физики.
Чтение в момент жеста даёт актуальный dpr после переноса окна на другой монитор.

### (d) Скорость в допуске

`Tolerance` хранит `distance` и `velocity: Option<f64>` (приватно): `Some(v)` — абсолютный порог,
`None` — производный от динамики (пружина `distance·ω`, трение — остаток пути `|v/ln d| ≤ distance`).
Отдельный enum не вводится: два публичных конструктора выражают оба смысла. Fling: `Tolerance::new(0.01,
f64::INFINITY)`. `DEFAULT` = `new(1e-3, 1e-3)` (поведение не-scroll пользователей не меняется).

### (e) Fling контроллера (D-14)

Fling строит пружину к `bound ± 0.01`, и `rest_time` = первый момент пересечения `bound` (сканирование
`[0, T_x]` шагом `1/(8ω)` и бисекция до 1e-12 с); для underdamped fling по-прежнему отказывает. Это
`pub(crate) fn SpringSimulation::until_crossing(bound)`; контроллер меняет одну строку в
`fling_with`. Эталон: 0.295369722567422 с, вместо 0.560 с сейчас.

### (f) BouncingScrollSimulation (D-15)

Позиция вне диапазона → пружина к ближайшему краю. Внутри: трение `drag`; если `final_x` внутри
`bounds` — только трение (покой по остатку пути). Иначе `t_edge = time_at_x(edge)`, `v_edge = dx(t_edge)`,
пружина от `edge` к `edge` со скоростью `v_edge`; `x(t) = friction.x(t)` при `t < t_edge`, иначе
`spring.x(t − t_edge)`; `rest_time = t_edge + spring.rest_time`. Позиция и скорость непрерывны в `t_edge`
по построению. Флаттеровская схема (передача на краю), но покой и вычисление — свои. Платформенный
spline-decay и коэффициенты замедления iOS вне scope (решение владельца 2026-10-06): scroll покрывают
трение и пружина края. Spring-сегмент keyframes composition вынесла из прохода; `rest_time` остаётся
`pub(crate)`, пока владелец не утвердит этот сегмент (R17).

## Публичный API (дельта)

```rust
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SimulationError {
    #[error("{parameter} = {value} is outside its admitted range")]
    OutOfRange { parameter: SimulationParameter, value: f64 },
    #[error("spring constants overflow f64")]
    Overflow,
    #[error("bounds must be finite and ordered, got [{min}, {max}]")]
    InvalidBounds { min: f64, max: f64 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SimulationParameter { Mass, Stiffness, Damping, DampingRatio, Duration, Bounce,
    DampingFraction, Drag, Position, Velocity, Distance, DevicePixelRatio }

pub struct SpringDescription { /* omega: f64, zeta: f64 */ }
impl SpringDescription {
    pub fn new(mass: f64, stiffness: f64, damping: f64) -> Result<Self, SimulationError>;
    pub fn with_damping_ratio(mass: f64, stiffness: f64, ratio: f64) -> Result<Self, SimulationError>;
    pub fn with_duration_and_bounce(duration: Duration, bounce: f64) -> Result<Self, SimulationError>;
    pub fn with_response_and_damping(response: Duration, damping_fraction: f64) -> Result<Self, SimulationError>;
    pub const SMOOTH: Self; pub const SNAPPY: Self; pub const BOUNCY: Self; // условно, см. «Решения владельца»
    pub fn spring_type(&self) -> SpringType;
}
pub struct Tolerance { /* distance: f64, velocity: Option<f64> */ }
impl Tolerance {
    pub const DEFAULT: Self;
    pub fn new(distance: f64, velocity: f64) -> Result<Self, SimulationError>;
    pub fn displacement(distance: f64) -> Result<Self, SimulationError>;
    pub fn for_device_pixel_ratio(dpr: f64) -> Result<Self, SimulationError>;
}
pub trait Simulation { fn x(&self, t: f64) -> f64; fn dx(&self, t: f64) -> f64;
    fn is_done(&self, t: f64) -> bool; }          // tolerance() удалён; supertrait `Send + Sync`
                                                  // снимает send-flip T5 (frame-path-state), не эта тема
impl SpringSimulation {
    pub fn new(spring: SpringDescription, start: f64, end: f64, velocity: f64,
               tolerance: Tolerance) -> Result<Self, SimulationError>;
    pub(crate) fn rest_time(&self) -> Duration;    // pub — только вместе со spring-сегментом keyframes (R17)
    pub fn spring_type(&self) -> SpringType;
}
pub struct SimulationBounds { /* min, max */ }
impl SimulationBounds { pub fn new(min: f64, max: f64) -> Result<Self, SimulationError>; }
impl FrictionSimulation {
    pub fn new(drag: f64, position: f64, velocity: f64, tolerance: Tolerance) -> Result<Self, SimulationError>;
    pub(crate) fn final_x(&self) -> f64; pub(crate) fn time_at_x(&self, x: f64) -> f64; // нужны только Bouncing (тот же крейт)
}
impl BoundedFrictionSimulation {
    pub fn new(drag: f64, position: f64, velocity: f64, bounds: SimulationBounds,
               tolerance: Tolerance) -> Result<Self, SimulationError>;
}
pub struct BouncingScrollSimulation { .. }          // пользователь: BouncingScrollPhysics
impl BouncingScrollSimulation {
    pub fn new(spring: SpringDescription, drag: f64, position: f64, velocity: f64,
               bounds: SimulationBounds, tolerance: Tolerance) -> Result<Self, SimulationError>;
}
// flui-widgets
pub struct ScrollMetrics { …, pub device_pixel_ratio: f64 }  // #[non_exhaustive] уже есть
impl ScrollMetrics { pub fn with_device_pixel_ratio(self, dpr: f64) -> Self; }
```

Время в `Simulation` остаётся `f64` секунд от начала прогона: это область математики (определены `t<0`,
NaN, `+inf`), его зовёт контроллер с уже вычисленным `cycle`; `Duration` — на входе параметров и на выходе
`rest_time`. Rustdoc каждого конструктора: допустимая область, `# Errors` по вариантам, пример.

**Удаляется:** `ScrollSpringSimulation`, `GravitySimulation`, `ClampedSimulation`,
`FrictionSimulation::{through, with_tolerance}`, `SpringSimulation::{with_tolerance, with_snap_to_end,
end_position}`, `SpringDescription::{bounce, damping_ratio, smooth(), snappy(), bouncy()}`, поля `pub`,
`Tolerance.time`, `Simulation::tolerance`; `smoothing` целиком (решение владельца, orchestration «Решения по развилкам»).
`AnimatedValue` остаётся (движок implicit-режимов retarget; его переписывает retarget T4).

## Инварианты и владение

- Каждая симуляция — `Clone`-значение без interior mutability; стирается один раз на границе хранения (R8 ниже): `Arc<dyn Simulation>`
  (иммутабелен; после flip frame-path-state F3 — `Rc<dyn Simulation>`). Нет `static` (ADR-0097), нет блокировок, нет колбэков.
- Построение `Ok` ⇒ `x`, `dx` конечны ∀ `t ∈ [−inf, +inf] ∪ {NaN}` (t вне `[0, +inf)` → начальное
  состояние). Доказательство: все множители — конечные константы, умноженные на `e^{λt}` с `λ ≤ 0`
  и ограниченные C, S; проверяется property R5.
- `is_done` монотонна; `x(rest_time..) = end`.
- Конструктор не публикует частично построенное значение: все проверки до создания.

## Миграция вызовов (`rg` на HEAD)

| Где | Что | Кол-во |
|---|---|---|
| `flui-animation/src/controller.rs:22-31, 1626-1635` | default spring → константа `(ω=√500, ζ=1)`; `until_crossing`; `FLING_TOLERANCE` удалён | 2 места |
| `flui-animation/src/spring.rs:118,139,149` | `SpringSimulation::new(..)?`, без `with_snap_to_end` | 3 (если тип остаётся) |
| `flui-animation/src/controller_tests.rs:177,539`; `tests/contracts/controller_sources.rs:96,243,306,483` | удалить `fn tolerance` | 6 impl |
| `flui-animation/src/simulation.rs` тесты `:1001-1060` | переезд в `tests/contracts/spring.rs` | 1 таблица |
| `flui-animation/tests/contracts/simulation.rs` | `FrictionSimulation::new(..)?`, `SmoothDamp` (удаление/правка) | 7 вызовов |
| `flui-animation/benches/animation_bench.rs:81-121` | spring/AnimatedValue/smoothing | 4 |
| `flui-animation/examples/smoothing_follow.rs` | удалить вместе с `smoothing` | 1 файл |
| `flui-animation/src/lib.rs:131-136, 186-189` | реэкспорты (через владельца `lib.rs`) | 2 блока |
| `flui-widgets/src/scroll/scroll_physics.rs:249,290,330,338,349` | `Result`→`None`, `BouncingScrollSimulation`, dpr-допуск | 5 |
| `flui-widgets/src/scroll/page_view.rs:118,154` | `SpringSimulation::new(.., tol)` | 2 |
| `flui-widgets/src/scroll/scrollable.rs:497-521, 677-679` | `WeakPipelineCell`, `with_device_pixel_ratio` | 2 |
| `flui-widgets/src/scroll/refresh_indicator.rs:426, 542-569` | то же | 3 |
| `flui-widgets/tests/scroll.rs:143-175` | комментарии про `ScrollSpringSimulation` | 1 |
| docs: `flui-animation/README.md` (7), `docs/GUIDE.md` (7), `docs/PATTERNS.md` (1) | примеры конструкторов | задача Z |

`packages/` и facade `src/` вызовов не имеют (`rg SpringDescription|Simulation packages src` пусто).
`flui-sdk` реэкспортирует крейт целиком (`flui-sdk/src/lib.rs:27`); `crates/flui-sdk/tests/surface.rs`
не закрепляет ни одного удаляемого или переименуемого элемента физики (сверено `rg`) — каждая задача
повторяет проверку и мигрирует `packages/` в том же PR, если вызов появится. Ограничение скорости
fling ±8000 (`scrollable.rs:663-664`, `refresh_indicator.rs:562`) — владение задачи I3
flui-interaction; эта тема его не трогает. Порядок PR: после listener-delivery и
controller-robustness, вместе с curves, до motion-clock.

**Непроверенные факты рынка.** Ветка `b < 0` (`ζ = 1/(1+b)`) и `response → ω = 2π/response`
помечены в market.md как [U]: реализуются (текущий код уже так считает, ветка совпадает с Framer
Motion), документируются в rustdoc как непроверенные по первоисточнику и закрепляются только
тестами внутренних свойств (R3). Эталонные строки — только для `b ≥ 0`, воспроизводящих пример из
документации Apple `Spring`.

## Adversarial review

- **Reentry слушателя / vsync-реестр.** Симуляция не вызывает пользовательский код; `x/dx/is_done`
  зовутся контроллером. Пользовательская `impl Simulation` с panic/reentry — тема controller-robustness
  (`drive_simulation` уже сэмплирует `x(0)` вне lock, `controller.rs:1715-1736`).
- **Снятие/добавление слушателей, последний владелец из колбэка, два контроллера на одном vsync,
  остановка realm.** Неприменимо к значению; стёртая симуляция иммутабельна и может делиться
  двумя контроллерами без гонки — каждый передаёт свой `t`.
- **dt = 0, время назад, огромный dt, NaN t.** R6: начальное состояние / `end`; нет накопления.
  `AnimatedValue` тикает `Vsync` (retarget): внешнего `dt` нет, NaN и отрицательное время непредставимы.
- **Переполнение и NaN.** Пара `(ω, ζ)` из `√k/√m` и `c/(2√k√m)`; проверка конечности всех констант
  (`a`, `ω²`, `λ_s`, `λ_f`, амплитуды от `x0`, `v0`) в конструкторе симуляции → `Overflow`. Очень
  большой ζ: `λ_s ≈ −ω/(2ζ)` представим и не обнуляется (R5, ζ = 1e12); `rest_time` огромен, как
  требует физика, и насыщается до `Duration::MAX`, если не представим.
- **Retarget в последнем кадре.** Новая симуляция стартует из `x(t)`, `dx(t)`; после `rest_time` это
  `end`, 0 — непрерывно (скачок ≤ distance уже произошёл в момент покоя). C¹ retarget — тема retarget.
- **Panic в пользовательском коде на каждом вызове наружу.** Вызовов наружу нет. Бисекция в
  конструкторе ограничена 128 итерациями — нет зависания.
- **Fling у границы.** Значение уже на границе со скоростью наружу: пересечение при `t=0`,
  `rest_time = 0`, прогон завершается на первом тике (как и по смыслу). Тест — строка R11.
- **dpr недоступен / borrow занят.** `upgrade()` = None (презентация закрыта) или `try_with` = None
  (кадр держит `with_mut`) → 1.0: допуск 0.5 px — безопасная сторона для dpr ≥ 1; для dpr < 1 конец
  раньше на ≤ 0.5 логического px. Риск принят.
- **Остаточные риски.** (1) Консервативный `rest_time` удлиняет пружины до ~11–25 %, а `DEFAULT` с
  абсолютной скоростью 1e-3 может удлинить controller-пружины по сравнению с выборкой; R7 ограничивает
  сверху, бенч Z измерит. (2) Огибающая у ζ→1⁻ использует полиномиальную ветку — корректна, но менее
  точна, чем амплитудная; проверено R7 на ζ ∈ [0.99, 1). (3) dpr < 1 (масштабирование вниз) —
  допуск фиксирован в логических px.

## ADR

Не нужен: решение не меняет межкрейтовый контракт из ADR — меняется API `flui-animation`, единственный
потребитель (`flui-widgets`) мигрируется в том же PR, `ScrollMetrics` — тип `flui-widgets`. Решения
фиксируются в rustdoc и в `crates/flui-animation/docs/ARCHITECTURE.md` (задача Z).

## Фрагмент changelog

```markdown
### Changed
- `SpringDescription`, `Tolerance`, `FrictionSimulation`, `BoundedFrictionSimulation` and
  `SpringSimulation` constructors validate their input and return `Result<_, SimulationError>`;
  spring fields are private and perceptual constructors take `Duration`.
- Springs settle at a precomputed rest time: `is_done` is monotonic and `x` is exactly the target
  afterwards. Scroll simulations rest within half a device pixel.
- `AnimationController::fling` completes on the frame that reaches the bound.
- `BouncingScrollPhysics` flings into an edge overscroll and spring back (`BouncingScrollSimulation`).
### Removed
- `ScrollSpringSimulation`, `GravitySimulation`, `ClampedSimulation`, `FrictionSimulation::through`,
  `Simulation::tolerance`, `Tolerance::time`, `SpringSimulation::with_snap_to_end`.
```

## Решения владельца

1. ~~`smoothing`~~ — решено: удаляется (orchestration, «Решения по развилкам»).
2. `AnimatedValue` — снято: retarget называет его движком implicit-режимов, тип остаётся и
   переписывается в retarget T4. Открыто только: пресеты `SMOOTH/SNAPPY/BOUNCY` — production-пути
   нет (retarget `.spring(..)` принимает `SpringDescription`); удалить или оставить как значения по
   умолчанию `.spring()` — решение владельца (X17).
3. Расширение разрешённых файлов задачи D: `controller/run.rs` (одна строка fling),
   `scroll/scrollable.rs` и `scroll/refresh_indicator.rs` (dpr) — пересечение с B и W2.

## Стирание `Simulation` один раз (R8 аудита абстракций)

Сейчас `scrollable.rs:681` отдаёт `Box<dyn Simulation>` в `animate_with<S>`, который снова
боксит (`controller.rs:1689`) и делает `Arc::from` (`:1712`): каждый `x()/dx()` тика идёт
Arc → Box → dyn, ради этого существует blanket `impl<S: Simulation + ?Sized> Simulation for Box<S>`
(`simulation.rs:95`). Решение: `animate_with(impl Simulation + 'static)` стирает один раз
(`Arc<dyn Simulation>`, после F3 — `Rc`), `drive_simulation` принимает уже стёртое;
`ScrollPhysics::create_ballistic_simulation` возвращает `Option<Arc<dyn Simulation>>` (4 сайта:
`scroll_physics.rs:177/237/327`, `page_view.rs:134`); blanket-impl для `Box<S>` удаляется.
`ClampedSimulation` (держал `Box<dyn Simulation>`) удаляется этой темой. Правка `controller/run.rs`
— одна строка `animate_with` рядом с fling (п. 3 выше). Трейт остаётся открытым (пользовательские
`ScrollPhysics`) и dyn-совместимым — compile-тест.

## Тест трения с относительным допуском (O17)

`tests/contracts/simulation.rs:54` сравнивает `|x(∞) − final_x| < 1e-9` при величине 7.2e19 —
абсолютный допуск; под Miri `exp_m1(-inf)` сдвигается на ulp → ошибка ≈ 7e3. Строка переходит на
относительный допуск `|Δ| ≤ 1e-12·max(1, |final_x|)` (задача T2 этой темы, `main`).

## Владение

Симуляции — неизменяемые значения; долгоживущих объектов и циклов тема не вводит. Стёртую симуляцию
держит прогон контроллера (`active_run`) и `RetiredSources` при замене; освобождение — вне guard
(controller-robustness). `ScrollMetrics.device_pixel_ratio` читается через `WeakPipelineCell`
в момент жеста; borrow `try_with` отпущен до вызова физики (одно выражение, не скрутини `match`).

## Паттерн

- **Валидирующий конструктор + приватные поля** — `SpringDescription { ω, ζ }`, `Tolerance`,
  `SimulationBounds` (C-VALIDATE; `Result<_, SimulationError>`).
- **Закрытый enum параметра ошибки** — `SimulationParameter`.
- **Открытый трейт, стёртый один раз** — `Simulation` (точка расширения `ScrollPhysics`).
- **Предвычисленный инвариант** — `rest_time` при построении вместо состояния «покоя».

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `SpringDescription { mass, stiffness, damping }` pub-поля | Dart data-класс | `(ω, ζ)` приватно, fallible-конструкторы |
| `with_duration_and_bounce(f64 секунды)` | SwiftUI `TimeInterval` | `Duration` |
| `isDone` мгновенной выборкой | Flutter `Tolerance` | монотонный `rest_time` |
| `Simulation::tolerance()` | поле базового класса Flutter | удалено (0 абстрактных вызовов) |
| `ScrollSpringSimulation` — сквозная обёртка | класс ради имени | удалена |
| `SmoothDamp`/`Smoothed` | Unity API | удалены (нет потребителя) |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Время — `Duration` в публичных параметрах | соответствует | design.md:145-146 | — |
| `try_new`/`Result` вместо `debug_assert!` | соответствует | design.md:143-180 | — |
| Параметр ошибки — enum | соответствует | design.md:138-139 | — |
| `pub` только с внешним потребителем | нарушала | design.md:170 `final_x`, `time_at_x` | `pub(crate)` |
| Dyn один раз на границе хранения | нарушала (код) | simulation.rs:95, controller.rs:1689/1712 | раздел «Стирание один раз» |
| Решения владельца применены | нарушала | design.md:193, 292-293 (`smoothing` открыт) | удаляется |
| Удалённое API не упоминается как живое | нарушала | design.md:248 `advance(Duration)` | `AnimatedValue` ведётся `Vsync` |
| Тест с относительным допуском | нарушала (код) | tests/contracts/simulation.rs:54 | раздел O17 |
| `f64` секунды внутри `Simulation::x/dx` | соответствует с обоснованием | design.md:186-188 | область математики, не API времени |
| `as` с потерей (spring.rs:89 `f64 → u8`) | нарушала (код) | spring.rs:89 | явная насыщающая политика (задача lint) |
