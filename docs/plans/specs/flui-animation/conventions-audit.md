# flui-animation — аудит спек по конвенциям Rust

- **Дата:** 2026-10-06 · **База спек:** `9bf335e07` (номера строк `design.md:N` — до правки)
- **Мерило:** [orchestration.md](orchestration.md) — «Конвенции Rust» (включая «Форма
  абстракций» и «Владение, блокировки, lifetimes, generics»), «Решения по развилкам спек», «По
  итогам adversarial review», «Граница с send-flip»; политика паники владельца: переход закоммичен
  до вызова, раунд дорабатывает, **первая** паника пробрасывается после раунда, следующий кадр
  тикает — для value-, status-слушателей, продолжений и тиков в `Vsync::tick_all`.
- **Входы:** [review.md](review.md); аудиты фич 1.99, линтов, абстракций (A1–A10, R1–R9) и
  владения (O1–O18, циклы C1–C8, порядок замков, раскол T6c/`main`, trybuild) — вне репозитория.
- **Что сделано:** каждое нарушение исправлено в тексте `design.md` (и в `requirements.md`/
  `tasks.md`, где они противоречили); в каждом `design.md` — разделы «Паттерн», «Отличия от
  Flutter/Compose/SwiftUI», «Конвенции Rust (аудит)» и таблица владения; задачи — в
  [tasks.md](tasks.md) «Задачи из аудитов». Код не правился.

## 1. Итог по спекам

| Спека | Нарушений найдено / исправлено | Открыто (решение владельца) |
|---|---|---|
| listener-delivery | 7 / 7 | sealing `Animation` (R3); одна модель подписки значений/статуса |
| ownership | 6 / 6 | — (форма `build_on(Option<&Vsync>)` — см. §6) |
| controller-robustness | 6 / 6 | — |
| frame-path-state | 6 / 6 | один стёртый тип кривой (R2) |
| motion-clock | 3 / 3 | — |
| reduce-motion | 3 / 3 | — |
| retarget | 9 / 9 | — |
| integration | 4 / 4 | — |
| composition | 7 / 7 | — |
| curves | 6 / 6 | R2; `Curves` как namespace-struct (naming) |
| interpolation | 5 / 5 | bound кривой виджета (R2) |
| physics | 6 / 6 | пресеты `SMOOTH/SNAPPY/BOUNCY` (X17) |
| **Всего** | **68 / 68** | 7 уникальных решений (§6) |

«Нарушала (код)» — нарушение в коде, которое спека теперь назначает задаче; оно входит в счёт.

## 2. Нарушения по спекам (что было → что стало)

### listener-delivery
| Пункт | Было (design.md) | Стало |
|---|---|---|
| Одна политика паники | :11, 105-107, 155-156, 165-166, 233-234, 251; R9 «логируется, не пробрасывается» | раунд дорабатывает, первый payload — `resume_unwind` после раунда; R9, ADR п. 3, changelog |
| `pub` без потребителя | :175, 181-197 `pub StatusChannel`, `pub inert()` | `pub(crate)`; сторонняя `Animation` делегирует крейтовой |
| ID — `NonZero<u64>` | :157 `Slot(u64)` | `Slot(NonZero<u64>)`, вечный отказ |
| Порядок замков | не назван | владелец → канал (листовой); реестр → контроллер |
| Принятая работа, конечный кадр | :286-288 livelock «решение владельца» | settle внутри `drain` → следующий тик (X3) |
| Владение, C2/C4/C6 | нет | таблица + «Drop == 1» |
| Имена `Ticker*` | :10, 109, 230, 269, 277 | `RunDelivery`/`RunFuture` |

### ownership
| Пункт | Было | Стало |
|---|---|---|
| Без `Deref` ради наследования | :82, 101-104, 199-201 | `controller()` + `AsRef`; `AnimationController::dispose` → `pub(crate)` (trybuild) |
| Один терминальный метод | :85-87 `build(Option) -> Result`, `build_manual` | `build_on(Option<&Vsync>) -> DrivenController`, `build()` |
| `Result` с причиной | :86 | infallible: `Exhausted` → `Unbound` |
| Имена `Ticker*` | requirements R6–R7 | `RunCanceled`/`RunFuture` |
| Владение, C7 (+ предпосылки C1, C3) | нет таблицы | таблица, `driven_controller_drop_unregisters` |
| Порядок замков | — | реестр → контроллер |

