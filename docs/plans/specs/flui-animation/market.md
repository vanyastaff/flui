# flui-animation — рыночная норма и статус FLUI

> Историческая матрица восстановлена из `801543a3f` от 2026-10-06.
> Статусы и ссылки на строки ниже относятся к старой базе. Текущая маршрутизация всех
> требований: [readiness-matrix.md](readiness-matrix.md); доказательства: [readiness-audit.md](readiness-audit.md).

- **Статус:** черновик, на согласовании у владельца
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed`
- **Источники:** три исходные матрицы рыночного эталона (SwiftUI/Compose/Core Animation;
  CSS Easing/Transitions/WAAPI/Color/Transforms, Motion, Flutter как чек-лист; Rust-крейты и
  первичные источники по физике) с полными списками URL и закреплёнными коммитами сведены здесь.
  На строку — один первичный URL (определения ссылок в конце). Факты о FLUI — из аудита семи зон
  (контроллер/vsync, кривые/tween, физика, композиция, derive, тесты, потребители); каждая ссылка
  `file:line` перепроверена по дереву на базе.

Строка закрывается, когда её статус становится «есть» или владелец утверждает scope-решение
([orchestration.md](orchestration.md), «Scope-решения»).

**Статусы.** `есть` — код и тест, который упал бы без него (тест назван). `частично` — код без
такого теста, тест не независимый, или покрыта часть требования. `нет` — кода нет. `сломано` —
код есть, но найденный дефект делает поведение неверным.
**Эталон.** `[U]` — исходная матрица пометила факт как непроверенный по первичному источнику; такой факт не закрепляется тестом как контракт, пока не сверен с первичным источником.
**Пути** без префикса — `crates/flui-animation/src/`; `tests/…`, `benches/…`, `docs/…` — тоже
относительно `crates/flui-animation/`. **Тема** — спека, которая закроет строку; `вынесено` —
решение владельца «вне прохода» (orchestration.md, «Scope-решения»); `—` у строк со статусом `есть`.

## Время

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-TIME-1 | Значение любого запуска (tween, simulation, repeat) — функция времени от старта: один интервал, разбитый на разное число кадров, даёт то же значение | Compose `getValueFromNanos` [ax-suspend] | частично | чистые функции времени: `controller.rs:1870`, `:1897`; тест разбиения только для repeat — `repeat_value_is_partition_invariant_across_a_skipped_cycle` (`controller_tests.rs:291`) | time |
| M-TIME-2 | Все анимации кадра сэмплируют один timestamp | WAAPI §4.4 [wa1]; Flutter `Ticker` [fl-ticker] | частично | `crates/flui-runtime/src/ui_realm/frame.rs:94`, `:120` (`tick_all(now)`); порядок не закреплён тестом (`crates/flui-runtime/ARCHITECTURE.md`, «Unasserted») | time |
| M-TIME-3 | Нефинитное или убывающее время кадра не ломает запуск | Compose `withFrameNanos`, строго монотонно [axrt-monotonic] | сломано | `vsync.rs:488` якорит `Some(now_secs)` без проверки: NaN замораживает запуск в начале, `has_running()` держит кадры бесконечно; время назад откатывает значение (`controller.rs:1897`) | time |
| M-TIME-4 | В простое кадры не запрашиваются: только пока есть живой запуск | Compose `BroadcastFrameClock` [axrt-broadcast]; egui [egui-context] | есть | `vsync.rs:338`, `controller.rs:1847`; тесты `tests/contracts/controller_sources.rs:841` («completed runs quiesce»), `a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value` (`controller_tests.rs:187`) | — |
| M-TIME-5 | Скрытое окно не тикает; поведение при возобновлении (продолжить или догнать) выбрано и закреплено | Compose `PausableMonotonicFrameClock` [axui-recomposer] | частично | runner выключает кадры (`crates/flui-app/src/app/runner/frame_pacing.rs:47`); при возобновлении time-запуск прыгает по wall clock; теста нет | time |
| M-TIME-6 | Mute ≠ pause: заглушённое поддерево не тикает и не держит кадры, время идёт | Flutter `Ticker.muted` / `TickerMode` [fl-ticker] | есть | `vsync.rs:299`; тест `a_muted_ancestor_starves_an_unmuted_descendant` (`vsync.rs:552`) | — |
| M-TIME-7 | Замедление времени монотонно (смена коэффициента ребейзит эпоху) и принадлежит презентации | Flutter `timeDilation` + `resetEpoch` [fl-binding] | сломано | `controller.rs:1897`, `:2521` делят всё прошедшее время запуска на текущий коэффициент: 0.5 s при смене 1 → 5 откатывает значение к 0.1; коэффициент — процесс-глобальный `static` (`crates/flui-scheduler/src/config.rs:43`) | time |
| M-TIME-8 | Детерминированные виртуальные часы для тестов | Compose `mainClock.autoAdvance` [adev-testing] | есть | `tick_at` (`controller.rs:1870`), `Vsync::tick_all` (`vsync.rs:440`), `ManualClock` в `HeadlessBinding` (`crates/flui-testing/src/lib.rs:198`); тест `second_run_ticks_from_its_own_start_not_a_stale_anchor` (`crates/flui-testing/tests/controller_restart.rs:21`) | — |
| M-TIME-9 | Время типизировано и различимо до микросекунд | WAAPI §4.2, §8.1 [wa1] | частично | конфигурация — `Duration`; тик — голые `f64` секунды (`controller.rs:1870`); Vsync — на целочисленной нс-сетке | time |
| M-TIME-10 | Скорость: аналитическая у симуляции, производная кривой у tween | Flutter `_InterpolationSimulation.dx` [fl-controller]; Motion `getVelocity` [mo-velocity] | сломано | `velocity()` (`controller.rs:1766`) игнорирует кривую и отдаёт линейное среднее (`:1782`); тест только знака (`repeat_reverse_leg_velocity_is_negative`, `controller_tests.rs:440`) | retarget |
| M-TIME-11 | Предпочтительная частота кадров / троттлинг анимации | Core Animation `preferredFrameRateRange` [ca-framerate]; GPUI `with_max_fps` [gpui-animation] | нет | — | вынесено (2026-10-06) |
| M-TIME-12 | Скорость воспроизведения на анимацию (знаковая, 0 допустим), смена без скачка | WAAPI §4.5.15 [wa1]; SwiftUI `speed(_:)` [swiftui-animation] | нет | только глобальный `TIME_DILATION` | motion-clock |

## Прерываемость

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-INT-1 | Retarget начинается с текущего отображаемого значения | CSS Transitions §3 [tr1]; Compose `Animatable` [ax-animatable] | частично | implicit: `forward_from(Some(0.0))` с `begin` = текущее (`crates/flui-widgets/src/animated/implicitly_animated.rs:152`); тест `animated_opacity_retargets_from_the_current_value_midflight` (`crates/flui-widgets/tests/implicit_animations.rs:61`) с допуском 0.2; `animate_to` контроллера (`controller.rs:1109`) не покрыт | retarget |
| M-INT-2 | Retarget пружины сохраняет скорость (C¹) | SwiftUI `spring(response:…)` [swiftui-spring-anim]; Compose `Animatable` [ax-animatable] | частично | `AnimatedValue::animate_to` (`spring.rs:133`) переносит x/dx; тест `retarget_preserves_velocity` (`spring.rs:191`) проверяет направление одного шага; у контроллера retarget со скоростью нет; implicit-виджеты перезапускают кривую с нуля (`implicitly_animated.rs:152`) — C¹-разрыв, как у Flutter [fl-implicit] | retarget |
| M-INT-3 | Наследует ли time-defined пружина скорость прерванной анимации — решено явно и закреплено | Motion: time-defined не наследуют [mo-spring] | нет | — | retarget |
| M-INT-4 | Длительность retarget масштабируется по оставшемуся расстоянию; вырожденная длительность не паникует | Flutter `_animateToInternal` [fl-controller] | сломано | `scaled_run_duration` (`controller.rs:2510`), тест `forward_from_mid_scales_run_duration` (`controller_tests.rs:29`); `base.mul_f64` (`:2514`) паникует при `Duration::MAX` под lock после частичной мутации запуска | controller-robustness |
| M-INT-5 | Разворот к исходной точке укорачивает длительность на пройденную долю (reversal shortening) | CSS Transitions §3.1 [tr1] | нет | implicit перезапускает полную длительность (`implicitly_animated.rs:152`) | retarget |
| M-INT-6 | Скорость жеста передаётся в settle-анимацию, нормированная на протяжённость | UIKit `initialVelocity` = v / Δ [uikit-velocity] | сломано | Drawer делит на ширину (`packages/flui-material/src/drawer.rs:618`); Dismissible умножает на фиксированные `1/300` (`crates/flui-widgets/src/interaction/dismissible.rs:107`, `:1175`) — скачок скорости на отпускании; back gesture скорость отбрасывает | integration |
| M-INT-7 | Разворот на лету | UIKit `isReversed` [uikit-isreversed] | есть | `reverse()` (`controller.rs:899`); тест `reverse_curve_locked_to_run_entry_direction` (`curved.rs:235`) | — |
| M-INT-8 | Scrub/seek: установка значения без запуска | UIKit `fractionComplete` [uikit-propertyanimator]; Compose `SeekableTransitionState` [ax-transition] | есть | `set_value` (`controller.rs:2151`); тесты `set_value_nan_is_canonicalized` (`controller_contract`, `controller_tests.rs:636`), `release_matrix_fling_and_slow_release` (`crates/flui-widgets/tests/back_gesture.rs:48`) | — |
| M-INT-9 | Новый запуск отменяет прежний; ожидающий получает отмену; порядок статусов определён | Compose `MutatorMutex` + `CancellationException` [ax-animatable] | есть | тесты `stop_cancels_the_active_run` (`controller_tests.rs:559`), `a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable` (`:493`) | — |
| M-INT-10 | Завершение доставляется ровно один раз; без движения — синхронно | SwiftUI `withAnimation(completion:)` [swiftui-withanimation] | есть | `controller.rs:856`, `:964`, `:1250`; тесты `repeat_with_zero_period_settles_synchronously_at_the_call` (`controller_tests.rs:404`), `simulation_run_future_resolves_ok_when_the_simulation_finishes` (`:544`) | — |
| M-INT-11 | `finish`: мгновенный переход в конец с доставкой завершения | WAAPI §4.5.13 [wa1]; Motion `complete` [motion-animate] | нет | — | controller-robustness |
| M-INT-12 | Ограниченный контроллер не выходит за bounds и не публикует NaN ни в одном виде запуска | Compose `BoundReached` [ax-animatable] | сломано | simulation клампится (`controller.rs:1963`), кривой запуск — нет (`:1937`-`1938` → `:2012`): `EaseOutBack`/Elastic выводят значение за `[lower, upper]`, `ElasticInCurve::new(0.0)` даёт NaN | controller-robustness |
| M-INT-13 | Аддитивное наложение или merge-политика для непружинных прерываний | SwiftUI `shouldMerge` [swiftui-merge]; Core Animation `isAdditive` [ca-additive] | нет | — | вынесено при условии: тест непрерывности retarget |

## Физика

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-PHY-1 | Пружина в замкнутой форме для ζ < 1, ζ = 1, ζ > 1, сверенная с независимым эталоном | Flutter `_SpringSolution` [fl-spring]; Compose [ax-springsim]; GPUI propagator [gpui-spring] | частично | `simulation.rs:430` (выбор режима), `:497`, `:531`, `:572`; тест `spring_regimes_preserve_initial_conditions_and_settle` (`simulation.rs:1000`) сверяет `dx` с конечной разностью `x`, не с аналитикой (эталоны — раздел ниже) | physics |
| M-PHY-2 | Непрерывность через ζ = 1 и конечность при больших t, ζ, m·k | GPUI: стабильный медленный корень, точный propagator [gpui-spring] | частично | медленный корень по Виету (`simulation.rs:531`); `x(+inf)` = NaN для critical/under (`:497`, `:572`); `4mk` переполняется при m = k = 1e200 → NaN; тестов нет | physics |
| M-PHY-3 | Параметризация stiffness + damping ratio с валидацией | Compose `spring(dampingRatio, stiffness)` [ax-springsim]; Flutter `withDampingRatio` [fl-spring] | есть | `with_damping_ratio` (`simulation.rs:162`); prod: scroll physics, page view; тест `spring_regimes_preserve_initial_conditions_and_settle` (`simulation.rs:1000`) | — |
| M-PHY-4 | duration + bounce и response + dampingFraction: ω = 2π/d, ζ = 1 − b (b ≥ 0), 1/(1 + b) (b < 0); вне диапазона — отказ | SwiftUI `Spring(duration:bounce:)` [swiftui-spring] (ветвь b < 0 [U]); Flutter `withDurationAndBounce` [fl-spring] | сломано | формулы верны (`simulation.rs:183`, `:213`), проверки только `debug_assert!` (`:186`, `:215`): b = 1 не затухает, b = −1 и d = 0 дают NaN, b = −2 классифицируется Critical и уходит в inf, `fling_with` его принимает (`controller.rs:1629`); тестов нет | physics |
| M-PHY-5 | Пресеты smooth/snappy/bouncy (d = 0.5; b = 0 / 0.15 / 0.3) и обратные геттеры | SwiftUI `.smooth/.snappy/.bouncy` [swiftui-animation] | частично | `simulation.rs:229`-`243`, `damping_ratio()` `:251`, `bounce()` `:257`; ни тестов, ни prod-пользователей | physics |
| M-PHY-6 | Параметры не обходят валидацию (публичные поля), нефинитное отвергается fallible-конструктором, состояние не латчит NaN | Compose: stiffness > 0, ratio ≥ 0 [ax-springsim]; Motion: невалидная физика → дефолты парой [mo-spring] | сломано | `pub` поля `SpringDescription` (`simulation.rs:121`-`128`); `AnimatedValue::advance(NaN)` (`spring.rs:156`) отравляет состояние навсегда, `is_settled` (`:178`) остаётся false | physics |
| M-PHY-7 | Оценка settling duration по ε | SwiftUI `settlingDuration`, ε = 0.001 [swiftui-settling]; Compose `SpringEstimation` [ax-springest] | нет | — | physics |
| M-PHY-8 | Порог покоя масштабирован под единицу значения; abs(v) ≤ ε·ω | Compose `visibilityThreshold` (px 1.0, dp 0.4, float 0.01) [ax-visthr]; GPUI `is_settled` [gpui-spring] | сломано | `Tolerance::DEFAULT` 1e-3 абсолютный (`simulation.rs:42`); `BoundedFrictionSimulation::new` без tolerance (`:922`): fling 8000 px/s заканчивается на 7.94 s при визуальном конце 4.83 s (~190 кадров субпиксельных тиков) | physics |
| M-PHY-9 | Fling к границе завершается при достижении границы | Flutter `fling`: tolerance (velocity ∞, distance 0.01) [fl-controller] | сломано | `FLING_TOLERANCE` (`controller.rs:27`) используется только для `distance`; пружина строится с дефолтным tolerance (`:1627`): запуск стоит у границы ~0.26 s (≈16 кадров), статус `Forward`, future опаздывает | physics |
| M-PHY-10 | Экспоненциальное затухание с замкнутыми x(t), v(t), `final_x` | Flutter `FrictionSimulation` [fl-friction]; Compose `exponentialDecay` [ax-decay] | есть | `simulation.rs:593`, `final_x` `:651`; тест `friction_preserves_small_decay_and_position_time_roundtrips` (`tests/contracts/simulation.rs:66`) | — |
| M-PHY-11 | `time_at_x` и `through` корректны во всей области | Flutter `timeAtX` (+inf за стартом), `.through` [fl-friction] | сломано | `time_at_x` за стартом даёт отрицательное время (`simulation.rs:665`); `through` паникует при underflow drag (`:699`) и с `end_velocity = 0` не завершается никогда (`:705`-`708`, `:730`) | physics |
| M-PHY-12 | Точка покоя decay для snap и передача той же скорости в пружину | Compose `calculateTargetValue` [ax-decayspec]; WWDC18 projection [wwdc18-fluid] | частично | `final_x` (`simulation.rs:651`) без prod-пути; рецепта snap нет | physics |
| M-PHY-13 | Hand-off friction → spring у границы (bouncing overscroll) | Motion `inertia` [mo-inertia] | нет | `BouncingScrollPhysics` в диапазоне использует `BoundedFrictionSimulation`: fling упирается в край без overscroll | physics |
| M-PHY-14 | Сглаживание независимо от частоты кадров: `1 − e^(−λ·dt)` | Bevy `smooth_nudge` [bevy-common]; Driscoll [driscoll] | частично | `exp_decay` точен (`smoothing.rs:29`), `exp_decay_half_life` (`:39`); тестов нет | physics |
| M-PHY-15 | Критически демпфированное следование без зависимости от dt и без паник | точный критический propagator (раздел ниже); Unity `SmoothDamp` — противопример с полиномом [unity-mathf] | сломано | полином GPG4 вместо `exp` (`smoothing.rs:187`) — ≈0.4 % между 30 и 60 Гц; отрицательный `max_speed` → panic в `clamp` (`:193`); тест `damped_motion_remains_usable_after_idle_ticks` (`tests/contracts/simulation.rs:119`) этого не ловит | physics |
| M-PHY-16 | Симуляция, которая не может завершиться (ζ = 0, гравитация от цели), отвергается или ограничена | Flutter `fling` отвергает underdamped [fl-controller] | нет | `simulation.rs:137`, `:162` допускают damping 0; `GravitySimulation` (`:740`) не завершается при движении от цели | physics |

## Кривые

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-CRV-1 | `cubic-bezier`: x1, x2 ∈ [0, 1] проверяются при любом способе создания | CSS Easing 2 §2.2 [eas2] | сломано | `Cubic::new` — `const fn` без проверки, `pub` поля + serde (`curve.rs:229`-`244`); x вне [0, 1] даёт немонотонный x(s), бисекция берёт произвольный корень | curves |
| M-CRV-2 | Решатель bezier точен по выходу (Newton 1e-7 + bisection) | WebKit `UnitBezier` [webkit-unitbezier] | сломано | допуск 1e-6 по x (`curve.rs:271`): у вертикальной касательной `EaseInOutExpo` (`:1045`) ошибка y до 9.1e-3; тест `cubic_solver_inverts_x` (`:1237`) проверяет x с допуском 1e-3 (`:1250`) | curves |
| M-CRV-3 | Именованные кривые с точными контрольными точками CSS/Material и верной документацией | CSS Easing 2 §2.2 [eas2]; Compose `Easing.kt` [ax-easing] | частично | `Ease` (`curve.rs:1087`), `EaseIn` `:1012`, `EaseOut` `:1015`, `EaseInOut` `:1018`, `FastOutSlowIn` `:1021` совпадают с эталоном; значения тестом не закреплены; описание `SlowOutFastIn` (`:1023`), `EaseInOutCubic`, `EaseIn/OutBack` противоречит точкам | curves |
| M-CRV-4 | Точные концы: transform(0) = 0, transform(1) = 1 у каждой кривой | Flutter `Curve.transform` [fl-curves] | сломано | `ReverseCurve` даёт 1 → 0 (`curve.rs:991`); `SawTooth(1) = 0` (`:131`); `CatmullRomCurve` — y первой/последней точки; Elastic прыгает на 2⁻¹⁰ у концов; теста-каталога нет | curves |
| M-CRV-5 | Interval (под-диапазон одного контроллера) | Flutter `Interval` [fl-curves] | частично | `curve.rs:141`, prod в hero flight и dismissible; тестов нет | curves |
| M-CRV-6 | `steps(n, jump-start / jump-end / jump-none / jump-both)` с before-flag | CSS Easing 1 §2.3.1 [eas1]; Bevy `JumpAt` [bevy-easing] | нет | только одноступенчатый `Threshold` (`curve.rs:197`) | curves |
| M-CRV-7 | `linear(...)`: каноникализация точек и экстраполяция | CSS Easing 2 §2.1 [eas2] | нет | — | curves |
| M-CRV-8 | Экстраполяция cubic-bezier вне [0, 1] по касательной в конце | CSS Easing 2 §2.2 [eas2]; WebKit [webkit-unitbezier] | нет | все кривые клампят t | вынесено (2026-10-06) |
| M-CRV-9 | Сплайн через ключи (x, y): Catmull-Rom с решением по x | SwiftUI `CubicKeyframe` [swiftui-cubickeyframe] | сломано | `CatmullRomCurve` игнорирует x (`curve.rs:854`): [(0,0),(0.9,0.5),(1,1)] при t = 0.9 даёт 0.932 вместо 0.5; документация tension инвертирована (`:800`); пустой `points` → underflow (`:831`) | composition (сегмент keyframes с решением по x), затем удаление `CatmullRom*` |
| M-CRV-10 | Пружина как easing-кривая (сэмплированная) | Motion `spring` → `linear()` [mo-spring]; GPUI `sampled_easing` [gpui-spring] | нет | — | curves |
| M-CRV-11 | Одна политика NaN и t вне [0, 1] для всех кривых и твинов | CSS Easing 1 §2: чистая функция [eas1] | частично | NaN → 0 (`Cubic`, `curve.rs:320`), → 1 (`Threshold`), иначе NaN; `IntTween` клампит (`tween_types.rs:117`), `Tween` — нет (`:64`) | curves |
| M-CRV-12 | Параметры кривых (midpoint, period, points) валидируются и при serde/литерале | Flutter asserts `ThreePointCubic` [fl-curves]; образец в FLUI — `Split` (`curve.rs:470`) | сломано | `ThreePointCubic`: `pub` + `Deserialize` (`curve.rs:335`) → деление на 0 (`:398`); `ElasticInCurve::new(0.0)` (`:595`) → `sin(±inf)` = NaN (`:619`) | curves |

## Интерполяция

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-INTP-1 | Числа — линейно, целые — через вещественные с округлением | CSS Values 4 §5.2.1 [val4] | есть | `crates/flui-foundation/src/geometry/lerp.rs:44`; `IntTween` (`tween_types.rs:114`); тест `integer_tweens_interpolate_across_the_full_range` (`tests/contracts/tween.rs:39`) | — |
| M-INTP-2 | Перцептивный цвет (Oklab) с premultiplied alpha | CSS Color 4 §13.2, §13.4 [col4] | сломано | `OklabColorTween` (`tween_types.rs:234`) → `lerp_oklab` (`crates/flui-painting/src/styling/color.rs:610`) с прямой alpha: красный → `TRANSPARENT` темнеет к середине; комментарий `color.rs:608` утверждает обратное; тестов нет | interpolation |
| M-INTP-3 | Дефолтный `ColorTween`: premultiplied, без 8-битной ступени на медленном переходе | CSS Color 4 §13.4 [col4] (Flutter `Color.lerp` — без premultiply) | частично | premultiplied sRGB (`color.rs:247`-`287`), каналы округляются до u8; тест `test_color_tween` (`tween_types.rs:627`) — только округление | interpolation |
| M-INTP-4 | Полярные пространства (OkLCh) с методами hue shorter/longer/increasing/decreasing | CSS Color 4 §13.5 [col4]; palette `Mix` [palette-mix] | нет | — | вынесено (2026-10-06) |
| M-INTP-5 | Матрица: декомпозиция с безопасным нулевым масштабом, slerp, skew/perspective, сингулярная — дискретно | CSS Transforms 1 §9-11 [tf1], 2 §13.1.2 [tf2] | сломано | `Matrix4::lerp` (`crates/flui-foundation/src/geometry/matrix4.rs:173`): нулевой масштаб → NaN во всех внутренних t; skew/perspective отбрасываются (`:168`); тест `matrix4_lerp_rotation_slerps_not_collapses` (`lerp.rs:129`) — только вращение | interpolation |
| M-INTP-6 | Углы без заворачивания: многооборотный поворот выразим | CSS Transforms 1 §10 [tf1] | нет | типа угла нет; slerp `Matrix4` идёт по кратчайшей дуге | interpolation |
| M-INTP-7 | Любой тип анимируется через вектор фиксированной размерности с покомпонентной скоростью | Compose `TwoWayConverter` [adev-customize] | частично | `TwoWayConverter` (`spring.rs:25`), `AnimatedValue` (`:100`); тест `retarget_preserves_velocity` (`:191`); production-пути нет | interpolation |
| M-INTP-8 | Derive для пользовательских типов: реализует одноимённый трейт, допускает вложенные Offset/Color | Compose converters [adev-customize]; Bevy `Animatable` [U] | сломано | `#[derive(Animatable)]` (`lib.rs:141`) генерирует `TwoWayConverter` (`crates/flui-macros/src/derive_animatable.rs:85`), а не `Animatable<T>`: тип нельзя tween-ить; только поля `f64`; тест `named_struct_round_trips_through_vector` (`tests/derive_animatable.rs:32`) | derive |
| M-INTP-9 | Порог покоя на тип через конвертер | Compose `defaultVisibilityThresholdFor` [ax-visthr] | нет | `AnimatedValue` берёт общий `Tolerance` | physics |
| M-INTP-10 | Дискретные значения переключаются только по opt-in, в p = 0.5 | CSS Transitions 2 `allow-discrete` [tr2] | нет | — | вынесено (2026-10-06) |

