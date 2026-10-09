# interpolation — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` 9a4daa3ed
- **Закрывает:** D-03, D-20; строки M-INTP-2, 3, 5, 6 ([../market.md](../market.md)); проверка
  выброса кривых за [0, 1] у геометрических значений и целочисленных твинов.
- **Решения владельца (2026-10-06):** `ColorTween` по умолчанию — Oklab с premultiplied alpha;
  интерполяция угла по кратчайшей дуге — в объёме, с production-потребителем; эта тема —
  единственный владелец `Matrix4::lerp` (flui-foundation) и `lerp_oklab` (flui-painting). Вне
  объёма: OkLCh и методы hue longer/increasing/decreasing (M-INTP-4), opt-in дискретной
  интерполяции (M-INTP-10).
- **ADR:** да — контракт интерполяции между flui-foundation, flui-painting и flui-animation;
  частично заменяет ADR-0098 §7 (пункт `Color::lerp`). Черновик ниже.

## Текущее состояние (чтение HEAD)

| Что | Где | Факт |
|-----|-----|------|
| Контракт `Lerp` | `crates/flui-foundation/src/geometry/lerp.rs:6-10` | реализации экстраполируют, t не клампят |
| `Matrix4::lerp` | `crates/flui-foundation/src/geometry/matrix4.rs:160-181` | glam `to_scale_rotation_translation`: нулевая ось масштаба → `recip` = ∞ → кватернион NaN → все элементы NaN при 0 < t < 1; skew и perspective отбрасываются (`:168-170`) |
| Концы `Tween` | `crates/flui-animation/src/tween_types.rs:64-74` | t = 0/1 отдаются как есть — поэтому NaN виден только внутри |
| `Color::lerp` | `crates/flui-painting/src/styling/color.rs:241-287` | premultiplied в gamma-sRGB, f32, каналы → u8; NaN t → `Color(0,0,0,0)` |
| `lerp_oklab` | `color.rs:599-623` | Oklab с **прямой** alpha: красный → `TRANSPARENT` темнеет к середине; rustdoc `:608` утверждает «как `Color::lerp`» |
| `Lerp for Color` | `crates/flui-painting/src/lerp_impls.rs:17-27` | `lerp_unclamped` (sRGB) |
| `OklabColorTween` | `tween_types.rs:206-259` | production-вызовов нет (пример, бенч) |
| `IntTween`/`StepTween` | `tween_types.rs:114-147` | клампят t; NaN → 0 (не `begin`) |
| Угол | — | типа нет; `Matrix4` slerp идёт по кратчайшей дуге, многооборотный поворот невыразим; `RotationTransition` (`crates/flui-widgets/src/transitions/rotation_transition.rs:50-54`) берёт обороты как f64 |

Потребители выброса (кривые с выбросом за [0, 1] — `EaseInBack`, `EaseOutBack`, `EaseInOutBack`,
Elastic — есть в публичном каталоге; пользователь передаёт их в implicit-виджеты):

| Потребитель | Где | Сейчас |
|-------------|-----|--------|
| `AnimatedPadding` | `crates/flui-widgets/src/animated/animated_padding.rs:97-102` | `clamp_non_negative()` — защищён |
| `AnimatedContainer` padding/margin/width/height | `crates/flui-widgets/src/animated/animated_container.rs:174-193` | не клампит; `RenderContainer::set_padding`/`set_margin` — `debug_assert!(is_non_negative)` (`crates/flui-objects/src/layout/container.rs:188-212`) → **panic в debug** при 16 → 0 с `EaseOutBack`; отрицательная ширина → `BoxConstraints::tight_for` с отрицательным min |
| `RenderAnimatedSize` | `crates/flui-objects/src/layout/animated_size.rs` (`dry_size_for`) | `constraints.constrain(...)` — защищён |
| Hero flight | `crates/flui-widgets/src/navigator/hero_flight.rs:229-235` → `Positioned` (`:1157`) | `Hero::curve` пользовательский; сжатие с выбросом даёт `Rect` с max < min → отрицательная ширина `Positioned` |
| `BorderRadius` | `BorderRadiusTween` | production-потребителя нет — только политика |

## Варианты и выбор

