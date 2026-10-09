# flui-animation — adversarial review спек и сквозная согласованность

- **Статус:** черновик ревью · **Дата:** 2026-10-06
- **Объект:** 12 тем под `docs/plans/specs/flui-animation/*/` на `260a0de63` (+ правки ниже).
- **Метод:** чтение всех спек; выборочная сверка `file:line` на `main` `9a4daa3ed`
  (`controller.rs:2371-2376`, `:2505-2515`; `switch.rs:442-451`; `vsync.rs:338-363`, `:440-511`;
  `vsync_scope.rs:76-80`; `status.rs:112-120`; `dismissible.rs:107`; `spring.rs:74-82`;
  `curve.rs:1187-1197`; `notifier_generic.rs:302-340`) — подтверждены. Сверено с ветками
  `origin/send-flip/owner-frame` (ADR-0136, send-flip T4–T7) и реестром ADR в
  `origin/main:docs/plans/specs/release/tasks.md`. Код не правился, тесты не запускались.

## 1. Находки (по убыванию серьёзности)

H — блокирует слияние темы, M — должно быть решено до реализации, L — исправить по ходу.

| ID | Сер. | Сценарий отказа | Спеки | Предложение |
|----|------|-----------------|-------|-------------|
| X1 | H | **Две политики паники слушателя.** listener-delivery R9: паника status-слушателя ловится, логируется, наружу не выходит (ADR-0109 §3). send-flip T6b/T6c + R11 + T7 `listener_panic_reaches_the_realm_report`: каждый колбэк под `catch_unwind`, раунд доводится, первая паника — `resume_unwind` после раунда и попадает в отчёт realm. После слияния обоих: паника value-слушателя (foundation после T6b) выходит из `tick_at` и доходит до отчёта, паника status-слушателя в том же кадре — только строка лога; строки `panic_first_of_three` (LD) и `owner_callback_panic_containment` для status (send-flip T6c) противоречат друг другу — одна из веток красная после ребейза. | listener-delivery, frame-path-state R5.3, send-flip | Одна политика на все колбэки realm. Рекомендация: модель send-flip — сдержать, довести раздачу/очередь, первый payload резюмировать после `drain` (как LD уже делает для продолжений), `Vsync::tick_all` (LD R10) доводит обход и резюмирует первый → отчёт realm; следующий кадр тикает. Поправить LD R9, п. 3 черновика ADR, changelog. **Решение владельца.** |
| X2 | H | **Reduce motion молча обходит implicit-анимации.** reduce-motion R6: `unbounded` ⇒ `Preserve`. retarget: `AnimatedValue` ведёт `DrivenController` «(unbounded, builder)» → `AnimatedOpacity/Padding/Align/Container` (главная цель reduce motion) под `Reduce` продолжают анимироваться. | reduce-motion R6, retarget механика 5 | Снять конвенцию «unbounded ⇒ Preserve» (правило — тип, а не побочный эффект границ): `Normal` по умолчанию всегда, три scroll-сайта и таймеры явно `.behavior(Preserve)`; `AnimatedValue` — явно `Normal`. Строка `implicit_opacity_settles_under_reduce` в reduce-motion R2 (падает с текущим R6). **Решение владельца** (R6 помечен в спеке). |
| X3 | M-H | **Livelock вместо краха.** Очередь LD раздаёт из внешнего кадра до опустошения. Синхронный settle (нулевая длительность — `settle_at_target`; ownership R12 «без часов») из status-слушателя порождает новый статус в ту же очередь: `|s| if s.is_completed() { c.reverse() } else if s.is_dismissed() { c.forward() }` на контроллере с `Duration::ZERO` или на unbound `DrivenController` (по умолчанию в тестах без `VsyncScope`) крутит `drain` вечно — кадр не завершается; раньше было переполнение стека. LD признаёт риск («решение владельца»). | listener-delivery, ownership R12, controller-robustness | Синхронный settle, начатый во время `drain` того же канала, не исполняется внутри неё: для привязанного к Vsync — якорь и settle на следующем тике (как reduce-motion R17); для unbound — settle откладывается до следующего публичного вызова/`rebind`, а `drain` ограничен N событиями с `error!("BUG: …")` и хвостом, оставленным в очереди (принятая работа не теряется). Строка `zero_duration_ping_pong_terminates_the_frame`. **Решение владельца.** |
| X4 | M-H | **Спека движения сравнивается по указателю.** `ArcCurve: PartialEq` — `Arc::ptr_eq` (`curve.rs:1190-1194`). retarget R21: смена только спецификации → новый сегмент из (x, v). Родитель, перестраивающийся каждый кадр (`AnimatedBuilder` выше), создаёт новый `ArcCurve` для `.curve(Curves::EaseInOut)` → `MotionSpec` «изменился» на каждом build → сегмент перезапускается, анимация не доходит до цели (каждый кадр новая длительность). | retarget R21, curves | `set_motion` только при структурном неравенстве: встроенные кривые сравнивать по значению (`ArcCurve::eq` — `ptr_eq` или downcast через `Any` + `PartialEq` кривой), и/или применять новую спецификацию только к **следующему** сегменту. Строка `rebuilding_with_an_equal_spec_keeps_the_segment`. |
| X5 | M | **Цвет implicit-анимации уходит из Oklab.** `TwoWayConverter for Color` — прямая alpha в sRGB-u8 (`spring.rs:74-82`). retarget R20 ведёт `AnimatedContainer` color покомпонентно через `AnimatedValue` → красный → прозрачный снова темнеет к середине (D-20), вопреки решению владельца «Oklab premultiplied по умолчанию» (interpolation). | retarget R20, interpolation | `TwoWayConverter for Color` переходит к interpolation: вектор premultiplied Oklab `(L·α, a·α, b·α, α)` с точными концами; либо color остаётся на `Tween` (только C⁰). Строка `animated_container_color_fades_premultiplied` (rgba(255,0,0,128) в середине). |
| X6 | M | **Пять тем правят якорь Vsync.** F2 (курсор), motion-clock (`AnimationTime`, последний тик, пауза скорости), reduce-motion (таймлайн по поведению, `parked`), ownership (`resume_elapsed` в `f64`), retarget (`Continue`, номер кадра) — каждая заводит своё поле в одной записи регистрации. Пример: `Continue` берёт «`now` последнего тика» Preserve-таймлайна у Normal-запуска при системном масштабе ≠ 1 → скачок на шве. | motion-clock, ownership, retarget, reduce-motion | Одна форма `RunAnchor { Fresh, Continue(AnimationTime), Resume(Duration) }`, таймлайн выбирается поведением, задаёт motion-clock T3; остальные — производители варианта. Внесено в motion-clock «Миграция», ownership inv. 4, retarget механика 3. |
| X7 | M | **`Deref` у владеющего handle.** `DrivenController: Deref<Target = AnimationController>` — не умный указатель (C-DEREF); `driven.clone()` через auto-deref даёт голый контроллер, чей `dispose()` обходит снятие регистрации — инвариант 1 ownership («в реестре ⇔ владеет живой handle») держится конвенцией. | ownership | Без `Deref`: `controller()` + проброс используемых методов, либо `AnimationController::dispose` → `pub(crate)` и `build_manual` возвращает отдельный владеющий тип. **Решение владельца** (форма). |
| X8 | M | **Коллизия терминальных методов builder.** controller-robustness: `build() -> AnimationController`, `build_on(&Vsync) -> (AnimationController, VsyncRegistration)`; ownership: `build(Option<&Vsync>) -> Result<DrivenController, AnimationError>` + `build_manual()`. Две миграции ~20 production-сайтов (147 вхождений); у `Result` ownership нет причины отказа (`ValueRange` уже проверен, `Exhausted` по R10 — unbound). | controller-robustness §2, ownership | B выпускает только `build()` (ручной), production мигрирует на `build()` + текущий `register`, без кортежа; ownership добавляет `drive(self, Option<&Vsync>) -> DrivenController` (infallible). **Решение владельца** (имена). |
| X9 | M | **Индикаторы и каретка под Reduce.** `ActivityIndicator`, `LinearProgressIndicator`, `CupertinoActivityIndicator` (composition), мигание каретки (curves T7) — бесконечные repeat с `Normal` → паркуются статичными; статичный спиннер читается как зависание. Платформенная норма [U]: системные индикаторы iOS/Android продолжают вращаться при reduce motion. | composition R23, curves, reduce-motion R3 | Индикаторы — `Preserve` (существенная информация «идёт работа»), каретка — `Normal` (статичная видимая). **Решение владельца**; composition R23 помечен. |
| X10 | M | **ADR дублируются.** ADR-0136 (send-flip) обещает разделы «owner-local controllers» и «listener panics»; frame-path-state пишет отдельный ADR о том же. ADR-0064 §4 правят трое (F «amends», LD «supersedes», B — §1, §6), ADR-0125 — трое (F, B, ownership). | frame-path-state, listener-delivery, controller-robustness, ownership | F — раздел ADR-0136 (без номера) или свой номер со ссылкой; в ADR-0064/0125 — одна строка `Superseded-by` на раздел (п. 5 ниже). |
| X11 | M | **Хвост `Send` в путь кадра integration.** `RenderAnimatedTransform` — `Arc<MotionSample>` на атомиках с обоснованием из `animated_opacity.rs`, которое frame-path-state признала ложным; F4 переводит opacity на `Rc<Cell>`. | integration | Атомики только до send-flip T4/T6d, затем `Rc<Cell>` (внесено). |
| X12 | M | **Hermite-сегмент рвёт C¹ у вертикальной касательной.** Кривая с `x1 = 0, y1 > 0` даёт `c'(0) = ∞`; контракт `slope` «неконечное → 0» → `r = v₀`, но `(b−a)·c(τ)` имеет бесконечную производную в 0. Домен property R1/R3 (`(0.05,0.7,0.1,1)`) случай обходит. | retarget, curves | Ограничить C¹-гарантию кривыми с конечной `c'(0)`; для остальных — сегмент `Spring` или стартовая кривая `Linear`; строка в R2. |
| X13 | M | **Две производные кривой.** composition считает касательную на стыке разностью `1e-4` при `build`; retarget вводит `Curve::slope` (default — та же разность, `Cubic` — аналитика) в `curve.rs`. Две реализации одной величины. | composition, retarget, curves | `Curve::slope` вводит curves (владелец `curve.rs`) первым; composition и retarget потребляют. |
| X14 | L-M | **Стек пути в обходе Vsync — мёртвая сложность.** B: `SmallVec<[*const (); 8]>` против цикла «из гонки двух потоков»; production-потоков нет, после F3 гонка непредставима типом, а инвариант «один родитель» + подъём `WouldCycle` при одном потоке цикл исключают. | controller-robustness R7.2 | Убрать стек и `walk_is_cycle_immune`; `debug_assert!` на подъёме. |
| X15 | L-M | **Неподключённый `pub`.** `StatusChannel::emit/subscribe` — production-владельцы (контроллер, proxy, switch) идут через `pub(crate)` `enqueue/drain`; `StatusSubscription::inert()` зовёт только крейт. | listener-delivery | Владельцы используют `emit`, либо `StatusChannel` — `pub(crate)` до стороннего потребителя (AGENTS «Unwired surface»). |
| X16 | L | **Разная семантика dispose у каналов.** Foundation дочитывает снимок value-слушателей после `dispose` в том же раунде (`notifier_generic.rs:316-321`), status — нет (LD R5). | LD, B R6.3, F R5.1 | Строки R5.1/R6.3 пинят value-канал явно; расхождение — в `## Mapping decisions`. |
| X17 | L | Пресеты `SMOOTH/SNAPPY/BOUNCY` без production-пути; `rest_time` был `pub` ради вынесенного spring-сегмента (исправлено на `pub(crate)`). | physics | Решение владельца по пресетам. |