## Композиция

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-CMP-1 | Ключевые кадры: времена, easing на сегмент, чистая оценка по времени и прогрессу | SwiftUI `KeyframeTimeline` [swiftui-keyframetimeline]; WAAPI §5.3 [wa1] | частично | `TweenSequence` по весам (`tween_types.rs:340`) + `ChainedTween` (`:553`); тесты `weighted_sequences_preserve_endpoints_and_relative_progress` (`tests/contracts/tween.rs:98`), `weighted_sequences_reject_invalid_edited_configuration` (`:153`); keyframe-типа с временами нет; production-пользователей нет | composition |
| M-CMP-2 | Delay (и end delay) на уровне анимации | SwiftUI `delay` [swiftui-animation]; WAAPI §4.6.5 [wa1] | нет | только `Interval(begin > 0)` | composition |
| M-CMP-3 | Repeat N / forever, autoreverse, период | SwiftUI `repeatCount` [swiftui-animation]; Flutter `repeat` [fl-controller] | есть | `repeat_with` (`controller.rs:1390`); тесты `repeat_contract` (`controller_tests.rs:671`) | — |
| M-CMP-4 | Последовательность и группа на времени родителя | WAAPI 2 §2.9 [wa2]; Motion timeline [motion-animate] | нет | только ожидание `TickerFuture` | composition |
| M-CMP-5 | Stagger с origin (first, last, center, index) | Motion `stagger` [mo-stagger] | нет | вручную через `Interval` | composition |
| M-CMP-6 | Полная модель тайминга WAAPI: fill, direction, iterationStart, before-flag | WAAPI §4.6-4.7 [wa1] | нет | — | вынесено (2026-10-06) |
| M-CMP-7 | Арифметическая композиция двух анимаций: конечный результат, определённый статус | WAAPI §5.4.4 composite [wa1] | сломано | `CompoundAnimation` (`compound.rs:15`): `Divide` публикует NaN/inf (`:190`), статус всегда первого (`:212`); тест `test_compound_animation_status` (`:268`) — только статус; production-пользователей нет | вынесено: удалено, рыночной нормы нет |
| M-CMP-8 | Переключение поездов (train hopping): слушатели получают статус нового поезда, родители опрашиваются вне lock | Flutter `TrainHoppingAnimation` [fl-animations] [U] | сломано | `AnimationSwitch` (`switch.rs:63`): на hop статус не рассылается (`:313`-`345`); `value()`/`status()` родителя под `Mutex` (`:445`, `:450`) — self-deadlock при reentry | listener-delivery |
| M-CMP-9 | Отдельные длительность и кривая обратного хода | Flutter `reverseDuration` [fl-controller] | есть | `set_reverse_duration` (`controller.rs:733`), `CurvedAnimation::with_reverse_curve`; тест `reverse_curve_locked_to_run_entry_direction` (`curved.rs:235`) | — |
| M-CMP-10 | Смена родителя прокси атомарна и доставляет уведомления | Flutter `ProxyAnimation` [fl-animations] [U] | сломано | `set_parent` (`proxy.rs:193`) меняет три `RwLock` по очереди (`:203`-`209`): параллельные вызовы дают `parent = B` с подпиской на A; panic в teardown (`:221`) теряет уведомления; тест `proxy_parent_queries_allow_reentrant_replacement` (`tests/contracts/proxy.rs:124`) — только reentry | listener-delivery |
| M-CMP-11 | Переходы нескольких свойств по состоянию; phase animator | Compose `updateTransition` [adev-value]; SwiftUI `PhaseAnimator` [swiftui-phaseanimator] | нет | — | composition (вынос, если keyframes закрывают сценарий) |

