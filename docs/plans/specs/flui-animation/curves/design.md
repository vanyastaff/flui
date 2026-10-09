# curves — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` 9a4daa3ed; `curve.rs` = `crates/flui-animation/src/curve.rs`
- **Закрывает:** D-17, D-18, D-19, D-37, D-38; строки матрицы M-CRV-1…7, 9, 11, 12
  ([../market.md](../market.md)). ADR не нужен: межкрейтовый контракт — документ трейта `Curve`.
- **Решения владельца (2026-10-06):** `steps()` реализуется вместе с production-потребителем;
  удаляются `CatmullRomCurve`, `CatmullRomSpline`, `Curve2D`, `Curve2DSample`, `ParametricCurve`,
  `ReverseCurve`/`Curve::reversed`; удаление Catmull-Rom — **после** того, как тема composition
  введёт ключевой кадр с cubic-сегментом, решаемым по x (M-CRV-9). Вне объёма: экстраполяция
  cubic-bezier вне [0, 1] (M-CRV-8). Дополнено оркестратором: `SawTooth` и `Threshold`
  удаляются (нет потребителя); потребитель `steps()` — `CupertinoActivityIndicator` (composition),
  а не каретка (`editable_text.rs` у text-ime); `Curve::slope` вводит эта тема первой (X13);
  встроенные кривые сравниваются по значению, пользовательская — по идентичности (X4).

## Текущее состояние (чтение HEAD)

| Что | Где | Факт |
|-----|-----|------|
| Контракт | `curve.rs:10-36` | «0 → 0, 1 → 1», «t в [0, 1]»; NaN и t вне диапазона не оговорены |
| Обход валидации | `curve.rs:141-148`, `197-200`, `230-239`, `336-350`, `587-590` | `pub`-поля + `derive(Deserialize)`; `Cubic::new` (`:242`) и `Elastic*::new` (`:595`) без проверок |
| Образец | `curve.rs:436`, `470-497` | `Split`: приватные поля, проверка при decode |
| Солвер | `curve.rs:271-312` | допуск 1e-6 только по x, Newton от s = x, 8 итераций, затем бисекция 32 |
| Ошибка y | `Curves::EaseInOutExpo` `:1045` | касательная вертикальна в s = 0.5; x = 0.5000009 → y = 0.5000013 против 0.5091229 (−9.1e-3) |
| `ThreePointCubic` | `:389-421` | деление на ширину/высоту сегмента; `Cubic` строится на каждый вызов |
| Elastic | `:606-704` | на концах скачок 2⁻¹⁰ (`ElasticOut(1−ε)` = 1.0009765625, затем ровно 1) |
| `SawTooth` | `:129-135` | `(1·n).fract()` = 0 при t = 1; `count = 0` — константа 0 |
| `ReverseCurve` | `:51-60`, `:977-996` | 0 → 1, 1 → 0; production-вызовов нет |
| `CatmullRomCurve` | `:791-861` | x игнорируется; концы = y первой/последней точки; tension документирована наоборот (`:800`); пустой `points` → underflow (`:831`) |
| NaN | `:320`, `:392`, `:558`, `:219` | `Cubic`/`ThreePointCubic`/`Split` → 0, `Threshold` → 1, прочие — NaN |
| Тесты | `curve.rs:1237-1254` | один: x-инверсия с допуском 1e-3; каталог констант не проверен |
| Документация | `:1023-1027`, `:1056-1060` | `SlowOutFastIn`, `EaseInOutCubic`, `EaseInBack`, `EaseOutBack` описаны противоположно точкам |

