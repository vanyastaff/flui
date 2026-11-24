# testing-platform: задачи

- **Статус:** утверждено владельцем 2026-11-24. Сделано: ADR-0400, эта спека, TP1 без `WaitOutcome::Idle`
  (см. TP1). Следующая: TP2.
- **Дизайн:** [design.md](design.md); требования: [requirements.md](requirements.md); решение:
  [ADR-0400](../../../adr/ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
- **База:** `main` @ `aa45aa8`. Числа сняты поиском по именам; перед правкой сверить.

## Правила исполнения

- Задача = ветка = PR, проверки по `AGENTS.md`: «Checking a change», «Before each push»,
  «Before a merge».
- Каждый PR правит документы, которые он меняет: `docs/testing.md`,
  `crates/flui-testing/ARCHITECTURE.md`, `crates/flui-testing/README.md`,
  `book/src/cookbook/testing.md`, строку «Current forms» в `AGENTS.md` той формы, которую он меняет,
  записи `docs/gaps.md`, факт которых он меняет (каждая задача ниже называет свои), и добавляет
  фрагмент `changelog.d/`. Принятый ADR, который предписывает убираемую форму (например,
  ADR-0083, ADR-0264, ADR-0272 про `HeadlessHost`), получает в том же PR пару `Supersedes` /
  `Superseded-by` на ту часть, которую PR меняет; его тело не переписывается.
- Старые формы, которые TP4 помечает `#[deprecated(note = "use Tester …")]`: `lay_out` и все
  `lay_out_*`, `LaidOut`, `Harness`, `harness::mount*`, `dispatch_pointer_*`, `PointerScript` и
  его четыре построителя, `find_by_render_type` и его `try_`/`find_all_` формы, `A11yQuery`,
  `ProbeRoot`, `StateCreations`, `ScriptedSink`, `HeadlessSink`, `pump_report`; `HeadlessHost`
  как публичный тип с TP17; `Window`, `ElementLocator`, `Element`, `Expect`, `SoftExpect` драйвера
  с TP18. Ещё не перенесённый модуль тестов несёт на строке `mod`, а где модуля нет (бенч,
  модульный тест в `src/`) на самом элементе,
  `#[expect(deprecated, reason = "ADR-0400: moves to Tester")]`.
- Прогресс считает использования, а не маркеры (маркер на модуле не видит новый вызов внутри):
  `rg -c '\b(lay_out\w*|LaidOut|harness::mount\w*|dispatch_pointer_\w+|PointerScript|(try_)?find(_all)?_by_render_type|A11yQuery|pump_report)\b' --glob '*.rs' crates packages src tests`,
  число только падает.
- Изменение схемы протокола называется в теле PR построчно; версия протокола берётся следующая
  свободная на момент PR.
- Опыт, названный в задаче, ставится до её размножения; его число записывается в `design.md`
  вместо предсказания, а если оно меняет решение, то и в ADR-0400.
- Перенос файлов тестов идёт по одному крейту: сначала один файл, результат читается, бриф
  исправляется, затем остальные.
- TP5, TP18, TP20 и TP21 трогают `flui-rendering`, `flui-painting` и `flui-engine`, которые
  `AGENTS.md` открывает только задачам render-scaling и исправлениям дефектов: эти задачи меняют
  лишь фичу `testing` или новые фичи тестовой поддержки и тесты крейта, и каждая ждёт решения
  координатора о том, что это не изменение конвейера, прежде чем начаться.

## Основание

### TP1. Протокол: соединение и рост словаря

- **Что:** `flui_protocol::client::Connection` (с `finish`) и
  `CallError::Unanswered { may_have_run, released }`; корневая форма `Locator`;
  `Expectation::{Visible, Description}`; у `UnsettledCause` поля `window`, `owner`, `site`,
  `cleared_by` и тип `ClearedBy`; `SettleScope` в `WaitOutcome`; новые `UnsettledReason` (причины
  хоста, тикер); `WaitOutcome::Idle`; `approx_eq` у `LogicalRect`; платформенная конвенция и
  безопасная область в `Placement`; виртуальный указатель в `InputSource`; `input::plan` с
  понижением в инъекцию, платформенный путь и строки `HostInput`; метод `element.scroll_until`;
  таймаут действия в запросе жеста, который разговор считает удержанием окна (ADR-0288 §4);
  жесты синтетического и семантического путей в таблице методов отвечает агент, путь `os`
  собирает драйвер; `AuditRule::TextContrast`; типы отчёта переезжают из `flui-driver`.
- **Проверка:** nextest `flui-protocol` с таблицей написаний, схемой и таблицей методов;
  `rg 'flui_driver::report' --glob '*.rs'` = 0.
- **Состояние:** сделано на ветке `claude/project-thread-5mrv8h`, кроме `WaitOutcome::Idle`.
  `Connection` реализует `flui_driver::Driver`; приватный трейт транспорта драйвера переименован в
  `Transport` и остался под ним: протокольный `Connection` обобщён по методу и не может быть
  объектом, а транспорту нужны hello, события и методы не окна. `Idle` не построен: §3
  ADR-0400 называет его вариантом `WaitOutcome`, а `SettleScope` полем того же типа; форму
  решает координатор. `site` причины несёт `build::SourceLocation`, а не новый тип. Значения до
  TP2: причины рантайма без `window`, `owner`, `site`, `cleared_by` из его предикатов, `scope`
  всегда `runtime_only`; жесты в таблице за агентом, рантайм отвечает на них `not_supported`, их
  собирает драйвер; `element.scroll_until` не отвечает никто; `agent_input` рантайма ещё
  понижает последовательность сам, мимо `input::plan`. Протокол 0.32, схема
  `tests/schema/protocol-0.32.json`, обязательное поле `UnsettledCause.cleared_by` записано в
  `BREAKING`.
- **Доказательства:** `cargo test` (heli и nextest на машине нет): `flui-protocol --all-features`
  18 + 57 + 2, без фич 9 + 26 + 2; `flui-driver` 380 + 1 + 1; `flui-semantics --all-features`
  68 + 81 + 7; `flui-runtime` 255; `flui-cli` 183 + 66; `flui-testing` 15 + 188 + 11 (3 ignored, ни одного нового). clippy
  `--all-targets -D warnings` чист у `flui-protocol` (с фичами и без), `flui-driver`,
  `flui-semantics`, `flui-runtime`, `flui-cli`, `flui-testing`. `rg 'flui_driver::report' --glob
  '*.rs'` = 0. Ни один случай trybuild не называет изменённые типы.

### TP2. Рантайм: один список причин, ответы на методы, жесты в агенте

- **Что:** `Unsettled` с `wake()`; `UiRuntime::unsettled` (половина рантайма: `cleared_by`
  выводится из его состояния, кадр, который должна поверхность, не входит) и
  `answer_settled(now, HostHalf)`; `unsettled_causes` удалён. `HeadlessHost` собирает `Unsettled`
  после своих шагов хода (`ask_owed_frames`, наблюдения), будится по `Unsettled::wake`, и по нему
  работают `pump_until_settled`, `time_to_next_frame` и ответы на `wait.settled`; ожидание на
  виртуальных часах ограничено и временем, и 10 000 ходов без сдвига часов; `Idle` только при
  `HostHalf::Complete` и причинах `Test`. Рантайм отвечает на `wait.settled`, `wait.frames`,
  `audit.a11y`, `perf.start`, `perf.stop`. `TargetSize` по области попадания внутри клипа живёт в
  `flui-semantics`, `TextContrast` в `flui-rendering::audit` (тот же код, что FD23 спеки
  `flui-driver`); `audit.a11y` зовёт оба. Агент сам отвечает на жесты: держит действие до
  actionable на двух кадрах в пределах его таймаута, срок удержания будит хост, по истечении
  действие снимается; вводит по границам кадра, где проверка прошла; так же `scroll_until`.
  `agent_input` зовёт `input::plan`. Из FD20 задача берёт ответы рантайма на `perf.start` и
  `perf.stop` с `PerfReport`, из FD21 правило размера цели и ответ `audit.a11y`; остальное (бюджеты
  и `perf.expect`, порядок фокуса, `audit.alternatives`, сценарий `a11y-audit`) остаётся в FD20 и
  FD21.
- **Опыт:** E4, E8.
- **Проверка:** строки подтверждения ADR-0400 для `flui-runtime`: таблица по строке на
  `UnsettledReason` (согласие settle хоста и `wait(Settled)`, имя причины и её `cleared_by`, общее
  устоявшееся состояние); удержанная работа не будит цикл и делает ложное ожидание `Idle` с её
  именем; `Absent` и причина `Outside` не дают `Idle`; срок в доли миллисекунды достигается;
  таймер, взводящийся на тот же миг, заканчивает ожидание по пределу ходов с его именем;
  перерисовка при взведённом повторе будит в момент повтора; тикер в скрытом `IndexedStack`
  назван; по тесту на каждый метод, убранный из `not_answered!`; действие, ждавшее устойчивости,
  вводит по границам кадра, на котором прошло, а просроченное не вводит никогда.

### TP3. `Tester` на `Connection`

- **Что:** `Headless<Observed>`; чтения как ходы без кадра; глагол заканчивается ходом, который
  разбирает поставленное его кадрами; цикл ожидания уходит из `Tester`; `Find<Q>`;
  матчеры-данные; `Tester::with(GestureOptions)`; `checks`; `Diagnosis`; отчёт держит первый
  отказ, мягкие отказы, «мог выполниться» и живые `TestError`, `Drop` по правилам ADR-0400 §7 в
  режиме «только отчёт»; `held_keys` удалён, ввод держит аренда агента; `connection_mut`; канал
  вместо `Arc<Mutex>`; `V: Clone` снят. Переходят 16 файлов с `Tester::mount`, восемь файлов
  `book/src/tutorial`, `book/src/mapping.md` и три шаблона `flui-cli`, которые пишут тест в новый
  проект, с тестом шаблонов.
- **Опыт:** E6 до начала.
- **Проверка:** nextest этих крейтов, `-p flui` и `-p flui-cli`;
  `rg '\.to_(exist|be_|have_)' --glob '!crates/flui-driver/**' crates packages src tests book` = 0
  (`Expect` драйвера живёт до TP18); чтение не меняет номер кадра; строки подтверждения
  ADR-0400 про `Drop` (паника, ошибка через `?`, ожидаемая ошибка до утёкшего тикера, клавиша,
  нажатая в конце теста); аккорд после `Gesture`, оставившего клавишу нажатой, не шлёт второго
  нажатия; compile-fail на `tap(&Find<Many>)`.

### TP4. `Frame`, жесты, `Unobserved`, `measure`; пилот на `flui-cupertino`

- **Что:** `Frame` и типизированные чтения, `Gesture`, `fixture::Constrain`, `WindowSpec`,
  `VirtualClock` с `pump`, `pump_for`, `settle` и `freeze_at`, `Headless<Unobserved>` с
  `perform`; `flui_testing::perf::measure` по ADR-0396 §1 с окном `perf_measured` под прежним
  именем, и `Tester::measure` над ней (пункт ADR-0396 в `docs/gaps.md` теряет эту часть);
  `scenarios!` с `each`; старые формы из списка правил помечены устаревшими, пункт ADR-0400 в
  `docs/gaps.md` называет их; весь `flui-cupertino` (11 файлов, 55 мест и его цель `perf`)
  переходит.
- **Опыт:** E1, E2, E3, E7.
- **Проверка:** nextest `flui-cupertino`; счётчик использований из правил по
  `packages/flui-cupertino` = 0, и `rg -w HeadlessHost --glob '*.rs' packages/flui-cupertino` = 0;
  `FLUI_PERF_BASELINE=check` по правилу E2; `measure_unobserved_matches_pump_report`;
  `freeze_at` на `ActivityIndicator` даёт снимок, повторяющийся байт в байт; compile-fail на
  `Frame` при обобщённом `C: Connection` и на `settle` у не-`VirtualClock`.

## Перенос

### TP5. `flui-view` и `flui-rendering`

- **Что:** `Recorder<V>`; `ElementBuildContext` удалён вместе с пунктом «ADR-0243 D2» в
  `docs/gaps.md`; `FrameRun::paint()` вместо `opacity_alpha` и `has_picture_layer`.
- **Проверка:** `rg 'struct Probe(State)?\b' --glob '*.rs'` падает на число `flui-view`; удалённых
  имён 0.

### TP6–TP8. `flui-material`

- **Что:** 42 файла тремя PR по областям.
- **Проверка в каждом:** nextest; счётчик использований по области = 0; perf по правилу E2.

### TP9–TP14. `flui-widgets`

- **Что:** 104 файла шестью PR: раскладка; текст и редактирование; прокрутка и сливеры (с
  `scroll_until` вместо рукописных циклов прокрутки); жесты и ввод; навигация и анимация; фокус,
  семантика и изображения. Рукописные `pump` и `const FRAME` удаляются вместе с файлом.
- **Проверка в каждом:** nextest `-p flui-widgets`; счётчик использований падает на число
  области.

### TP15. Цель `perf` в `flui-widgets`

- **Что:** сценарии на `measure` с вводом через `perform`; `perf_measured` остаётся окном сбора,
  `FRAME` удалён.
- **Проверка:** все 13 файлов под `check` по правилу E2; полный прогон `gh workflow run CI --ref`
  с заданием `perf-gates`, ссылка в теле PR.

### TP16. Доступность и движение

- **Что:** файлы на `a11y` и проверка движения переходят на `Tester::semantics` и чтение кадра;
  `A11yQuery` удалён; таблицы соответствия уходят за фичу `conformance`.
- **Проверка:** `rg -w A11yQuery --glob '*.rs'` = 0.

### TP17. `HeadlessHost`, `AppRun` и остальные пользователи

- **Что:** около 30 внешних файлов на `HeadlessHost` переходят на `Tester` и `WindowControl`;
  тесты самого хоста в `crates/flui-testing/tests` становятся модульными тестами кита;
  `AppRun: Connection + VirtualClock` над `HeadlessRun`, раннер собирает свою половину
  `Unsettled` (`HostHalf::Complete`), его часы на шве `HostClock`; тесты агента `flui-app` на
  строках уходят к `flui_runtime::agent::Session` в тесты `flui-runtime`; переходят `flui-sdk`,
  `flui-devtools`, `flui-system`, корневые `tests/` (`facade_smoke.rs`,
  `composited_layer_update_readback.rs`, `ui_pass/owner_callbacks.rs` и trybuild-фикстура
  `ui/thread_boundary/owner_state_stays_on_its_thread.rs` со своим `.stderr`), бенчи
  `flui-widgets` и `flui-material`; `HeadlessHost` становится `pub(crate)` (линия `headless`
  драйвера держит его до TP18 через фичу `testing` кита); строка `HeadlessHost` в «Current forms»
  `AGENTS.md`, пункт моста хоста в `docs/gaps.md` и `Superseded-by` в ADR-0083, ADR-0264 и
  ADR-0272 на их части про `HeadlessHost`.
- **Проверка:** `rg -w HeadlessHost --glob '*.rs' --glob '!crates/flui-testing/src/**' --glob '!crates/flui-driver/**' crates packages src tests`
  = 0; `app_run_waits_in_virtual_time`: ожидание в 5 с занимает меньше 100 мс настенного времени.

### TP18. Драйвер и CLI

- **Что:** `Driver` становится `Device: Connection`; `Window`, `ElementLocator`, `Element`,
  `Expect`, `SoftExpect`, `Launch::headless`, `compose.rs` (кроме сборки жеста для пути `os`),
  `scenario.rs`, `steps.rs`, закрытый `trait Connection`, `in_process.rs` и фича `headless`
  удалены; методы только устройства (`perform_via`, `os_tree`, `release_input`, пиксельные
  предикаты) остаются у `Device`; кодек PNG уходит в фичу `codec` `flui-painting` рядом с фичей
  `decode` и её границей размера, метрика и хранилище эталонов в её фичу `pixels`, дорожки,
  трасса и снятие кадров драйвера зовут их, инварианты `pixels::Baselines` и `pixels::codec`
  переезжают из `crates/flui-driver/ARCHITECTURE.md` в `flui-painting`; клиент агента в
  `flui-cli` заменён на `Device`, его пункт в `docs/gaps.md` снят; нативным раннерам, ещё не
  собирающим половину хоста, заводится пункт в `docs/gaps.md` и строка в
  `crates/flui-native/platform-parity.toml`; первый пример с целью `device`, которую ночные
  платформенные прогоны запускают через `each::<Device>`. Спека `flui-driver` правится тем же PR.
- **Проверка:** nextest драйвера, CLI и `flui-painting` с тестом живого агента;
  `rg -w 'ElementLocator|SoftExpect' --glob '*.rs'` и `rg 'Launch::headless' --glob '*.rs'` = 0;
  `cargo heli tree -p flui-driver -e normal` не называет `flui-testing`; compile-fail на `pump_for`
  у `Tester<Device>`; прогон цели `device` на Linux в CI-образе, ссылка в теле PR.

### TP19. Двойники

- **Что:** по одному PR на шов, каждый сначала проверяет импорты переносимого типа: буфер обмена
  и первичное выделение в `flui_interaction::testing`, хаптика, текстовый ввод, `TestSink`,
  двойники окна.
- **Проверка:** каждого удалённого имени 0; набор `flui-native` на Linux; PR называет ручные
  прогоны на Windows и macOS.

## Новые инструменты

### TP20. Золотые снимки

- **Что:** фича `golden` с швом `golden::Rasterize`, который реализует фича `testing`
  `flui-engine`, а фича `golden` фасада соединяет их; `goldens.toml` с названным адаптером (имя,
  версия драйвера, версия его компилятора, ширина вектора) и одним потоком растеризатора; цель
  `golden` в `flui-material`, около 30 снимков, шаг в задании `test` рядом с readback на Vulkan,
  который запускает цель отдельным вызовом в один поток, `flui test --goldens`; два снимка
  `.snap` `flui-objects` становятся текстовыми снимками того же держателя (шесть снимков
  сгенерированных файлов `flui-cli` остаются как есть).
- **Опыт:** E5, E10.
- **Проверка:** несовпадение пишет diff; упавший прогон ничего не переснимает; адаптер, который
  отличается от названного, даёт `CannotVerify`; compile-fail на `golden` вне
  `Settled<'_, Headless<_>>`; ссылка на полный прогон в теле PR.

### TP21. Устойчивость

- **Что:** модули `strategy` в китах `flui-foundation`, `flui-painting`, `flui-rendering`; `model`;
  32 блока `proptest!` на общих стратегиях; первые модели на редактировании текста и стеке
  `Router`; та же модель как ночная фаззинг-цель.
- **Проверка:** подложенный дефект воспроизводится по напечатанному зерну и сжимается за 1 024
  прогона.

### TP22. Умолчания и удаление

- **Что:** проверки завершения и аудит `Unnamed` включены по умолчанию; флаги `flui test`;
  `widgets.rs`, `widgets/`, `replay.rs` без `Gesture`, `control_cost.rs` удалены; пункт ADR-0400 в
  `docs/gaps.md` снят, ADR-0400 становится `Implemented`; «Current forms» получает по строке на
  глубину.
- **Опыт:** E9 до включения.
- **Проверка:** счётчик использований из правил = 0, и
  `rg -c 'ADR-0400: moves to Tester' --glob '*.rs'` = 0; те же имена по `book`, `docs/testing.md`,
  `docs/crates.md`, `docs/extending.md`, `crates/flui-testing/README.md`,
  `crates/flui-animation/docs/GUIDE.md`, `crates/flui-native/platform-parity.toml`, каждому
  `ARCHITECTURE.md` и `AGENTS.md` = 0.