## Доступность

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-A11Y-1 | Системный reduce motion доходит до фреймворка и обновляется реактивно | Media Queries 5 `prefers-reduced-motion` [mq5]; SwiftUI `accessibilityReduceMotion` [swiftui-reducemotion] | нет | `AccessibilityFeatures` (`crates/flui-semantics/src/accessibility.rs:15`) лежит в `crates/flui-app/src/app/runtime.rs:97`, никто не пишет и не читает; ОС не опрашивается; `did_change_accessibility_features` без источника (`crates/flui-view/src/binding.rs:1461`) | reduce-motion |
| M-A11Y-2 | Политика приложения user / always / never поверх ОС | Motion `MotionConfig reducedMotion` [mo-reduced] | нет | — | reduce-motion |
| M-A11Y-3 | Opt-out на анимацию: essential motion (скролл) не отключается | Flutter `AnimationBehavior` normal/preserve [fl-controller] | частично | `AnimationBehavior` (`status.rs:114`) есть, контроллер и виджеты его не читают | reduce-motion |
| M-A11Y-4 | Сокращение, а не пропуск: статус и завершение доставляются; oneshot — в конечном, repeat — в начальном состоянии | Flutter: длительность × 0.05 [fl-controller]; GPUI `reduce_motion` [gpui-animation] | нет | — | reduce-motion |
| M-A11Y-5 | Системный масштаб длительностей: 0 — конец в следующем кадре, < 0 — отказ | Compose `MotionDurationScale` [axui-mds] | нет | `TIME_DILATION` — debug-коэффициент, к системе не подключён | reduce-motion |
| M-A11Y-6 | Отдельный флаг «предпочитать cross-fade» | UIKit `prefersCrossFadeTransitions` [uikit-crossfade] | нет | — | вынесено (2026-10-06) |