**Выброс геометрии.** (A) клампить в `Lerp` для `Size`/`Edges`/`Radius` — ломает контракт
экстраполяции и неверен там, где отрицательное значение законно (`Edges` как смещение);
(B) клампить в `Tween` — тот же недостаток; (C) `Lerp` экстраполирует, **потребитель свойства
клампит в домен свойства**. **Выбран C**: домен знает только свойство (padding ≥ 0, ширина ≥ 0,
`Alignment` и `Offset` — без ограничений). Правило записывается в модульный doc `lerp.rs`.
Исправляются `AnimatedContainer` (padding/margin — `clamp_non_negative`, width/height —
`max(0.0)`) и hero (`Rect` с неотрицательными размерами от `min`). NaN-политика `Lerp`: типы с
NaN-представлением (f64-геометрия) распространяют NaN; `Color` и `i32` возвращают `begin`.

**`Matrix4`.** (A) только защита нулевой оси, SRT остаётся — skew/perspective по-прежнему
теряются; (B) декомпозиция CSS Transforms 2 §13.1 (translate, scale, skew, perspective,
кватернион; slerp) с заимствованием вращения и skew у другого конца, если у одного конца ось
масштаба вырождена; неразложимая матрица (m33 = 0 или вырожденная перспектива) — дискретно,
смена в t = 0.5 (CSS Transforms 1 §9, X15); (C) удалить `Lerp for Matrix4` и `Matrix4Tween` —
production-потребителя нет. **Выбран B** с потребителем: `AnimatedContainer::transform`
(сейчас «не анимируется», `animated_container.rs:26`). Кватернион — slerp по кратчайшей дуге
(знак переворачивается при dot < 0); спецификация знак не переворачивает, Flutter делает nlerp
и теряет skew — выбор осознанный, закреплён тестом. Концы точны (t = 0/1 — short-circuit).

**Цвет.** (A) оставить sRGB по умолчанию, починить alpha у `lerp_oklab`; (B) один путь:
`Color::lerp` и `Lerp for Color` — premultiplied Oklab (CSS Color 4 §13.2, §13.4); `lerp_oklab`
и `OklabColorTween` удаляются как дубли. **Выбран B** (решение владельца). Конвейер: оба конца →
Oklab (f32), L/a/b × α, lerp, ÷ α интерполированное; при α ≤ 0 — прямая интерполяция L/a/b;
обратно в sRGB с клампом гаммы по каналу, alpha насыщается. t = 0/1 — точные концы (обход
неточного round-trip через u8). `lerp_multi_stop` (`color.rs:633`) идёт через `Color::lerp` и
наследует Oklab — совпадает с дефолтом градиентов ADR-0098 §7. Квантование до u8 остаётся
(ограничение типа `Color`).

**Угол.** (A) `Tween<f64>` радиан — кратчайшая дуга невыразима; (B) `Angle` (радианы, f64) в
flui-foundation: `Lerp` численный (CSS Transforms 1 §10: 45° → 1215° — 3.25 оборота) и
`Angle::nearest_equivalent(self, reference) -> Angle` — представитель того же направления в
полуобороте от `reference`; путь выбирает потребитель. **Выбран B**. Потребитель —
`AnimatedRotation` (implicit, flui-widgets) с `RotationPath { Numeric, Shorter }`; строит
`RotationTransition` из оборотов, поэтому после темы integration получает перерисовку без
rebuild. Ничья ровно в полуоборот — в положительную сторону (правило CSS Color 4 §13.5.1
`Δ > 180 → θ1 += 360` оставляет Δ = 180).

**Целочисленные твины.** NaN → `begin`; округление и кламп t не меняются (round half away from
zero; правило ничьих CSS Values 4 в ledger не процитировано — [U], контрактом не закрепляется).

## Публичный API (дельта)

