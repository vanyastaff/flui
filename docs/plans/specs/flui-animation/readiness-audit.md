# Animation: аудит готовности, 2026-10-08

## Вердикт и метод

**Production ready не подтверждён.** На `91bb1fe1d2a349f62915846303f6f25780ae5c6d`
75 обычных тестов крейта с `serde` проходят; все 32 игнорируемых контрактных теста падают.
137 doctests проходят. Это наблюдения на Windows x86_64, а не результат полного workspace,
GPU, Linux, wasm или native-platform прогона. Команды и имена тестов —
[readiness-evidence.md](readiness-evidence.md).

Метод: 10x-Team discovery, роли architect и QA; Matt `codebase-design` и exploration из
`improve-codebase-architecture`. Архитектурный проход выполнялся отдельно от сверки требований
и прогонов. Оценивались обязанности вызывающего кода, владение, отказ/восстановление,
production-потребители и внешние проверяемые эффекты. Формальный `code-review` всего
исторического diff не проводился: это аудит текущего состояния и восстановление требований.

Классы свидетельств: **воспроизведено** — выполненный публичный тест; **код** — конкретный
путь или отсутствие связки подтверждены чтением/поиском; **гипотеза** — нужен новый сценарий;
**не проверено** — отсутствие доказательства не превращается в дефект.

## Что действительно есть

| Область | Доказательство на закреплённой базе | Предел вывода |
|---|---|---|
| Кривые, slope, Steps, checked serde | `curve.rs`; `tests/contracts/{curve,curves}.rs`, зелёный all-features | Не доказывает непрерывность implicit-виджета |
| Физика и численные крайние случаи | `simulation.rs`; `tests/contracts/{simulation,spring}.rs` | Fling к bound ещё имеет красный контракт |
| Численная C⁰/C¹ retarget-модель | `retarget.rs`, `spring.rs`; `tests/contracts/retarget.rs` | Нет production-подключения к implicit-пути |
| Keyframes/Stagger | `keyframes.rs`, `stagger.rs`; `tests/contracts/keyframes.rs` | `.cubic(...)` в widget/package source поиском не найден; нужен реальный сценарий |
| Индикаторы как потребители композиции | `flui-widgets/src/controls/activity_indicator.rs:113`; material `progress_indicator.rs:116`; cupertino `activity_indicator.rs:79` | Widget/package тесты в этом аудите не запускались |
| MotionClock | `motion.rs:175`; runtime `presentation.rs:410,956`; runtime `ui_realm/frame.rs:122` | Типизированное время превращается обратно в f64, скорость production-презентации остаётся default |
| Render-driven slide/scale/rotation | widgets `transitions/{slide,scale,rotation}_transition.rs`; objects `proxy/animated_transform.rs` | Headless-каталог существует; GPU и semantics-паритет здесь не доказаны |
| Oklab/Angle/Matrix4, derive TwoWayConverter | foundation/painting, ADR-0149; `tests/derive_animatable.rs` | Cross-crate regression suites отдельно здесь не запускались |
| Аллокации установившегося тика | `tests/tick_allocation.rs`, прошёл | Только измеренный сценарий, до четырёх value-listeners; не весь frame path |

Пути без префикса в этой таблице относятся к `crates/flui-animation/`.

## Standards: архитектура и обязанности вызывающего кода

### S1. Ручное владение остаётся частью интерфейса каждого потребителя — код + воспроизведение

`controller.rs:257` хранит `Arc<Mutex<...>>`; `vsync.rs:44,99` даёт регистрационный идентификатор
и сильное владение контроллером в реестре, без владеющего Drop-токена. В
`flui-widgets/src/animated/implicitly_animated.rs:58,91,175` вызывающий код координирует
создание, регистрацию, смену реестра, отписку и dispose. Последний handle работающего
контроллера не освобождает его: `ownership::last_handle_drop_releases_a_running_controller`
падает. Тесты освобождения status-listeners у Reverse/Curved/Tween тоже падают.

Deletion test Matt: убрать ручной протокол из интерфейса можно только если его обязанности
перейдут внутрь владеющего модуля. Новая обёртка, оставляющая register/dispose у виджета,
не решит проблему. Существующий `ParentSubscription` (`animation.rs:125–160`) уже вынимает
teardown из guard; это полезное поведение следует сохранить. Владелец изменения — send-flip
T6c совместно с T4, а не независимая замена типов только в одном крейте.

### S2. Отказ и reentry не проходят весь путь уведомления — воспроизведено

`controller.rs:2372` вызывает status callbacks напрямую; `vsync.rs:462,511` напрямую обходит
детей и контроллеры. Паникующий callback прерывает хвост; удалённый слушатель получает
уведомление; reentrant reverse доставляет `[Forward, Reverse, Completed]` вместо порядка
коммитов `[Forward, Completed, Reverse]`. 13 тестов `status_delivery` красные.
Два switch reentry и два frame-scheduled hook reentry заканчиваются 10-секундным
таймаутом дочернего процесса. Нужны первый authoritative failure, завершение допустимой
раздачи, освобождение captures вне guard/borrow и работа следующего кадра.

### S3. Два пути времени и process-global dilation — код

`MotionClock` принадлежит презентации, но `frame.rs:122` передаёт
`tick.now().as_duration().as_secs_f64()` в старый `Vsync::tick_all(f64)`.
`controller.rs:1897,2521` всё ещё читает process-global time dilation из scheduler
`config.rs:43`. В тестовом host `flui-testing/src/widgets/host.rs:194` и дополнительной
presentation `flui-testing/src/lib.rs:1440` остаётся сырой путь времени.
Удаление этих параллельных путей концентрирует правила rebasing/паузы в одном модуле.
Тест `nan_frame_time_is_skipped` сейчас падает.