### Adversarial-матрица по темам

`ок` — сценарий закрыт требованием с тестом; `→X` — дыра выше; `—` — неприменимо (значение без состояния).

| Тема | reentry | add/remove в раздаче | последний владелец | 2 контроллера / Vsync | realm стоп | dispose в тике | retarget посл. кадр | dt=0 / огромный / назад | overflow, NaN | panic | Rc-циклы после flip |
|---|---|---|---|---|---|---|---|---|---|---|---|
| frame-path-state | ок R5.2 | ок R5.4 | ок R5.5 | ок R5.6 | ок R5.7 | ок R5.1 | ок R3.2 | ок R5.8 | ок R5.9 | →X1 | ок I5 (остаток: слушатель с клоном `Vsync` — разрывает owner) |
| listener-delivery | ок R1 | ок R3, R5 | ок R8 | ок R4 | ок R17 | ок R5 | ок R1 | ок R18 (NaN-строка → motion-clock) | ок R6 | →X1, →X3 | ок (форвардеры на `Weak`, R14) |
| controller-robustness | ок R1.2 | → LD | ок R6.5 | ок R7.4 | ок R9.5 | ок R6.4 | ок R9.3 | ок R9.4 | ок R4, R5 | ок R9.2 | ок (Ticker удалён) |
| motion-clock | ок R19 | — | ок R19 | ок R21 | ок R22 | ок R19 | ок R19 | ок R5, R7, R24 | ок R7, R8 | ок R20 | — |
| reduce-motion | ок R17 | → LD | ок R17 | ок R19 | ок R20 | ок R17 | ок R17 | ок R21 | ок R8, R4 | ок R18 (→X1) | — ; →X2, →X9 |
| physics | — | — | — | — | — | — | ок (R6) | ок R6 | ок R5, R13 | — | — |
| curves | — | — | — | — | — | ок C13 | — | — | ок C6, C8 | — | — |
| interpolation | — | — | — | — | — | — | ок I11 | — | ок I6, I9 | — | — ; →X5 |
| composition | → LD | → LD | ок R18 | ок R19 | ок R20 | ок R18 | ок | ок R6, R22 | ок R7, R8 | ок R21 | — ; →X9, →X13 |
| integration | ок | → LD | ок R13 | ок R9 | ок R11 | ок R10 | ок R8 | ок R14 | ок R12 | ок R15 | ок (кэш, не узел) ; →X11 |
| ownership | ок R1 | → LD | ок R6 | ок R6 | ок (teardown) | ок | → retarget | ок (типы MC) | ок R10 | ок R7 | ок ; →X3, →X7, →X8 |
| retarget | ок R16 | → LD | ок | ок R4 | ок | ок | ок R11 | ок R13 | ок R14 | ок R15 | ок ; →X2, →X4, →X5, →X12 |