```rust
// flui-foundation::geometry
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Angle { radians: f64 }
impl Angle {
    pub const ZERO: Self;
    pub const fn from_radians(r: f64) -> Self;
    pub fn from_degrees(d: f64) -> Self;
    pub fn from_turns(t: f64) -> Self;
    pub const fn radians(self) -> f64;
    pub fn turns(self) -> f64;
    /// The angle pointing the same way as `self` within half a turn of `reference`;
    /// an exact half turn resolves toward increasing angle. NaN in → NaN out.
    pub fn nearest_equivalent(self, reference: Angle) -> Angle;
}
impl Lerp for Angle { /* numeric, extrapolates */ }
// Matrix4::lerp — сигнатура прежняя, контракт новый (rustdoc: декомпозиция CSS, заимствование
// у вырожденной оси, дискретный переход для неразложимых, точные концы, конечный результат).

// flui-painting: Color::lerp — сигнатура прежняя, premultiplied Oklab; удалить Color::lerp_oklab.
// flui-animation: удалить OklabColorTween; ColorTween = Tween<Color> (теперь Oklab).
// flui-animation (spring.rs): TwoWayConverter for Color — вектор premultiplied Oklab
//   (L·α, a·α, b·α, α) с точными концами (решение X5; потребитель — retarget AnimatedValue<Color>).

// flui-animation (tween_types.rs, tween.rs, ext.rs): выход Animatable определяется реализацией →
// ассоциированный тип (R1 аудита абстракций; владелец tween_types.rs — эта тема)
#[diagnostic::on_unimplemented(message = "`{Self}` cannot be animated", label = "implement `Animatable` or use a `Tween<T>`")]
pub trait Animatable { type Value; fn transform(&self, t: f64) -> Self::Value; }
pub struct ReverseTween<A> { tween: A }            // было <T, A> + PhantomData<T>
pub struct TweenAnimation<A> { parent: Rc<dyn Animation<f64>>, animatable: A }
// TweenSequence удаляет composition; hero: Box<dyn Animatable<Value = Rect>> (без `+ Send + Sync`
// после send-flip). `Tween<T>` хранит T — `PhantomData` не нужен; где T не хранится —
// `PhantomData<fn() -> T>` (без лишних auto-trait и derive-bound).

// flui-widgets
pub struct AnimatedRotation { /* angle: Angle, path, duration, curve, alignment, child */ }
impl AnimatedRotation {
    pub fn new(angle: Angle, child: impl IntoView) -> Self;
    #[must_use] pub fn path(self, path: RotationPath) -> Self;
    #[must_use] pub fn duration(self, d: Duration) -> Self;
    /// Bound — общее правило animated-виджетов (решение владельца R2: `+ Send + Sync` уходит
    /// вместе с переходом стёртой кривой на owner-local); до решения — как у соседей.
    #[must_use] pub fn curve(self, curve: impl Curve + 'static) -> Self;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RotationPath { #[default] Numeric, Shorter }
// AnimatedContainer::transform(self, Matrix4) -> Self
```

## Требования (сценарии)

