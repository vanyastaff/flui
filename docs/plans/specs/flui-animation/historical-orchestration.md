> Историческая спецификация от 2026-10-06, восстановлена из `801543a3f`.
> Текущий аудит и порядок работ: [readiness-plan.md](readiness-plan.md).
> Позднейшее решение `SystemPreferences` в [orchestration.md](orchestration.md) имеет приоритет.

# flui-animation — оркестрация

- **Статус:** в работе
- **Дата:** 2026-10-06
- **Цель:** `flui-animation` на уровне рыночной нормы современного UI-фреймворка, без допущений
  «потребителей пока нет». Pre-1.0 ломаем API только с миграцией всех вызовов в workspace,
  фрагментом `changelog.d/` и ADR, если меняется межкрейтовый контракт.
- **Спеки:** `docs/plans/specs/flui-animation/<topic>/{requirements,design,tasks}.md`. ID требований
  и задач живут только здесь; в код, тесты и коммиты не попадают (`cargo xtask markers`).

## Критерий готовности

1. Матрица рыночного эталона ([market.md](market.md)): каждая строка «есть» или вынесена в
   scope-решение, утверждённое владельцем (раздел «Scope-решения» ниже).
2. Каждый дефект и разрыв — draft-PR: тест, падающий без фикса (вывод красного прогона в PR),
   зелёный `cargo xtask check-changed`, SHA.
3. Property-тесты кривых (точные концы, монотонность), пружины (сходимость, конечность, независимость
   от разбиения времени на кадры) и retarget (непрерывность позиции и скорости) проходят на
   виртуальном clock.
4. Бенчи `benches/` до и после на одном хосте; аллокации в тике измерены.
5. Все потребители в workspace мигрированы.
6. `docs/{ARCHITECTURE,GUIDE,PATTERNS,PERFORMANCE}.md` и `README.md` крейта соответствуют коду.

Запрещено без владельца: merge, force-push, правка `.github/workflows/`, ослабление gates, snapshot
и допусков.

## Этапы

| Этап | Что | Выход |
|------|-----|-------|
| 1 | Факты по 7 зонам (read-only): контроллер/vsync/status; кривые/tween; физика; композиция и поверхность; derive `Animatable`; тесты/бенчи/примеры; потребители | ledger на зону, вне репозитория |
| 2 | Рыночный эталон по первичным источникам: SwiftUI/Compose/Core Animation; CSS Easing/WAAPI/CSS Color/Framer Motion/Flutter (чек-лист); Rust-крейты и физика | [market.md](market.md) |
| 3 | Спека на значимый разрыв; мелкий фикс — карточка в [tasks.md](tasks.md). Каждый design — adversarial review | `<topic>/` |
| 4 | Сначала задача-контракт (публичные типы + падающие тесты), затем [P]-задачи параллельно, ветка и worktree на задачу | draft-PR |

Приоритет: корректность и дефекты → недостающее поведение → производительность → эргономика API.

## Adversarial review каждого design

Reentry слушателя в контроллер и vsync-реестр; снятие и добавление слушателей во время
уведомления; освобождение последнего владельца из колбэка; два контроллера на одном тикере;
остановка realm посреди анимации; panic в слушателе (первый отказ остаётся первым, следующий кадр
тикает); dispose во время тика; retarget в последнем кадре; dt = 0, огромный dt, время назад;
переполнение и NaN в физике и интерполяции.

## Роли

| Роль | Skill | Отвечает за |
|------|-------|-------------|
| Оркестратор | `10x-Team:engineering-manager` | волны, журнал, конвейер ревью |
| Архитектор | `10x-Team:principal-architect`, `engineering:architecture` | design.md, ADR |
| Ревьюер | `10x-Team:staff-engineer`, `10x-Team:senior-engineer`, `code-review` | adversarial review, ревью веток |
| QA | `10x-Team:qa-engineer`, `engineering:testing-strategy` | failure-матрицы, property-тесты, откат фикса |
| Реализация | `10x-Team:sde` | [P]-задачи |
| Производительность | `10x-Team:sre` | бенчи до/после, аллокации |

## Владельцы общих файлов

Один владелец на волну, остальные присылают изменения через него: `crates/flui-animation/Cargo.toml`,
`src/lib.rs` (реэкспорты), регистрация тестов в `tests/`, `docs/ARCHITECTURE.md` крейта,
`crates/flui-macros` (derive), миграция вызовов в `flui-widgets`.

