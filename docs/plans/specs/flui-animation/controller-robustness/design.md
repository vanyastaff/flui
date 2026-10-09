# controller-robustness — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md). `file:line` — на `main` 9a4daa3ed (до разбиения
  Q0 `controller.rs` → `controller/{mod,run,tick,status,dispose}.rs`).

## 1. Текущий код

| Факт | Где |
|------|-----|
| Контроллер хранит `ticker: Option<Ticker>`; четыре семейства конструкторов (8 функций): `new`/`with_bounds` (тикер на `UpdateScheduler`), `without_ticker[_bounds]`, `with_detached_ticker[_bounds]`, `unbounded_without_ticker`/`unbounded_with_detached_ticker`/`unbounded` | `controller.rs:282, 410-548, 683-728` |
| В production ни один `UpdateScheduler` контроллера не прокачивается: 5 сайтов строят выброшенный `&UpdateScheduler::new()` ради `is_animating` | `flui-material` `drawer.rs:691`, `ink_well.rs:524`, `scaffold_messenger.rs:553,672`; `flui-cupertino` `button.rs:499` |
| Реальные часы — `Vsync::tick_all` → `tick_at(elapsed)` | `vsync.rs:440-511` |
| `restart_ticker` под guard вызывает `Ticker::start` → `request_frame` → `on_frame_scheduled` (чужой код) после частичной мутации (D-32) | `controller.rs:2313-2340` |
| Без тикера каждый старт пишет `warn!("has no ticker; the animation will not advance")` — ложь для всех Vsync-контроллеров | `controller.rs:2346-2350`, вызовы `888, 993, 1273, 1547, 1658, 1760` |
| `is_animating` контроллера = `Ticker::is_running`; дефолт трейта = `status().is_running()`; обёртки не пробрасывают (D-11) | `controller.rs:2715-2728`; `animation.rs:88` |
| Curved-ветка: `start + (target-start)·curve(t)` без проверки конечности и clamp (D-04); simulation-ветка clamp'ит и завершает прогон на не конечном | `controller.rs:1937-1938, 2012`; `1950-1965` |
| `scaled_run_duration`: `base.mul_f64(fraction)` паникует на `Duration::MAX` (f64 округляет до 2^64 с) — после `clear_run_modes` и записи направления/целей (D-26) | `controller.rs:2505-2515`; мутации `850-855`, `957-963`, `1229-1243` |
| `dispose` не трогает `notifier`; `set_value` и `add_status_listener` работают после dispose (D-31) | `controller.rs:2151-2214, 2693-2700` |
| `tick()` = `tick_at(ticker.elapsed_secs())`; без тикера — `tick_at(0.0)`, откат (D-34) | `controller.rs:1854-1860` |
| `attach_child`: проверка цикла до lock (TOCTOU); дубли регистраций/детей не отсекаются (D-35) | `vsync.rs:224-243, 179-201` |
| `AnimationError`: `InvalidBounds(String)`, `InvalidSpring(String)`, `NonFiniteTarget(String)`, мёртвый `TickerNotAvailable` (D-47) | `error.rs:28-95` |
| `AnimationControllerBuilder` держит `UpdateScheduler`, `bounds()` → `Result` посреди цепочки, второй раз валидирует в `build` | `builder.rs` |
| `TickerFuture/TickerCompleter/TickerDelivery/TickerCanceled` не зависят от `Ticker` (`TickerFuture::pending()`) | `flui-scheduler/src/ticker.rs:1563-1900`; `controller.rs:875` |

## 2. Варианты

**Часы.** (A) Оставить `Ticker` как опцию, починить D-32 переносом `restart_ticker` после `finish`.
Сохраняет два времени (стена и виртуальное) и 8 конструкторов; `is_animating` по-прежнему зависит от
выбора конструктора. (B) **Vsync и явный `tick_at` — единственные часы; `Ticker` удаляется из
workspace.** Ни один production-путь не прокачивает тикер контроллера, все 19 production-сайтов уже
регистрируются в `Vsync`; удаление снимает D-32 целиком (нет чужого кода при старте), D-34 (нет второго
источника времени), ложный warn и цикл `ticker → callback → controller` (ADR-0064 §6). (C) Тикер как
trait-объект «источник кадров» в контроллере — новое per-controller состояние под `Mutex`, запрещено
решением владельца. **Выбрано B.**