| ID | КОГДА …, СИСТЕМА ДОЛЖНА … | Тест (раннер; свойство?) |
|----|---------------------------|--------------------------|
| I1 | КОГДА `Matrix4::scaling(0,0,1)` интерполируется к `IDENTITY` при t = 0.5, результат ДОЛЖЕН быть конечным и равным `scaling(0.5,0.5,1)` (аналитика) | `matrix4_lerp_decomposes_like_css_transforms` (flui-foundation `tests/main.rs`, `run_table`) |
| I2 | КОГДА у одного конца нулевая ось и вращение `rotation_z(1.0)` у другого, середина ДОЛЖНА равняться `rotation_z(1.0)·scaling(0.5,1,1)` (вращение заимствовано) | там же |
| I3 | КОГДА `rotation_z(0)` → `rotation_z(0.6)`, середина ДОЛЖНА быть `rotation_z(0.3)` с точностью 1e-12 | там же (перенос `lerp.rs:129`) |
| I4 | КОГДА `skew_2d(a, 0)` → `IDENTITY`, середина ДОЛЖНА иметь shear-коэффициент `tan(a)/2`; КОГДА перспектива m[2][3] = −1/d → `IDENTITY`, середина — −1/(2d) (определение декомпозиции CSS) | там же |
| I5 | КОГДА матрица неразложима (m33 = 0), результат ДОЛЖЕН быть `begin` при t < 0.5 и `end` при t ≥ 0.5 | там же |
| I6 | КОГДА t ∈ {0, 1}, результат ДОЛЖЕН побитово совпасть с концом; КОГДА t ∈ [−0.5, 1.5] и концы — случайные SRT+skew с ненулевым масштабом, все элементы ДОЛЖНЫ быть конечны | `matrix4_lerp_endpoints_and_finiteness` (proptest) |
| I7 | КОГДА чёрный → белый при t = 0.5, `Color::lerp` ДОЛЖЕН дать `rgb(99,99,99)` (Oklab L = 0.5 → Y = 0.125 → sRGB 0.3886, по определению Oklab для ахроматических) | `color_lerp_is_premultiplied_oklab` (flui-painting `tests/main.rs`, `run_cases`) |
| I8 | КОГДА `rgb(255,0,0)` → `TRANSPARENT` при t = 0.5, результат ДОЛЖЕН быть `rgba(255,0,0,128)` (premultiplied сохраняет цветность); `Tween<Color>` — то же | там же |
| I9 | КОГДА t ∈ {0, 1} для случайных цветов, результат ДОЛЖЕН равняться концу; КОГДА t = 1.5 или −0.5 — без panic, каналы насыщаются; КОГДА t = NaN — `begin` | там же (proptest для концов) |
| I10 | КОГДА `Angle::nearest_equivalent`: (270°, ref 0°) → −90°; (10°, ref 350°) → 370°; (180°, ref 0°) → 180°; (750°, ref 0°) → 30°; NaN → NaN | `angle_nearest_equivalent_takes_the_shorter_arc` (flui-foundation, `run_table`) |
| I11 | КОГДА `AnimatedRotation` меняет угол 0 → 0.75 оборота с линейной кривой, на половине длительности поворот ребёнка ДОЛЖЕН быть −0.125 оборота при `Shorter` и +0.375 при `Numeric` (через `PipelineOwner::transform_to`) | `animated_rotation_takes_the_requested_arc` (flui-widgets `tests/implicit_animations.rs`) |
| I12 | КОГДА `AnimatedContainer` анимирует padding/margin 16 → 0 и width 10 → 0 с `Curves::EaseOutBack` в debug-сборке, кадры ДОЛЖНЫ проходить без panic, отступы ребёнка и размер ≥ 0 | `implicit_widgets_clamp_overshoot_to_property_domain` (там же, таблица) |
| I13 | КОГДА hero сжимается с кривой с выбросом, ширина и высота shuttle ДОЛЖНЫ быть ≥ 0 на каждом кадре | строка `hero_flight` в `crates/flui-widgets/tests/hero_flight.rs` |
| I14 | КОГДА `AnimatedContainer` анимирует `transform` от `scaling(0,0,1)` к `IDENTITY`, промежуточные кадры ДОЛЖНЫ давать конечную матрицу слоя `scaling(s,s,1)` | `animated_container_animates_its_transform` (implicit_animations) |
| I15 | КОГДА `IntTween`/`StepTween` получают NaN, результат ДОЛЖЕН быть `begin` | строка в `integer_tweens_interpolate_across_the_full_range` (`tests/contracts/tween.rs`) |

Эталоны: аналитика (I1–I5, I7, I8, I10, I11), определение Oklab Оттоссона для ахроматической
оси (I7). Значения Oklab хроматических цветов из памяти не берутся до проверки по CSS Color 4.

## Миграция (rg на HEAD)

- `Color::lerp` (значения меняются): `crates/flui-painting/tests/color_property.rs:27,29,78,85`
  — `:78` (чёрный → белый = 128) становится строкой I7 со значением 99, правило округления
  остаётся на `with_opacity`; `:27,29` сравнивают с `Color::lerp` и не меняются; бенч
  `crates/flui-painting/benches/color_bench.rs:20`.
- `lerp_oklab`/`OklabColorTween`: `tween_types.rs:206-259`, `lib.rs:16-17,157`,
  `examples/animated_box_app.rs:286`, `crates/flui-animation/examples/oklab_gradient.rs`
  (переписать: premultiplied затухание), `benches/animation_bench.rs:24,45`. `flui-sdk`
  `tests/surface.rs` их не закрепляет; `packages/` вызовов нет.