## 2. Качество Rust и тестов

- **Простота/типы:** хорошо — `ValueRange`, `RunPlan` (отказ до мутации непредставим), `FrameTick`
  чеканит только `MotionClock`, `SimulationBounds`, приватные поля кривых с serde `try_from`,
  RAII `StatusSubscription`, `Keyframes` с длительностями сегментов. Плохо — X7 (`Deref`), X14
  (стек указателей), X15 (`pub` без потребителя), конвенция X2 вместо явного `behavior`.
- **Владение:** `share.rs` (F2) — правильная точка перехода; все темы должны брать примитивы
  оттуда (правка в listener-delivery и retarget внесена).
- **Тесты, проходящие в обе стороны:**
  - controller-robustness R1.2 `run_start_reenters_vsync_without_deadlock` после T8: путь удалён
    вместе с конструкторами — откат не воспроизводим; это guard, не доказательство.
  - retarget R7 (граница `|r|·D·4/27`) требует в тесте `r = v₀ − Δ·c'(0)/D` — перепись формулы
    production; проверять аналитические свойства: `x(D) = b` точно, `x'(0) = v₀` и `x'(D)`
    конечными разностями.
  - motion-clock R2/R11 и reduce-motion R7 (PB): эталон — целые нс, production — `f64` на кадр
    через `try_from_secs_f64`; нужен явный допуск (≤ 1 нс на кадр), иначе тест или нестабилен,
    или принуждает production повторять эталон.
  - physics R7: «эталонное решение» для `t_last` должно быть независимой формой в тесте
    (трёхрежимная учебная), не `SpringSimulation::x`.
  - frame-path-state R6 «каждая строка быстрее» на общем 32-ядерном хосте шумна — фиксировать
    доверительные интервалы criterion, прогон на простаивающем хосте.
  - frame-path-state R2.4, R7.1 — заявленные guard'ы с мутацией: корректно оформлено.

