# flui-animation / physics — требования

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` 9a4daa3ed
- **Закрывает:** D-12, D-13, D-14, D-15, D-23, D-24, D-39, D-40 ([../tasks.md](../tasks.md))
- **Design:** [design.md](design.md); задачи — [tasks.md](tasks.md)
- **Связанные темы:** [retarget](../retarget/) (судьба `AnimatedValue` и пресетов пружины),
  [composition](../composition/) (keyframes со spring-сегментами), [frame-path-state](../frame-path-state/)
  (симуляции — простые значения без внутренних блокировок), controller-robustness (`error.rs`, `controller/run.rs`).

## Термины

- **Допустимая область пружины** — `ω = √(k/m)` и `ζ = c/(2√(km))` конечны, `ω > 0`, `ζ > 0`, и все
  производные константы решения (`ζω`, `ω²`, корни, амплитуды) конечны.
- **Момент покоя** `rest_time` — время с построения, начиная с которого симуляция гарантированно в
  допуске; `is_done(t) ⇔ t ≥ rest_time`. **Эталон** — аналитика (таблица ниже), не production-формула.

## Эталонные значения (аналитическое решение, перекрёстно проверено expm и единой формой)

| ζ | ω | x0 | v0 | t | x(t) | v(t) |
|---|---|---|---|---|---|---|
| 0.5 | 10 | 1 | 0 | 0.25 | −0.0233595799066923 | −2.74109898705702 |
| 1 | 10 | −1 | 5 | 0.2 | −0.270670566473225 | 2.03002924854919 |
| 2 | 10 | 0 | 10 | 0.3 | 0.129208025818252 | −0.346074592636829 |
| 0.2 | 2π | −1 | 5 | 0.3 | 0.588257945757625 | 2.62367533686863 |
| 0.7 | 4π | −1 | 0 | 0.25 | −0.0159125090286291 | 1.52626471111652 |
| 1 | √1500 | −1 | 0 | 0.05 | −0.423468514838734 | 10.8156746718564 |

Непрерывность у ζ=1 (ω=10, x0=1, v0=0, t=0.3): ζ=1−1e-6 → 0.199147825387572; ζ=1 → 0.199148273471459;
ζ=1+1e-6 → 0.199148721554790. Перцептивные параметры (первоисточник — документация Apple `Spring`:
`Spring(duration: 0.5, bounce: 0.3)` = `(1.0, 157.9, 17.6)`, `(1, 100, 10)` = `(0.63, 0.5)`):
(d=0.5 с, b=0.3) → k=157.913670417430, c=17.5929188601028; (d=1, b=0.5) → ζ=0.5, ω=2π.
**Не проверено по первоисточнику** (market.md [U]) и поэтому не закрепляется эталонным тестом:
ветка `b < 0` (`ζ = 1/(1+b)`, совпадает с Framer Motion, у Apple текста нет) и формула
`response → ω = 2π/response`. Большие t, overdamped:
ζ=3, ω=100, x0=1, v0=0, t=3 → x=4.56068935005893e-23 (наивная форма даёт NaN). Fling по умолчанию
(ω=√500, ζ=1, x0=−1.01, v0=1): пересечение границы 0.295369722567422 с. Friction drag 0.135, v0=8000:
остаток пути ≤ 0.5 px при t=4.48741315404835 с, ≤ 0.25 px при t=4.83355743878508 с.

## Требования

**R1 (D-12).** КОГДА вызывается конструктор пружины (`new(mass, stiffness, damping)`,
`with_damping_ratio`, `with_duration_and_bounce(Duration, bounce)`,
`with_response_and_damping(Duration, damping_fraction)`), СИСТЕМА ДОЛЖНА вернуть `Ok` ровно для
допустимой области и `Err(SimulationError)` с именем параметра иначе, в debug и в release одинаково:
NaN/±inf в любом аргументе; `mass ≤ 0`, `stiffness ≤ 0`, `damping ≤ 0`, `ratio ≤ 0`; `bounce ∉ (−1, 1)`;
`duration`/`response` = 0; `damping_fraction ≤ 0`; переполнение констант (`m=k=1e200`, `c=1e200`).
Тест: `spring_constructors_refuse_outside_the_admitted_domain` (таблица `run_table`,
`tests/contracts/spring.rs`). Нулевое затухание отклоняется: незатухающая пружина никогда не покоится.

**R2 (D-12).** Поля `SpringDescription` СИСТЕМА ДОЛЖНА держать приватными: значение вне допустимой
области непредставимо. Тест: trybuild-фикстура `spring_description_literal` (compile-fail, в существующем
trybuild-прогоне крейта или новом `[[test]]`, если его нет).

**R3.** КОГДА пружина задана через `(duration, bounce)` или `(response, damping_fraction)`, СИСТЕМА
ДОЛЖНА давать `ω = 2π/d`, `ζ = 1 − b` при `b ≥ 0`, `ζ = 1/(1 + b)` при `b < 0`, `ζ = damping_fraction`;
для `b ≥ 0` движение совпадает с пружиной `(m=1, k, c)` из документированных Apple пар с относительной
ошибкой ≤ 1e-12. Тест: `perceptual_springs_match_published_conversions` (таблица, только строки
`b ≥ 0`). Ветки `b < 0` и `response` документируются в rustdoc как непроверенные по первоисточнику и
тестируются только на внутренние свойства (без эталонных чисел): непрерывность ζ в `b = 0`,
монотонность ζ по `b`, `ζ > 1` при `b < 0`, `with_response_and_damping(d, 1 − b) ≡
with_duration_and_bounce(d, b)` при `b ∈ [0, 1)` — тест `perceptual_branches_are_consistent`. Когда
первоисточник найден (цитата в market.md), строки эталона добавляются в первую таблицу.

**R4 (D-39).** КОГДА симуляция пружины вычисляет `x(t)`/`dx(t)`, СИСТЕМА ДОЛЖНА совпадать с эталоном с
относительной ошибкой ≤ 1e-12 (абсолютной ≤ 1e-15 у нуля) и быть непрерывной по ζ через 1 (строки
непрерывности, разность соседних значений ≈ 0.448·Δζ ± 1e-9). Тест: `spring_matches_analytic_reference`.

**R5 (D-39).** Для всей допустимой области, начального смещения и скорости с конечной разностью
`|start − end|` и любого `t ∈ [0, +∞]` СИСТЕМА ДОЛЖНА публиковать конечные `x`, `dx`. Включая ζ ∈ {1e8,
1e12} (медленный корень не обнуляется: `x(1 с) ≠ x(0)`), большие t overdamped, `t = +inf` (→ `end`).
Тест: property `spring_outputs_are_finite_over_the_admitted_domain` (proptest, ≥ 4096 случаев, ω ∈
[1e-3, 1e4], ζ ∈ [1e-3, 1e8] лог-равномерно, |x0| ≤ 1e6, |v0| ≤ 1e7).

**R6.** КОГДА `t < 0` или `t` = NaN, СИСТЕМА ДОЛЖНА возвращать начальное состояние (`x = start`,
`dx = velocity`, `is_done = false`); КОГДА `t ≥ rest_time` (включая `+inf`) — ровно `end` и `0.0`;
`is_done` монотонна по t. Тест: `simulation_time_domain_edges` (таблица: spring, friction, bouncing).

**R7 (D-13).** КОГДА симуляция построена с допуском `Tolerance`, СИСТЕМА ДОЛЖНА выбрать `rest_time`
консервативно: ∀ t ≥ rest_time `|x(t) − end| ≤ distance` и `|dx(t)| ≤ velocity` (для производного
допуска velocity = distance·ω у пружины, остаток пути `|dx/ln d| ≤ distance` у трения); и не позже
`1.25·t_last + 2/ω`, где `t_last` — последний момент нарушения допуска эталонным решением, найденный
плотной выборкой (шаг 1e-4 с). Тест: property `rest_time_is_conservative_and_tight`.

**R8.** КОГДА пружина ведётся контроллером на виртуальном clock с кадрами 30/60/120/144 Гц, СИСТЕМА
ДОЛЖНА давать одинаковое значение в одинаковый момент (≤ 1e-12) и завершать прогон на первом кадре с
`t ≥ rest_time`. Тест: property `spring_run_is_independent_of_frame_partition`
(`AnimationController::animate_with` + `AnimationController::tick_at` с явными временами; тип
времени — по motion-clock).

**R9 (D-13).** `Tolerance` СИСТЕМА ДОЛЖНА строить только валидной: `Tolerance::new(distance, velocity)`
(distance ∈ (0, +inf), velocity ∈ (0, +inf]), `Tolerance::displacement(distance)` (скорость
производная), `Tolerance::for_device_pixel_ratio(dpr)` = `displacement(0.5/dpr)` (полпикселя
устройства); NaN, ≤ 0 и неконечный dpr → `Err`. Поле `time` и `Simulation::tolerance()` удалены.
Тест: `tolerance_constructors_validate_and_scale_with_dpr` (таблица).

**R10 (D-13).** КОГДА `Scrollable`, `PageView` или `RefreshIndicator` запускают баллистику, СИСТЕМА
ДОЛЖНА передать физике dpr презентации (`ScrollMetrics::device_pixel_ratio`; без pipeline owner — 1.0),
и симуляция завершается по допуску полпикселя устройства. Fling 8000 px/s, drag 0.135: при dpr 1 прогон
кончается на первом кадре ≥ 4.4874 с (сейчас 7.94 с), при dpr 2 — ≥ 4.8336 с. Тест:
`scroll_fling_rest_scales_with_device_pixel_ratio` (`flui-widgets/tests/scroll.rs`, ряд в
`tests/contracts.rs`; dpr задаётся через `PipelineOwner::set_device_pixel_ratio` харнесса). Предел
скорости fling (±8000) — владение задачи I3 flui-interaction; физика его только потребляет, тест
задаёт скорость ниже предела.

**R11 (D-14).** КОГДА `AnimationController::fling` достигает границы, СИСТЕМА ДОЛЖНА завершить прогон
(статус `Completed`/`Dismissed`, future разрешён) на первом кадре с t ≥ 0.29537 с для эталона по
умолчанию, а не через ~0.26 с после. Тест: `fling_completes_on_the_frame_that_reaches_the_bound`
(`tests/contracts/controller_sources.rs`, 60 Гц: завершение на кадре 18, не на 17).

**R12 (D-15).** КОГДА fling `BouncingScrollPhysics` в пределах диапазона уводит позицию трения за край,
СИСТЕМА ДОЛЖНА пройти край с непрерывными позицией и скоростью (скачок `dx` в момент передачи ≤ 1e-9
относительно), выйти за край, вернуться пружиной и остановиться ровно на крае; если финальная позиция
трения внутри диапазона — остановиться там без пружины. Тесты: `bouncing_simulation_hands_friction_to_spring_at_the_edge`
(таблица, `tests/contracts/simulation.rs`, эталон момента края — `ln(1 + ln d·Δx/v0)/ln d`) и
`bouncing_fling_into_the_edge_overscrolls_and_returns` (`flui-widgets/tests/scroll.rs`).

**R13 (D-40).** КОГДА границы симуляции заданы, СИСТЕМА ДОЛЖНА принимать их только как
`SimulationBounds::new(min, max)` (конечные, `min ≤ max`), иначе `Err`; ни одна симуляция не паникует
на границах. Scroll-физика с неупорядоченными экстентами возвращает `None`. Тест:
`bounds_refuse_unordered_and_nan` (таблица) и строка `inverted_extents_do_not_fling` в `tests/scroll.rs`.

**R14 (D-23).** `FrictionSimulation::new(drag, …)` СИСТЕМА ДОЛЖНА возвращать `Err` для `drag ∉ (0,1)`
и неконечных входов; покой — по остатку пути; `time_at_x` для позиции позади старта — `+inf`.
`FrictionSimulation::through` удаляется (нет production-вызова). Тест: `friction_rest_and_time_queries`
(расширение существующей таблицы `friction_preserves_small_decay_and_position_time_roundtrips`).

**R15 (D-24).** Модуль `smoothing` (нет production-пользователя) СИСТЕМА ДОЛЖНА удалить вместе с
примером и бенчем — решение владельца. Если владелец оставляет: `SmoothDamp` — точная `exp`,
`max_speed > 0` валидирован, `dt` типа `Duration`; тест `smooth_damp_is_frame_partition_independent`.

**R16 (D-39).** `AnimatedValue` остаётся: retarget делает его движком implicit-режимов (ведомым
Vsync через `DrivenController`, без `advance`) и владеет его переписыванием (retarget T4: отказ
неконечных компонент до мутации — retarget R14, конечность при повторных прерываниях — R6). Здесь —
только перевод `spring.rs` на `SpringSimulation::new(..) -> Result` (T1), без изменения поведения
`AnimatedValue`; тесты D-39 для `AnimatedValue` — строки retarget R6/R14.

**R17.** СИСТЕМА ДОЛЖНА давать `x`/`dx` пружины от заданных `(x0, v0)`; момент покоя
`rest_time` — `pub(crate)` (его читают `is_done`, fling `until_crossing`, settle-сетка). Публичным
он становится вместе со spring-сегментом keyframes, если владелец его утвердит (composition
design «(e)», requirements «Вне scope»). Тест: строка `rest_time_reports_the_settle_boundary` в
`rest_time_is_conservative_and_tight` (через `is_done`).

## Сценарии отказа

Симуляция — значение без колбэков и блокировок; слушатели и реестр — темы listener-delivery, controller-robustness.
| Сценарий | Поведение | Доказательство |
|---|---|---|
| dt = 0 / время назад | `t ≤ 0` → начальное состояние; контроллер не отматывает | R6 `simulation_time_domain_edges` |
| огромный dt, `t = +inf` | `end`, `dx = 0`, done | R5, R6 |
| NaN/inf во входах | `Err` при построении; ни одного NaN в выводе | R1, R9, R13, R14, R16 |
| переполнение `4mk`, `c²` | ω, ζ считаются без `4mk`; переполнение констант → `Err` | R1, R5 |
| retarget на последнем кадре | `AnimatedValue`: старт с `end`, v=0; fling-retarget — тема retarget | R16 |
| panic в пользовательской `Simulation::x` | вне темы: сдерживание в контроллере (controller-robustness) | — |
| dispose / два контроллера / остановка realm | вне темы: симуляция не держит общего состояния, `Arc<dyn Simulation>` иммутабелен | — |