## Правила реализации

- Rust API Guidelines; `#[non_exhaustive]` на публичных enum; `Duration`/типизированное время вместо
  голых `f64` секунд; enum вместо `bool`-параметров; builders и типизированное состояние.
- Путь кадра синхронный. Lock на per-animation состоянии в тике обоснован или убран; guard и borrow
  отпущены до вызова слушателя; guard не в публичных сигнатурах.
- Panic в слушателе не ломает контроллер и vsync-реестр; следующий кадр тикает.
- Никаких новых `static` (ADR-0097). Промежуточная арифметика не публикует NaN/inf; диапазон
  параметров и поведение на вырожденных значениях документированы.
- Каждый новый `pub` используется production-путём в workspace либо удаляется. Rustdoc: контракт,
  единицы времени, ошибки, panics, пример.
- Тесты через публичный API, семейства — таблицы существующего раннера; время — виртуальный clock;
  эталоны физики и кривых — из независимого источника (аналитика, опубликованная таблица).
- Один тяжёлый прогон на хост (`CARGO_BUILD_JOBS=6`, `NEXTEST_TEST_THREADS=4`).

## Scope-решения

Строки матрицы, сознательно не реализуемые в этом проходе; каждая — с решением владельца и датой.

Правило владельца: рыночная норма делается вместе с production-потребителем в `flui-widgets`, а не
выносится из-за его отсутствия.

**Выносится** (утверждено владельцем 2026-10-06):

| Строка матрицы | Обоснование |
|----------------|-------------|
| M-INTG-5 FLIP layout; shared-element сверх Hero | зависит от router и слоя layout-проекции, которых нет; Hero покрывает переходы маршрутов |
| M-CMP-6 полная модель тайминга WAAPI (fill, direction, iterationStart, before-flag) | у UI-фреймворка нет документной временной шкалы; повтор, реверс и задержка покрываются композицией |
| M-INTG-7 платформенный spline-decay и скорости замедления iOS | скролл покрыт friction + пружиной у края (physics) |
| M-INT-13 additive/merge-анимации | прерывание покрывается retarget с сохранением позиции и скорости; условие — тест непрерывности C⁰/C¹ при повторных прерываниях в retarget |
| M-CMP-7 арифметическая композиция двух анимаций | `CompoundAnimation` удаляется; рыночной нормы нет (WAAPI composite — наложение эффектов, а не арифметика анимаций) |
| M-TIME-11 предпочтительная частота кадров | частотой кадров владеют презентация и платформа, не анимация |
| M-CRV-8 экстраполяция cubic-bezier вне [0, 1] | вход кривых клампится; экстраполяция не нужна ни одному потребителю |
| M-INTP-4 OkLCh и методы hue | цвет интерполируется в Oklab по умолчанию; полярные пространства не нужны ни одному потребителю |
| M-INTP-10 дискретные значения по opt-in | дискретные свойства виджетов не анимируются |
| M-A11Y-6 флаг «предпочитать cross-fade» | сценарий покрывает reduce motion; отдельного системного флага нет на Windows и Linux |
| M-INTG-10 настраиваемый hit-testing во время анимации | hit-testing следует за анимированным transform (integration); настройка не нужна |
| M-CMP-11 переходы по состоянию, phase animator | выносится, если keyframes и sequence закрывают сценарий; иначе — обоснование в спеке composition |

**Делается в этом проходе** (с потребителем и тестами): keyframes/sequence/stagger, включая сегмент
keyframes с решением по x (M-CRV-9; после него удаляется `CatmullRom*`); playback rate на анимацию
(M-TIME-12); Oklab по
умолчанию для `ColorTween` (+ `changelog.d`); угол по кратчайшей дуге и `steps()`; seek и
playback rate в объёме time dilation для devtools и тестов.

**Отдельная спека + ADR, не выносится:** `Animation: Send + Sync` → состояние анимации у realm.
Lock на per-animation состоянии в пути кадра нарушает AGENTS.md и чинится в этом проходе
(тема `frame-path-state`).

**Удаляется** (с миграцией и `changelog.d` на каждое): `CatmullRom*`, `Curve2D`, `ParametricCurve`,
`CompoundAnimation`, `ReverseCurve` (заменяется комбинатором), `prelude`, pass-through реэкспорты
`flui-scheduler`, статики `ALWAYS_*` (их используют `flui-widgets/src/navigator/transition_route.rs:59`,
`:78` — только с миграцией), `Ticker` в `flui-scheduler` после перехода контроллера на Vsync.
`AnimationControllerBuilder` не удаляется: становится основным способом создания контроллера, если
он идиоматичнее текущих конструкторов; иначе — обоснование в спеке controller-robustness.