Production-вызовы вне крейта (rg): `Interval::new` (`crates/flui-widgets/src/interaction/dismissible.rs:1431`),
`Interval::linear` (`crates/flui-widgets/src/navigator/hero_flight.rs:218`), `Curves::*` и
`FlippedCurve` через `.flipped()`; `Cubic::new` — только тест `curved.rs:250`. Удаляемые типы
вне `curve.rs` и `lib.rs:147-152` встречаются только в `README.md:171,183`, `docs/GUIDE.md:203-217`
(doctest через `include_str!`), `docs/PERFORMANCE.md:184,324`, `docs/ARCHITECTURE.md:108`,
`docs/PATTERNS.md:146`. `crates/flui-sdk/tests/surface.rs` ни один из них не закрепляет.

## Варианты

**Валидация.** (A) `debug_assert` в конструкторах, поля открыты — дёшево, но serde и литерал
обходят; (B) приватные поля, `const fn new` с `assert!` (для констант — ошибка компиляции),
`try_new -> Result<_, CurveError>` для входа во время выполнения, serde через
`#[serde(try_from = "…Spec", into = "…Spec")]` с теми же именами полей; (C) newtype-параметры
(`UnitX(f64)`) — громоздко для `const`-пресетов. **Выбран B**: формат сериализации не меняется,
нелегальное состояние непредставимо. Для `Interval<C>` `try_from` безопасен —
`C: Copy`, деструкторов нет; `Split` остаётся на своём ручном decode, но отдаёт `CurveError`.

**Солвер.** (A) оставить, сузить допуск — не лечит вертикальную касательную; (B) алгоритм
WebKit `UnitBezier`/Chromium `gfx::CubicBezier` в f64 с остановкой по y: таблица 11 отсчётов →
≤ 4 шага Newton → бисекция; (C) аналитический корень кубики (kurbo `solve_cubic`) — точен, но
ветвится на вырожденных коэффициентах и дороже на горячем пути. **Выбран B** с границей
ошибки по выходу: при `x1, x2 ∈ [0, 1]` функция x(s) монотонна, поэтому бисекция корректна, а
|Δy| ≤ Y'·|Δs|, где Y' = 3·max(|y1|, |y2 − y1|, |1 − y2|) — граница |dy/ds|.

**Catmull-Rom, `ReverseCurve`, `Curve2D`.** Исправлять (x-solve, монотонность) или удалить.
Production-потребителей нет; владелец решил удалить. `ReverseCurve` заменяется
`ReverseAnimation` (обратный вход) или `.flipped()` (ease-in ↔ ease-out).

**`steps()`.** (A) обобщить `Threshold`; (B) новый `Steps { count, jump: JumpAt }` по CSS Easing 1
§2.3.1. **Выбран B**; `Threshold` удаляется вместе с `SawTooth` (нет production-потребителя,
решение владельца; порог — `Steps::new(1, JumpAt::End)` над `Interval`). Потребитель —
`CupertinoActivityIndicator` (composition): индекс активного тика.

## Выбранный контракт

Текст rustdoc трейта `Curve` (единая политика, D-38, M-CRV-11):

- `transform` определена на всём f64. При t ∈ [0, 1] результат конечен; `transform(0.0) == 0.0`
  и `transform(1.0) == 1.0` точно.
- Конечное t вне [0, 1] и ±∞ клампятся: результат равен значению на ближайшем конце.
  Экстраполяции нет (вход кривой — прогресс контроллера, он всегда в [0, 1]).
- `transform(NaN)` возвращает NaN. Кривая не маскирует сломанный вход под валидный прогресс;
  контроллер считает неконечный сэмпл отказом прогона (тема controller-robustness, D-04).
- Монотонность — свойство конкретной кривой, указывается в её rustdoc («монотонная» /
  «с выбросом за [0, 1]» / «немонотонная») и закрепляется таблицей.

Комбинаторы наследуют: `FlippedCurve` (`1 − c(1 − t)`) сохраняет и концы, и NaN.

## Публичный API (дельта)