### controller-robustness
| Пункт | Было | Стало |
|---|---|---|
| Узкая ошибка значения | :119 `ValueRange::new -> AnimationError` | `-> Result<_, RangeDefect>` |
| Один терминальный метод | :89-92, 113, 233; R2.1-R2.2 `build_on -> (ctl, reg)` | только `build()`; кортежа нет |
| Мёртвая сложность, сырые указатели | :79-82 `SmallVec<[*const (); 8]>`, :268; R7.2 | стек убран, `debug_assert!` на подъёме, строка `nested_registry_ticks_once` |
| Политика паники | :266 | первый после раунда |
| Решённое не висит открытым | :44, 278-279 `RunFuture` | решено |
| Владение, C1/C3, порядок замков | нет | раздел «Владение и порядок замков» |

### frame-path-state
| Пункт | Было | Стало |
|---|---|---|
| Конструкторы `impl Animation + 'static` (R5) | :172, 275 `Rc<dyn Animation>` от вызывающего | стирание внутри; `CurvedAnimation` не generic |
| Ext-трейт без пользователя | :172 `AnimationExt::curved(self: Rc<Self>)` | удаляется (composition) |
| Borrow через user code (2024) | :122-124 только proxy | раздел «Реентри родителя», строки proxy/switch |
| Политика паники | :236-237 | первая после раунда |
| Владение C5/C7, порядок замков F2 | :154-161 без таблицы | таблица, `animated_opacity_releases_proxy_after_tree_drop` |
| Один стёртый тип кривой | :176 «`ArcCurve` `Send + Sync` — данные» | помечено решением владельца (R2) |

### motion-clock
| Пункт | Было | Стало |
|---|---|---|
| `RefMut` наружу | :159-160 `-> &mut MotionClock` из `RefCell` | `with_motion_clock(f)` |
| `thiserror` + `TryFrom` | :119-120 без `#[error]` | `#[error]`, `TryFrom<f64> for PlaybackRate` |
| Владение и borrow-правило | нет | таблица; `let tick = …borrow_mut().frame(raw);` до `tick_all` |

### reduce-motion
| Пункт | Было | Стало |
|---|---|---|
| Явный `behavior` (X2) | :63-64, 215-216, 240, 258; R6 «unbounded ⇒ Preserve» | `Normal` по умолчанию, `Preserve` явно |
| Индикаторы (X9) | не названы | `Preserve` |
| Владение | нет | таблица |

### retarget
| Пункт | Было | Стало |
|---|---|---|
| `PartialEq` по значению (X4) | :79-83 `ArcCurve` = `ptr_eq` | встроенные — по значению |
| Одна производная (X13) | :64-69, tasks T2 — `slope` вводит retarget | вводит curves T9 |
| Цвет в Oklab (X5) | :210-211 sRGB u8 | `TwoWayConverter for Color` — interpolation |
| `behavior` (X2) | :146 | `AnimatedValue` — `Normal` |
| `Ticker*` | :91-106 | `RunFuture`/`RunCanceled` |
| Стирание `Simulation` (R8) | :151 `Arc<dyn Simulation>` | один раз, `Rc` после F3 |
| Derive (R9) | :121 `#[derive(Animatable)]` | `#[derive(TwoWayConverter)]` |
| Решения владельца | :215-222 открыты | default `Curve(EaseInOut, 200 ms)`; back gesture по скорости |
| Владение | нет | таблица, `animated_value_drop_releases_segments` |

### integration
| Пункт | Было | Стало |
|---|---|---|
| Временные атомики запрещены | :59, 94-97, 111-117, 165-169 `Arc<MotionSample>`, `AtomicBool`, `Arc::ptr_eq` | `Rc<Cell<_>>`, `Cell<bool>`, `Rc::ptr_eq` |
| Колбэк хранит минимум | захват не назван (форма C5) | `Weak` proxy |
| `impl Animation + 'static` | :93, 97; R8 | R5 |
| Владение | нет | таблица, `animated_transform_releases_proxy_after_tree_drop` |

