# motion-clock — design

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md).

## Итог

1. **`MotionClock`** — значение (не `Arc`, без lock) в `flui-animation`, одно на презентацию;
   владеют `PresentationState` (`flui-runtime`) и `HeadlessBinding` (`flui-testing`). Переводит
   сырое время кадра (`Duration`) во время анимации: скорость, ребейз эпохи, `step`, монотонность,
   насыщение. Только он чеканит `FrameTick`.
2. **`Vsync::tick_all(&FrameTick)`** вместо `tick_all(f64)`; **`AnimationController::tick_at(Duration)`**
   вместо `f64` (решение, отложенное controller-robustness): нефинитное время непредставимо (D-33).
3. **Скорость на анимацию** — `AnimationController::set_playback_rate(PlaybackRate)`, ≥ 0, 0 —
   пауза, применяется с ближайшего тика с сохранением локального времени. Потребитель —
   таймер показа SnackBar (пауза под указателем).
4. **Скрытая презентация не тикается; при показе — догон.**
5. **`TIME_DILATION` и мёртвый epoch-API `flui-scheduler` удаляются**; правки крейта
   `flui-scheduler` (включая удаление `Ticker` из controller-robustness T9) идут одной
   последовательностью в этой теме. Скорость и шаг презентации — production-путь devtools
   (агентская операция `motion`) и `flui-testing`.
6. **Шов для reduce-motion** — `FrameTick` (ниже). ADR нужен (черновик ниже).

## Текущее состояние (чтением, база `9a4daa3ed`)

- `flui-scheduler/src/config.rs:43` — `static TIME_DILATION: AtomicU64`; `:56` `time_dilation()`,
  `:83` `set_time_dilation`, `:208` `SERVICE_EXT_TIME_DILATION`, `:214` `adjust_duration_for_epoch`;
  реэкспорт `lib.rs:164-167`; запись `globals` с `exit = "ADR-0097"` — `Cargo.toml:110`.
- `scheduler.rs:3119` `reset_epoch` обнуляет `epoch_start` (`:850`); `:3143`
  `current_frame_time_stamp` = `adjust(epoch, 0)` — всегда ≈ 0; `:3154` `adjust_for_epoch`;
  `:3277` `debug_assert_no_time_dilation`. Production-вызовов нет ни у одного (rg).
- `controller.rs:1897` и `:2521` делят **всё** прошедшее время запуска на текущий глобальный
  коэффициент: смена 1 → 5 на 0.5 s откатывает значение к 0.1 (D-25). Вход — `tick_at(f64)`
  (`:1870`); `velocity()` (`:1766`) берёт `cycle_elapsed_secs` (`:2520`).
- `vsync.rs:440` `tick_all(now_secs: f64)` якорит `Some(now_secs)` без проверки (`:488`);
  `elapsed_since` (`:81`) на NaN даёт NaN → запуск заморожен, `has_running()` (`:338`) истинен
  навсегда (D-33). Убывающее время проходит молча (док `:377-382`).
- `flui-runtime/src/ui_realm/frame.rs:94` берёт одно `now_secs()` и тикает **каждую** презентацию
  (`:95-120`), включая скрытые; продолжение кадров — `:922-929`. Источник — `frame_clock.rs:85`;
  `:113` `set_now_secs_for_test(f64)` без проверки. Скрытость — `FrameClock::set_hidden`
  (`presentations.rs:506`). В Hidden/Paused кадров нет (`flui-app/src/app/runner/frame_pacing.rs:47`,
  `desktop.rs:448`): при возврате time-запуски прыгают по стене — не выбрано и не закреплено.
- Скорости на анимацию нет (market.md M-TIME-12). Таймер SnackBar — контроллер длительности
  показа (`packages/flui-material/src/scaffold_messenger.rs:549-569`), наведения указателя
  messenger не видит.
- Devtools-агент знает `windows | read | act` (`packages/flui-devtools/src/agent/session.rs:254`);
  окно — `AgentWindow` (`flui-view/src/dev_agent.rs:275`) над `AgentPort`
  (`flui-view/src/__runtime.rs:44`), реализация — `DevAgentPort` (`flui-runtime/src/ui_realm/agent.rs:533`).