```rust
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum CurveError {
    #[error("curve parameter {parameter} is not finite")]
    NonFinite { parameter: CurveParameter },
    #[error("curve parameter {parameter} = {value} is outside its admitted range")]
    OutOfRange { parameter: CurveParameter, value: f64 },   // диапазон — в Display параметра
    #[error("interval begin {begin} is after end {end}")]
    IntervalReversed { begin: f64, end: f64 },
    #[error("steps with `JumpAt::None` need at least two steps")]
    TooFewSteps,
}
/// Параметр — enum, не `&'static str` (та же форма, что `SimulationParameter` в physics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CurveParameter { X1, Y1, X2, Y2, Midpoint, Begin, End, Period, StepCount, Split }
impl fmt::Display for CurveParameter { /* имя и допустимый диапазон: "x1 in [0, 1]" */ }

pub trait Curve {
    fn transform(&self, t: f64) -> f64;
    /// `d transform / dt`, конечна; provided — разность второго порядка (`h = 1e-4`),
    /// неконечное → 0. Вводится здесь первым (X13); retarget и composition — потребители.
    fn slope(&self, t: f64) -> f64 { /* default */ }
    fn flipped(self) -> FlippedCurve<Self> where Self: Sized;
}

impl Cubic {           // поля a, b, c, d — приватные
    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self;      // # Panics: x вне [0,1], неконечное
    pub fn try_new(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError>;
}
impl ThreePointCubic { // хранит два готовых Cubic-сегмента
    pub const fn new(a1, b1, midpoint, a2, b2) -> Self;               // + x точек внутри своего сегмента
    pub fn try_new(...) -> Result<Self, CurveError>;
}
impl<C: Curve + Copy> Interval<C> { pub fn new(..) -> Self; pub fn try_new(..) -> Result<Self, CurveError>; }
impl Elastic{In,Out,InOut}Curve { pub const fn new(period: f64) -> Self; pub fn try_new(period: f64) -> Result<Self, CurveError>; }
impl<B: Curve, E: Curve> Split<B, E> { pub fn try_with_curves(split: f64, b: B, e: E) -> Result<Self, CurveError>; }
// Bounds — на impl, не на struct (`Interval<C>`, `Split<B, E>`, `FlippedCurve<C>` без bound в определении).

/// Стёртая кривая: внутри закрытый enum `{ Builtin(BuiltinCurve), Custom(Arc<dyn Curve …>) }`;
/// `PartialEq` — по значению для встроенных (`BuiltinCurve: PartialEq`), по идентичности
/// указателя для пользовательских (X4); `From<C>` для каждой встроенной кривой.
/// Bound `Send + Sync` и один ли это тип для owner-local хранения — решение владельца (R2).
pub struct ArcCurve(/* приватный enum */);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum JumpAt { Start, #[default] End, None, Both }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Steps { count: u32, jump: JumpAt }
impl Steps { pub const fn new(count: u32, jump: JumpAt) -> Self; pub fn try_new(count: u32, jump: JumpAt) -> Result<Self, CurveError>; }
impl Curve for Steps { .. }
```

Удаляются: `ParametricCurve`, `Curve2D`, `Curve2DSample`, `CatmullRomCurve`, `CatmullRomSpline`,
`ReverseCurve`, `Curve::reversed`. Публичных геттеров полей нет: production их не читает, а
serde идёт через приватные `…Spec` (`try_from`/`into`).

Правила (rustdoc каждого конструктора):
- `Cubic`: все четыре числа конечны, `x1, x2 ∈ [0, 1]` (CSS Easing 2 §2.2); y не ограничен.
- `ThreePointCubic`: midpoint строго внутри единичного квадрата; `a1.x, b1.x ∈ [0, mid.x]`,
  `a2.x, b2.x ∈ [mid.x, 1]` — иначе пересчитанный сегмент нарушил бы правило `Cubic`.
- `Interval`: `0 ≤ begin ≤ end ≤ 1`, конечны.
- `Elastic*`: `period` конечен и > 0. `Steps`: `count ≥ 1`,
  для `JumpAt::None` — `count ≥ 2`.

## Алгоритмы

**Солвер `Cubic`.** Конструктор вычисляет полиномиальные коэффициенты x(s), y(s) и таблицу
x(i/10), i = 0..10 (как WebKit). `solve_x(x)`: начальное s — линейная интерполяция по таблице;
до 4 шагов Newton, шаг принимается, если |x'(s)| ≥ 1e-7 и s остаётся в [lo, hi] текущей вилки;
выход из Newton — по |Δs|·Y' < 1e-7. Иначе бисекция по s на вилке, сужаемой по таблице и
Newton-итерациям, до (hi − lo)·Y' < 1e-7 (≤ 40 шагов при Y' ≤ 3·max|y|). Гарантия: |Δy| < 1e-7
для любого валидного `Cubic` и любого x ∈ [0, 1]. Бенч `curved_value` до/после.

**Elastic.** Огибающая нормируется, чтобы на защищённом конце не было скачка:
`e(t) = (2^(−10t) − 2^(−10)) / (1 − 2^(−10))` вместо `2^(−10t)` (для In и половин InOut —
зеркально). Форма меняется не более чем на 2⁻¹⁰ ≈ 0.098 %; концы точны без особых случаев
(guard `t == 0/1` остаётся как защита от округления).

**`Steps`** (CSS Easing 1 §2.3.1 без before flag): `step = floor(t·n)`; `+1` для `Start`/`Both`;
`jumps = n` (`Start`/`End`), `n − 1` (`None`), `n + 1` (`Both`); `step = min(step, jumps)`;
результат `step / jumps`. Концы по контракту трейта: t = 0 → 0 (совпадает с CSS при before
flag), t = 1 → 1. Before flag не моделируется: у FLUI нет фаз before/after.

## Потребитель `Steps`: `CupertinoActivityIndicator`

Индикатор (composition, `packages/flui-cupertino/src/activity_indicator.rs`) держит один
повторяющийся контроллер с периодом 1 s; дорожка непрозрачности тика — 8 сегментов
`to(A[k], 125 мс, Steps::new(1, JumpAt::Start))` (скачок в начале сегмента, затем плато), тик `i`
читается со сдвигом `Stagger`. Painter читает `value()` в `paint`, rebuild нет;
`behavior(Preserve)` (X9). Задача — одна ветка с composition T6. Каретка не делается: файл
`editable_text.rs` принадлежит text-ime (решение владельца).

## Требования (сценарии)

| ID | КОГДА …, СИСТЕМА ДОЛЖНА … | Тест (раннер; свойство?) |
|----|---------------------------|--------------------------|
| C1 | КОГДА берётся любая константа `Curves::*` или кривая из допустимого домена конструктора, она ДОЛЖНА давать ровно 0 и 1 на концах и конечные значения внутри | `curve_catalog_endpoints_and_monotonicity` (`run_table`, proptest по домену) |
| C2 | КОГДА кривая объявлена монотонной, для t1 < t2 ДОЛЖНО быть f(t1) ≤ f(t2) | там же, proptest; список монотонных — в строках таблицы |
| C3 | КОГДА `Cubic` задан точками CSS `ease`, `ease-in`, `ease-out`, `ease-in-out`, `(0.05,0.7,0.1,1)`, вывод ДОЛЖЕН совпадать с эталоном y(x) (market.md, 200 итераций бисекции) с |Δy| < 1e-6 | `cubic_bezier_matches_css_reference_values` (`run_table`) |
| C4 | КОГДА x близок к вертикальной касательной `EaseInOutExpo` (x = 0.5 ± 1e-6, ± 9e-7), |Δy| ДОЛЖНА быть < 1e-6 против аналитики s = 0.5 + ∛((x − 0.5)/4), y = 3s² − 2s³ | там же |
| C5 | КОГДА x1, x2, y1, y2 случайны в допустимом домене, |y − y_ref| ДОЛЖНА быть < 1e-6, где y_ref — тестовая бисекция 200 шагов | `cubic_solver_bounds_output_error` (proptest) |
| C6 | КОГДА параметр нарушает правило (x вне [0,1], NaN, ±∞, period ≤ 0, begin > end, midpoint на границе, `count = 0`, `None` с 1 шагом), `try_new` ДОЛЖЕН вернуть соответствующий `CurveError`, `new` — panic, serde — ошибку decode с тем же текстом | `curve_parameters_reject_invalid_input` (`run_table`, строка на правило × способ) |
| C7 | КОГДА валидная кривая сериализуется и читается обратно, она ДОЛЖНА совпасть, а имена полей — с текущим форматом | `curve_serde_round_trip_keeps_the_wire_format` (feature `serde`) |
| C8 | КОГДА t — NaN, каждая кривая ДОЛЖНА вернуть NaN; КОГДА t = ±∞ или вне [0,1], — значение ближайшего конца | `curve_input_policy_for_nan_and_out_of_range` (`run_table` по каталогу) |
| C9 | КОГДА `Steps(4, j)` вычисляется в 0, 0.25, 0.3, 0.75, 1 для каждого `JumpAt`, результат ДОЛЖЕН совпасть с вычисленным вручную по CSS Easing 1 §2.3.1 (напр. 0.3: End 0.25, Start 0.5, None 1/3, Both 0.4) | `steps_follow_css_easing_algorithm` (`run_table`) |
| C10 | КОГДА t → 1⁻ и t → 0⁺ (1e-12), Elastic In/Out/InOut ДОЛЖНЫ отличаться от значения на конце меньше 1e-9; отличие от прежней формулы внутри < 2⁻¹⁰ | `elastic_curves_are_continuous_at_endpoints` |
| C11 | КОГДА `slope` берётся у `Linear`, `Cubic` (CSS `ease` в 5 точках, включая `x'(s) = 0` у ease-out), `Steps`, `FlippedCurve`, `Interval`, результат ДОЛЖЕН совпасть с аналитикой с допуском 1e-6 и быть конечным; у default — с центральной разностью эталона | `curve_slope_matches_the_analytic_derivative` (`run_table`) |
| C12 | КОГДА `CupertinoActivityIndicator` идёт N виртуальных периодов, непрозрачность каждого тика ДОЛЖНА меняться скачком ровно 8 раз за период (`Steps(1, Start)` на сегмент), без rebuild элементов (`FrameReport.build.elements_built` = 0 на кадрах тика) | `cupertino_activity_indicator_steps_without_rebuilds` (flui-cupertino `tests/`) |
| C13 | КОГДА индикатор удаляется во время тика, контроллер ДОЛЖЕН быть освобождён (Drop == 1), следующий кадр не помечает paint удалённого узла | `cupertino_activity_indicator_releases_its_controller` (там же) |
| C14 | КОГДА константа заявлена как точка CSS/Material (`Ease`, `EaseIn`, `EaseOut`, `EaseInOut`, `FastOutSlowIn`), её контрольные точки ДОЛЖНЫ совпадать с эталоном | строка C3 (через выход в 5 точках) |

Эталоны — только из market.md (бисекция, независимая от солвера), CSS-алгоритма, выполненного
вручную, и аналитики; формула production не используется как оракул. Каждый фикс поведения —
прогон с откатом production-ханка в отдельном worktree (падает по названной причине).

## Миграция

- `crates/flui-animation/src/lib.rs:147-152` — экспорт `CurveError`, `JumpAt`, `Steps`; убрать
  удалённые типы. `crates/flui-sdk/tests/surface.rs` — без изменений (ни один удалённый тип не
  закреплён); `flui-sdk` реэкспортирует крейт целиком (`flui-sdk/src/lib.rs:27`), новые типы
  пакетам доступны без правки SDK.
- `curved.rs:250` (`Cubic::new(0.0, 0.0, 1.0, 1.0)`) — валиден, не меняется.
- `README.md:129-131,171-183`, `docs/GUIDE.md:172,203-217`, `docs/PERFORMANCE.md:33,184,324,463`,
  `docs/ARCHITECTURE.md:108`, `docs/PATTERNS.md:146` — убрать удалённые типы, исправить
  `flipped`, точность солвера, контракт NaN. `GUIDE.md`/`PERFORMANCE.md` — doctest-источники.
- `docs/plans/specs/naming/renames.md:98` (`Curve2D`) — строка снимается.
- `packages/` — вызовов нет (rg по удалённым именам пуст).

## Changelog (`changelog.d/<branch-slug>.md`)

```markdown
### Changed

- **`flui-animation`**: curve parameters are validated however a curve is made: `Cubic`,
  `ThreePointCubic`, `Interval`, `Elastic*` and `Split` have private
  fields, `const fn new` that panics on invalid input, `try_new` returning `CurveError`, and serde
  decoding that rejects what `try_new` rejects. `Curve::transform(NaN)` now returns NaN for every
  curve; `Cubic` and `ThreePointCubic` used to return 0.
- **`flui-animation`**: the cubic-bezier solver bounds its output error below 1e-7 (it bounded
  only x before; `Curves::EaseInOutExpo` was off by up to 9e-3 next to its vertical tangent).
  Elastic curves no longer jump by 2^-10 on their last and first frames.

### Added

- **`flui-animation`**: `Steps` with `JumpAt::{Start, End, None, Both}` (CSS `steps()`);
  `CupertinoActivityIndicator` ticks with it. `Curve::slope`, the curve's derivative.
  `CurveError` names the parameter with `CurveParameter`.

### Removed

- **`flui-animation`**: `ReverseCurve` and `Curve::reversed` (they mapped 0 to 1); use
  `ReverseAnimation` to reverse the input or `.flipped()` to turn an ease-in into an ease-out.
  `ParametricCurve`, `Curve2D`, `Curve2DSample`, `CatmullRomCurve` and `CatmullRomSpline` (they
  ignored x); keyframes with cubic segments replace them. `SawTooth` and `Threshold` (no user;
  a threshold is `Steps::new(1, JumpAt::End)` over an `Interval`).
```

## Adversarial review

- **Реентрантность, слушатели, владельцы, vsync, realm.** Кривые — чистые `Fn(f64) -> f64` без
  состояния и блокировок; в реестр vsync не входят. Касается только потребителя-индикатора:
  удаление во время тика — C13; painter читает `value()` в `paint`, слушателей не держит.
- **Panic в пользовательском коде.** Пользовательский `impl Curve` может паниковать внутри
  `CurvedAnimation::value()`; это зона controller-robustness/listener-delivery. Встроенные кривые
  не паникуют после конструктора: все деления защищены валидированными инвариантами
  (`ThreePointCubic` хранит готовые сегменты; `Interval` при `end − begin < 1e-6` — ступень).
- **Panic при decode.** `try_from` возвращает ошибку без частично построенного значения; у
  `Interval<C>` `C: Copy`, деструкторов нет; `Split` сохраняет нынешний путь удержания (`:470-497`).
- **NaN / ∞ / переполнение.** Вход: политика C8. Параметры: только конечные (C6). Солвер:
  коэффициенты ≤ 3·max|y| конечны; Newton ограничен вилкой, деление защищено порогом 1e-7.
  Elastic: `2^(−10t)` при t ∈ [0, 1] не переполняется. `Steps`: t ∈ [0, 1], поэтому `t·n ≤ n` конечно.
- **dt = 0 / огромный dt / время назад / retarget на последнем кадре.** Кривая видит только
  прогресс в [0, 1]; эти случаи — у контроллера. Огромный прогресс клампится (C8).
- **Два контроллера на одном vsync.** Кривые разделяемы (`Copy`/`ArcCurve`), состояния нет.
- **Остаточные риски.** (1) Смена NaN → 0 на NaN → NaN у `Cubic` видима потребителю, который
  вызывает кривую напрямую с NaN: rg находит только вызовы со значением контроллера
  (`dismissible.rs:1431`, `hero_flight.rs:218`), конечным по контракту controller-robustness; до
  слияния той темы NaN дойдёт до слоя — порядок PR: controller-robustness раньше curves.
  (2) Индикатор — одна ветка с composition T6. (3) Пользовательские
  `impl Curve` не обязаны соблюдать политику; проверочного набора для них нет (кандидат в
  `flui-testing`, если появится второй внешний реализатор).

## Владение

Кривые — `Copy`/`Clone`-значения без внутренней изменяемости, колбэков и `static`; долгоживущих
объектов и циклов тема не вводит. Стёртая кривая (`ArcCurve`) — разделяемое неизменяемое значение.
Единственный потребитель с состоянием — индикатор (контроллер у `DrivenController`, тест C13).

## Паттерн

- **Открытый трейт как точка расширения** — `Curve` (пользовательские кривые документированы);
  `slope` — provided-метод, `flipped` — `Self: Sized`; dyn-совместимость — compile-тест.
- **Приватные поля + `const fn new` / `try_new`** (C-VALIDATE); serde через `try_from`-спеку.
- **Закрытый enum** — `JumpAt`, `CurveParameter`, внутренний `BuiltinCurve` стёртой кривой.
- **Bounds на impl, не на struct** — `Interval<C>`, `Split<B, E>`, `FlippedCurve<C>`, `CurvedAnimation`
  (`C: Clone` с определения убирается; не-generic форма — frame-path-state F3, R5).

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `pub struct Curves;` с `#[expect(non_upper_case_globals)]` константами | Dart `Curves.easeIn` namespace-класс | модуль `curves::EASE_IN` — переименование в спеке naming (до 0.2.0), здесь не делается |
| `ParametricCurve<T>`, `Curve2D`, `CurveExt::then` | иерархия классов Flutter | удалены (трейт без пользователя; `then` = `with_curve`) |
| `ReverseCurve` (0 → 1) | Flutter `ReverseTween`-подобный | удалён; `ReverseAnimation` или `.flipped()` |
| pub-поля кривых + `assert` в конструкторе | Dart `final` поля + `assert` debug | приватные поля, `try_new`, serde `try_from` |
| `CurveError { parameter: &'static str }` | строка-метка | `CurveParameter` enum |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Параметр ошибки — enum, не `&'static str` | нарушала | design.md:86-95 | `CurveParameter` |
| Каждый `pub` с потребителем / решение владельца | нарушала | design.md:61-64, 106, 108, 151-152 (`Threshold`, `SawTooth`) | удалены |
| Потребитель `steps()` по решению владельца | нарушала | design.md:62-64, 154-164, C12-C13 | `CupertinoActivityIndicator` |
| Одна реализация производной (X13) | нарушала | retarget design.md:64-69 | `Curve::slope` вводится здесь (T9) |
| Значение сравнивается по значению (X4) | нарушала | `ArcCurve: PartialEq` = `ptr_eq` (curve.rs:1190) | внутренний enum, по значению для встроенных |
| Один стёртый тип кривой (`Send + Sync`) | открыто | curve.rs:1188, 1227; send-flip 264 vs 21/39 | решение владельца (R2) |
| Bounds на impl, не на struct | нарушала (код) | curved.rs:56 `C: Clone` | убрать с определения (карточка в tasks.md) |
| `try_new` + `new` с `# Panics`, называющим `try_new` | соответствует | design.md:41-47 | — |
| `#[non_exhaustive]` на `CurveError`, `JumpAt` | соответствует | design.md:85, 112 | — |
| `Eq`/`Hash` где нет float | соответствует | `Steps`, `JumpAt` | — |
| NaN-политика задокументирована | соответствует | design.md:68-79 | — |
