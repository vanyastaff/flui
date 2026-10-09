# flui-animation / composition — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md).

## Итог

Одна чистая дорожка ключевых кадров `Keyframes<T>` с типизированным временем (`Duration`),
сегментами «к значению по кривой», `cubic` (Catmull-Rom по времени), `hold`, `jump`, и
`Stagger` — задержки по индексу с origin. Задержка = ведущий `hold`, группа = несколько дорожек
с общим `total` на одном контроллере, цикл = `repeat` контроллера + циклическое чтение.
`TweenSequence`/`TweenSequenceItem` удаляются. Потребители: `ActivityIndicator` (+ спиннер
`RefreshIndicator`), `LinearProgressIndicator`, `CupertinoActivityIndicator`. ADR не нужен.

## Текущий код

| Что | Где | Факт |
|-----|-----|------|
| `Animatable<T>` | `tween_types.rs:35` | `transform(t) -> T`, без супертрейтов |
| `TweenSequence` | `tween_types.rs:340`, `:401`, `:452` | веса без единиц; clamp `t`; panic в `new` на пустом списке и невалидных весах; pub-поля `TweenSequenceItem` обходят проверку до `new` |
| `Interval` | `curve.rs:141`, `:177` | окно на прогрессе; 2 production-пользователя |
| `CatmullRomCurve`/`Spline` | `curve.rs:796`, `:869` | x игнорируется (D-19); без пользователей |
| `ArcCurve` | `curve.rs:1188` | `Arc<dyn Curve + Send + Sync>`, `Clone` дёшев |
| `TwoWayConverter` | `spring.rs:25` | вектор компонент `[f64; N]` для f64, `Offset`, `Size`, `Color`, derive |
| повтор | `controller.rs:1390` | `repeat_with(min, max, reverse, period, count)`, фаза — чистая функция времени |
| painter с repaint | flui-rendering `delegates/custom_painter.rs` `repaint()` | анимация без пересборки |
| спиннер | `refresh_indicator.rs:23-24`, `:465`, `:492` | статичный `ColoredBox`, DEFERRED |

Ссылки `rg TweenSequence` вне `docs/plans`: `src/tween_types.rs` (23), `src/lib.rs` (3: `:157-158`,
prelude `:193`), `tests/contracts/tween.rs` (8), `README.md` (5), `docs/GUIDE.md` (5),
`docs/PERFORMANCE.md` (2), `docs/ARCHITECTURE.md` (1 + имена тестов `:918-919`). В
`packages/`, `flui-sdk/tests/surface.rs` и других крейтах — 0.

## Варианты

### (a) Модель времени дорожки

1. **Абсолютные смещения-доли** (WAAPI `offset`, Compose `atFraction`): ошибка «не по порядку»
   становится ошибкой времени выполнения; доли не несут единиц.
2. **Абсолютное время `at: Duration`** (Compose `at`): единицы есть, порядок — проверка в
   `build`.
3. **Длительность сегмента `Duration`, сегменты подряд** (SwiftUI `KeyframeTrack`), плюс общий
   `total` у builder: порядок по построению, единицы типизированы, `total` выравнивает дорожки
   группы; остаток после последнего сегмента — hold.

**Выбрано 3.** Единственная ошибка размещения — переполнение `total` (`Overrun`).

### (b) Тип значения

1. `T: Lerp` — все tween-типы, но cubic требует сложения и масштабирования.
2. `T: TwoWayConverter` — cubic и проверка конечности в векторе, но линейный сегмент цвета
   перестал бы совпадать с `ColorTween` (Oklab по умолчанию — спека `interpolation`).
3. **`T: Lerp + TwoWayConverter`**: «к значению» через `Lerp::lerp_to` (тот же путь, что
   `Tween`), cubic и проверки — в векторе. `Matrix4` не поддерживается: трансформ — отдельные
   дорожки (поворот, масштаб, сдвиг), как у SwiftUI.

**Выбрано 3.** Если derive-карточка переименует `TwoWayConverter`, bound переименуется с ней.

### (c) TweenSequence

Оставить рядом — два способа одного (запрещено pre-1.0); слить веса в `Keyframes` — веса без
единиц возвращают проблему (a1). **Удалить**, тесты `weighted_sequences_*` переписать как
строки `keyframes_*` (эндпоинты, относительный прогресс, огромные длительности).

### (d) Stagger и цикл

1. **N контроллеров с задержкой старта**: N регистраций в Vsync, N слушателей, рассинхронизация
   при паузе `TickerMode`, N dispose — стоимость и поверхность отказов растут с N.