## 3. Сквозная согласованность (B)

| # | Конфликт | Итог |
|---|----------|------|
| 1 | motion-clock специфицирует reduce motion | В `260a0de63` уже снято: motion-clock R1–R24 без политики, есть «Не здесь» и «Шов для reduce-motion». Устарели ссылки в reduce-motion («его R11–R19, R23–R25 переходят», «решено в motion-clock (d)», правило «не завершилась — как Full») — **исправлено**. Правило никогда-не-завершающейся симуляции — сетка reduce-motion R4, ownership R13 на неё ссылается. |
| 2 | Псевдонимы `Shared`/`Cell` и удаление `CompoundAnimation` в listener-delivery | **Исправлено**: канал берёт `share::{Shared, StateCell}`; `CompoundAnimation` удаляет controller-robustness **T10** (не T8, как писала frame-path-state, — тоже исправлено). |
| 3 | physics: `rest_time` pub для вынесенного spring-сегмента | **Исправлено**: `pub(crate)`; pub — только вместе со spring-сегментом по решению владельца. |
| 4 | physics удаляет `AnimatedValue` | **Исправлено**: тип остаётся, переписывает retarget T4; physics T8 — только `smoothing` (решение владельца не снято). |
| 5 | Номера ADR | Раздел 5. |
| 6 | Владение файлами | Раздел 6. |
| 7 | `tick_at` `f64` vs `Duration` | motion-clock: `tick_at(Duration)`, `tick_all(&FrameTick)`. Темы до неё пишут на `f64`; строки о неконечном времени на входе становятся непредставимыми и удаляются в motion-clock T1 (внесено). physics R8 `Vsync::tick_at` → `AnimationController::tick_at`; ownership/retarget переведены на `Duration`/`AnimationTime` (**исправлено**). |
| 8 | Имя handle | `DrivenController` — в ownership, retarget, reduce-motion (как «settle ownership»); composition/curves/B говорят «владеющий handle» без другого имени — расхождений нет. Открыто: `ClockedController` (ownership «Открытые вопросы») и терминальный метод (X8). |
| 9 | Порядок F2 → A/B | listener-delivery говорила «frame-path-state в любом порядке»; спека F (ей оркестрация отдала порядок) — A и B после F2. **Исправлено** в listener-delivery tasks/requirements и controller-robustness tasks. |
| 10 | controller-robustness R6.2 «вернуть id» для status-слушателя vs `StatusSubscription` | **Исправлено**: статусная часть — listener-delivery R5 (инертная подписка, строка `subscribe_after_dispose_is_inert`). |
| 11 | ownership R12 settle repeat («начальное значение ноги» + `Ok`) vs reduce-motion R3 (park, future pending) при «одном settle-пути» | **Исправлено**: ownership R12 ссылается на reduce-motion R2–R4. |
| 12 | `Simulation: Send + Sync` в API physics vs снятие supertrait (send-flip T5 / F) | **Исправлено**: physics не меняет supertrait; `Arc<dyn Simulation>` → `Rc` после F3. |
| 13 | retarget `NonFiniteTarget` vs структурный `AnimationError` B | **Исправлено**: `NonFiniteInput { Target \| Velocity }`, `SpanOverflow`. |
| 14 | composition R23 «reduce motion из motion-clock» | **Исправлено** → reduce-motion (+ X9). |
| 15 | reduce-motion R6 «конструкторы `unbounded*`» при удалённых конструкторах | **Исправлено** формулировкой через builder; суть оспорена (X2). |
| 16 | Имена `TickerFuture`/`TickerCanceled` vs `RunFuture`/`RunCanceled` | Не правилось: переименование ждёт ответа владельца (B §10); ownership/retarget/LD пишут старые имена — заменить механически после ответа. |