- `HeadlessBinding` тикает `Vsync` секундами `ManualClock` (`flui-testing/src/lib.rs:1005-1006`,
  `:1368-1370`), `LaidOut` — стеной (`widgets/host.rs:194`).

## Варианты

### (a) Где живёт скорость презентации

| Вариант | За | Против |
|---|---|---|
| A1. Внутри `Vsync` (`set_rate` на реестре) | без нового типа | `Vsync` — `Arc<Mutex>`, состояние под lock в кадре (против frame-path-state); реестр подменяем (`set_vsync`) и вложен (`TickerMode`) — у какого скорость?; `f64`-вход остаётся |
| A2. `FrameClock` в `flui-scheduler` | рядом с produce-gate | scheduler ниже `flui-animation` — `FrameTick` пришлось бы держать там; смешивает физическое время кадра и время анимации |
| **A3. `MotionClock` — значение в `flui-animation`, владелец — презентация** | `&mut self`, без lock; один тип для runtime и тестов; только он чеканит `FrameTick` → монотонность и конечность — свойство типа | новый pub-тип; 41 вызов `tick_all` |

Выбран **A3** (AGENTS «Make rules types»; ADR-0097 без статиков; frame-path-state без lock).

### (b) Смена скорости презентации

Flutter: `epochStart = adjust(lastRaw)`, `firstRawInEpoch = null` (market-B T4, [fl-binding]) —
интервал от последнего кадра до первого после смены теряется. Compose делит
`(frameTime − start)/scale` живым масштабом (`SuspendAnimation.kt` L333-338, [ax-suspend]) —
переписывает историю, как D-25. **Выбрано:** ребейз на последнем сыром времени (`epoch_raw =
last_raw`, `epoch_time = now`): ни скачка, ни потери интервала.

### (c) Скорость на анимацию

- **Знак.** WAAPI допускает отрицательную скорость с переворотом фаз (market-B K2, [wa1] §4.5.15);
  у контроллера направление уже задаёт запуск (`reverse`, `animate_back` с масштабом
  длительности), и знаковая скорость дала бы второй способ развернуть с другой семантикой
  статусов. **Выбрано:** ≥ 0, 0 — пауза; разворот — существующими запусками.
- **Момент применения.** Мгновенный ребейз «сейчас» требует времени презентации в момент вызова,
  которого контроллер не знает (тики приходят только из `Vsync`); ребейз на последнем тике после
  паузы прыгнул бы на всю паузу. **Выбрано:** WAAPI-модель «pending rate» — новая скорость
  применяется на ближайшем тике: время до него — по старой, локальное время на нём сохраняется.
  При 1 → 0 переход дотягивает ≤ один кадр; при 0 → 1 скачка нет при любом простое.
- **Где хранится.** Поля в существующем состоянии контроллера (`rate`, `pending_rate`,
  `epoch_elapsed`, `epoch_local`) — форма по frame-path-state; нового `Mutex` нет.
- **Потребитель.** Таймер SnackBar: `MouseRegion` вокруг SnackBar → `set_playback_rate(PAUSED)`
  на входе, `NORMAL` на выходе, затем `rebuild.schedule` для кадра. Альтернатива «пересоздать
  контроллер с остатком» теряет future и порядок статусов. Обоснование паузы на наведении —
  наше решение, тест закрепляет его, а не внешний эталон.

### (d) Скрытая презентация: продолжить или догнать

Рынок догоняет: Compose паузит часы ниже `STARTED` ([axui-recomposer]), но время запуска —
`frameTime − start` от Choreographer ([ax-suspend]): после паузы анимация прыгает к стене;
Flutter `Ticker.muted` — «time still elapses» ([fl-ticker]); GPUI считает от `Instant`.
Продолжение (заморозка на скрытие) оставило бы futures, которых ждёт логика (dismiss → удаление),
висеть всё скрытие, проиграло бы устаревший переход через 10 минут и удлинило бы таймеры на
контроллерах (SnackBar). **Выбрано: догон.** Скрытая презентация не тикается и не держит кадры,
её сырое время идёт; первый видимый кадр сэмплирует всё сразу — запуски — функции времени
(M-TIME-1). Пауза по желанию — скорость 0 (devtools, контроллер).

## Публичный контракт

`flui-animation`, новый модуль `motion.rs`, реэкспорт из `lib.rs`:

```rust
/// Время анимации презентации: монотонно, от начала её часов.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationTime(Duration);
impl AnimationTime { pub const ZERO: Self; pub const fn as_duration(self) -> Duration;
    pub fn saturating_since(self, earlier: Self) -> Duration; }

/// Скорость времени: конечна, ≥ 0; 0 — пауза.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct PlaybackRate(f64);
impl PlaybackRate { pub const NORMAL: Self; pub const PAUSED: Self;
    pub fn new(rate: f64) -> Result<Self, InvalidPlaybackRate>;
    pub const fn get(self) -> f64; pub fn is_paused(self) -> bool; }
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)] #[non_exhaustive]
pub enum InvalidPlaybackRate { #[error("playback rate {0} is not finite")] NonFinite(f64),
    #[error("playback rate {0} is negative")] Negative(f64) }
impl TryFrom<f64> for PlaybackRate { type Error = InvalidPlaybackRate; } // = new (C-CONV-TRAITS)

/// Один кадр реестра. Чеканит только `MotionClock`; поля приватны.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTick { now: AnimationTime }
impl FrameTick { pub fn now(&self) -> AnimationTime; }

#[derive(Debug, Clone, Default)]
pub struct MotionClock { /* rate, epoch_raw, epoch_time, last_raw, now */ }
impl MotionClock {
    pub fn new() -> Self;
    /// Сырое время кадра → тик. Назад — держит; переполнение — насыщение.
    pub fn frame(&mut self, raw: Duration) -> FrameTick;
    pub fn now(&self) -> AnimationTime;
    pub fn rate(&self) -> PlaybackRate;
    pub fn set_rate(&mut self, rate: PlaybackRate);        // ребейз на last_raw
    pub fn step(&mut self, dt: Duration) -> AnimationTime; // +dt при любой скорости
    pub fn is_paused(&self) -> bool;
}

impl Vsync { pub fn tick_all(&self, tick: &FrameTick); } // has_running: без запусков со скоростью 0
impl AnimationController {
    /// Время запуска с его якоря, во времени презентации.
    pub fn tick_at(&self, elapsed: Duration);
    /// С ближайшего тика; локальное время на нём сохраняется. Переживает перезапуски.
    pub fn set_playback_rate(&self, rate: PlaybackRate);
    pub fn playback_rate(&self) -> PlaybackRate;
}
```

Удаляются: `flui_scheduler::{time_dilation, set_time_dilation, SERVICE_EXT_TIME_DILATION}`,
`config::{InvalidTimeDilation, adjust_duration_for_epoch}`, `UpdateScheduler::{set_time_dilation,
reset_epoch, current_frame_time_stamp, adjust_for_epoch, debug_assert_no_time_dilation}`, поле
`epoch_start`. Обратного seek нет: время презентации монотонно (Compose `withFrameNanos` строго
монотонно, [axrt-monotonic]); перемотка одной анимации — `set_value` (M-INT-8).

Вне крейта: `flui-protocol` — `MotionRequest { rate: Option<f64>, step_ms: Option<u64> }`,
`MotionState { rate: f64, time_ms: f64 }` (snake_case, ADR-0095); `flui-view` —
`AgentWindow::motion(MotionRequest) -> Result<AgentAnswer<MotionState>, AgentFault>` и
`AgentPort::motion`; `flui-testing` — `HeadlessBinding::with_motion_clock<R>(&mut self, f: impl FnOnce(&mut
MotionClock) -> R) -> R`, `with_presentation_motion_clock<R>(&mut self, PresentationId, f) -> Option<R>` (часы живут в `RefCell` презентации — `RefMut` наружу не отдаётся);
`flui-runtime` — `UiRealm::set_now_for_test(Duration)` (было `set_now_secs_for_test(f64)`).

### Шов для reduce-motion

`FrameTick` — единственный канал «что знает кадр» от часов презентации до каждого контроллера:
`Vsync` передаёт `&FrameTick` в crate-private тик контроллера вместе с `elapsed`. Спека
reduce-motion добавляет поле политики в `FrameTick` (приватные поля — не ломающее изменение) и её
setter в `MotionClock`, а контроллер читает её в том же тике. Эта спека не определяет ни
политику, ни платформенный сигнал, ни `AnimationBehavior`.