Строк `scope?` в [market.md](market.md) не осталось (обновлено 2026-10-06).

**Отдельная спека + ADR:** reduce motion (M-A11Y-1..5) — трейт в `flui-platform-api`, бэкенды в
`flui-platform`, доступ через `LifecycleContext` (тема `reduce-motion`).

**Чужой владелец:** M-INTG-6 (единый лимит скорости fling) — задача I3 сессии `flui-interaction`;
здесь только потребитель.

## Межкрейтовые правила

- `flui-sdk` реэкспортирует весь крейт (`crates/flui-sdk/src/lib.rs:27`): каждое удаление и
  переименование сверяется с `crates/flui-sdk/tests/surface.rs` (ADR-0088 §4), `packages/`
  мигрируются в том же PR.
- Один владелец на каждый внешний крейт: `flui-scheduler` (статик `TIME_DILATION`, `Ticker` — запись
  `globals` и ADR) — тема motion-clock; `flui-foundation` (`Matrix4::lerp`) и `flui-painting`
  (`lerp_oklab`) — тема interpolation.
- Факт с пометкой `[U]` не закрепляется тестом как контракт, пока не сверен с первичным источником.

## PR и порядок

PR — на тему спеки, не на дефект.

**Граница с send-flip** (решение владельца 2026-10-06). Задача T6c спеки send-flip владеет
`crates/flui-animation/**` в ветке `send-flip/core` (`AnimationController`/`Vsync` на `Rc<RefCell>`;
слияние ядра ~11-09); T4 трогает `tween.rs`, `proxy.rs` и `crates/flui-widgets/src/animated/*` в `main`.
Поэтому:

1. **В `main` сейчас** (чистые функции, не трогают `controller.rs`, `vsync.rs`, `proxy.rs`,
   `switch.rs`, `compound.rs`, `reverse.rs`): physics, curves, interpolation (`lerp_oklab`,
   `Matrix4::lerp`), математика времени из motion-clock (перебазирование dilation, отказ от NaN и
   обратного времени), quality (бенчи, docs, маркеры, тест-бинарь). Перед стартом каждого PR —
   проверка пересечения файлов с T4 и T6c.
2. **listener-delivery, controller-robustness, ownership, frame-path-state** — только контрактные
   тесты через публичный API. Зелёные на `main` — обычные строки; красные — строки с
   `#[ignore = "contract: <поведение>"]` по правилам send-flip (T5 вносит, T6x снимает). Реализация —
   в T6c на ядре.
3. Временных atomics в `main` нет.
4. Правки `controller.rs`, `vsync.rs`, `proxy.rs`, `switch.rs`, `compound.rs`, `reverse.rs` из этого
   прохода — отдельная ветка, вход для владельца T6c; в `main` не идут.
5. Владелец send-flip получает ссылки на спеки listener-delivery, controller-robustness, ownership,
   frame-path-state и список требований, которые ложатся на T6c. Строки, которые T6c не покрывает
   (M-OWN-7 типизированные ID, M-OWN-9 снятый слушатель, M-CMP-8 `AnimationSwitch`, M-CMP-10
   атомарный `set_parent`), распределяются по договорённости с ним: кто и на какой базе.

**Договорённость с владельцем send-flip (2026-10-06):** M-OWN-7 (типизированные подписки статуса) и
M-OWN-9 (снятый слушатель не вызывается) — в T6c. M-CMP-8 (`AnimationSwitch`) и M-CMP-10
(атомарный `set_parent`) — этот проход: ветки от `send-flip/core` после коммита хранилища T6c
(цель 10-21, SHA придёт от владельца send-flip), PR в `send-flip/core`, слияние до T7 (10-27). На
`Rc<RefCell>` lock в `AnimationSwitch` исчезает; строка контракта — без зависания при reentry и с
объявлением hop. `set_parent` атомарен для слушателей: старый родитель отписан, новый подписан,
одно уведомление; строка — слушатель, повторно входящий в `set_parent`. До появления ядра строки
четырёх требований лежат в `main` с `#[ignore = "contract: …"]`. График ядра: T5 10-14→10-19, T6c
10-19→10-27, T7 10-27→10-30, слияние в `main` ~11-09.