### Внесённые правки (все — `docs/plans/specs/flui-animation/`)

- `frame-path-state/design.md`: T8 → T10; согласование `share.rs` отмечено исполненным.
- `frame-path-state/requirements.md` R5.3: политика паники — ссылка на LD и X1.
- `listener-delivery/requirements.md`: шапка (T10, F2); R5 — подписка после `dispose`, строка теста.
- `listener-delivery/design.md`: «(b)» → T10; блок хранения на `share::{Shared, StateCell}`.
- `listener-delivery/tasks.md`: зависимости и граф с F2; T10.
- `controller-robustness/requirements.md` R6.2: статусная часть → LD R5.
- `controller-robustness/tasks.md`: база — после F2.
- `physics/requirements.md`: R8 (`tick_at`), R16 (AnimatedValue за retarget), R17 (`rest_time` `pub(crate)`).
- `physics/design.md`: трейт `Simulation` без supertrait, `rest_time` `pub(crate)`, удаления, инварианты, keyframes, решение владельца 2.
- `physics/tasks.md`: граф, T8, тесты T2/T8, DoD о `pub`.
- `composition/requirements.md` R23, `composition/design.md` риск (4): reduce-motion.
- `reduce-motion/requirements.md`: связанные спеки, R6; `reduce-motion/design.md`: «Граница», «(b)», миграция.
- `ownership/requirements.md`: R12, строка «NaN `now`»; `ownership/design.md`: инв. 4, «Переполнение и NaN»; `ownership/tasks.md`: `resume_elapsed`.
- `retarget/requirements.md` R14; `retarget/design.md`: ошибки API, `InvalidExtent`, механика 3 и 5, инв. 5, переполнение.
- `motion-clock/design.md` «Миграция»: судьба `f64`-строк, единый `RunAnchor`.
- `integration/design.md` инв. 5: атомики до send-flip, затем `Rc<Cell>`.