## Инварианты и владение

- **I1** `MotionClock::now()` не убывает; время конечно по типу. `Δraw` — `u128` нс;
  `rate·Δ` → `Duration::try_from_secs_f64`, ошибка → `Duration::MAX`; сложение насыщается.
- **I2** Любая смена скорости презентации — ребейз на `last_raw`; `step` двигает `epoch_time`.
- **I3** `Vsync` хранит якоря как `AnimationTime` и последний принятый тик; ранний тик →
  последний (R6).
- **I4** Локальное время запуска = `epoch_local + (elapsed − epoch_elapsed)·rate`; pending-скорость
  складывается на ближайшем тике; старт запуска обнуляет обе эпохи, скорость сохраняет.
- **I5** Запуск со скоростью 0 (без pending > 0) не тикается и не входит в `has_running()`.
- **I6** Часы — поле `PresentationState` (`RefCell<MotionClock>`, owner-поток, `!Send`);
  `frame()` и отпускание borrow — **до** `tick_all`; пользовательский код под borrow не идёт.
  Новых `static`, `Mutex`, `Arc` нет.
- **I7** Скрытая презентация: `frame()` не зовётся, сырое время идёт; первый видимый кадр — догон.
- **I8** `velocity()` = производная по локальному времени × скорость контроллера.

## Миграция

- `tick_all(f64)` → `tick_all(&FrameTick)`: rg `tick_all\(` — production 4 (`frame.rs:120`,
  `flui-testing/src/lib.rs:1006`, `:1370`, `widgets/host.rs:194`), тесты 32 (`vsync.rs` 15,
  `controller_tests.rs` 3, `tests/contracts/controller_sources.rs` 14), бенч 6
  (`benches/vsync_registry.rs`). Тестам — помощник `tick(&mut MotionClock, ms)` в `tests/support`.
- Строки тем, слитых раньше и написанных на `f64`-времени, мигрируют здесь механически; строки о
  неконечном времени на входе контроллера/реестра (listener-delivery R18 `nan_time_no_flip`,
  frame-path-state R5.8 NaN/±inf в `tick_all`, controller-robustness R9.4 `tick_at(f64::MAX)` →
  `Duration::MAX`) становятся непредставимыми: удаляются со ссылкой на R8 здесь (граница
  `set_now_for_test`), а не переписываются под новый тип.
- Якорь реестра — одна форма `RunAnchor { Fresh, Continue(AnimationTime), Resume(Duration) }` в
  `vsync.rs` (T3): `Continue` — retarget, `Resume` — ownership, таймлайн по поведению — reduce-motion;
  эти темы добавляют только производителей варианта.
- `tick_at(f64)` → `tick_at(Duration)`: вызовы после controller-robustness (Q0 переносит тесты в
  `tests/`) — пересчитать `rg "tick_at\("` в задаче T1; сейчас 1 production-вызов (`vsync.rs:509`).
- `time_dilation()` — 2 места (`controller.rs:1897`, `:2521`), удаляются.
- `set_now_secs_for_test` — тесты `flui-runtime` (rg в T4).
- `flui-scheduler` одной последовательностью (T6): удаление dilation; затем, после слияния
  controller-robustness T7/T8, ханки `flui-scheduler` из её T9 (`Ticker`, `TickerGroup`,
  `TickerProvider`, `ticker.rs`, `lib.rs`, `scheduler.rs:82`, тесты/пример) — чтобы два PR не
  правили `lib.rs`/`scheduler.rs`/`ticker.rs` одновременно.

## Платформы и CI

Платформенного кода нет. Исполняется в CI: `flui-animation`, `flui-testing`, `flui-runtime`
(headless), `flui-devtools` agent-тесты на Linux (сокет Unix). Named pipe агента на Windows — только
clippy в CI; локальный прогон `motion_op_round_trips_over_the_endpoint` на Windows — в PR.

## Черновик ADR

**ADR-NNNN: Время анимации принадлежит презентации; скорость и гигиена времени кадра.**
Amends ADR-0097 (выполняет выход `TIME_DILATION`), ADR-0027 §8.