2. **Один контроллер, `Interval` на ребёнка**: дёшево, но только окна, без цикла.
3. **Один контроллер + `Stagger::delay(i)` + чтение дорожки со сдвигом**: окно —
   `value_at(elapsed.saturating_sub(delay))`, цикл — `value_at_looped(elapsed + total − delay)`.

**Выбрано 3.** Одна регистрация на индикатор при любом N; сдвиг — арифметика `Duration`.

### (e) Spring-сегменты

Скорость на стыке уже вычисляется для cubic (R10), так что spring-сегмент встаёт без изменения
модели: `SpringSimulation::new(spring, p0, p1, v_in)` по компонентам вектора, обрезка на
длительности сегмента или на `rest_time()` из спеки `physics` (её R17). Потребителя нет — не
реализуется (requirements «Вне scope»). Следствие: `physics` называет composition единственным
пользователем `SpringSimulation::rest_time` — без spring-сегмента он остаётся без production-пути
и должен уйти из той спеки, либо владелец утверждает spring-сегмент (тогда задача T3b по образцу
T3, тест `spring_keyframes_carry_velocity`: скорость на входе = `dx` предыдущего сегмента).

## Публичный API (новый модуль `crates/flui-animation/src/keyframes.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframes<T> { start: T, total: Duration, segments: Box<[Segment<T>]> }
#[must_use]
pub struct KeyframesBuilder<T> { start: T, total: Duration, segments: Vec<Segment<T>>, error: Option<KeyframesError> }