## 4. Связи с другими сессиями

- **send-flip** (`send-flip/core`, T5 ≈ 10-14→10-19, ядро в `main` ≈ 11-09): F3 = T6c; T4 (в `main`)
  трогает `flui-animation` tween/proxy и `flui-widgets/src/animated/*` — до A и ownership, иначе
  ребейз; T6a трогает `flui-scheduler` (ticker) и `flui-runtime` `frame*.rs` — удаление `Ticker`
  (B T9 через motion-clock T6) лучше слить в `main` до T6a, чтобы ядро ребейзилось на удаление;
  T6b меняет политику паники foundation (X1).
- **text-ime:** curves T7 (`editable_text.rs`) — после text-ime T5 (≤ 11-04); motion-clock T4 и
  text-ime T3 правят `flui-runtime` `frame.rs`.
- **flui-interaction I3:** integration T8 и физика лимита fling — только потребитель.

## 5. ADR: номера

В `main` последний — ADR-0142; реестр `release/tasks.md` (владелец — оркестратор release)
закрепил 0128–0141 за другими фичами, 0135/0142 заняты. Ветки `origin/*` сверх реестра номеров не
заводят (проверено `git ls-remote` + `git ls-tree docs/adr` по всем головам, `gh pr list`).
Предложение (строки в реестр release добавляет его владелец; файлы не переименовывались):

| Номер | Тема | Название | Правит |
|---|---|---|---|
| — / ADR-0150 | frame-path-state | Состояние анимации принадлежит потоку realm | предпочтительно раздел «owner-local controllers» ADR-0136; иначе 0150. ADR-0064 §4 (форма замка), ADR-0125 (токен — `rc::Weak`) |
| ADR-0143 | listener-delivery | Animation status delivery: queued channels, contained observers, scoped subscriptions | supersedes ADR-0064 §4 (порядок при реентри) |
| ADR-0144 | controller-robustness | `AnimationController` driven only by `Vsync`; scheduler `Ticker` removed | supersedes ADR-0064 §1, §6; ADR-0125 — форма отказа `attach_child` |
| ADR-0145 | motion-clock | Время анимации принадлежит презентации | amends ADR-0097 (выход `TIME_DILATION`), ADR-0027 §8 |
| ADR-0146 | reduce-motion | Reduced motion: сигнал ОС, предпочтение, поведение | — |
| ADR-0147 | ownership | Контроллер на часах принадлежит `DrivenController` | supersedes фразу ADR-0125 о ручном снятии |
| ADR-0148 | retarget | Retarget сохраняет значение и скорость | — |
| ADR-0149 | interpolation | Interpolation contracts | supersedes ADR-0098 §7 (`Color::lerp`) |

Номер выдаётся по теме, не по порядку слияния. В ADR-0064 и ADR-0125 — по одной строке
`Superseded-by` на раздел (X10). Риск коллизии: спека `naming` другой сессии держит «ADR-NNNN» без
номера — оркестратору release зарезервировать 0143–0150 за flui-animation до первого PR.

## 6. Владение общими файлами и последовательность

Порядок в ячейке = порядок слияния правок файла. Кто сливается позже — ребейзит.

