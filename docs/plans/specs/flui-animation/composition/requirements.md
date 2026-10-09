# flui-animation / composition — требования

- **Статус:** черновик
- **Дата:** 2026-10-06
- **База:** `main` 9a4daa3ed; `file:line` — на этот коммит.
- **Оркестрация:** [../orchestration.md](../orchestration.md) («Делается в этом проходе»:
  keyframes/sequence/stagger), [../tasks.md](../tasks.md); матрица — [../market.md](../market.md)
  M-CMP-1, M-CMP-2, M-CMP-4, M-CMP-5, M-CMP-11, M-CRV-9.
- **Связанные спеки:** `curves` (удаляет `CatmullRom*` после этой), `interpolation`
  (`tween_types.rs`), `physics`, `retarget`, `ownership`, `motion-clock`, `integration`.

## Пользовательский сценарий

Автор виджета описывает движение ключевыми кадрами во времени («90° за 300 мс, пауза до
1500 мс, ещё 90°…»), несколько дорожек на одном контроллере, задержку старта и сдвиг фазы по
индексу. Значение — чистая функция времени; тест на виртуальных часах сэмплирует любую точку.
Потребители в workspace: `ActivityIndicator` (flui-widgets) и спиннер `RefreshIndicator`,
`LinearProgressIndicator` (flui-material), `CupertinoActivityIndicator` (flui-cupertino).

## Текущее состояние (чтением)

- `TweenSequence`/`TweenSequenceItem` (`tween_types.rs:340`, `:452`): относительные веса без
  единиц времени, одна кривая на элемент через вложенный `Animatable`; production-пользователей
  нет; panic на пустом списке и невалидных весах (`:357-370`).
- `Interval` (`curve.rs:141`, `:177`) — единственный способ задержки и окна; используется в
  `hero_flight.rs:218`, `dismissible.rs:1431`.
- `CatmullRomCurve` (`curve.rs:796`) игнорирует x (M-CRV-9, D-19).
- Нет задержки, stagger, группы; повтор — `repeat_with` (`controller.rs:1390`).
- Спиннер `RefreshIndicator` — статичный `ColoredBox` (`refresh_indicator.rs:23-24`, `:492`);
  индикаторов прогресса в workspace нет; роли `LoadingSpinner`/`ProgressBar` уже есть
  (`flui-protocol semantics.rs:95`, `accesskit_translation.rs:169`).

## Требования

ID — только в этом каталоге. «Таблица» = строка `run_table` в `tests/contracts/keyframes.rs`
(модуль `tests/main.rs` после Q0); «prop» = proptest; widget-тесты — `flui-testing`
`pump_for` на виртуальных часах, кадр читается из `DrawOp` display list.

### Дорожка ключевых кадров

- **R1.** КОГДА сегменты заданы длительностями `Duration`, СИСТЕМА ДОЛЖНА размещать их подряд от
  нуля (монотонность по построению) и держать последнее значение до `total`.
  Тест: `keyframes_segments_follow_each_other` (таблица).
- **R2.** КОГДА время равно границе сегмента, СИСТЕМА ДОЛЖНА вернуть значение ключевого кадра
  точно (клон, без интерполяции); в границе со скачком — значение после скачка
  (непрерывность справа, WAAPI §5.3.4 [wa1]). Тест: `keyframes_boundaries_are_exact` (таблица +
  prop: любые длительности, все границы).
- **R3.** КОГДА сегмент «к значению» несёт кривую, СИСТЕМА ДОЛЖНА применять её к локальному
  прогрессу этого сегмента (кривая принадлежит сегменту, ведущему к значению — без
  двусмысленности Compose `using`). Эталон: значения на плато и границах; внутри — монотонность
  для монотонной кривой. Тест: `keyframes_curve_belongs_to_arriving_segment` (таблица).
- **R4.** КОГДА `elapsed > total`, СИСТЕМА ДОЛЖНА вернуть значение в `total`; КОГДА вызвано
  циклическое чтение, — значение в `elapsed mod total`, включая `Duration::MAX`.
  Тест: `keyframes_clamp_and_loop` (таблица; строка `duration_max_loops_without_panic`).