impl<T: Lerp + TwoWayConverter> Keyframes<T> {
    pub fn builder(start: T, total: Duration) -> KeyframesBuilder<T>;
    #[must_use] pub fn total(&self) -> Duration;
    #[must_use] pub fn value_at(&self, elapsed: Duration) -> T;        // clamp к [0, total]
    #[must_use] pub fn value_at_looped(&self, elapsed: Duration) -> T; // elapsed mod total
}
impl<T: Lerp + TwoWayConverter> KeyframesBuilder<T> {
    pub fn to(self, value: T, over: Duration, curve: impl Curve + 'static) -> Self; // owner-local (send-flip 39)
    pub fn cubic(self, value: T, over: Duration) -> Self;
    pub fn hold(self, over: Duration) -> Self;
    pub fn jump(self, value: T) -> Self;
    pub fn build(self) -> Result<Keyframes<T>, KeyframesError>;
}
impl<T: Lerp + TwoWayConverter> Animatable for Keyframes<T> { type Value = T; /* t∈[0,1] → t·total; NaN → 0 */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyframesError {
    #[error("keyframes total must be non-zero")] ZeroTotal,
    #[error("segment {index} ends at {end:?}, past total {total:?}")] Overrun { index: usize, end: Duration, total: Duration },
    #[error("segment durations overflow Duration at segment {index}")] DurationOverflow { index: usize },
    #[error("keyframe {index} has a non-finite component")] NonFiniteValue { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stagger { step: Duration, origin: StaggerOrigin }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StaggerOrigin { First, Last, Center, Index(usize) }
impl Stagger {
    pub const fn new(step: Duration, origin: StaggerOrigin) -> Self;
    #[must_use] pub fn delay(&self, index: usize, count: usize) -> Duration; // насыщение
}
```

`Segment<T>` приватен: `To { end: Duration, value: T, curve: ArcCurve }`, `Cubic { end, value,
v_in: T::Vector, v_out: T::Vector }`, `Hold { end }`, `Jump { at, value }`. Builder копит первую
ошибку и возвращает её из `build` (цепочка без `Result` посреди, в отличие от
`AnimationControllerBuilder::bounds`). Rustdoc: единицы, правая непрерывность, политика NaN,
отсутствие `Matrix4`, пример индикатора; doctest — пример из R9.

Удаляется: `TweenSequence`, `TweenSequenceItem` (+ строки `lib.rs:157-158`, `:193` — последнюю
удаляет prelude-чистка Z, если она раньше). Реэкспорт: `Keyframes, KeyframesBuilder,
KeyframesError, Stagger, StaggerOrigin` в корне `lib.rs`; строка в
`flui-sdk/tests/surface.rs` `measured` (ADR-0088 §4).

## Вычисление и инварианты

- `build` один раз считает концы сегментов (`checked_add`), скорости стыков cubic и проверяет
  конечность ключей; `value_at` не аллоцирует: бинарный поиск по `end` (`partition_point`),
  локальная доля `(elapsed − start).as_nanos() as f64 / len.as_nanos() as f64`.
- Правая непрерывность: при `elapsed == end_k` берётся сегмент `k+1` с долей 0 → клон его
  начального значения, т.е. ключа `k` (R2). `Jump` — сегмент нулевой длины; поиск пропускает его
  и возвращает значение после скачка.
- `To`: `p0.lerp_to(&p1, curve(s))`; результат кривой не конечен → `p0` (R8). Перекрут
  (`EaseOutBack`) экстраполирует, как `Tween`.
- `Cubic`: Эрмит в векторе `h00·p0 + h10·d·m0 + h01·p1 + h11·d·m1`, `s` — доля времени, `d` —
  длительность в секундах. Касательные: между cubic — `(p_{k+1} − p_{k−1})/(t_{k+1} − t_{k−1})`
  (неравномерный Catmull-Rom по времени ключей); у соседа `To` — `curve.slope(1|0)·(p1 − p0)/d`
  (`Curve::slope` из curves T9 — одна реализация производной, X13); у края, `hold`, `jump` — 0. Неконечный
  компонент → `p0` (R8). Это закрывает M-CRV-9: значение — функция времени, решения по x не
  требуется.
- Время — только `Duration` (не бывает NaN и отрицательным); `Animatable::transform` —
  единственный вход `f64`: clamp, NaN → 0, `total.mul_f64(t)` безопасен при `t ∈ [0,1]`.
- `value_at_looped`: `elapsed.as_nanos() % total.as_nanos()` в `u128` — `Duration::MAX` не
  паникует.
- `Stagger::delay`: `k = |origin − i|` в `f64` (center дробный), `Duration::try_from_secs_f64(
  step.as_secs_f64() · k)`, ошибка → `Duration::MAX`. `Index(j)` при `j ≥ count` — как есть.
- Без состояния, без lock, без `static`; `Keyframes<T>: Send + Sync`, если `T` такой.

## Потребители

| Виджет | Где | Дорожки | Контроллер | Отрисовка |
|--------|-----|---------|-----------|-----------|
| `ActivityIndicator` (новый) | `flui-widgets/src/controls/activity_indicator.rs` | 3 на `total` 6 с: глобальный поворот `to(1080°, 6 с, Linear)`; ступени `to(90°,300мс,EmphasizedDecelerate)`+`hold(1200мс)`×4; длина дуги `0.1 → to(0.87,3 с,Standard) → to(0.1,3 с,Standard)` | один, `repeat`, период 6 с | `CustomPaint` + `repaint()` = контроллер; `DrawOp::Arc` |
| `RefreshIndicator` | `scroll/refresh_indicator.rs:492` | — | у `ActivityIndicator` | замена `ColoredBox`, удаление DEFERRED-строк `:23-24`, `:465`, `:491` |
| `LinearProgressIndicator` (новый) | `packages/flui-material/src/progress_indicator.rs` | 4 на 1750 мс: `hold(delay).to(1, d, EmphasizedAccelerate)` для (0,1000), (250,1000), (650,850), (900,850) | один при `value = None`; нет при `Some` | `CustomPaint`, роли R17 |
| `CupertinoActivityIndicator` (новый) | `packages/flui-cupertino/src/activity_indicator.rs` | 1 на 1 с: 8 сегментов `to(A[(8−k) mod 8], 125мс, Steps::new(1, JumpAt::Start))` (потребитель `Steps`, curves); тик `i` — `value_at_looped(t + 1 с − Stagger(125мс, First).delay(i, 8))` | один, `repeat` | 8 `RRect` |

Кривые M3 — приватные константы в виджетах: Standard `Cubic(0.2,0,0,1)`, EmphasizedAccelerate
`(0.3,0,0.8,0.15)`, EmphasizedDecelerate `(0.05,0.7,0.1,1)` (androidx `MotionTokens.kt`
@ `e5da4a9c`). Отличие от Compose: там `using` на ключе-плато задаёт кривую следующего
интервала, и шаги поворота 0→90° фактически линейны; мы задаём кривую сегменту, ведущему к
90°, — намеренно (R3), закреплено тестом R13 на плато и монотонности. Контроллер создаётся по
контракту `ownership` (владеющий handle), до его слияния — текущий шаблон
`Vsync::register` + dispose (`refresh_indicator.rs:429-435`, `:592-604`).

Слои: material и cupertino видят новые типы через `flui_sdk::animation` (реэкспорт всего
крейта, `flui-sdk/src/lib.rs:27`); `ActivityIndicator` экспортируется рядом с `Slider`
(`flui-widgets/src/lib.rs:171`); `cargo xtask module-dag` должен разрешить `scroll → controls`
(проверка в T4).

## Adversarial review

- **Reentry слушателя в контроллер и vsync-реестр.** Дорожки слушателей не имеют; painter лишь
  читает `controller.value()` в `paint`. Повторная регистрация из колбэка — контракт
  `listener-delivery`. Тест потребителя: `two_indicators_share_one_vsync`.
- **Снятие/добавление слушателей во время раздачи.** `RenderCustomPaint` подписывается на
  `repaint()` при attach; размонтирование индикатора из value-слушателя соседа того же кадра —
  `indicator_unmount_mid_frame_releases_controller`.
- **Последний владелец освобождён из колбэка.** Индикатор держит контроллер в state; Drop
  state'а из колбэка снимает регистрацию через владеющий handle (`ownership`); тест тот же.
- **Два контроллера на одном vsync.** Каждый индикатор — один контроллер; N тиков не
  создают N регистраций (вариант d3). `two_indicators_share_one_vsync`.
- **Realm остановлен посреди анимации.** Повтор бесконечен; остановка realm должна снять
  регистрацию без кадра после — `indicator_realm_stop_mid_repeat` (зависит от `ownership`).
- **Retarget в последнем кадре.** Неприменим к дорожке; у `LinearProgressIndicator` смена
  `None → Some(v)` в последнем кадре цикла останавливает контроллер и рисует долю —
  `linear_progress_switches_to_determinate`.
- **dt = 0, огромный dt, время назад.** Чистая функция `Duration`; цикл по модулю `u128`.
  `keyframes_evaluation_is_order_independent`, `keyframes_clamp_and_loop`, строки R22.
- **Переполнение и NaN.** Сумма длительностей — `checked_add` (`DurationOverflow`); ключи —
  проверка конечности вектора; кривая и Эрмит — замена неконечного сэмпла на `p0`; stagger —
  насыщение. `keyframes_build_rejects_invalid_tracks`, `keyframes_never_publish_non_finite`,
  `stagger_delays_follow_origin`.
- **Panic в пользовательском коде.** Вызовы наружу: `Curve::transform` (в `build` — производная,
  в `value_at`), `Lerp::lerp_to`, `TwoWayConverter`. Панику в `build` получает вызывающий, частично
  собранная дорожка не публикуется (builder поглощён). Паника в `value_at` не меняет дорожку —
  `keyframes_survive_panicking_curve`; сдерживание в paint — политика рендера, не этой темы.
- **Аллокации в тике.** `value_at` без аллокаций (кроме `T::clone`); тест аллокаций Q0 получает
  строку `keyframes_value_at` для `f64`.

Остаточные риски: (1) точность касательной стыка cubic — точность `Curve::slope` (аналитика у
`Cubic`, разность второго порядка у прочих); (2) точная формула касательных SwiftUI не опубликована
([U]) — паритет не заявляется, контракт — наш (R9, R10); (3) Material-движение в нейтральном
`ActivityIndicator` — выбор вида по умолчанию, тема Material/Cupertino может его заменить
позже; (4) индикаторы под reduce motion — `Preserve` (решение X9: индикатор — обратная связь о
состоянии, не декоративное движение); контроллер создаётся с явным `.behavior(Preserve)`.

## Решения владельца (orchestration, 2026-10-06)

1. Оконный stagger — вместе с меню Material (B2); в этом проходе `Stagger` — тики
   `CupertinoActivityIndicator`.
2. Cubic-сегмент — исключение из правила «pub с потребителем» до появления потребителя-keyframes.
3. Spring-сегмента в этом проходе нет (`SpringSimulation::rest_time` — `pub(crate)`); M-CMP-11
   выносится.

## Чистка поверхности (назначено этой теме)

- **R4 аудита абстракций:** удалить `AnimationExt` (1 production-вызов `.curved(` —
  `packages/flui-cupertino/src/button.rs:562`, двойной `Arc` + cast; арифметика дублирует
  удаляемый `CompoundAnimation`), `CurveExt` (`then` = `with_curve`); `AnimatableExt` сводится к
  `animate` (`reversed` сталкивается по имени с `Curve::reversed`, `chain`/`with_curve` — 0–2
  вызова в крейте). Строка `flui-sdk/tests/surface.rs:20`. После R5 (frame-path-state) вызов
  становится `CurvedAnimation::new(controller.clone(), Curves::Decelerate)`.
- **R9 (карточка derive, тот же PR, что `Keyframes`):** `#[derive(Animatable)]` →
  `#[derive(TwoWayConverter)]`, вывод `TwoWayConverter + Lerp` (без `Lerp` производный тип не входит
  в `Keyframes<T: Lerp + TwoWayConverter>`); `is_f32`/`first_non_f32_field` → имена по `f64`
  (`flui-macros/src/derive_animatable.rs:102/117`); 1 пользователь (`tests/derive_animatable.rs:24`),
  реэкспорт `lib.rs:141`, `flui-macros/src/lib.rs:266-288`; trybuild не-`f64` поля уже есть.

## Владение

| Объект | Сильные | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `Keyframes<T>` | виджет-индикатор (значение) | — | с виджетом |
| контроллер индикатора | `DrivenController` в state (ownership) | — | drop state → unregister + dispose; `rebind` в `did_change_dependencies`; teardown — как unmount |
| `RenderCustomPaint` repaint-подписка | render object | — | `detach` |

Циклов нет: дорожка — значение, painter читает `controller.value()`, не захватывает state.
«Drop == 1» — `indicator_unmount_mid_frame_releases_controller` (Weak-сентинел контроллера).

## Паттерн

- **Builder с накоплением первой ошибки** — `KeyframesBuilder` (`#[must_use]`, `build -> Result`).
- **Закрытый приватный enum сегментов** — `Segment<T> { To, Cubic, Hold, Jump }`.
- **Ассоциированный тип** — `impl Animatable for Keyframes<T> { type Value = T; }` (R1).
- **Чистое значение без состояния** — время только `Duration`, без lock/`static`.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `TweenSequence` с весами без единиц | Flutter | `Keyframes` с `Duration` сегментов |
| N контроллеров с задержкой для stagger | Flutter `Interval` на ребёнка / N `AnimationController` | один контроллер + `Stagger::delay` |
| `AnimationExt`, `CurveExt` | Dart extension-методы — второй API | удалены (R4) |
| `#[derive(Animatable)]`, выводящий другой трейт | имя по Flutter-классу | `#[derive(TwoWayConverter)]` + `Lerp` (R9) |
| `using` на ключе задаёт кривую следующего интервала (Compose) | кривая на ключе | кривая на входящем сегменте (R3) |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Owner-local bound кривой (`impl Curve + 'static`) | нарушала | design.md:101 `+ Send + Sync` | снят (send-flip 39); хранение — стёртый тип по R2 |
| Ассоциированный тип вместо `Animatable<T>` | нарушала | design.md:107 | `type Value = T` |
| Одна реализация производной (X13) | нарушала | design.md:152-153, 216 | `Curve::slope` |
| Ext-трейты как второй API удаляются | нарушала (код) | ext.rs:151, tween_types.rs:602 | R4 здесь |
| Derive называется по выводимому трейту | нарушала (код) | flui-macros lib.rs:284 | R9 здесь |
| Решения владельца применены | нарушала | design.md:220-231 (X9, вопросы 1–3) | `Preserve`; вопросы закрыты |
| Builder: `#[must_use]`, ошибка `thiserror` + `#[non_exhaustive]` | соответствует | design.md:91-116 | — |
| Время — `Duration`, переполнение — `checked_add` | соответствует | design.md:142-161 | — |
| Bounds на impl, не на struct | соответствует | design.md:90-100 | — |
| Таблица владения | нарушала | — | раздел «Владение» |
## ADR

Не нужен: тип — значение внутри `flui-animation`, межкрейтового контракта нет (виджеты лишь
потребляют API). Решения (правая непрерывность, кривая у входящего сегмента, касательные,
политика NaN) идут в `crates/flui-animation/docs/ARCHITECTURE.md` через задачу Z с именами
тестов.

## Фрагмент `changelog.d/<branch-slug>.md`

```markdown
### Added
- `flui_animation::Keyframes`: keyframe tracks timed by `Duration` with per-segment curves,
  Catmull-Rom `cubic` segments, `hold` and `jump`; `Stagger` for per-index delays.
- `ActivityIndicator` widget; `RefreshIndicator` now shows an animated spinner.
- `flui-material` `LinearProgressIndicator`; `flui-cupertino` `CupertinoActivityIndicator`.

### Changed
- `#[derive(Animatable)]` is `#[derive(TwoWayConverter)]` and also derives `Lerp`.

### Removed
- `TweenSequence` and `TweenSequenceItem`: build a `Keyframes` track instead.
- `AnimationExt` and `CurveExt`; `AnimatableExt` keeps `animate` only. Use `CurvedAnimation::new`
  and `Tween::animate`.
```