- `Tween<Color>` в production: только `AnimatedContainer` color (`animated_container.rs:134`).
  Snapshot'ов с промежуточным кадром цвета rg не нашёл (`tests/snapshots/*.snap` — статичные
  демо); любой изменившийся `.snap` объясняется в PR и перепрогоняется без обновления.
- `Matrix4::lerp`: вызовов вне `lerp.rs:116-121` нет; in-src тест `lerp.rs:129` переносится в
  таблицу I3.
- `AnimatedRotation`, `RotationPath`, `Angle`: новые, с production-потребителем в том же PR.

## Черновик ADR

**ADR-NNNN: Interpolation contracts.** Supersedes: ADR-0098 §7 (пункт `Color::lerp`; ADR-0098
получает `Superseded-by` с указанием пункта).
1. `Lerp` экстраполирует за [0, 1]. Домен значения (неотрицательный отступ, размер, радиус)
   клампит потребитель свойства в точке применения, не `Lerp` и не `Tween`. Типы с
   NaN-представлением распространяют NaN; `Color` и целые возвращают `begin`.
2. `Color::lerp` и `Lerp for Color` интерполируют в Oklab с premultiplied alpha; концы точны;
   вне гаммы — кламп по каналу. Других публичных путей интерполяции цвета нет.
3. `Matrix4::lerp` — декомпозиция CSS Transforms 2 §13.1 со slerp по кратчайшей дуге;
   вырожденная ось заимствует вращение и skew у другого конца; неразложимая матрица
   переключается в t = 0.5; результат конечен при конечных концах.
4. Угол — `Angle`; численная интерполяция по умолчанию, кратчайшая дуга — выбор потребителя
   через `nearest_equivalent`.

## Changelog (`changelog.d/<branch-slug>.md`)

```markdown
### Changed

- **`flui-animation`**: `Animatable` has an associated `Value` type instead of a type parameter;
  `ReverseTween`, `TweenAnimation` lose their phantom value parameter. `TwoWayConverter for Color`
  works in premultiplied Oklab.
- **`flui-painting`**: `Color::lerp` (and so `Tween<Color>`/`ColorTween`) interpolates in Oklab
  with premultiplied alpha; a black-to-white midpoint is now `rgb(99, 99, 99)`, not 128.
- **`flui-foundation`**: `Matrix4::lerp` decomposes like CSS transforms: it keeps skew and
  perspective, stays finite when an endpoint has a zero scale axis, and switches at `t = 0.5`
  between matrices that cannot be decomposed.

### Added

- **`flui-foundation`**: `Angle`, with numeric interpolation and `nearest_equivalent`.
- **`flui-widgets`**: `AnimatedRotation` with `RotationPath::{Numeric, Shorter}`;
  `AnimatedContainer::transform`.

### Removed

- **`flui-painting`**/**`flui-animation`**: `Color::lerp_oklab` and `OklabColorTween`;
  `Color::lerp` and `ColorTween` do the same, premultiplied.

### Fixed

- **`flui-widgets`**: `AnimatedContainer` with an overshooting curve no longer hands a negative
  padding, margin or size to layout (a debug panic before); hero flights keep a non-negative size.
```

## Adversarial review

- **Реентрантность, слушатели, владельцы, vsync, realm, два контроллера.** `Lerp`/`Angle`/
  `Matrix4::lerp`/`Color::lerp` — чистые функции без состояния и блокировок. `AnimatedRotation`
  использует существующий `ImplicitAnimation` (владение, dispose, регистрация Vsync — как у
  `AnimatedOpacity`); его риски — темы ownership и listener-delivery.
- **Panic в пользовательском коде.** Пользовательская кривая в `AnimatedRotation`/
  `AnimatedContainer` вызывается в `CurvedAnimation`; изоляция — controller-robustness.
  Сами интерполяции не паникуют: `debug_assert` домена в render-объектах больше не достижим из
  implicit-виджетов (I12).