**Future завершения без Ticker.** Completer создаётся контроллером (`TickerFuture::pending()`), тикер
к нему не прикасается (ADR-0064 §1). Семейство переносится в `flui-animation::completion` и
переименовывается: `RunFuture`, `RunCanceled` (pub), `RunCompleter`, `RunDelivery` (`pub(crate)` —
единственный производитель — контроллер). Поведение ADR-0064 §2–5 и ADR-0106 не меняется; тесты
`flui-scheduler/tests/ticker_future_recovery.rs` переезжают без изменения строк. Альтернатива —
оставить имена в `flui-scheduler`: имя лжёт («ticker» больше нет), а слой `flui-scheduler` не
использует тип. Решено: переименование и перенос (orchestration, «Решения по развилкам»).

**NaN на выходе кривой.** (a) Завершить прогон на последнем конечном (как simulation-ветка): у
time-прогона известный конец, досрочный `Completed` при `value ≠ target` лжёт статусом и сокращает
длительность. (b) Вернуть ошибку: у тика нет вызывающего, `Result` некому отдать. (c) **Держать
последнее опубликованное значение, без записи и value-уведомления на этом кадре, warn один раз
(существующий латч `non_finite_warned`), прогон продолжается и на `t >= 1` публикует точную цель.**
Сохраняет контракт прогона (длительность, `Completed`, `Ok`, точный конец), не публикует NaN.
Simulation-ветка остаётся с (a): у неё конец определяет `is_done`, без конечного сэмпла его нет.
**Выбрано (c).** Конечный выход за границы — clamp в `[lower, upper]` (инвариант ограниченного
контроллера; overshoot делается `Tween` над `CurvedAnimation` или неограниченным контроллером).
Сверено с Flutter `AnimationController._tick` (`clampDouble(_simulation!.x(..), lowerBound, upperBound)`,
`animation_controller.dart` L939) и Compose `Animatable` (`lowerBound/upperBound`, `BoundReached`).

**Отказ до мутации.** (i) Точечные `checked_*` в существующих телах — порядок «проверка, затем
мутация» держится ревью. (ii) **plan/commit**: каждая точка старта — `fn plan_*(&Inner, args) ->
Result<RunPlan, AnimationError>` (только чтение, вся арифметика и валидация), затем
`fn commit(&mut Inner, RunPlan) -> Option<RunDelivery>` — только присваивания и `replace/take`, без
арифметики, способной паниковать, и без чужого кода. Тип `RunPlan` делает «отказ после мутации»
непредставимым: у `plan_*` нет `&mut`. **Выбрано (ii).** `scaled_run_duration` тотальна:
`fraction >= 1 → base`, иначе `Duration::try_from_secs_f64(base.as_secs_f64()·fraction)
.map_or(base, |d| d.min(base))`.

**Dispose.** `set_value` → `Result<(), AnimationError>` (`Disposed`): ошибка, вызванная
вызывающим, по PANIC-POLICY — `Result`; `debug_assert` — panic в debug, политика запрещает.
`add_listener`/`add_status_listener` (сигнатуры трейтов `Listenable`/`Animation` вернуть `Result` не
могут без смены `flui-foundation`) после dispose — инертная регистрация: колбэк ретирован вне lock,
id выпущен из того же пространства (не совпадает с живым; типизацию id делает listener-delivery,
D-27), `warn!` после снятия guard. Отличается от `ChangeNotifier::check_disposed` (debug-panic,
`notifier_generic.rs:146-150`) — расхождение в `flui-foundation` вынести отдельной задачей.