### S4. Неподключённая поверхность и незавершённая миграция — код

Поиск по production source не находит widget-потребителя `AnimatedValue`/`MotionSpec`.
`AnimationBehavior` в `status.rs:97` сам описан как неиспользуемый контроллером носитель
политики. `lib.rs:146–147,193–200` продолжает экспортировать CompoundAnimation, ALWAYS_*,
scheduler и prelude, хотя восстановленное решение владельца предписывает миграцию/удаление.
`Animation` остаётся `Send + Sync` (`animation.rs:68`); owner-local и sealed изменения
из исторических спек не завершены. Нельзя закрывать эти пункты наличием теста чистой функции.

## Spec: поведение и недостающие связи

### F1. Controller lifecycle и численная защита — воспроизведено

Девять `controller_robustness` контрактов падают: NaN кривой публикуется вместо последнего
конечного значения; overshoot 1.5 выходит за [0,1]; Duration::MAX не проходит полный запуск;
после dispose меняется значение и удерживаются новые listeners; `is_animating` не соответствует
установленному run и обёрткам. `controller.rs:2151,2194,2693,2722,2740` показывает причины.
Это обязательства safety/lifecycle, а не новые функции рынка. Сначала закрыть их существующими
публичными тестами, затем расширять пользовательский motion-путь.

### F2. Implicit retarget по-прежнему старый — код; дополнительные C⁰-сценарии как гипотеза

`implicitly_animated.rs:145,273` меняет кривую и перезапускает controller/tween; новая кривая
может быть установлена до чтения отображаемой позиции. В `controller.rs:1782` скорость
curved run — линейное среднее span/duration. Численный `AnimatedValue` в production-путь
не включён. Допуск `<0.2` у opacity-теста (`tests/implicit_animations.rs:94`) не доказывает C¹.
Нужен тест отображаемого значения и производной при смене target, curve, duration и обоих
параметров, затем миграция общей implicit-логики. Само наличие численного алгоритма — частичное
выполнение требования, не основание переписывать его заново.

### F3. Системная motion policy отсутствует на этой базе — код

В production animation/runtime/widget пути не найден потребитель системного motion setting.
Один host `SystemPreferences` и проекция animation уже выбраны в `orchestration.md:82–102`.
PR [#1515](https://github.com/vanyastaff/flui/pull/1515) прочитан 2026-10-08: OPEN, head
`2f3e0b017cc289753953a085064cff535783fed3`; он несёт смежную доставку preferences, но не входит
в проверенный SHA. Перед работой по policy перечитать его конечный merged diff, чтобы не
создавать второй producer. Не возвращать старый SystemMotion/оконные запросы.

### F4. Экранная геометрия жеста и semantics — код, новая runtime-проверка нужна

Dismissible документирует нормирование по max constraints, а не размеру laid-out child
(`interaction/dismissible.rs:1105,1120–1140`). Проверочный сценарий: ребёнок 150 px внутри
loose max 1200 px, отпускание 1500 px/s; проверять экранную скорость drag и settle.
Там же `:1424–1441` сохраняется rebuild/FractionalTranslation путь. Drawer вызывает обычный
fling (`flui-material/src/drawer.rs:615`); конкретный дефект его settle сначала воспроизвести.

Semantics traversal складывает offsets (`flui-rendering/src/pipeline/owner/semantics.rs:605,881`),
не применяя paint transforms. Нужна проверка bounds анимированного semantic child, вложенных
transform/clip и вырожденного случая. GPU-readback для retained transform в этом аудите не
запущен; его отсутствие в узком поиске не доказывает отсутствие всех косвенных покрытий.

### F5. Документация, scope и критерии качества расходятся — код/документы

Из текущего плана были утрачены 12 тем и матрица; старые числовые статусы нельзя переносить
на сегодняшний код. Старый резерв ADR-0143 конфликтует с принятым input ADR. Market M-TIME-12
говорит о знаковой скорости, а motion-clock R4 явно запрещает отрицательную — уточнённый
контракт темы должен заменить старую формулировку, а не получить ложный статус «нет».
Steps реализован без before flag по утверждённой модели композиции; full WAAPI был вынесен.
Color хранит u8 (`flui-painting/src/styling/color.rs:20–28`), поэтому требование «без 8-битной
ступени» M-INTP-3 не закрывается одним Oklab. Его стоимость и scope требуют отдельного решения.
Шесть усиленных clippy-lints остаются отложены (`lib.rs:96–107`). Исторические performance
таблицы и scratch/prototype разделы не являются замером текущего дерева.

## Первичные источники, проверенные в этом проходе

- [Media Queries 5, prefers-reduced-motion](https://www.w3.org/TR/mediaqueries-5/#prefers-reduced-motion):
  системное предпочтение меньшего движения — вход политики, не приказ «не уведомлять».
  Это Working Draft; FLUI не заявляет CSS conformance.
- [CSS Transitions 1, starting transitions](https://www.w3.org/TR/css-transitions-1/#starting):
  reference для прерывания/разворота; FLUI C¹ — собственный более сильный контракт.
- [CSS Color 4, interpolation](https://www.w3.org/TR/css-color-4/#interpolation):
  reference для цвета и premultiplied alpha; u8 endpoint identity и overshoot сверяются также
  с принятым ADR-0149.

Остальные ссылки восстановленной матрицы здесь повторно не проверялись; пометки `[U]`
сохраняют силу. Это не новый полный сравнительный обзор библиотек.