## Владение и жизненный цикл

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-OWN-1 | Один писатель на значение; пользовательский Curve/Simulation не коммитит устаревший сэмпл | Compose `MutatorMutex` [ax-animatable] | есть | тест `controller_sources_allow_reentry_and_preserve_run_ownership` (`tests/contracts/controller_sources.rs:1694`) | — |
| M-OWN-2 | Анимация привязана к области UI: регистрация и dispose автоматически при unmount | Compose `remember` / `LaunchedEffect` [adev-value] | сломано | пары `register`/`unregister`/`dispose` руками в ~14 файлах; `FloatingHeaderHostState::dispose` не вызывает `dispose()` контроллера (`crates/flui-widgets/src/scroll/sliver_persistent_header.rs:244`, `:292`); 4 конструктора на одну роль, пакеты создают одноразовый `UpdateScheduler` | ownership |
| M-OWN-3 | После dispose мутации отвергаются, слушатели очищены | UIKit animator state machine [uikit-propertyanimator]; контракт `docs/ARCHITECTURE.md:836` | сломано | `set_value` (`controller.rs:2151`) без проверки; `add_status_listener` принимается после `dispose` (`:2194`) → цикл `Inner → cb → controller`; тест `disposed_controller_rejects_forward` (`controller_tests.rs:92`) — только `forward` | controller-robustness |
| M-OWN-4 | Контроллер следует за сменой ambient-часов (замена Vsync, перенос поддерева по GlobalKey) | Compose: часы берутся из контекста композиции [axrt-monotonic] | сломано | `VsyncScope::update_should_notify` всегда `false` (`crates/flui-widgets/src/animated/vsync_scope.rs:76`); перерегистрация есть только в `TickerMode` | ownership |
| M-OWN-5 | `is_animating` и статус одинаковы через любую обёртку | UIKit animator state [uikit-propertyanimator] | сломано | default по статусу (`animation.rs:88`), контроллер — по тикеру (`controller.rs:2722`); Proxy/Reverse/Compound/Switch не пробрасывают | controller-robustness |
| M-OWN-6 | Слушатель, добавленный через комбинатор, живёт не дольше комбинатора; `dispose` отпускает замыкания | AGENTS «Treat user code as reentrant» | сломано | Compound/Reverse регистрируют status-слушатель на родителе (`compound.rs:216`, `reverse.rs:91`); `AnimationSwitch::dispose` (`switch.rs:402`) держит `on_switched` → цикл proxy ↔ switch | ownership |
| M-OWN-7 | ID слушателя — типизированная идентичность, не метка | AGENTS «Identity is not a label»; ADR-0125 (так уже у Vsync) | сломано | счётчик с 1 в каждом реестре (`controller.rs:658`), `+= 1` без `checked_add` (`:2697`); value- и status-ID одного типа (`animation.rs:81`) | listener-delivery |
| M-OWN-8 | Panic в status-слушателе изолирован: остальные получают переход, остальные контроллеры кадра тикают | SwiftUI completion «exactly one time» [swiftui-withanimation]; `ChangeNotifier` для value-слушателей | сломано | `fire_status` без containment (`controller.rs:2372`); маркер продвинут до рассылки (`:2473`) — хвост перехода не получит никогда; так же `proxy.rs:43`, `switch.rs:260`, `:270`; через `Vsync::tick_all` пропускается остаток кадра (`vsync.rs:462`) | listener-delivery |
| M-OWN-9 | Снятый во время рассылки слушатель не вызывается; reentrant-переходы приходят по порядку | `ChangeNotifier` в flui-foundation (пропускает снятых) | сломано | `fire_status` идёт по снимку (`controller.rs:2372`-`2376`): после `dispose`/`remove` слушатель вызывается; при `reverse()` из слушателя следующий получает `Completed` после `Reverse` | listener-delivery |