**Реестр.** (1) Проверка цикла под lock `self`: обход потомков `child` берёт их lock'и → AB/BA
deadlock при встречных `attach_child`; глобальный lock топологии — `static`, запрещён ADR-0097.
(2) **Инвариант одного родителя + иммунный обход.** `VsyncInner.parent: Option<Weak<Mutex<VsyncInner>>>`
(состояние разделяемой инфраструктуры, не per-animation) ставится под lock ребёнка только если
пусто (`AlreadyAttached`) — атомарно; цикл отсекается подъёмом по `parent` от `self`
(`WouldCycle`). Стека пути в обходе нет: production-потоков нет, после send-flip T6c реестр
`!Send` и гонка непредставима типом, а один родитель + проверка подъёмом исключают цикл при одном
потоке; подъём несёт `debug_assert!` (внутренний инвариант). Дубли: под lock
реестра — `controllers` и `children` проверяются по `Arc::ptr_eq` → `AlreadyRegistered`/
`AlreadyAttached` (O(N) при регистрации, не на кадре). **Выбрано (2).** Дубль одного контроллера в
двух разных реестрах остаётся возможен — закрывает владеющий handle (`ownership`, W2).

**Конструирование.** Builder против конструкторов: 8 функций — матрица {тикер, без, detached} ×
{границы, unit, unbounded}; после удаления тикера ось одна, а `bounds`/`unbounded`/`reverse_duration`/
`initial_value` — независимые опции; builder с типизированными границами убирает матрицу и
`Result` посреди цепочки. **Выбран builder** (решение владельца подтверждено). Терминальный метод
этой темы один — `build() -> AnimationController`; production-сайты переходят на `build()` +
текущий `Vsync::register`, без промежуточного кортежа. `build_on(Option<&Vsync>) ->
DrivenController` добавляет ownership (решение X8, orchestration «По итогам adversarial review»).

## 3. Публичный API (дельта)

```rust
// flui-animation
pub struct AnimationControllerBuilder { /* duration, reverse_duration, bounds, initial_value */ }
impl AnimationController {
    #[must_use] pub fn builder(duration: Duration) -> AnimationControllerBuilder;
    pub fn set_value(&self, value: f64) -> Result<(), AnimationError>;        // было ()
    // удалены: new, with_bounds, without_ticker[_bounds], with_detached_ticker[_bounds],
    //          unbounded, unbounded_without_ticker, unbounded_with_detached_ticker, tick,
    //          run_generation (→ pub(crate))
}
impl AnimationControllerBuilder {
    #[must_use] pub fn reverse_duration(self, d: Duration) -> Self;
    #[must_use] pub fn bounds(self, range: ValueRange) -> Self;
    #[must_use] pub fn unbounded(self) -> Self;
    #[must_use] pub fn initial_value(self, v: f64) -> Self; // не конечное: правило set_value
    #[must_use] pub fn build(self) -> AnimationController;  // без часов: продвигает только tick_at
    // build_on(Option<&Vsync>) -> DrivenController — тема ownership
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueRange { lower: f64, upper: f64 }
impl ValueRange {
    pub const UNIT: Self;                                                     // [0, 1]
    /// # Errors — узкая ошибка значения, а не весь `AnimationError` контроллера.
    pub fn new(lower: f64, upper: f64) -> Result<Self, RangeDefect>;
    #[must_use] pub const fn lower(self) -> f64; #[must_use] pub const fn upper(self) -> f64;
}
pub trait Animation<T> { fn is_animating(&self) -> bool; /* без default */ }
pub mod completion { pub struct RunFuture; pub struct RunCanceled; } // бывш. Ticker*

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)] #[non_exhaustive]
pub enum AnimationError {
    #[error("animation controller is disposed")] Disposed,
    #[error("invalid bounds [{lower}, {upper}]: {defect}")]
    InvalidBounds { lower: f64, upper: f64, defect: RangeDefect },
    #[error("invalid repeat range [{min}, {max}] within [{lower}, {upper}]: {defect}")]
    InvalidRepeatRange { min: f64, max: f64, lower: f64, upper: f64, defect: RangeDefect },
    #[error("{input} is not finite: {value}")]
    NonFiniteInput { input: AnimationInput, value: f64 },
    #[error("{input} = {value} runs toward an infinite bound")]
    UnboundedTarget { input: AnimationInput, value: f64 },
    #[error("span {from} -> {to} overflows f64")] SpanOverflow { from: f64, to: f64 },
    #[error("fling needs a non-oscillating spring (damping ratio {damping_ratio})")]
    UnderdampedFlingSpring { damping_ratio: f64 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)] #[non_exhaustive]
pub enum RangeDefect { #[error("bound is not finite")] NonFinite,
    #[error("lower >= upper")] Inverted, #[error("span overflows f64")] SpanOverflow }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] #[non_exhaustive] // Display вручную
pub enum AnimationInput { Target, From, Velocity, SimulationStart, RepeatRange }

// flui-animation::vsync
#[non_exhaustive] pub enum VsyncRegistrationError {
    Exhausted, AlreadyRegistered, AlreadyAttached, WouldCycle }
impl Vsync { pub fn attach_child(&self, child: &Vsync)
    -> Result<VsyncRegistration, VsyncRegistrationError>; /* было Option */ }

// flui-scheduler: удалены Ticker, TickerGroup, TickerProvider (+ impl для UpdateScheduler),
// TickerCallback, TickerState, TickerId; Ticker* future-семейство переехало (см. выше).
```