Решение. (1) Время анимации презентации — `MotionClock` в `flui-animation`, значение, которым
владеет презентация (и тестовый двойник). Процесс-глобального коэффициента нет. (2) Реестр
`Vsync` тикается только `FrameTick`, который чеканит `MotionClock`: время конечно, не убывает;
смена скорости ребейзит эпоху на последнем сыром времени; контроллер принимает `Duration`.
(3) Скорость на анимацию — неотрицательная, 0 — пауза, применяется с ближайшего тика с
сохранением локального времени, переживает перезапуски, умножается на скорость презентации.
(4) Скрытая презентация не тикается; при показе время догоняет. (5) Скорость и шаг презентации
доступны devtools (агентская операция `motion`) и `flui-testing`; обратного seek нет.
(6) `FrameTick` — канал кадровых фактов к контроллеру (политику движения добавляет ADR reduce-motion).
Альтернативы: состояние в `Vsync`, в `FrameClock`, знаковая скорость, заморозка на скрытие —
отвергнуты (design.md «Варианты»). Последствия: ломаются `tick_all(f64)`, `tick_at(f64)`;
удаляется API dilation в `flui-scheduler`. Верификация: R1, R2 (PB), R10, R11 (PB), R15, R18.
ADR-0097: в статусе — «`TIME_DILATION` удалён по ADR-NNNN».

## Фрагмент changelog

```markdown
### Changed

- **`flui-animation`**: `Vsync::tick_all` takes a `FrameTick` minted by the presentation's
  `MotionClock`, and `AnimationController::tick_at` takes a `Duration`; frame time is typed,
  finite and never moves backwards.
- **`flui-runtime`**: a hidden presentation is not ticked; its animations catch up on the first
  visible frame.

### Added

- **`flui-animation`**: `MotionClock` (per-presentation rate and step) and
  `AnimationController::set_playback_rate`.
- **`flui-material`**: a SnackBar's display timer pauses while the pointer is over it.
- **`flui-devtools`**: the agent `motion` operation sets a window's animation rate and steps it.

### Removed

- **`flui-scheduler`**: the process-wide time dilation (`set_time_dilation`, `time_dilation`,
  `SERVICE_EXT_TIME_DILATION`, epoch helpers).
```

## Adversarial review

- **Реентри слушателя в контроллер и реестр.** Тик выдан и borrow часов отпущен до обхода (I6);
  lock реестра снят до тика контроллера (как сейчас, `vsync.rs:506-510`). `set_playback_rate` из
  слушателя пишет pending — применяется следующим тиком (R19); перезапуск получает новое
  поколение → якорь следующим кадром (курсор только вперёд).
- **Снятие/добавление слушателей во время уведомления** — контракт listener-delivery; своих
  списков слушателей эта спека не вводит. Регистрация в реестре во время обхода — за `fence`.
- **Drop последнего владельца из колбэка.** Обход держит клон контроллера на шаг
  (`RegistryWalkStep::Running`); строка R19.
- **Два контроллера на одном тикере** — один `FrameTick`, свои скорости (R21). Один реестр, два
  источника часов — I3, R6.
- **Realm остановлен посреди анимации** — после `Stopping` кадров нет (R22); futures закрывает
  dispose контроллера (controller-robustness). `MotionClock` обязательств не держит.
- **Retarget в последнем кадре** — новый запуск, эпохи обнулены, скорость сохранена (R13, R19).
- **dt = 0, огромный dt, время назад, NaN, переполнение** — R24, R7, R5, R8; rate 1e300 ×
  Δ 10⁶ s → ошибка `try_from_secs_f64` → `Duration::MAX`; `rate·(e − e0)` у контроллера — так же.
- **Скорость 0 и `is_animating`.** Запуск на паузе остаётся «running» (статус Forward/Reverse),
  но не держит кадры (I5) — потребитель, ждущий future, ждёт до снятия паузы: это контракт паузы.
- **Panic в пользовательском коде** — кривая/симуляция/слушатель внутри `tick_all`: часы уже
  продвинуты (R20), сдерживание — listener-delivery; агентская операция исполняется в inbox
  owner-потока, ответ — `send_reply` (агентский путь уже сдерживает отказы хука).
- **Пауза devtools при бесконечном repeat** — спроса нет (R3); кадры от других источников тикают
  с dt = 0 вхолостую, корректно.

## Остаточные риски