### composition
| Пункт | Было | Стало |
|---|---|---|
| Owner-local bound | :101 `impl Curve + Send + Sync + 'static` | `impl Curve + 'static` |
| Ассоциированный тип | :107 `Animatable<T>` | `type Value = T` |
| Одна производная | :152-153, 216 разность `1e-4` | `Curve::slope` |
| Ext-трейты (R4) | не назначено | `AnimationExt`, `CurveExt` удаляются; `AnimatableExt` = `animate` |
| Derive (R9) | не назначено | `#[derive(TwoWayConverter)]` + `Lerp` |
| Решения владельца | :220-231 (X9, три вопроса) | `Preserve`; вопросы закрыты |
| Владение | нет | таблица |

### curves
| Пункт | Было | Стало |
|---|---|---|
| Параметр ошибки — enum | :86-95 `&'static str` | `CurveParameter` |
| `pub` с потребителем | :61-64, 106, 108, 151-152 `Threshold`, `SawTooth` | удалены |
| Потребитель `steps()` | :154-164, C12-C13 каретка | `CupertinoActivityIndicator` (`Steps(1, Start)` на сегмент) |
| Одна производная | (retarget) | `Curve::slope`, задача T9 |
| `PartialEq` по значению | `ArcCurve` = `ptr_eq` | внутренний enum `Builtin`/`Custom` |
| Bounds на impl (код) | curved.rs:56 `C: Clone` | карточка |

### interpolation
| Пункт | Было | Стало |
|---|---|---|
| Ассоциированный тип (R1, код) | tween_types.rs:35, 176, 340, 452; tween.rs:42-51; ext.rs:53 | назначено сюда |
| `PhantomData<fn() -> T>`, bounds на impl (код) | tween.rs:41-53, tween_types.rs:179, 461 | R1 убирает `T` |
| `TwoWayConverter for Color` (X5) | не назначено | здесь, premultiplied Oklab |
| `#[must_use]` builder | :111-113 | добавлено |
| `#[diagnostic::on_unimplemented]` | — | на `Animatable` |

### physics
| Пункт | Было | Стало |
|---|---|---|
| `pub` без внешнего потребителя | :170 `final_x`, `time_at_x` | `pub(crate)` |
| Стирание один раз (R8, код) | simulation.rs:95, controller.rs:1689/1712 | раздел «Стирание `Simulation` один раз» |
| Решение владельца | :193, 292-293 `smoothing` открыт | удаляется |
| Удалённое API | :248 `AnimatedValue::advance(Duration)` | ведётся `Vsync` |
| Тест с относительным допуском (код) | tests/contracts/simulation.rs:54 | O17, physics T2 |
| `as` с потерей (код) | spring.rs:89 `f64 → u8` | задача линтов |

## 3. Выбранные Rust-паттерны