## Интеграция

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-INTG-1 | Анимированные значения применяются в layout/paint без перестройки дерева | Compose `graphicsLayer {}`, лямбда-модификаторы [adev-quick] | частично | paint-only: `AnimatedOpacity`, `FadeTransition`; rebuild на тик: `SlideTransition` (`crates/flui-widgets/src/transitions/slide_transition.rs:111`), Scale/Rotation, Dismissible, Drawer; `RefreshIndicator` перестраивает поддерево на каждый пиксель (`crates/flui-widgets/src/scroll/refresh_indicator.rs:449`) | integration |
| M-INTG-2 | Анимация изменения размера | Compose `animateContentSize` [adev-quick] | есть | тест `animated_size_interpolates_to_a_new_child_size_over_frames` (`crates/flui-widgets/tests/animated_size.rs:78`); harness `harness_render_animated_size_*` | — |
| M-INTG-3 | Shared element между маршрутами, прерываемый | SwiftUI `matchedGeometryEffect` [swiftui-matched]; Compose [adev-shared] | есть | тест `a_push_flight_interrupted_by_a_pop_diverts_in_place` (`crates/flui-widgets/tests/hero_flight.rs:131`) | — |
| M-INTG-4 | Enter/exit и смена контента; exit держит поддерево до конца | Compose `AnimatedVisibility` / `AnimatedContent` [adev-quick] | частично | `AnimatedSwitcher` убирает ребёнка на `Dismissed` (`crates/flui-widgets/src/animated/animated_switcher.rs:361`); тестов нет | integration |
| M-INTG-5 | Layout-анимация FLIP через transform с коррекцией масштаба | Motion layout animations [motion-layout] | нет | — | вынесено (2026-10-06) |
| M-INTG-6 | Скорость fling ограничена одним параметром жестов; один конвейер drag → fling | Compose `FlingCalculator` [axa-fling] | частично | литерал `clamp(-8_000.0, 8_000.0)` в `crates/flui-widgets/src/scroll/scrollable.rs:664` и копия конвейера в `refresh_indicator.rs`, мимо `DEFAULT_MAX_FLING_VELOCITY` (`crates/flui-interaction/src/settings.rs:101`) | сессия flui-interaction, задача I3 (здесь только потребитель) |
| M-INTG-7 | Платформенное ощущение fling: spline decay Android, deceleration rate iOS | Compose `splineBasedDecay` [axa-fling]; iOS 0.998 / 0.99 [U] | нет | только `BoundedFrictionSimulation` | вынесено (2026-10-06) |
| M-INTG-8 | Implicit-анимация без явного `VsyncScope` тикает или явно сообщает об отсутствии часов | Compose `animate*AsState` берёт часы композиции [adev-value] | сломано | `ImplicitController` создаётся `without_ticker` (`crates/flui-widgets/src/animated/implicitly_animated.rs:74`) и регистрируется только при наличии scope; документация обещает fallback (`vsync_scope.rs:21`) — анимация молча замирает | integration |
| M-INTG-9 | Debug slow-motion управляется из devtools | Flutter `timeDilation` [fl-binding]; Slint `SLINT_SLOW_ANIMATIONS` [slint-anim] | частично | `set_time_dilation` (`crates/flui-scheduler/src/scheduler.rs:3136`); потребителей вне scheduler нет | time |
| M-INTG-10 | Hit-testing во время анимации настраивается | UIKit `isManualHitTestingEnabled` [uikit-propertyanimator] | нет | — | вынесено (2026-10-06) |