Остальные темы (composition, retarget, integration, reduce-motion, ownership-миграция потребителей)
идут после ядра или на нём.

## Решения по развилкам спек

Принято оркестратором по рекомендациям авторов 2026-10-06; владелец проверяет список.

| Тема | Решение |
|------|---------|
| listener-delivery | panic слушателя статуса (и значения): переход закоммичен до вызова, остальные слушатели получают его, первая паника сохраняется и пробрасывается после раунда (`resume_unwind`), следующий кадр тикает остальные контроллеры — решение владельца 2026-10-06, совпадает с send-flip T6b для нотификаторов foundation и заменяет политику ADR-0109 §3 «перехват без проброса». Без ограничения на «пинг-понг» нулевых прогонов (ограничение отбросило бы принятую работу). `remove_status_listener` удаляется, `add_status_listener` возвращает `#[must_use] StatusSubscription`; `StatusChannel` публичный для сторонних `Animation` |
| controller-robustness | `TickerFuture`/`TickerCanceled` → `RunFuture`/`RunCanceled` в `flui-animation` (Ticker удаляется). `set_value` → `Result` (`Disposed`). Повторная регистрация в `Vsync` — panic с указанием на `try_register`, как правило исчерпания. До handle из ownership `build_on` возвращает пару (контроллер, регистрация) |
| physics | `smoothing` удаляется (нет потребителя, не рыночная норма). `AnimatedValue` остаётся (потребитель — implicit spring mode из retarget). `settling_duration` не публичный. Пружинного сегмента keyframes в этом проходе нет → `SpringSimulation::rest_time` не публичный |
| motion-clock | скорость анимации ≥ 0 (0 — пауза; разворот — `reverse`). Потребитель — пауза таймера SnackBar под курсором (`flui-material`). Devtools: операция агента `motion` (скорость, шаг) в `flui-protocol`. Обратного seek нет; scrub одной анимации — `set_value` |
| composition | `Stagger` в этом проходе — тики `CupertinoActivityIndicator`; stagger списка — вместе с меню Material. Сегмент `cubic` — замена `CatmullRom*` (M-CRV-9), исключение из правила «pub с потребителем» до появления потребителя-keyframes. M-CMP-11 выносится: keyframes с repeat и retarget закрывают оба сценария |
| ownership | handle — `DrivenController`. Повторный `did_change_dependencies` при reparent по GlobalKey — отдельная задача в `flui-view` |
| retarget | implicit по умолчанию — `Curve(EaseInOut, 200 ms)` с непрерывной скоростью. Back gesture: settle по скорости пальца и оставшемуся пути вместо фиксированных 350 ms |
| curves | `SawTooth` и `Threshold` удаляются (нет потребителя). Потребитель `steps()` — `CupertinoActivityIndicator` (а не каретка: `editable_text.rs` у text-ime) |
| interpolation | при нулевом масштабе на конце `Matrix4::lerp` берёт вращение другого конца — собственное правило FLUI, закреплено тестом. Oklab по умолчанию — `changelog.d` с примером изменённого кадра |
| integration | перенос чистых сдвигов на слой (обратное решение `flui-rendering/ARCHITECTURE.md`) — только если бенч integration T7 показывает выигрыш; иначе остаётся текущий путь |
| reduce-motion | метода `LifecycleContext` нет: реактивное значение через `MediaQuery::motion_of(ctx)` (ADR-0078 — для действующих handle; отклонение от исходного указания владельца). Сигнал платформы — два метода существующего `PlatformWindow`. Переходы маршрутов под Reduce — мгновенные. Linux-чтение — за фичей `a11y` |
| frame-path-state | F1 (контрактные тесты, бенчи) — в `main`; F2 (`share.rs`) трогает `controller.rs` и обёртки, поэтому вместе с F3 (переход на `Rc`/`Cell`) — вход и работа send-flip T6c |

**По итогам adversarial review** ([review.md](review.md)), оркестратор, 2026-10-06:

| Находка | Решение |
|---------|---------|
| две политики паники | одна для всех колбэков (значение, статус, продолжение, тик в обходе): переход закоммичен, раунд дорабатывает, первая паника пробрасывается после раунда — как send-flip T6b/R11 |
| reduce motion пропускает implicit-анимации | `AnimationBehavior` задаётся явно у каждого контроллера; «unbounded ⇒ Preserve» не выводится. Implicit-анимации — `Normal`, скролл — `Preserve` |
| зависание очереди на «пинг-понге» | прогон, завершающийся внутри раздачи (нулевая длительность, нет часов), откладывается на следующий кадр; очередь кадра конечна, принятая работа не отбрасывается |
| `ArcCurve` сравнивается по указателю | кривые сравниваются по значению (`PartialEq` у встроенных кривых); пользовательская — по идентичности, документировано |
| цвет в retarget уходит из Oklab | `TwoWayConverter for Color` работает в Oklab с premultiplied alpha |
| пять якорей в регистрации `Vsync` | один `RunAnchor`, владелец — motion-clock |
| `Deref` у `DrivenController` | без `Deref` (правило владельца «без Deref ради наследования»): `controller()` возвращает `&AnimationController`; `dispose` только через handle |
| два разных `build` у builder | один: `build_on(&Vsync) -> DrivenController` (ownership) и `build()` для контроллера без часов; промежуточной пары нет — миграция один раз |
| индикаторы загрузки под Reduce | `Preserve`: индикатор — обратная связь о состоянии, не декоративное движение |
| номера ADR | 0143 listener-delivery, 0144 controller-robustness, 0145 motion-clock, 0146 reduce-motion, 0147 ownership, 0148 retarget, 0149 interpolation, 0150 frame-path-state (или раздел ADR-0136 — по решению владельца send-flip); резерв — до первого PR |

**По итогам аудита конвенций** ([conventions-audit.md](conventions-audit.md)), оркестратор, 2026-10-06:

| Развилка | Решение |
|----------|---------|
| R3: открыть или запечатать `Animation` | запечатать: внешних реализаций в workspace и `packages/` нет; канал статуса — `pub(crate)`. Распечатывание — ADR с названным потребителем |
| R2: один стёртый тип кривой для owner-local хранения | согласуется с владельцем send-flip (строки 21/39 против 264); до ответа спеки пишут `impl Curve + 'static` |
| X17: пресеты `SMOOTH`/`SNAPPY`/`BOUNCY` | удаляются: production-потребителя нет; умолчание пружины в `MotionSpec` задаёт retarget явными параметрами |
| R6: `Listenable::remove_all_listeners` и единый RAII-токен для слушателей значения | рефакторинг `flui-foundation` — передаётся send-flip T5/T6b (владелец нотификаторов) |
| `build_on(Option<&Vsync>)` | принято: виджет без `VsyncScope` получает handle без часов (ownership R12) |
| A10: `Curves` — модуль констант вместо `pub struct Curves;` | передаётся спеке naming (переименования до 0.2.0) |
| R15: повторный `did_change_dependencies` при `activate` в `flui-view` | отдельная задача в `flui-view` после ownership |


## Конвенции Rust

Toolchain 1.99, edition 2024. Всё ниже проверено компиляцией на 1.99 (scratch-крейт); API Guidelines — по ID чеклиста.
**Типы вместо проверок**
- Время — `Duration`, не `f64` секунд (сейчас нарушено: `SpringDescription::with_duration_and_bounce(duration_secs: f64, …)`,
  `with_response_and_damping(response: f64, …)`). Единицы и ID — newtype (C-NEWTYPE), ID — `NonZero<u64>`
  с выпуском через `checked_add` и вечным отказом при исчерпании (как `VsyncRegistrationError::Exhausted`).
- Ограниченные параметры (`Interval` begin/end, `Threshold`, tension, bounds) — `try_new(..) -> Result` (C-VALIDATE);
  `new` — либо тотальная, либо удобный двойник с `# Panics`, называющим `try_new`. `debug_assert!` как
  единственная валидация запрещён (сейчас: simulation.rs:186, :215).
- enum вместо bool/Option-флагов; `#[non_exhaustive]` на публичных enum, которые будут расти (статусы, ошибки);
  sealed-трейты для закрытых семейств (C-SEALED); поля структур приватны (C-STRUCT-PRIVATE).
- `#[must_use]` на билдерах, guard'ах подписок/регистраций (`#[must_use = "dropping unsubscribes"]`) и на трейтах guard'ов.
- Сообщения трейтов-ошибок: `#[diagnostic::on_unimplemented]` на `Animatable`/`Tween`-трейтах (1.78),
  `#[diagnostic::do_not_recommend]` на blanket-impl (1.85).