- Pending-модель: при 1 → 0 запуск дотягивает до ближайшего тика (≤ кадра) — задокументировано;
  если кадров нет долго (окно скрыто), пауза наступит на первом видимом кадре **после** догона
  всего скрытого интервала по старой скорости.
- Скрытая презентация больше не тикается: тесты `flui-runtime`, опиравшиеся на тик скрытой
  презентации, надо прогнать целиком.
- Пересечение файлов с controller-robustness (`vsync.rs`, `controller/tick.rs`) и frame-path-state
  (где лежат поля скорости) — порядок в tasks.md.
- Пробуждение кадра при `set_playback_rate` вне кадра — общий вопрос старта запуска вне кадра
  (controller-robustness, «Остаточные риски»); потребитель SnackBar сам планирует перестройку.

## Владение и заимствования

| Объект | Владелец | Слабые | Unmount / замена `VsyncScope` / teardown realm |
|---|---|---|---|
| `MotionClock` | `PresentationState` (`RefCell<MotionClock>`), `HeadlessBinding` | — | живёт с презентацией; замена `Vsync` его не трогает; teardown — drop презентации |
| `FrameTick` | `Copy`-значение на один обход | — | обязательств не держит |
| поля скорости контроллера | состояние контроллера (frame-path-state `mutate`) | — | с контроллером |

Циклов нет: часы не держат ни контроллеров, ни колбэков. Borrow часов — один оператор:
`let tick = presentation.clock.borrow_mut().frame(raw);` — временное `RefMut` умирает на `;`,
`tick_all(&tick)` идёт после; запрещено `match presentation.clock.borrow_mut().frame(raw) { .. }`
и let-chain `if let … = clock.borrow_mut()… && …` вокруг `tick_all` (edition 2024: guard жил бы
через обход). Замков нет; новые `static` не вводятся (выход `TIME_DILATION`, ADR-0097). Panic в
обходе — политика listener-delivery R9/R10: раунд дорабатывает, первый — после (часы уже
продвинуты, R20).

## Паттерн

- **Capability by construction** — `FrameTick` с приватными полями чеканит только `MotionClock`:
  монотонность и конечность — свойство типа (AGENTS «Make rules types»).
- **Newtype** — `AnimationTime(Duration)`, `PlaybackRate(f64)` (проверка в `new`/`TryFrom`).
- **Значение с `&mut self`** вместо разделяемого состояния — `MotionClock` без lock и `Arc`.
- **Закрытый enum** — `RunAnchor { Fresh, Continue, Resume }`, форма задаётся здесь (X6).

## Отличия от Flutter/Compose/SwiftUI

| Где было похоже | Чужая форма | Rust-форма здесь |
|---|---|---|
| `timeDilation` — процесс-глобальная переменная | Flutter global | значение `MotionClock` у презентации |
| `tick_all(f64)`, `tick_at(f64)` секунды | Dart `double` время | `FrameTick`, `Duration` |
| `set_playback_rate`/`playback_rate` | пара setter/getter | оставлено с обоснованием: `&self` на разделяемом handle — единственная форма изменения состояния анимации; имена по C-GETTER |
| знаковая скорость (WAAPI `playbackRate < 0`) | второй способ разворота | `PlaybackRate ≥ 0`, разворот — `reverse` |

## Конвенции Rust (аудит 2026-10-06)

| Пункт конвенции | Статус | Где (до правки) | Правка |
|---|---|---|---|
| Guard/`RefMut` наружу не отдаётся | нарушала | design.md:159-160 `-> &mut MotionClock` из `RefCell` | `with_motion_clock(f)` |
| `thiserror` с сообщениями, `TryFrom` | нарушала | design.md:119-120 | `#[error]`, `TryFrom<f64> for PlaybackRate` |
| Время — `Duration`/newtype | соответствует | design.md:107-148 | — |
| `f64 → Duration` через `try_from_secs_f64` | соответствует | I1 | — |
| Нет `static`/lock на пути кадра | соответствует | I6 | — |
| Borrow отпущен до user code (2024) | уточнено | I6 | раздел «Владение и заимствования» |
| `#[non_exhaustive]` на ошибке | соответствует | design.md:119 | — |
| Таблица владения | нарушала | — | добавлена |