`AnimationError` теряет `Eq` (поля `f64`) — нет внешних `Eq`-зависимостей (`rg "AnimationError"`
вне крейта: один doc-link в `transition_route.rs:689`). `serde`-derive сохраняется. Rustdoc каждого нового pub: контракт, единицы (`Duration`), ошибки, panics, пример через `build()` + `tick_at`.

Удаления по решению владельца (без поведения, отдельная задача T8): `CompoundAnimation`,
`AnimationOperator`, `prelude`, реэкспорты `flui_scheduler::{BudgetPolicy, FrameBudget, FramePhase,
FrameTiming, Priority, TaskQueue, Ticker, TickerCallback, TickerProvider, UpdateScheduler,
TickerCanceled, TickerFuture, TickerState}` (`lib.rs:167-171`), `ALWAYS_COMPLETE`/`ALWAYS_DISMISSED`
(`constant.rs`, строки `globals` в `Cargo.toml:67-68`) → `ConstantAnimation::completed(1.0)` /
`dismissed(0.0)` в `transition_route.rs:59,72,78`. Каждое удаление сверяется с `flui-sdk/tests/surface.rs` (ADR-0088 §4), `packages/` — в том же PR; правки `flui-scheduler` исполняет владелец motion-clock.

## 4. Инварианты

1. Контроллер продвигается только `tick_at`; старт прогона не вызывает чужой код под guard.
2. `is_animating() ⇔ !disposed && active_run.is_some()` — тот же факт, что `walk_probe().live_running`;
   один crate-private предикат на оба места.
3. Опубликованное `value` конечно; на ограниченном контроллере ∈ `[lower, upper]` (все ветки тика).
4. Старт прогона = `plan` (без `&mut`) → `commit` (без арифметики и чужого кода) → `finish`.
5. После `dispose`: ни одна операция не меняет значение/статус; `add_*` не удерживает колбэк;
   оба канала пусты.
6. Реестр: у ребёнка ≤ 1 родителя; контроллер и ребёнок в одном реестре — не более одного раза;
   обход тикает каждый реестр ≤ 1 раза за вызов.
7. Нового per-controller состояния под `Mutex` нет: поле `ticker` удаляется, латч
   `non_finite_warned` переиспользуется; `parent` — в реестре (инфраструктура).

## 5. Владение

`RunCompleter` — в `active_run`, разрешается только в `finish` (ADR-0064 §4 без изменений).
Ретирование колбэков, кривых, симуляций — `RetiredSources` вне guard (как сейчас). Инертный колбэк
после dispose — `Opaque`, дроп после снятия guard; при unwind — удержание (ADR-0127). Удаление
`Ticker` снимает цикл `ticker → callback → controller`: `Arc` контроллера держат только владельцы и
реестр.

## 6. Миграция (rg на 9a4daa3ed)