- **R5.** КОГДА дорожка читается как `Animatable` по прогрессу `t`, СИСТЕМА ДОЛЖНА
  отображать `t ∈ [0,1]` в `t·total`, `t` вне диапазона — зажимать, NaN — читать как 0.
  Тест: `keyframes_progress_maps_to_time` (таблица: 0, 1, −1, 2, NaN, ±inf).
- **R6.** КОГДА значение одно и то же при любом порядке запросов, СИСТЕМА ДОЛЖНА быть чистой:
  запрос «назад во времени», повтор, `dt = 0` дают тот же результат. Тест:
  `keyframes_evaluation_is_order_independent` (prop: случайная перестановка запросов).
- **R7.** КОГДА сегменты не помещаются в `total`, `total = 0`, сумма длительностей переполняет
  `Duration` или компонент значения ключевого кадра не конечен, СИСТЕМА ДОЛЖНА вернуть
  `KeyframesError` из `build()` с типизированной причиной, без panic.
  Тест: `keyframes_build_rejects_invalid_tracks` (таблица, по строке на вариант).
- **R8.** КОГДА пользовательская кривая возвращает NaN/inf или вычисление в векторном
  пространстве переполняется, СИСТЕМА НЕ ДОЛЖНА публиковать неконечное значение: сэмпл
  заменяется значением начала сегмента. Тест: `keyframes_never_publish_non_finite`
  (таблица: кривая-NaN, кривая-inf, значения `±1e308` в cubic).

### Cubic-сегменты (M-CRV-9)

- **R9.** КОГДА подряд идут cubic-сегменты, СИСТЕМА ДОЛЖНА строить сплайн Catmull-Rom с узлами
  во времени ключевых кадров (решение по x-времени, а не по параметру): ключи `(0,0)`,
  `(0.9·T,0.5)`, `(T,1)` в `0.9·T` дают ровно `0.5`. Тест: `cubic_keyframes_pass_through_keys`
  (таблица; эталон — сами ключи и аналитика Эрмита: ключи 0,1,0 в 0,1,2 с при нулевых
  крайних скоростях дают `0.5` в 0.5 с).
- **R10.** КОГДА cubic-сегмент стоит после или перед сегментом другого вида, СИСТЕМА ДОЛЖНА
  брать скорость на стыке у соседа (SwiftUI `CubicKeyframe` [swiftui-cubickeyframe]); у края
  дорожки, после `hold` и `jump` — ноль. Тест: `cubic_keyframes_are_c1_at_joins` (prop:
  левая и правая конечные разности на каждом стыке совпадают в пределах `1e-6·|Δ|/d`).

### Stagger, задержка, группа

- **R11.** КОГДА задан шаг и origin (`First`, `Last`, `Center`, `Index`), СИСТЕМА ДОЛЖНА давать
  задержку `step·|origin − i|` (Motion `stagger` [mo-stagger]; center = `(n−1)/2`), с
  насыщением на `Duration::MAX` вместо panic. Тест: `stagger_delays_follow_origin` (таблица;
  эталон — формула Motion, посчитанная в тесте вручную для n = 1, 4, 5; строка
  `huge_count_saturates`).
- **R12.** КОГДА нужна задержка старта или пауза в конце, СИСТЕМА ДОЛЖНА выражать её ведущим
  или хвостовым `hold` той же дорожки; КОГДА несколько дорожек с одним `total` сэмплируются
  одним контроллером, они ДОЛЖНЫ видеть одно время (группа). Тест:
  `keyframes_group_shares_one_clock` (таблица: три дорожки индикатора на одном прогрессе).

### Потребители

- **R13.** КОГДА `ActivityIndicator` смонтирован и `VsyncScope` есть, СИСТЕМА ДОЛЖНА рисовать
  дугу, чья длина и поворот в моменты 0, 300, 1500, 1800, 3000, 4800, 6000 мс равны эталону из
  констант Material 3 Compose `ProgressIndicator.kt` (androidx `e5da4a9c`: 6000 мс, 0.1/0.87,
  шаги поворота 90° за 300 мс через 1500 мс, глобальный поворот 1080°), повторяясь каждые 6 с,
  без пересборки поддерева. Тест: `activity_indicator_arc_follows_keyframes` (flui-widgets
  `tests/main.rs`, `DrawOp::Arc`; плюс счётчик build не растёт между кадрами).