| Паттерн | Где | Почему |
|---|---|---|
| RAII guard | `StatusSubscription`, `DrivenController` | время жизни подписки/регистрации — тип; забытая пара add/remove, register/dispose непредставима |
| Закрытый enum (`#[non_exhaustive]`) | `MotionSpec`, `TransformMotion`, `Seat`, `RunAnchor`, `SystemMotion`, `MotionPolicy`, `AnimationBehavior`, `JumpAt`, `RotationPath`, `Segment<T>`, ошибки | набор известен крейту; enum вместо bool/флагов и вместо трейта |
| Открытый трейт как точка расширения | `Curve`, `Simulation`, `Animatable`, `TwoWayConverter` | пользователь пишет свои; dyn — один раз на границе хранения |
| Sealed trait | `Animation` — кандидат | решение владельца (R3) |
| Newtype с проверкой | `ValueRange`, `PlaybackRate`, `AnimationTime`, `DurationScale`, `Angle`, `Slot(NonZero<u64>)` | единицы и инварианты — тип (C-NEWTYPE, C-VALIDATE) |
| Capability by construction | `FrameTick` (чеканит только `MotionClock`) | монотонное конечное время — свойство типа |
| Builder | `AnimationController::builder`, `KeyframesBuilder` | матрица конструкторов → независимые опции; первая ошибка копится до `build` |
| plan/commit | старт прогона (`RunPlan`) | отказ после мутации непредставим (нет `&mut` в `plan`) |
| Owner-local core | `Rc<ControllerCore>`: `Cell<Published>` + `RefCell` с одной точкой `mutate` | чтение без замка и заимствования; send-flip a1 |
| Arena/slab + generational ID | только токены регистрации `Vsync` (ADR-0125) | для контроллера отклонено (frame-path-state (a)): в paint нет контекста |
| Push-кэш + `Weak`-захват | `RenderAnimatedTransform`, `RenderAnimatedOpacity` | paint не вызывает чужой код; нет self-цикла proxy |
| Ассоциированный тип | `Animatable::Value`, `TwoWayConverter::Vector` | выход определяется реализацией (R1) |
| Видимость как правило | `register`/`unregister`/`dispose` → `pub(crate)` | ручную пару не написать (trybuild) |

## 4. Где спеки повторяли чужую форму

| Форма | Откуда | Решение |
|---|---|---|
| `Deref<Target = AnimationController>` у handle | наследование класса | `controller()`/`AsRef` (ownership) |
| `add_status_listener`/`remove_status_listener(id)` | Flutter пара | RAII `StatusSubscription` |
| публичный `StatusChannel` как база сторонних анимаций | `ChangeNotifier`/mixin-база | `pub(crate)`, делегирование |
| «report and continue» без проброса паники | Flutter `reportError` | первый payload после раунда |
| 8 именованных конструкторов контроллера | Dart named constructors | builder + `ValueRange` |
| `Ticker`/`TickerProvider`/`TickerFuture` | Flutter vsync-mixin | только `Vsync`; `RunFuture` |
| `is_animating` с default, переопределённый с другим смыслом | Flutter getter | обязательный метод, один предикат |
| `build_manual` + `build(Option) -> Result` | два вида одним методом | `build()` / `build_on` |
| `timeDilation` — процесс-глобальная | Flutter global | `MotionClock` у презентации |
| `MediaQuery.disableAnimations` в каждом виджете; два bool в `AccessibilityFeatures` | Flutter | обход `Vsync` + `MotionPolicy` enum |
| «unbounded ⇒ Preserve» | побочный эффект конструктора | `behavior` явно |
| `Animatable<T>` + `PhantomData<T>` | Dart generic-класс | ассоциированный тип |
| `AnimationExt`, `CurveExt`, `ParametricCurve`, `Curve2D` | extension-методы / иерархия классов | удалены |
| `#[derive(Animatable)]`, выводящий `TwoWayConverter` | имя по Flutter-классу | `#[derive(TwoWayConverter)]` + `Lerp` |
| `pub struct Curves;` с константами | Dart namespace-класс | модуль констант — спека naming (открыто) |
| `CurveError { parameter: &'static str }` | строка-метка | `CurveParameter` |
| pub-поля `SpringDescription`/кривых + `assert` | Dart data-класс | приватные поля, `try_new` |
| `restart_from_zero` при retarget, `advance(dt)` | Flutter implicit / игровой `Update(dt)` | сегмент из (x, v), ведомый `Vsync` |
| три `RenderXTransition` | иерархия классов | один объект + enum |
| `set_playback_rate`/`playback_rate`, сеттеры render object | setter/getter | оставлено с обоснованием: `&self` на разделяемом handle; конвенция `flui-objects` (`RenderUpdateImpact`) |
| `TweenSequence` с весами | Flutter | `Keyframes` с `Duration` |

## 5. Владение: циклы и разрывы