| Файл | Темы (порядок) | Правило |
|---|---|---|
| `flui-animation/src/controller/{mod,run,tick,status,dispose}.rs` | Q0 (split) → F2 → LD (`finish`, `take_status_change`, `channel.dispose`) → B (plan/commit, clamp, builder, `set_value`) → physics T4 (одна строка fling) → MC T3 (`tick_at(Duration)`, скорость) → RM T2 (`settle_run`) → OW T2 (`driven.rs`, `ClockBinding`) → RT T2/T3 (`velocity`, `retarget`) ; F3 — в ядре | `tick.rs` — владелец MC; `run.rs` — B; `status.rs` — LD |
| `flui-animation/src/vsync.rs` | F2 (курсор) → LD T3 (сдерживание) → B T5 (attach/дубли) → MC T3 (`FrameTick`, `RunAnchor`) → RM T2 (`parked`, таймлайн) → OW T2/T6 (`Resume`, `pub(crate)`) → RT T3 (`Continue`) | форму `RunAnchor` задаёт MC (X6) |
| `animation.rs`, `proxy.rs`, `switch.rs`, `curved.rs`, `reverse.rs`, `tween.rs`, `constant.rs` | send-flip T4 → F2 → LD (сигнатура, канал) → B T6 (`is_animating`), B T10 (`ALWAYS_*`) | LD — владелец; B — по одному методу |
| `curve.rs` | curves T1–T5 (+ `Curve::slope`, X13) → RT T2 (использует) → curves T8 (после composition T3) | владелец curves |
| `spring.rs` | physics T1 (сигнатуры) → interpolation (`TwoWayConverter for Color`, X5) → RT T4 (переписывает `AnimatedValue`) → derive (имя трейта) | владелец RT после physics |
| `simulation.rs` | physics | — |
| `tween_types.rs` | interpolation T3, T8 → composition T4 (`TweenSequence`) | владелец interpolation |
| `builder.rs`, `error.rs` | B → RM (`behavior`) → OW (терминальный метод, X8) → RT (`InvalidExtent` через B) | владелец B |
| `status.rs`, `motion.rs` | MC T1/T2 (`motion.rs`) → RM T1/T2 (`AnimationBehavior`, политика) | — |
| `lib.rs`, `Cargo.toml`, `tests/main.rs`, `benches/*` | все — только свои строки | интегратор Z |
| `flui-scheduler/src/{config,scheduler,ticker,lib}.rs` | MC T6 (dilation, затем `Ticker` по B T9) → send-flip T6a | одна последовательность у MC |
| `flui-runtime/src/{presentation.rs,ui_realm/frame*.rs}` | MC T1/T4 → RM T3 ; send-flip T6a, text-ime T3 | — |
| `flui-widgets/src/scroll/scrollable.rs` | LD T7 → B T8 → physics T1/T6 → OW T4 → integration T8 (после I3) | — |
| `flui-widgets/src/scroll/refresh_indicator.rs` | B T8 → physics T1/T6 → OW T4 → integration T6 → composition T5 → integration T8 | integration T6 и composition T5 — одна ветка или строго подряд |
| `flui-widgets/src/scroll/{scroll_physics,page_view}.rs` | physics | — |
| `flui-widgets/src/scroll/scroll_controller.rs` | LD T7 → B T8 → RT T5 | — |
| `flui-widgets/src/interaction/dismissible.rs` | LD T7 → B T8 → OW T4 → RT T6 → integration T5 | — |
| `flui-widgets/src/navigator/{transition_route,hero_flight,back_gesture}.rs` | LD T7 → B T7/T8/T10 → interpolation T8 (hero) → OW T4 → RT T7 ; карточка shuttles — после F3 | — |
| `flui-widgets/src/animated/{implicitly_animated,animated_*}.rs` | send-flip T4 → LD T7 → B T8 → interpolation T5/T6/T8 → OW T3 → RT T5 | `animated_container.rs`: interpolation T5+T8 одной веткой |
| `flui-widgets/src/animated/{ticker_mode,vsync_scope}.rs` | B T5 → OW T3 | — |
| `flui-widgets/src/text/editable_text.rs` | text-ime T5 → curves T7 | чужой владелец |
| `packages/flui-material/src/scaffold_messenger.rs` | LD T7 → B T8 → MC T8 → RM T9 → OW T5 | 5 тем на файл: правки строго подряд |
| `packages/flui-material/src/{drawer,ink_well}.rs` | LD T7 → B T8 → RM T9 (ink_well) → OW T5 → RT T6 (drawer) → integration T4 (drawer) | — |
| `packages/flui-cupertino/src/{button,route}.rs` | B T7/T8 → OW T5 ; RM T9 (route), integration T3 (тесты route) | — |
| `flui-objects/src/proxy/animated_opacity.rs`, `sliver/sliver_animated_opacity.rs` | F4 (в ядре, с send-flip T6d) | — |
| `flui-objects/src/sliver/sliver_persistent_header.rs` | OW T5 ; send-flip T6d | — |
| `crates/flui-sdk/tests/surface.rs` | все, по строке | интегратор Z |