## Качество

| ID | Требование | Эталон | FLUI | Доказательство | Тема |
|----|------------|--------|------|----------------|------|
| M-QLT-1 | Property-тесты кривых: точные концы, диапазон, монотонность | CSS Easing 1 §2 [eas1]; эталоны y(x) ниже | нет | `proptest` есть в workspace (корневой `Cargo.toml:472`), но не в dev-deps крейта (`crates/flui-animation/Cargo.toml:44`) | quality |
| M-QLT-2 | Пружина, decay и сглаживание сверены с независимыми эталонами | раздел «Эталонные значения» | нет | только самосогласованность x/dx (`simulation.rs:1050`); гравитация — по знаку | physics |
| M-QLT-3 | Независимость от разбиения кадров для всех запусков и сглаживания | Driscoll [driscoll] | частично | только repeat (`controller_tests.rs:291`) | time |
| M-QLT-4 | Бенчи измеряют рабочий путь | criterion | сломано | `controller/tick_at` (`benches/animation_bench.rs:137`-`142`) после ~18 итераций меряет ранний выход остановленного контроллера; `docs/PERFORMANCE.md:27` выдаёт это за «frame advance ~8.8 ns» | quality |
| M-QLT-5 | Аллокации в тике измерены тестом | образец `crates/flui-scheduler/tests/frame_telemetry_allocation.rs` | нет | — | quality |
| M-QLT-6 | Документация крейта соответствует коду | AGENTS «Docs» | сломано | `docs/ARCHITECTURE.md:664`-`672` («NaN невозможен»), `:836`-`839` (dispose); `docs/PERFORMANCE.md` (размеры, «custom cubic медленнее»); `README.md:129` («выход в [0, 1]»); `docs/GUIDE.md:216` (`flipped`) | quality |
| M-QLT-7 | Примеры в документации компилируются | rustdoc | частично | 54 doctest выполняются, 74 блока `ignore` (README 30, GUIDE 31, PERFORMANCE 12, lib.rs 1) | quality |
| M-QLT-8 | Тесты через публичный API в `tests/main.rs`, один раннер | AGENTS «Writing tests» | частично | интеграционный бинарь назван `tests/derive_animatable.rs`; публичные тесты в `src/`; два раннера; мёртвый `static SERIAL` (`controller_tests.rs:16`) | quality |
| M-QLT-9 | Каждый `pub` достигается production-путём | AGENTS «Unwired surface» | частично | не подключены: `repeat`, `repeat_with`, `velocity`, `tick`, `reverse_from`, `animate_back(_with)`, `fling_with`, `AnimationBehavior`, `TickerNotAvailable` (`error.rs:63`), `CompoundAnimation`, builder, `prelude`, smoothing, `AnimatedValue`, ~25 кривых и твинов | quality |
| M-QLT-10 | Нет маркеров процесса и истории в коде и тестах | AGENTS «No internal process-ID markers» | сломано | `controller_tests.rs:230` (`B1c`); номера issue и «red before the fix» в тестах и бенчах | quality |
| M-QLT-11 | Фича `serde` исполняется в тестах | AGENTS «Green defaults are not coverage» | частично | serde-строка `controller_sources` компилируется, но в прогоне по умолчанию не выполняется | quality |

## Эталонные значения для тестов

Независимые от FLUI значения для reference- и property-тестов. Все — IEEE double; точность
приведена полностью.

**Пружина.** x — смещение от цели, единичная масса, `x'' + 2ζω x' + ω² x = 0`. Замкнутая форма,
сверена со scaling-and-squaring Taylor `expm` (отн. разница ≤ 4e-13) и с унифицированной формой
`x = e^(−at)[x0(C + aS) + v0·S]`, `v = e^(−at)[−ω² x0·S + v0(C − aS)]`, a = ζω, q = ω²(1 − ζ²)
(≤ 7e-15). Источник формы — GPUI `propagator` [gpui-spring].

| ζ | ω | x0 | v0 | t | x(t) | v(t) |
|---|---|----|----|---|------|------|
| 0.5 | 10 | 1 | 0 | 0.1 | 0.659700153391702 | −5.33507195114693 |
| 0.5 | 10 | 1 | 0 | 0.25 | −0.0233595799066923 | −2.74109898705702 |
| 0.5 | 10 | 1 | 0 | 1.0 | −0.00217011673932620 | −0.0538548061605957 |
| 1 | 10 | 1 | 0 | 0.1 | 0.735758882342885 (= 2e⁻¹) | −3.67879441171442 |
| 1 | 10 | 1 | 0 | 0.5 | 0.0404276819945128 | −0.336897349954273 |
| 1 | 10 | −1 | 5 | 0.2 | −0.270670566473225 | 2.03002924854919 |
| 2 | 10 | 1 | 0 | 0.1 | 0.822263423901810 | −2.13909130260279 |
| 2 | 10 | 1 | 0 | 0.5 | 0.282171173975153 | −0.756075360853215 |
| 2 | 10 | 0 | 10 | 0.3 | 0.129208025818252 | −0.346074592636829 |
| 0.2 | 2π | −1 | 5 | 0.3 | 0.588257945757625 | 2.62367533686863 |
| 0.7 | 4π (d = 0.5, b = 0.3) | −1 | 0 | 0.25 | −0.0159125090286291 | 1.52626471111652 |
| 0 | 2π | 1 | 0 | 0.125 | 0.707106781186548 (= cos π/4) | −4.44288293815837 |
| 1 | √1500 (дефолт Compose) | −1 | 0 | 0.05 | −0.423468514838734 | 10.8156746718564 |