- **NaN / ∞ / переполнение.** `Matrix4`: декомпозиция делит на длины осей и m33 — нулевые
  перехвачены (заимствование / дискретный переход); при конечных концах и t ∈ [−0.5, 1.5]
  результат конечен (I6); огромные масштабы (1e300) дают ∞ при t ≫ 1 — экстраполяция до
  переполнения публикуется как есть, render-объекты трансформации трактуют неконечную матрицу
  как вырожденную (тема integration). `Color`: `cbrt` отрицательных LMS при экстраполяции
  определён, кламп гаммы до u8; деление на α защищено `α ≤ 0`. `Angle::turns` на 1e308 —
  конечное умножение на 1/τ. Целые: насыщающий cast, NaN → `begin`.
- **dt = 0 / огромный dt / время назад / retarget на последнем кадре.** Видны только как t;
  t вне [0, 1] от выброса — экстраполяция (контракт 1), retarget `AnimatedRotation` берёт текущий
  угол как `reference` для `nearest_equivalent`, поэтому смена цели посреди поворота не
  крутит лишний оборот.
- **Остаточные риски.** (1) Смена цвета по умолчанию меняет промежуточные кадры каждого
  `Tween<Color>`; тесты пикселей с цветовой анимацией rg не нашёл, но внешние потребители
  увидят другой оттенок. (2) Стоимость Oklab — два `cbrt` и `powf` на канал за вызов;
  бенч `animated_value_color_frame` до/после. (3) Заимствование вращения у вырожденного конца —
  решение FLUI, не CSS (CSS для списка функций интерполирует примитивно и такого случая не имеет).

## Миграция `Animatable` → ассоциированный тип (R1)

`Animatable<` — 33 места (5 вне крейта: `hero.rs` ×2, `hero_controller.rs`, `animated_opacity.rs`,
тест `slide_transition`); `ReverseTween<`/`TweenAnimation<`/`ChainedTween<` — 26, все в крейте.
`AnimatableExt<T>` → `AnimatableExt` с одним методом `animate` (остальное удаляет composition, R4).
`Animation<T>` параметр сохраняет (130 `dyn Animation<f64>`; запись в `## Mapping decisions`).
Hot path не меняется; generic-параметров меньше — меньше мономорфизации.

## Владение

Интерполяции — чистые функции значений; долгоживущих объектов, `static`, замков и циклов нет.
`AnimatedRotation` использует `ImplicitAnimation` (владение — ownership/retarget).

## Паттерн

- **Ассоциированный тип** вместо фантомного параметра (`Animatable::Value`).
- **Newtype** — `Angle` (радианы), путь — закрытый enum `RotationPath`.
- **Чистая функция с документированным доменом** — кламп в потребителе свойства, не в `Lerp`.
- **`#[diagnostic::on_unimplemented]`** на `Animatable`/`Lerp`.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `Animatable<T>` + `PhantomData<T>` во всех обёртках | Dart generic-класс `Animatable<T>` | `trait Animatable { type Value; }` |
| `Matrix4Tween` через nlerp SRT (Flutter) | теряет skew/перспективу | декомпозиция CSS Transforms 2 |
| `Color.lerp` в sRGB | Flutter | premultiplied Oklab |
| `OklabColorTween` рядом с `ColorTween` | два способа одного | один путь |
| `RotationTransition(turns: f64)` | число оборотов | `Angle` + `RotationPath` |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Выход, определяемый реализацией, → ассоциированный тип | нарушала (код) | tween_types.rs:35, 176, 340, 452; tween.rs:42-51; ext.rs:53 | R1 здесь |
| `PhantomData<fn() -> T>` / bounds на impl | нарушала (код) | tween.rs:52, tween_types.rs:179, 461; tween.rs:41-53, constant.rs | R1 убирает `T`; bounds — на `impl Animation` |
| `TwoWayConverter for Color` в Oklab (X5) | нарушала | не назначено | здесь, `spring.rs` |
| `#[must_use]` на builder-методах | нарушала | design.md:111-113 | добавлено |
| Bound `Send + Sync` у кривой виджета | открыто | design.md:113 | правило R2 (владелец) |
| `#[diagnostic::on_unimplemented]` | нарушала | — | на `Animatable` |
| Время/угол — newtype | соответствует | `Angle` | — |
| Политика NaN/∞ задокументирована, нефинитное не публикуется | соответствует | ADR п. 1, I6 | — |
| `#[non_exhaustive]` на `RotationPath` | соответствует | design.md:116 | — |