- **R14.** КОГДА `RefreshIndicator` в фазе refreshing, СИСТЕМА ДОЛЖНА показывать
  `ActivityIndicator` вместо `ColoredBox`; после `finish()` контроллер спиннера остановлен и
  снят с Vsync. Тест: `refresh_indicator_spins_while_refreshing`.
- **R15.** КОГДА `LinearProgressIndicator` без значения, СИСТЕМА ДОЛЖНА двигать две полосы по
  четырём дорожкам с ведущими задержками 0/250/650/900 мс и длительностями 1000/1000/850/850 мс
  в цикле 1750 мс (Compose, тот же коммит); со значением — рисовать долю без контроллера.
  Тест: `linear_progress_indeterminate_keyframes` и `linear_progress_switches_to_determinate`
  (flui-material `tests/main.rs`).
- **R16.** КОГДА `CupertinoActivityIndicator` анимируется, СИСТЕМА ДОЛЖНА давать тику `i` в
  момент `t` альфу `A[(i − ⌊8·t/1 с⌋) mod 8]`, `A = [47,47,47,47,72,97,122,147]` (Flutter
  `activity_indicator.dart:139`, `:177-182` @ `d454b1b8`); формула считается в тесте
  независимо от production. Тест: `cupertino_activity_indicator_ticks_step`.
- **R17.** КОГДА индикатор виден, СИСТЕМА ДОЛЖНА публиковать роль `LoadingSpinner`
  (`ProgressBar` со значением у determinate) и label. Тест: строки в тестах R13, R15, R16
  через `a11y_tree`, падающие без `SemanticsConfiguration`.

## Сценарии отказа

- **R18.** КОГДА индикатор размонтирован посреди тика или из value-слушателя того же кадра,
  СИСТЕМА ДОЛЖНА снять регистрацию и не рисовать после dispose; следующий кадр тикает
  остальные контроллеры. Тест: `indicator_unmount_mid_frame_releases_controller`.
- **R19.** КОГДА два индикатора на одном Vsync, СИСТЕМА ДОЛЖНА тикать оба независимо; снятие
  одного из колбэка другого не пропускает второй. Тест: `two_indicators_share_one_vsync`.
- **R20.** КОГДА realm остановлен посреди повтора, СИСТЕМА ДОЛЖНА освободить контроллер без
  кадра после остановки (контракт `ownership`). Тест: `indicator_realm_stop_mid_repeat`.
- **R21.** КОГДА painter паникует (пользовательская кривая в дорожке), СИСТЕМА ДОЛЖНА оставить
  дорожку неизменной (она без состояния): следующий кадр сэмплирует заново; первый отказ —
  по политике paint (`docs/PANIC-POLICY.md`). Тест: `keyframes_survive_panicking_curve`
  (таблица: catch_unwind вокруг `value_at`, затем тот же запрос с другим временем).
- **R22.** КОГДА `pump_for(0)` или скачок времени в часы, СИСТЕМА ДОЛЖНА рисовать кадр,
  равный чистой функции от времени (циклическое чтение), без накопления. Тест: строки
  `zero_dt` и `ten_hours` в `activity_indicator_arc_follows_keyframes`.
- **R23.** КОГДА включён `TickerMode(false)` или политика reduce motion (спека `reduce-motion`;
  поведение индикатора `Normal`/`Preserve` — открытое решение владельца в `review.md`),
  СИСТЕМА ДОЛЖНА рисовать конечный статичный кадр (время 0). Тест:
  `indicator_paused_paints_static_frame`.

Дорожка — значение без слушателей: reentry, add/remove во время раздачи и retarget в последнем
кадре — темы `listener-delivery`, `controller-robustness`, `retarget`; потребители проходят R18–R20.

## Вне scope (предложение; утверждает владелец)

- **Spring-сегмент** (SwiftUI `SpringKeyframe`): в workspace нет потребителя; builder-метод
  `spring` добавляется без поломки API, когда появится потребитель и `physics` даст время
  успокоения.
- **M-CMP-11** (`PhaseAnimator`, `updateTransition`): непрерывный цикл фаз = дорожки с `hold` +
  `repeat`; переходы по состоянию = implicit-виджеты + `retarget`. Отдельного типа нет.
- **Оконный stagger списков** (Motion, Flutter `PopupMenu` `popup_menu.dart:705-711`):
  потребитель — меню Material (B2); в этом проходе `Stagger` используется в циклическом
  размещении. Решение владельца: см. design «Вопросы владельцу».