- Непрерывность через ζ = 1 (ω = 10, x0 = 1, v0 = 0, t = 0.3, x по `expm`): ζ = 0.999 →
  0.198699920833023; 1 − 1e-6 → 0.199147825387572; 1 → 0.199148273471459; 1 + 1e-6 →
  0.199148721554790; 1.001 → 0.199596088409306. dx/dζ ≈ 0.448 при ζ = 1.
- Конечность при больших t (overdamped): `e^(−at)·cosh(st)` в форме `½(e^(λs·t) + e^(λf·t))`
  даёт 4.56068935005893e-23 (ζ = 3, ω = 100, t = 3) и 3.68342164948964e-44 (ζ = 50, ω = 1000,
  t = 10); наивная форма даёт NaN.
- Медленный корень (ω = 10): устойчивая форма `−ω/(ζ + √(ζ² − 1))` при ζ = 1e6 даёт −5.0e-6;
  наивная `ω(−ζ + √(ζ² − 1))` — −5.000038e-6, а при ζ ≥ 1e8 ровно 0 (пружина не движется).

**Параметризации** (ω = 2π/d, ζ = 1 − b при b ≥ 0, ζ = 1/(1 + b) при b < 0 [U]; c = 2ζω, k = ω²).
Пример Apple [swiftui-spring]: `Spring(duration: 0.5, bounce: 0.3)` → (1.0, 157.9, 17.6);
`Spring(mass: 1, stiffness: 100, damping: 10)` → (0.63, 0.5). Расчёт: k = 100, c = 10 →
d = 0.628318530717959, b = 0.5.

| d | b | k | c | ζ | ω |
|---|---|---|---|---|---|
| 0.5 | 0.3 | 157.913670417430 | 17.5929188601028 | 0.7 | 12.5663706143592 |
| 0.5 | 0 | 157.913670417430 | 25.1327412287183 | 1 | 12.5663706143592 |
| 0.3 | 0.3 | 438.649084492860 | 29.3215314335047 | 0.7 | 20.9439510239320 |
| 1 | 0.5 | 39.4784176043574 | 6.28318530717959 | 0.5 | 2π |
| 0.5 | −0.5 [U] | 157.913670417430 | 50.2654824574367 | 2 | 12.5663706143592 |

- Compose stiffness → SwiftUI response (`response = 2π/√k`, выведено): High 10000 → 0.063 s,
  Medium 1500 → 0.162 s, MediumLow 400 → 0.314 s, Low 200 → 0.444 s, VeryLow 50 → 0.889 s.
- Время успокоения (огибающая `t_s = ln(A/ε)/(ζω)`, A = √(x0² + ((v0 + ζωx0)/ω_d)²)) против
  фактического последнего момента abs(x) > ε: ζ = 0.5, ω = 10, ε = 1e-3 — 1.41031926304161 s
  против 1.27016 s; ζ = 0.7, ω = 4π, ε = 1e-3 — 0.823561753819363 s против 0.81874 s; ζ = 0.3,
  ω = 2π, ε = 0.02 — 2.10040934738401 s против 1.78732 s. Оценка всегда консервативна.

**Экспоненциальное затухание.** Compose `exponentialDecay` [ax-decay]: f = −4.2·mult,
`x(t) = x0 − v0/f + (v0/f)·e^(f·t)`, длительность `T = ln(thr/abs(v0))/f`, thr = 0.1, x0 = 0:
v0 = 1000, mult = 1 → T = 2.19293818380385 s, цель 238.071428571429, x(0.1) = 81.6555190916532;
v0 = 1000, mult = 2 → T = 1.09646909190193 s, цель 119.035714285714; v0 = −3000 →
T = 2.45451253824864 s, цель −714.261904761905.

iOS `decelerationRate` (значения 0.998 и 0.99 — [U]): `x(t) = x0 + v0·(d^(1000t) − 1)/(1000 ln d)`,
`τ = −1/(1000 ln d)`; v0 = 1000 pt/s.
d = 0.998: x(0.1) = 90.6258507889872, x(0.5) = 315.928022678510, x(1) = 432.035126737630,
x∞ = 499.499833166455, дискретная проекция WWDC18 `(v/1000)·d/(1 − d)` = 499.000000000000 [U],
τ = 0.499499833166455 s.
d = 0.99: x(0.1) = 63.0792510785500, x(0.5) = 98.8454049136560, x(1) = 99.4948669704618,
x∞ = 99.4991624734221, проекция 99.0, τ = 0.0994991624734221 s.

**Сглаживание** [driscoll]. λ = ln 10 (за 1 s остаётся 0.1): α = 1 − e^(−λ·dt) = 0.0738812718712065
(30 fps), 0.0376493736019115 (60 fps), 0.0158630101149868 (144 fps). Наивное α = 0.1 на кадр
оставляет за 1 s 0.0423911582752162 (30 fps), 0.00179701029991443 (60 fps), 2.57585468675900e-7
(144 fps). SmoothDamp-полином [unity-mathf] против точного критического шага (smooth time 0.3,
dt = 1/60, 0 → 1): один шаг 0.00559201036148560 против 0.00573409242847800; через 1 s при 60 fps
0.990159131044547 против 0.990243140856395.

**Cubic-bezier y(x).** Бисекция 200 итераций в f64, совпадает с 50-итерационным Newton до 1e-14;
ease-in(x) = 1 − ease-out(1 − x) во всех знаках, ease-in-out(0.5) = 0.5.

| кривая | y(0.1) | y(0.25) | y(0.5) | y(0.75) | y(0.9) |
|--------|--------|---------|--------|---------|--------|
| ease (0.25, 0.1, 0.25, 1) | 0.0947963057160432 | 0.408510591355396 | 0.802403387584857 | 0.960458978348974 | 0.994316477484557 |
| ease-in (0.42, 0, 1, 1) | 0.0170266096515629 | 0.0934646507188248 | 0.315356812572539 | 0.621861869174890 | 0.839427845762466 |
| ease-out (0, 0, 0.58, 1) | 0.160572154237533 | 0.378138130825110 | 0.684643187427461 | 0.906535349281175 | 0.982973390348437 |
| ease-in-out (0.42, 0, 0.58, 1) | 0.0197224535483112 | 0.129161931047320 | 0.5 | 0.870838068952680 | 0.980277546451689 |
| (0.05, 0.7, 0.1, 1) | 0.621384395577187 | 0.831529746487112 | 0.950247475324022 | 0.990510956928150 | 0.998666345989190 |

## Ссылки