**Владение и заимствования**
- Состояние кадра owner-local: `Rc<RefCell<_>>`/`Cell` по send-flip; `Arc<Mutex<_>>` только там, где реально
  пересекается поток. Клонирование `Rc/Arc` — явное `Arc::clone(&x)` (lint `clone_on_ref_ptr`); не клонировать данные ради borrowck.
- Ни borrow, ни guard не живёт во время пользовательского кода. Проверенные правила 2024:
  `if let Some(v) = *cell.borrow() {..} else { cell.borrow_mut() }` — OK (временное освобождается до `else`;
  в 2021 это паника); хвостовое выражение `c.borrow().len()` освобождает guard до локальных переменных.
  НО скрутини `match x.borrow() {..}` держит guard во всех ветках — сначала `let v = *x.borrow();`, потом `match v`.
  `significant_drop_in_scrutinee` ловит только блокировки, не `RefCell`.
- Отписка — RAII-guard; удаляемые слушатели вынимаются (`extract_if`/`mem::take`) и дропаются вне borrow (ADR-0127).
- `Cell::update` возвращает `()` (не значение) — читать через `get()` после.
**Ошибки и паники** (docs/PANIC-POLICY.md)
- `thiserror`-enum, `Result` в публичном API, ошибка сопоставима по варианту (C-GOOD-ERR).
- `expect("BUG: <инвариант>")` только для внутренних инвариантов. Сейчас без `BUG:`: controller.rs:413/431/487.
  `panic!` удобного двойника называет `try_`-форму (vsync.rs:171 — не называет `try_register`).
**API** — C-COMMON-TRAITS (`Debug`, `Clone`, `PartialEq`, `Eq`/`Hash` где нет float — 9 типов без `Eq`, `Default`
где есть естественное значение), C-CONV-TRAITS (`From`/`TryFrom`, не `impl From` с паникой), C-BUILDER,
C-CALLER-CONTROL (принимать `impl Into<..>`/владение, не клонировать внутри), итераторы `impl Iterator + use<..>`
вместо `Vec` (C-ITER), апкаст `Arc<dyn Animation<T>>` → `Arc<dyn Listenable>` (1.86) вместо шимов.
`SmallVec`/`Cow` — только с замером в `benches/`.
**Арифметика**
- Счётчики/ID — `checked_*` + типизированный отказ; `strict_*` (1.91) — только где переполнение = BUG.
- Каждая float-функция документирует политику NaN/±inf и не публикует нефинитный результат.
  Сортировка — `total_cmp`; `clamp` паникует на NaN-границах — границы валидировать раньше.
- `f64 → Duration` только через `Duration::try_from_secs_f64` (NaN/отрицательное/переполнение → `Err`;
  `from_secs_f64` паникует). `Duration::mul_f64` паникует — множитель должен быть конечным и ≥0 (как в
  controller.rs:2514). `Duration::SECOND`, `checked_mul_f64`, знаковый `div_ceil` на 1.99 нестабильны/нет.
- Вместо `as` с потерей — `try_from` или явная насыщающая политика с комментарием (controller.rs:1510 `f64→u128`,
  curve.rs:833/897 `f64→usize`, spring.rs:89 `f64→u8`). `mul_add` не вводить: другое округление, медленно без FMA.
**unsafe** — в крейте нет; если появится, `// SAFETY:` называет инвариант, который код устанавливает.
**Документация** — `# Errors`, `# Panics`, `# Examples` (C-FAILURE, C-EXAMPLE); примеры компилируются.
Сейчас 74 doctest'а под `ignore` (README 30, GUIDE 31, PERFORMANCE 12, lib.rs 1) — перевести в компилируемые
или `no_run`, `ignore` только с причиной в тексте.
**Линты** — `[lints] workspace = true` запрещает добавлять записи в Cargo.toml крейта (проверено), атрибут крейта
перекрывает workspace-`allow` (проверено). Включить в `src/lib.rs` после исправления (либ-хиты, тесты 0):
`missing_panics_doc` 8, `missing_errors_doc` 1, `allow_attributes_without_reason` 8, `cast_possible_truncation` 4,
`cast_sign_loss` 4 (те же места), `derive_partial_eq_without_eq` 9, `clone_on_ref_ptr` 6, и с нулём хитов как
барьер: `return_self_not_must_use`, `lossy_float_literal`, `unwrap_in_result`, `fallible_impl_from`.
Отклонены как шум: `float_arithmetic` 166, `suboptimal_flops` 54, `use_self` 49, `missing_const_for_fn` 37,
`arithmetic_side_effects` 24, `as_conversions` 23, `indexing_slicing` 16, `significant_drop_tightening` 10 (nursery;
места в controller.rs/vsync.rs проверить руками), `float_cmp` 6 (точные сравнения намеренны).
`#[allow]` → `#[expect(lint, reason = "…")]` (1.81); `allow` только для целевозависимых случаев (lib.rs:89).
**Форма абстракций** (решение владельца 2026-10-06; применяется в каждом design.md)
- Закрытый набор вариантов, известный крейту → enum (статусы, режимы пружины, `TransformMotion`, `MotionSpec`);
  трейт — только точка расширения пользователя (`Curve`, `Simulation`, `Animatable`-тип); если внешняя реализация
  недопустима — sealed. Минимум обязательных методов, остальное — provided или extension-трейт с blanket impl.