Конструкторы `AnimationController::{new,with_bounds,without_ticker*,with_detached_ticker*,unbounded*}`
и `AnimationControllerBuilder::new`/`builder`: **147 вхождений в 52 файлах** (с тестами, доками,
примерами). Production — 19 сайтов в 15 файлах: flui-material `drawer.rs:691`, `ink_well.rs:524`,
`scaffold_messenger.rs:553,672`; flui-cupertino `button.rs:499`; flui-widgets
`implicitly_animated.rs:74`, `animated_switcher.rs:317`, `animated_size.rs:181`,
`transition_route.rs:639`, `dismissible.rs:638,1287`, `sliver_persistent_header.rs:244`,
`scroll_controller.rs:588`, `scrollable.rs:347`, `refresh_indicator.rs:400`, doc-примеры
`slide_transition.rs:43`, `fade_transition.rs:21`; examples `animated_box_app.rs:200`,
`workload_probe.rs:468`, `lifecycle_probe.rs:153`, `vertical_slice_demo/frame_histogram.rs:121`.
`set_value(`: 30 production-сайтов (`let _ =` там, где dispose невозможен до вызова; иначе ветка).
`TickerFuture`/`TickerCanceled`: 24 production-вхождения в 10 файлах (navigator, cupertino button).
`attach_child`: `ticker_mode.rs:143`. `flui-sdk/tests/surface.rs:24` — пины `TickerFuture`,
`UpdateScheduler` → `RunFuture`; SDK `0.N`. Устаревшие комментарии: `transition_route.rs:410-422,
629-638`, `animated_size.rs:172-176`, `dismissible.rs:629-634`, `navigator/binding.rs:238-241`,
`flui-runtime` `pump.rs:15`, `ui_realm/pump.rs:77`. flui-scheduler: `ticker.rs` (Ticker-часть),
`scheduler.rs:82, 3358-3370`, `tests/integration_tests.rs`, `examples/animation_ticker.rs`,
`ARCHITECTURE.md` (89 упоминаний), `flui-foundation/src/id.rs:706-709` (`TickerId`).

## 7. ADR — да

### Черновик ADR

**ADR-NNNN: `AnimationController` driven only by `Vsync`; the scheduler `Ticker` is removed.**
Supersedes: ADR-0064 §1 и §6; ADR-0125 — только форма отказа `attach_child`.
Context: ни один production-контроллер не прокачивается своим `Ticker`; тикер вносил чужой код
(`on_frame_scheduled`) под guard контроллера, второй источник времени (`tick()`), ложное
предупреждение и цикл владения. Decision: (1) контроллер продвигается только `tick_at`, который
вызывает `Vsync::tick_all` презентации или тест; `Ticker`, `TickerGroup`, `TickerProvider`
удалены из `flui-scheduler`. (2) Future завершения — `flui_animation::completion::RunFuture`,
контракт ADR-0064 §2–5 и ADR-0106 без изменений; completer создаёт и разрешает только контроллер.
(3) `is_animating` — обязательный метод `Animation`; для контроллера — «прогон установлен», обёртки
пробрасывают источник. (4) Старт прогона — plan/commit: отказ до любой мутации. (5) Реестр: один
родитель, отказ `attach_child` типизирован (`WouldCycle`, `AlreadyAttached`, `Exhausted`),
повторная регистрация — `AlreadyRegistered`; цикл отсекается при `attach_child`. Consequences: запуск прогона вне
кадра не будит цикл кадров сам — будит кадр, в котором `has_running()` проверяется (как у всех
Vsync-контроллеров сегодня). Тесты: `controller_robustness_contract`, `vsync_admission_contract`,
`run_future_contract`.

## 8. Changelog (`changelog.d/controller-robustness.md`)