[adev-customize]: https://developer.android.com/develop/ui/compose/animation/customize
[adev-quick]: https://developer.android.com/develop/ui/compose/animation/quick-guide
[adev-shared]: https://developer.android.com/develop/ui/compose/animation/shared-elements
[adev-testing]: https://developer.android.com/develop/ui/compose/animation/testing
[adev-value]: https://developer.android.com/develop/ui/compose/animation/value-based
[ax-animatable]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/Animatable.kt
[ax-decay]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/FloatDecayAnimationSpec.kt
[ax-decayspec]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/DecayAnimationSpec.kt
[ax-easing]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/Easing.kt
[ax-springest]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/SpringEstimation.kt
[ax-springsim]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/SpringSimulation.kt
[ax-suspend]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/SuspendAnimation.kt
[ax-transition]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/Transition.kt
[ax-visthr]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation-core/src/commonMain/kotlin/androidx/compose/animation/core/VisibilityThresholds.kt
[axa-fling]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/animation/animation/src/commonMain/kotlin/androidx/compose/animation/FlingCalculator.kt
[axrt-broadcast]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/BroadcastFrameClock.kt
[axrt-monotonic]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/runtime/runtime/src/commonMain/kotlin/androidx/compose/runtime/MonotonicFrameClock.kt
[axui-mds]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/MotionDurationScale.kt
[axui-recomposer]: https://github.com/androidx/androidx/blob/92190a7681d9b80dd20b2ab9d68c8fdcbb1e8dd5/compose/ui/ui/src/androidMain/kotlin/androidx/compose/ui/platform/WindowRecomposer.android.kt
[bevy-common]: https://github.com/bevyengine/bevy/blob/d08bd60564981fe76b5406322ccce505398f9cf8/crates/bevy_math/src/common_traits.rs
[bevy-easing]: https://github.com/bevyengine/bevy/blob/d08bd60564981fe76b5406322ccce505398f9cf8/crates/bevy_curve/src/easing.rs
[ca-additive]: https://developer.apple.com/documentation/quartzcore/capropertyanimation/isadditive
[ca-framerate]: https://developer.apple.com/documentation/quartzcore/cadisplaylink/preferredframeraterange
[col4]: https://drafts.csswg.org/css-color-4/
[driscoll]: https://www.rorydriscoll.com/2016/03/07/frame-rate-independent-damping-using-lerp/
[eas1]: https://drafts.csswg.org/css-easing-1/
[eas2]: https://drafts.csswg.org/css-easing-2/
[egui-context]: https://github.com/emilk/egui/blob/35b9cbf27afd1756f5896bdd1325f155a415c054/crates/egui/src/context.rs#L3700-L3797
[fl-animations]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/animation/animations.dart
[fl-binding]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/scheduler/binding.dart
[fl-controller]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/animation/animation_controller.dart
[fl-curves]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/animation/curves.dart
[fl-friction]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/physics/friction_simulation.dart
[fl-implicit]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/implicit_animations.dart
[fl-spring]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/physics/spring_simulation.dart
[fl-ticker]: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/scheduler/ticker.dart
[gpui-animation]: https://github.com/zed-industries/zed/blob/73be6ac6b1975f428328bbf7fe185d3b671afec4/crates/gpui/src/elements/animation.rs
[gpui-spring]: https://github.com/zed-industries/zed/blob/73be6ac6b1975f428328bbf7fe185d3b671afec4/crates/gpui/src/spring.rs
[mo-inertia]: https://github.com/motiondivision/motion/blob/55eb6bbd5f861785592992b6b8bed2cf81fe9103/packages/motion-dom/src/animation/generators/inertia.ts
[mo-reduced]: https://github.com/motiondivision/motion/blob/55eb6bbd5f861785592992b6b8bed2cf81fe9103/packages/framer-motion/src/utils/reduced-motion/use-reduced-motion-config.ts
[mo-spring]: https://github.com/motiondivision/motion/blob/55eb6bbd5f861785592992b6b8bed2cf81fe9103/packages/motion-dom/src/animation/generators/spring.ts
[mo-stagger]: https://github.com/motiondivision/motion/blob/55eb6bbd5f861785592992b6b8bed2cf81fe9103/packages/motion-dom/src/utils/stagger.ts
[mo-velocity]: https://github.com/motiondivision/motion/blob/55eb6bbd5f861785592992b6b8bed2cf81fe9103/packages/motion-dom/src/value/index.ts
[motion-animate]: https://motion.dev/docs/animate
[motion-layout]: https://motion.dev/docs/react-layout-animations
[mq5]: https://drafts.csswg.org/mediaqueries-5/
[palette-mix]: https://github.com/Ogeon/palette/blob/71011dee471c8fd3ffdb5169de1fc88c882dc0ed/palette/src/macros/mix.rs
[slint-anim]: https://github.com/slint-ui/slint/blob/5aa14ea3636893478f6b2d511c87d537dedfae94/internal/core/animations.rs
[swiftui-animation]: https://developer.apple.com/documentation/swiftui/animation
[swiftui-cubickeyframe]: https://developer.apple.com/documentation/swiftui/cubickeyframe
[swiftui-keyframetimeline]: https://developer.apple.com/documentation/swiftui/keyframetimeline
[swiftui-matched]: https://developer.apple.com/documentation/swiftui/view/matchedgeometryeffect(id:in:properties:anchor:issource:)
[swiftui-merge]: https://developer.apple.com/documentation/swiftui/customanimation/shouldmerge(previous:value:time:context:)
[swiftui-phaseanimator]: https://developer.apple.com/documentation/swiftui/phaseanimator
[swiftui-reducemotion]: https://developer.apple.com/documentation/swiftui/environmentvalues/accessibilityreducemotion
[swiftui-settling]: https://developer.apple.com/documentation/swiftui/spring/init(settlingduration:dampingratio:epsilon:)
[swiftui-spring]: https://developer.apple.com/documentation/swiftui/spring
[swiftui-spring-anim]: https://developer.apple.com/documentation/swiftui/animation/spring(response:dampingfraction:blendduration:)
[swiftui-withanimation]: https://developer.apple.com/documentation/swiftui/withanimation(_:completioncriteria:_:completion:)
[tf1]: https://drafts.csswg.org/css-transforms-1/
[tf2]: https://drafts.csswg.org/css-transforms-2/
[tr1]: https://drafts.csswg.org/css-transitions-1/
[tr2]: https://drafts.csswg.org/css-transitions-2/
[uikit-crossfade]: https://developer.apple.com/documentation/uikit/uiaccessibility/preferscrossfadetransitions
[uikit-isreversed]: https://developer.apple.com/documentation/uikit/uiviewanimating/isreversed
[uikit-propertyanimator]: https://developer.apple.com/documentation/uikit/uiviewpropertyanimator
[uikit-velocity]: https://developer.apple.com/documentation/uikit/uispringtimingparameters/initialvelocity
[unity-mathf]: https://github.com/Unity-Technologies/UnityCsReference/blob/def4ed521d3613a67619d5ac5111e3fd8c17a605/Runtime/Export/Math/Mathf.cs#L276-L311
[val4]: https://drafts.csswg.org/css-values-4/
[wa1]: https://drafts.csswg.org/web-animations-1/
[wa2]: https://drafts.csswg.org/web-animations-2/
[webkit-unitbezier]: https://github.com/WebKit/WebKit/blob/a441a3749a6818edabba72def3c0a6d7d2f92fe7/Source/WebCore/platform/graphics/UnitBezier.h
[wwdc18-fluid]: https://developer.apple.com/videos/play/wwdc2018/803/
