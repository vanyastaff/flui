# testing-platform: задачи

- **Статус:** утверждено владельцем 2026-11-24. Сделано: ADR-0400 и эта спека. Следующая: TP1.
- **Дизайн:** [design.md](design.md); требования: [requirements.md](requirements.md); решение:
  [ADR-0400](../../../adr/ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
- **База:** `main` @ `aa45aa8`. Числа сняты поиском по именам; перед правкой сверить.

## Правила исполнения

- Задача = ветка = PR, проверки по `AGENTS.md`: «Checking a change», «Before each push»,
  «Before a merge».
- Каждый PR правит документы, которые он меняет: `docs/testing.md`,
  `crates/flui-testing/ARCHITECTURE.md`, `book/src/cookbook/testing.md`, строку «Current forms» в
  `AGENTS.md` той формы, которую он меняет, записи `docs/gaps.md`, факт которых он меняет (каждая
  задача ниже называет свои), и добавляет фрагмент `changelog.d/`. Принятые ADR сохраняют свой
  текст: поиск старых имён идёт по коду и живым документам, не по `docs/adr/`.
- С TP4 старые формы помечены `#[deprecated(note = "use Tester …")]`, `docs/gaps.md` держит пункт
  ADR-0400, а каждый ещё не перенесённый модуль тестов несёт на строке `mod`, а где модуля нет
  (бенч, модульный тест в `src/`) на самом элементе,
  `#[expect(deprecated, reason = "ADR-0400: moves to Tester")]`. Прогресс:
  `rg -c 'ADR-0400: moves to Tester' --glob '*.rs' crates packages src`, число только падает.
- Изменение схемы протокола называется в теле PR построчно; версия протокола берётся следующая
  свободная на момент PR.
- Опыт, названный в задаче, ставится до её размножения; его число записывается в `design.md`
  вместо предсказания, а если оно меняет решение, то и в ADR-0400.
- Перенос файлов тестов идёт по одному крейту: сначала один файл, результат читается, бриф
  исправляется, затем остальные.

## Основание

### TP1. Протокол: соединение и рост словаря

- **Что:** `flui_protocol::client::{Connection, VirtualClock}` и `CallError::Unanswered { may_have_run }`;
  корневая форма `Locator`; `Expectation::{Visible, Description}`; у `UnsettledCause` поля
  `window`, `owner`, `site`, `cleared_by` и тип `ClearedBy`; новые `UnsettledReason` (причины хоста,
  тикер); `WaitOutcome::Idle`; виртуальный указатель в `InputSource`; `input::plan` с понижением в
  инъекцию, платформенный путь и строки `HostInput`; метод `element.scroll_until`; жесты
  синтетического и семантического путей в таблице методов отвечает агент; типы отчёта переезжают
  из `flui-driver`.
- **Проверка:** nextest `flui-protocol` с таблицей написаний, схемой и таблицей методов;
  `rg -w 'flui_driver::report' --glob '*.rs'` = 0.

### TP2. Рантайм: один список причин у хоста, ответы на методы, жесты в агенте

- **Что:** `UiRuntime::unsettled` (половина рантайма, без кадра, который должна поверхность) и
  `answer_settled(now, HostHalf)`; `unsettled_causes` удалён. `HeadlessHost` собирает один список
  после своих шагов хода (`ask_owed_frames`, наблюдения), и по нему, по правилу пробуждения через
  `cleared_by`, работают `pump_until_settled`, `time_to_next_frame` и ответы на `wait.settled`;
  `Idle` только при `HostHalf::Complete`. Рантайм отвечает на `wait.settled`, `wait.frames`,
  `audit.a11y`, `perf.start`, `perf.stop`. Правила аудита (`TargetSize` по области попадания
  внутри клипа, `TextContrast` по цветам команд) живут в `flui-semantics`, рантайм и кит зовут
  их. Агент сам отвечает на жесты: держит действие до actionable на двух кадрах и вводит по
  границам кадра, где проверка прошла; так же `scroll_until`. `agent_input` зовёт `input::plan`.
  Эта задача забирает FD20 и FD21 спеки `flui-driver` (там помечены перенесёнными).
- **Опыт:** E4, E8.
- **Проверка:** таблица по строке на `UnsettledReason`: settle хоста и `wait(Settled)` согласны,
  называют причину и её `cleared_by`, и устаиваются на одном состоянии; удержанная работа не будит
  цикл и делает ложное ожидание `Idle` с её именем; срок ждущего ожидания достигается в виртуальном
  времени; `HostHalf::Absent` не даёт `Idle`; тикер в скрытом `IndexedStack` назван; по тесту на
  каждый метод, убранный из `not_answered!`; действие, ждавшее устойчивости, вводит по границам
  кадра, на котором прошло.

### TP3. `Tester` на `Connection`

- **Что:** `Headless<Observed>`; чтения как ходы без кадра; глагол заканчивается ходом, который
  разбирает поставленное его кадрами; цикл ожидания уходит из `Tester`; `Find<Q>`;
  матчеры-данные; `checks`; `Diagnosis`; отчёт держит первый отказ, мягкие отказы и «мог
  выполниться», `Drop` по правилам ADR-0400 §7 в режиме «только отчёт»; `held_keys` удалён, ввод
  держит аренда агента; канал вместо `Arc<Mutex>`; `V: Clone` снят. Переходят 16 файлов с
  `Tester::mount`, семь файлов `book/src/tutorial`, `book/src/mapping.md` и три шаблона
  `flui-cli`, которые пишут тест в новый проект, с тестом шаблонов.
- **Опыт:** E6 до начала.
- **Проверка:** nextest этих крейтов, `-p flui` и `-p flui-cli`;
  `rg '\.to_(exist|be_|have_)' --glob '!crates/flui-driver/**' crates packages src book` = 0
  (`Expect` драйвера живёт до TP18); чтение не меняет номер кадра; пойманная паника и следующий
  `Drop`, и ошибка, вернувшаяся через `?`, и следующий `Drop` не запускают проверок завершения и не
  теряют отказа; аккорд после `Gesture`, оставившего клавишу нажатой, не шлёт второго нажатия;
  compile-fail на `tap(&Find<Many>)`.

### TP4. `Frame`, жесты, `Unobserved`, `measure`; пилот на `flui-cupertino`

- **Что:** `Frame` и типизированные чтения, `Gesture`, `fixture::Constrain`, `WindowSpec`,
  `Headless<Unobserved>` с `perform`, `pump`, `pump_for` и `freeze_at` на `VirtualClock`,
  `Tester::measure` над `perf::measure` с окном `perf_measured` под прежним именем, `scenarios!`;
  старые формы помечены устаревшими, пункт ADR-0400 в `docs/gaps.md` называет их; весь
  `flui-cupertino` (11 файлов, 55 мест и его цель `perf`) переходит.
- **Опыт:** E1, E2, E3, E7.
- **Проверка:** nextest `flui-cupertino`;
  `rg -w 'lay_out|PointerScript|find_by_render_type|HeadlessHost|pump_report' --glob '*.rs' packages/flui-cupertino`
  и `rg 'harness::' --glob '*.rs' packages/flui-cupertino` = 0; `FLUI_PERF_BASELINE=check` по
  правилу E2; `measure_unobserved_matches_pump_report`; `freeze_at` на `ActivityIndicator` даёт
  снимок, повторяющийся байт в байт; compile-fail на `Frame` при обобщённом `C: Connection`.

## Перенос

### TP5. `flui-view` и `flui-rendering`

- **Что:** `Recorder<V>`; `ElementBuildContext` удалён вместе с пунктом «ADR-0243 D2» в
  `docs/gaps.md`; `FrameRun::paint()` вместо `opacity_alpha` и `has_picture_layer`.
- **Проверка:** `rg 'struct Probe(State)?\b' --glob '*.rs'` падает на число `flui-view`; удалённых
  имён 0.

### TP6–TP8. `flui-material`

- **Что:** 42 файла тремя PR по областям.
- **Проверка в каждом:** nextest; маркеры области сняты; perf по правилу E2.

### TP9–TP14. `flui-widgets`

- **Что:** 104 файла шестью PR: раскладка; текст и редактирование; прокрутка и сливеры (с
  `scroll_until` вместо рукописных циклов прокрутки); жесты и ввод; навигация и анимация; фокус,
  семантика и изображения. Рукописные `pump` и `const FRAME` удаляются вместе с файлом.
- **Проверка в каждом:** nextest `-p flui-widgets`; счётчик маркеров падает на число области.

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
  `AppRun: Connection + VirtualClock` над `HeadlessRun`, раннер собирает свою половину списка
  (`HostHalf::Complete`), его часы на шве `HostClock`; тесты агента `flui-app` на строках уходят к
  `flui_runtime::agent::Session` в тесты `flui-runtime`; переходят `flui-sdk`, `flui-devtools`,
  `flui-system`, `tests/` фасада, бенчи `flui-widgets` и `flui-material`; `HeadlessHost`
  становится `pub(crate)`; строка `HeadlessHost` в «Current forms» `AGENTS.md` и пункт моста хоста
  в `docs/gaps.md` переписываются.
- **Проверка:** `rg -w HeadlessHost --glob '*.rs' --glob '!crates/flui-testing/src/**' crates packages src`
  = 0; `app_run_waits_in_virtual_time`: ожидание в 5 с занимает меньше 100 мс настенного времени.

### TP18. Драйвер и CLI

- **Что:** `Driver` становится `Device: Connection`; `Window`, `ElementLocator`, `Element`,
  `Expect`, `SoftExpect`, `Launch::headless`, `compose.rs` (кроме сборки жеста для пути `os`),
  `scenario.rs`, `steps.rs`, закрытый `trait Connection`, `in_process.rs` и фича `headless`
  удалены; `Tester<Device>` получает `perform_via`, `os_tree`, `release_input`; кодек PNG уходит в
  фичу `codec` `flui-painting`, метрика и хранилище эталонов в её фичу `pixels`, дорожки, трасса и
  снятие кадров драйвера зовут их, инварианты `pixels::Baselines` и `pixels::codec` переезжают из
  `crates/flui-driver/ARCHITECTURE.md` в `flui-painting`; клиент агента в `flui-cli` заменён на
  `Device`, его пункт в `docs/gaps.md` снят; нативным раннерам, ещё не собирающим половину хоста,
  заводится пункт в `docs/gaps.md` и строка в `crates/flui-native/platform-parity.toml`.
  Спека `flui-driver` правится тем же PR.
- **Проверка:** nextest драйвера, CLI и `flui-painting` с тестом живого агента;
  `rg -w 'ElementLocator|SoftExpect' --glob '*.rs'` и `rg 'Launch::headless' --glob '*.rs'` = 0;
  `cargo heli tree -p flui-cli -e normal` не называет `flui-testing`; compile-fail на `pump_for` у
  `Tester<Device>`.

### TP19. Двойники

- **Что:** по одному PR на шов, каждый сначала проверяет импорты переносимого типа: буфер обмена
  и первичное выделение в `flui_interaction::testing`, хаптика, текстовый ввод, `TestSink`,
  двойники окна.
- **Проверка:** каждого удалённого имени 0; набор `flui-native` на Linux; PR называет ручные
  прогоны на Windows и macOS.

## Новые инструменты

### TP20. Золотые снимки

- **Что:** фича `golden`, `goldens.toml` с названным адаптером (имя, версия драйвера, версия его
  компилятора) и одним потоком растеризатора, цель `golden` в `flui-material`, около 30 снимков,
  шаг в задании `test` рядом с readback на Vulkan, `flui test --goldens`; два снимка `.snap`
  `flui-objects` становятся текстовыми снимками того же держателя (шесть снимков сгенерированных
  файлов `flui-cli` остаются как есть).
- **Опыт:** E5, E10.
- **Проверка:** несовпадение пишет diff; упавший прогон ничего не переснимает; адаптер, который
  отличается от названного, даёт `CannotVerify`; compile-fail на `golden` вне `Settled` и у
  `AppRun`; ссылка на полный прогон в теле PR.

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
- **Проверка:** `rg -c 'ADR-0400: moves to Tester' --glob '*.rs'` = 0;
  `rg -w 'LaidOut|lay_out|control_cost|PointerScript|pump_report|find_by_render_type' --glob '*.rs' crates packages src`
  и `rg 'harness::mount' --glob '*.rs' crates packages src` = 0; те же имена по `book`,
  `docs/testing.md`, `docs/crates.md`, каждому `ARCHITECTURE.md` и `AGENTS.md` = 0.