- Трейт с одной реализацией или без пользователя как абстракции — не вводится/удаляется (`ParametricCurve`,
  `Curve2D`, `AnimationExt`, `CurveExt`); моки — не причина.
- Выход, однозначно определяемый реализацией → associated type (`Animatable::Value` вместо параметра `T`
  с `PhantomData`). Параметр трейта — только если один тип реализует его для многих `T` (`Animation<T>`: 130 dyn-
  написаний, параметр остаётся). GAT — только при реальном заимствовании; иначе `impl Iterator + '_`/RPITIT.
- Гетерогенное хранение (слушатели, родители обёрток, симуляции) → dyn один раз на границе хранения; трейт
  dyn-compatible (compile-тест). Конструкторы принимают `impl Animation<f64> + 'static` и стирают внутри, а не
  `Arc<dyn …>` от вызывающего. Стоимость dyn на пути кадра — строкой бенча, не догадкой.
- Состояния с разными допустимыми операциями → typestate, если это убирает проверку из публичного API; иначе enum.
  Const generics — для фиксированных арностей, если стабильно на 1.99 (`[f64; N]` в `TwoWayConverter` требует
  нестабильного `generic_const_exprs` — остаётся associated const/тип).
- Преобразования — `From`/`TryFrom`/`AsRef`; без `Deref` ради «наследования» (handle → `controller()`/`AsRef`);
  без supertrait-иерархий по классам Flutter; supertrait — только если каждая реализация обязана им быть.
- Ошибки трейтов — associated `Error` или тип крейта; параметр ошибки — enum (`SimulationParameter`), не `&'static str`.
**Владение, блокировки, lifetimes, generics**
- Для каждого долгоживущего объекта design называет владельца, сильные/слабые ссылки и точку освобождения при
  unmount, замене `VsyncScope` и teardown realm. Цикл `Rc` разрывается `Weak`, RAII-guard подписки или явным
  `dispose`; тест — `Drop` ровно один раз после unmount.
- Колбэк хранит минимум (`Weak` на владельца); `Drop` без пользовательского кода и без паники; guard/подписка
  объявлены полем раньше того, что защищают (порядок drop).
- Каждый `Mutex`/`RwLock` назван с защищаемым состоянием и порядком захвата; lock на per-animation состоянии в
  пути кадра — дефект (исправляется в send-flip T6c). Self-deadlock воспроизводится тестом с таймаутом.
- Lifetimes по elision; имя со смыслом, если нужно (`'frame`); guard/`RefMut` наружу не отдаётся — `with(|v| ..)`
  или копия; `'static` на колбэке — только если он хранится; `use<..>`, где RPIT захватывает лишнее.
- Generics: минимальные bounds на `impl`, не на `struct`; тяжёлое тело — во внутреннюю не-generic функцию;
  `PhantomData<fn() -> T>`, если `T` не хранится. Owner-local типы — `!Send`, trybuild-фикстура.
- Паника в колбэке: переход закоммичен, раунд дорабатывает, первая паника пробрасывается после раунда,
  следующий кадр тикает; матрица — одиночный отказ, два в конкуренции, паника в `dispose`, следующая операция.

## Отчётность

Журнал задач — git-ignored `TASKS.md` в worktree: зона, задача, ветка, SHA, статус, доказательство.
В отчётах различать: прочитано / скомпилировано / запущено / прошло / упало / недоступно.