```markdown
### Changed
- `AnimationController` is created with `AnimationController::builder(duration)` (`.bounds(ValueRange)`,
  `.unbounded()`, `.reverse_duration`, `.initial_value`, `.build()`) and advances
  only through `Vsync`/`tick_at`.
- `TickerFuture`/`TickerCanceled` are `flui_animation::completion::{RunFuture, RunCanceled}`.
- `AnimationController::set_value` returns `Result` and refuses a disposed controller.
- `Animation::is_animating` is required; every wrapper reports its source; for a controller it means a run is installed.
- `AnimationError` variants carry structured fields; `Vsync::attach_child` returns `Result`.
### Fixed
- Curved runs clamp to bounds and never publish a non-finite value.
- `Duration::MAX` no longer panics a run start; a refused start leaves the running animation untouched.
- A disposed controller drops late listeners and clears its value listeners.
- Duplicate `Vsync` registrations and attachments are refused; a registry cycle cannot hang the frame.
### Removed
- `flui_scheduler::{Ticker, TickerGroup, TickerProvider, TickerCallback, TickerState, TickerId}`.
- The eight `AnimationController` constructors, `AnimationController::tick`, `AnimationError::TickerNotAvailable`.
- `CompoundAnimation`, `AnimationOperator`, `flui_animation::prelude`, `ALWAYS_COMPLETE`/`ALWAYS_DISMISSED`,
  and `flui_animation`'s re-exports of `flui_scheduler` types.
```

## 9. Adversarial review

| Сценарий | Что делает дизайн | Тест |
|----------|-------------------|------|
| Reentry слушателя в контроллер из старта | старт не зовёт чужой код под guard; слушатели — после `finish` | `run_start_reenters_vsync_without_deadlock` |
| Reentry в реестр из тика | неизменно: lock реестра снят до `tick_at` (`vsync.rs:506-510`) | `dispose_from_listener_during_tick` |
| Добавление/снятие слушателя во время раздачи | порядок и снятие — listener-delivery; после dispose `add_*` инертен | `disposed_controller_drops_late_listeners` |
| Последний владелец отпущен из колбэка | `finish(&self)` держит живой handle; `Inner::drop` после `tick_at` без lock | terminal-owner матрица (R6.5) |
| Два контроллера на одном Vsync | курсорный обход неизменен; дубли отказаны | `two_controllers_one_vsync`, `duplicate_registration_is_refused` |
| Realm остановлен посреди анимации | реестр дропает клоны; держатель виджета сохраняет прогон до `dispose` | `dropped_vsync_leaves_run_until_dispose` |
| dispose во время тика | `matches_sample` отбрасывает устаревший сэмпл; future отменён | `dispose_from_listener_during_tick` |
| Retarget в последнем кадре | новый прогон после `Completed`; якорь со следующего кадра | `retarget_from_completed_listener` |
| dt=0, огромный dt, время назад | чистая функция времени; `t` зажат в `[0,1]` | `time_edges_on_curved_run` |
| NaN/inf кривой, overflow span/длительности | hold + clamp; тотальная `scaled_run_duration`; span проверен в plan | R4, R5 |
| Panic: кривая | без lock; прогон остаётся; контроллер пригоден | `curve_panic_leaves_controller_usable` |
| Panic: слушатель статуса/value, continuation | переход закоммичен, раунд дорабатывает, первый payload — после раунда (listener-delivery R9/ADR-0106); future `Ok` | `a_panicking_status_listener_leaves_the_finished_run_ok`, `status_listener_panic_finishes_the_round` |
| Panic: `Drop` ретированного колбэка в `add_*` после dispose | ретирование вне guard через `Opaque`; первая паника поднимается | строка `late_listener_drop_panics_outside_guard` |
| Цикл реестров | `WouldCycle` при `attach_child`; межпоточная гонка непредставима после T6c (реестр `!Send`) | `attach_child_refuses_a_cycle` |

## 10. Остаточные риски и вопросы владельцу

- Прогон, запущенный вне кадра (таймер, future) без другого изменения дерева, ждёт следующего
  кадра — существующее поведение Vsync-контроллеров; будить цикл из старта — вопрос
  `motion-clock`/`ownership` (нужен hook реестра вне guard).
- Контроллер в двух разных реестрах тикается дважды до владеющего handle (`ownership`).
- `tick_at` остаётся `f64`-секундами до решения motion-clock о типизированном времени.
- Per-controller `Mutex` остаётся до frame-path-state; этот design не добавляет состояния под ним.
- Решено владельцем (orchestration): `RunFuture`/`RunCanceled` в `flui-animation`; `set_value` →
  `Result` (30 сайтов); повторная регистрация в `Vsync` — panic, называющий `try_register`.