## 7. Глобальный порядок PR

Одна тема — один PR. «‖» — параллельная разработка в своих worktree; слияние — по рёбрам.

```text
Q0 quality-base ──┬─► F1 ─► F2 ─┬─► A listener-delivery ─┐
                  │             └─► B controller-robust. ─┼─► C motion-clock ─► RM reduce-motion ─► OW ownership ─┬─► RT retarget ─┐
                  ├─► D physics ───────────────────(слить после B)──────────────────────────────────────────────┘                ├─► IN integration ─► CO consumers ─► Z
                  ├─► E curves T1–T5 ─(слить после B; Curve::slope до RT)                                    OW ─► IN ──────────┘
                  ├─► IP interpolation ‖ G derive ‖ CO core (keyframes T1–T3) ─► E T8 (Catmull-Rom)
                  └─► RM-platform (backends T4–T8: разработка сразу, в PR RM)
send-flip T4,T5,T6b (core) + B ─► F3 (= T6c) ─► F4 ‖ F5   (в ядре send-flip, ~11-09 в main)
text-ime T5 ─► E T7 (каретка; ещё после OW)        I3 (interaction) ─► IN T8
```

- **Сразу (волна 1, от Q0):** Q0 → F1 → F2 (последовательно, блокирует A/B); параллельно —
  D physics, E curves T1–T5, IP interpolation, G derive, CO core T1–T3, разработка бэкендов
  reduce-motion T4–T8 (не зависят от анимации). Слияние D и E — после B (`controller/run.rs`,
  политика NaN кривых).
- **Волна 2:** A и B параллельно после F2; слияние A → B (B R6.2 опирается на канал A; сигнатура
  трейта из A). Затем C (после B; удаляет dilation и `Ticker` до send-flip T6a).
- **Волна 3:** RM (после A, B, C) → OW (settle-путь RM) → RT (OW T2, D, E `slope`) ‖ IN (A, OW;
  dismissible/drawer — после RT T6).
- **Волна 4:** CO consumers T5–T7 (после OW, IN T6), E T7 (после text-ime T5, OW), E T8 (после CO T3),
  Z (lib.rs, docs, бенчи «после»).
- **Ядро send-flip:** F3 после T4, T5, T6b и желательно B; F4 ‖ F5 после F3.
- **Хост Windows:** тяжёлые прогоны идут через общий замок xtask — одновременно разумно 3–4
  worktree разработки (`CARGO_BUILD_JOBS=6`, `NEXTEST_TEST_THREADS=4`); бенчи «до/после»
  (F1/F5, Q0/Z, curves T3, interpolation T3) — только на простаивающем хосте, по одному.

## 8. Решения владельца

1. X1 — единая политика паники колбэков (рекомендация: модель send-flip, резюмировать первую после раунда).
2. X2 — `behavior` явно, без «unbounded ⇒ Preserve».
3. X3 — livelock: отложенный settle внутри `drain` и предел очереди.
4. X7/X8 — форма владеющего handle (без `Deref`) и имена терминальных методов (`build`/`drive`), имя `DrivenController` vs `ClockedController`.
5. X9 — индикаторы `Preserve`.
6. X10/§5 — ADR frame-path-state как раздел ADR-0136; резерв 0143–0150 в реестре release.
7. Уже открытые в спеках: `RunFuture` rename (B), `smoothing` и пресеты пружин (physics), spring-сегмент и оконный stagger (composition), R15 `activate` в flui-view (ownership), default implicit и back gesture (retarget), F «A и B ждут F2».