| Цикл | Разрыв | Тест «Drop == 1» | Спека |
|---|---|---|---|
| C1 контроллер → value-слушатель → контроллер | `dispose` очищает value-слушателей | `dispose_clears_value_listeners` | controller-robustness |
| C2 контроллер → status-колбэк → контроллер | `StatusSubscription` | `status_subscription_drop_releases_captures` | listener-delivery |
| C3 `Inner` → `Ticker` → колбэк → контроллер | `Ticker` удалён | `last_handle_drop_cancels_running_run` | controller-robustness |
| C4 родитель → слушатель обёртки → обёртка | подписка у подписчика | `wrapper_status_listener_dies_with_subscription` | listener-delivery |
| C5 proxy → его слушатель → proxy | `Weak` proxy в колбэке render object | `animated_opacity_releases_proxy_after_tree_drop`, `animated_transform_releases_proxy_after_tree_drop` | frame-path-state F4 (T6d), integration |
| C6 proxy → switch → `on_switched` → proxy | `switch.dispose` изымает колбэки; route — `Weak` proxy | `switch_dispose_releases_callbacks`, `hopping_route_dropped_without_dispose_frees_proxy` | listener-delivery + `main` widgets |
| C7 `Vsync` → контроллер → слушатель → `Vsync` | регистрацию держит `DrivenController` | `driven_controller_drop_unregisters` | ownership |
| C8 completer → продолжение → контроллер | одноразовый, до разрешения прогона | — (документируется) | controller-robustness |

**Порядок замков до send-flip T6c** (после — те же рёбра для `borrow_mut`): реестр `Vsync` →
контроллер → канал статуса (листовой); реестры вкладываются только вверх (ребёнок → предок при
`attach_child`); обёртка копирует `Rc`/`Arc` родителя и отпускает себя до вызова; под guard
контроллера чужой код не вызывается (снимает ребро L2 → hook → L1, O2), `Ticker` не дропается под
guard (O15). Self-deadlock — тест с таймаутом в дочернем процессе. Edition 2024: скрутини `match` и
let-chain держат guard во всей ветке — значение копируется `let` до `match`/`if let`; clippy
`significant_drop_in_scrutinee` этого не видит для let-chain и `RefCell` (O11).

## 6. Открытые решения владельца

1. **R3:** запечатать `Animation` (trybuild `animation_is_sealed`) или оставить открытым — тогда
   публичный канал статуса только с названным внешним потребителем и ADR.
2. **R2:** один стёртый тип кривой для owner-local хранения: send-flip строка 264 держит `ArcCurve`
   `Send`, строки 21/39 принимают `impl Curve + 'static`; от решения зависят bound в
   composition/interpolation/виджетах и фикстура `rc_curve_is_accepted`.
3. **X17:** пресеты `SpringDescription::{SMOOTH, SNAPPY, BOUNCY}` — удалить или сделать
   значениями по умолчанию `.spring()`.
4. **R6:** убрать `Listenable::remove_all_listeners` из трейта (foundation; естественный дом —
   send-flip T5) и свести value-подписки к тому же RAII-токену, что статус.
5. **Форма `build_on`:** решение X8 записано как `build_on(&Vsync)`; спеки используют
   `build_on(Option<&Vsync>)` — без `Option` виджет без `VsyncScope` не получит unbound handle
   (ownership R12). Подтвердить.
6. **A10:** `pub struct Curves;` → модуль констант — в спеку naming до 0.2.0.
7. Уже открытые в спеках и не снятые этим аудитом: R15 `activate` в `flui-view` (ownership), ADR
   frame-path-state как раздел ADR-0136 и резерв 0143–0150 (X10).

## 7. Задачи

Карточки — [tasks.md](tasks.md) «Задачи из аудитов»: (a) контрактные строки T6c по O1–O8, O13–O15
с отметкой, что уже лежит в `origin/animation/contract-tests` (O1, O7, O8 — есть; O3 — частично);
(b) правки `main` вне файлов T6c (C6 в `transition_route`, O10–O12, D-42/O16, O17, O9 через
владельца T6d, generic-bounds кривых и tween); (c) линты крейта (~40 мест) и 74 doctest'а под
`ignore` — тема quality; (d) trybuild-фикстуры с темой-владельцем.