## Владение и порядок замков

| Объект | Сильные ссылки | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `AnimationController` (`Arc<Mutex<Inner>>` до T6c, `Rc<ControllerCore>` после) | владелец (`DrivenController` после ownership), обёртки, колбэки | — (тикера больше нет) | handle снимает регистрацию и disposes; teardown — drop `Vsync`, run у живого handle pending до `dispose` |
| `RunCompleter` | `active_run` | — | разрешается только в `finish` |
| value/status-слушатели | канал контроллера | — | `dispose` очищает оба канала (R6.3) |
| `VsyncInner.parent` | — | `Weak` родителя | `detach_child`, drop родителя |

Циклы: **C1** (value-слушатель захватывает контроллер) — `dispose` очищает value-слушателей,
«Drop == 1» `dispose_clears_value_listeners` (строка `disposed_drops_late_value_listener` уже в
`animation/contract-tests`); **C3** (`Inner` → `Ticker` → колбэк → контроллер) — `Ticker`
удалён, `last_handle_drop_cancels_running_run`. Порядок замков до T6c: реестр (L1) → контроллер
(L2); реестры вкладываются только вверх (ребёнок → предок при `attach_child`); под L2 ничего
чужого не вызывается (снимает ребро L2 → hook → L1, O2); `Ticker` больше не дропается под L2
(O15). Self-deadlock `run_start_calls_no_foreign_code_under_controller_state` — тест с таймаутом в
дочернем процессе.

## Паттерн

- **Builder** — `AnimationController::builder(d)`; `#[must_use]` на методах (C-BUILDER).
- **plan/commit** — `RunPlan` без `&mut` делает «отказ после мутации» непредставимым.
- **Newtype с проверкой** — `ValueRange` (приватные поля, `new -> Result<_, RangeDefect>`).
- **Закрытые `#[non_exhaustive]` enum ошибок** со структурными полями вместо `String`.

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| 8 конструкторов `new/unbounded/with_*` | именованные конструкторы Dart | builder + `ValueRange` |
| `Ticker`/`TickerProvider` у контроллера | Flutter vsync-mixin | только `Vsync` + `tick_at` |
| `TickerFuture`/`TickerCanceled` | имя от удалённого Flutter-типа | `RunFuture`/`RunCanceled` |
| `is_animating` с default по статусу | Flutter getter, переопределённый с другим смыслом | обязательный метод, один предикат |
| ошибки `String` | исключения с сообщением | `thiserror`-enum, сопоставимый по варианту |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Узкий тип ошибки у конструктора значения | нарушала | design.md:119 `ValueRange::new -> AnimationError` | `Result<Self, RangeDefect>` |
| Один терминальный метод builder | нарушала | design.md:89-92, 113, 233; R2.1, R2.2 | только `build()`; `build_on` — ownership |
| Нет мёртвой сложности/сырых указателей | нарушала | design.md:79-82 `SmallVec<[*const (); 8]>`, :268; R7.2 | стек убран, `debug_assert!` на подъёме |
| Одна политика паники | нарушала | design.md:266 | первый payload после раунда |
| Решённые вопросы не висят открытыми | нарушала | design.md:44, 278-279 | `RunFuture` — решено |
| Порядок замков, таблица владения, C1/C3 | нарушала | — | раздел «Владение и порядок замков» |
| `Duration`, `try_from_secs_f64`, тотальная `scaled_run_duration` | соответствует | design.md:63-65 | — |
| `#[non_exhaustive]` на ошибках, `thiserror` | соответствует | design.md:125-144 | — |
| `Result` вместо `debug_assert` для ошибки вызывающего | соответствует | design.md:67-68 | — |
| Нет нового per-animation `Mutex` | соответствует | инв. 7 | — |
| `expect("BUG: …")`, `panic!` называет `try_` | соответствует | design.md:112 (теперь ownership) | — |
