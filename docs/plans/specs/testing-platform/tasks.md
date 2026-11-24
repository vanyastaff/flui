# testing-platform: задачи

- **Статус:** черновик 2026-11-24; ждёт утверждения владельцем. Сделано: ничего.
- **Дизайн:** [design.md](design.md); требования: [requirements.md](requirements.md); решение:
  [ADR-0400](../../../adr/ADR-0400-a-test-is-one-tester-over-a-connection-and-every-claim-has-one-instrument.md)
- **База:** `main` @ `aa45aa8`. Числа сняты поиском по именам; перед правкой сверить.

## Правила исполнения

- Задача = ветка = PR, проверки по `AGENTS.md`: «Checking a change», «Before each push»,
  «Before a merge».
- Каждый PR правит документы, которые он меняет: `docs/testing.md`,
  `crates/flui-testing/ARCHITECTURE.md`, `book/src/cookbook/testing.md`, строку «Current forms» в
  `AGENTS.md`, и добавляет фрагмент `changelog.d/`.
- С TP4 старые формы помечены `#[deprecated(note = "use Tester …")]` с одной записью в
  `docs/gaps.md`, а каждый ещё не перенесённый модуль тестов несёт на строке `mod`
  `#[expect(deprecated, reason = "docs/gaps.md#one-tester")]`. Прогресс: число таких строк
  (`rg -c 'gaps.md#one-tester'`), оно только падает.
- Изменение схемы протокола называется в теле PR построчно; версия протокола берётся следующая
  свободная на момент PR.
- Опыт, названный в задаче, ставится до её размножения; его число записывается в `design.md`
  вместо предсказания.
- Перенос файлов тестов идёт по одному крейту: сначала один файл, результат читается, бриф
  исправляется, затем остальные.

## Основание

### TP1. Протокол: соединение и рост словаря

- **Что:** `flui_protocol::client::Connection`; корневая форма `Locator`;
  `Expectation::{Visible, Description}`; новые `UnsettledReason` и поля `owner`, `site` у
  `UnsettledCause`; `AuditRule::TextContrast`; `input::plan`; типы отчёта переезжают из
  `flui-driver`.
- **Проверка:** nextest `flui-protocol` с таблицей написаний и схемой; `rg 'flui_driver::report'`
  = 0.

### TP2. Рантайм: один предикат и ответы на методы

- **Что:** `HostWork` и `UiRuntime::unsettled`; `HeadlessHost` реализует шов, а
  `pump_until_settled` и `time_to_next_frame` читают предикат; рантайм отвечает на `wait.settled`,
  `wait.frames`, `audit.a11y`, `perf.start`, `perf.stop`; места создания причин; `TargetSize` по
  области попадания внутри клипа; `TextContrast` по цветам команд; `agent_input` зовёт
  `input::plan`.
- **Опыт:** E4, E8.
- **Проверка:** таблица по строке на причину: и `pump_until_settled`, и `wait(Settled)` остаются
  неустоявшимися и называют причину; рантайм с выключенными кадрами устаивается; тикер в скрытом
  `IndexedStack` назван; по тесту на каждый метод, убранный из `not_answered!`.

### TP3. `Tester` на `Connection`

- **Что:** `Headless<Observed>`; чтения как ходы без кадра; цикл ожидания уходит из `Tester`;
  `Find<Q>`; матчеры-данные; `checks`; `Diagnosis`; страж отравления и завершение в `Drop` в
  режиме «только отчёт»; канал вместо `Arc<Mutex>`; `V: Clone` снят. 16 файлов с `Tester::mount`
  переходят.
- **Опыт:** E6 до начала.
- **Проверка:** nextest этих крейтов и `-p flui`; `rg '\.to_(exist|be_|have_)'` = 0; чтение не
  меняет номер кадра; пойманная паника и следующий за ней `Drop` не запускают проверок
  завершения; compile-fail на `tap(&Find<Many>)`.

### TP4. `Frame`, жесты, `Unobserved`, `measure`; пилот на `flui-cupertino`

- **Что:** `Frame` и типизированные чтения, `Gesture`, `fixture::Constrain`,
  `Headless<Unobserved>`, `perf::measure`, `scenarios!`; старые формы помечены устаревшими; весь
  `flui-cupertino` (11 файлов, 55 мест и его цель `perf`) переходит.
- **Опыт:** E1, E2, E3, E7.
- **Проверка:** nextest `flui-cupertino`;
  `rg 'lay_out|harness::|PointerScript|find_by_render_type|HeadlessHost|pump_report' packages/flui-cupertino`
  = 0; `FLUI_PERF_BASELINE=check` без изменений; `measure_unobserved_matches_pump_report`.

## Перенос

### TP5. `flui-view` и `flui-rendering`

- **Что:** `Recorder<V>`; `ElementBuildContext` удалён; `FrameRun::paint()` вместо
  `opacity_alpha` и `has_picture_layer`.
- **Проверка:** `rg 'struct Probe(State)?\b'` падает на число `flui-view`; удалённых имён 0.

### TP6–TP8. `flui-material`

- **Что:** 42 файла тремя PR по областям.
- **Проверка в каждом:** nextest; маркеры области сняты; perf без изменений.

### TP9–TP14. `flui-widgets`

- **Что:** 104 файла шестью PR: раскладка; текст и редактирование; прокрутка и сливеры; жесты и
  ввод; навигация и анимация; фокус, семантика и изображения. Рукописные `pump` и `const FRAME`
  удаляются вместе с файлом.
- **Проверка в каждом:** nextest `-p flui-widgets`; счётчик маркеров падает на число области.

### TP15. Цель `perf` в `flui-widgets`

- **Что:** сценарии на `measure`; `perf_measured` и его `FRAME` удалены.
- **Проверка:** все 13 файлов под `check` без изменений.

### TP16. Доступность и движение

- **Что:** файлы на `a11y` и проверка движения переходят на `Tester::semantics` и чтение кадра;
  `A11yQuery` удалён; таблицы соответствия уходят за фичу `conformance`.
- **Проверка:** `rg A11yQuery` = 0.

### TP17. `HeadlessHost` и `AppRun`

- **Что:** около 29 внешних файлов на `HeadlessHost` переходят на `Tester` и `WindowControl`;
  `AppRun: Connection` над `HeadlessRun`, раннер реализует `HostWork`, его часы на шве
  `HostClock`; `HeadlessHost` становится `pub(crate)`.
- **Проверка:** `rg HeadlessHost --glob '!crates/flui-testing/src/**'` = 0;
  `app_run_waits_in_virtual_time`: ожидание в 5 с занимает меньше 100 мс настенного времени.

### TP18. Драйвер и CLI

- **Что:** `Device: Connection`; `Window`, `ElementLocator`, `Element`, `Expect`, `SoftExpect`,
  `Launch::headless`, `in_process.rs` и фича `headless` удалены; метрика, база и кодек пикселей
  уходят в `flui_testing::golden`; клиент агента в `flui-cli` заменён на `Device`. Спека
  `flui-driver` правится тем же PR.
- **Проверка:** nextest драйвера и CLI с тестом живого агента;
  `rg 'ElementLocator|SoftExpect|Launch::headless'` = 0; `cargo heli tree -p flui-cli -e normal`
  не называет `flui-testing`.

### TP19. Двойники

- **Что:** по одному PR на шов, каждый сначала проверяет импорты переносимого типа: буфер обмена
  и первичное выделение в `flui_interaction::testing`, хаптика, текстовый ввод, `TestSink`,
  двойники окна.
- **Проверка:** каждого удалённого имени 0; набор `flui-native` на Linux; PR называет ручные
  прогоны на Windows и macOS.

## Новые инструменты

### TP20. Золотые снимки

- **Что:** фича `golden`, цель `golden` в `flui-material`, около 30 снимков, шаг в задании `test`
  рядом с readback на Vulkan, `flui test --goldens`; 8 файлов `.snap` становятся текстовыми
  снимками того же держателя.
- **Опыт:** E5, E10.
- **Проверка:** несовпадение пишет diff; упавший прогон ничего не переснимает; не канонический
  адаптер даёт `CannotVerify`; ссылка на полный прогон в теле PR.

### TP21. Устойчивость

- **Что:** модули `strategy` в китах `flui-foundation`, `flui-painting`, `flui-rendering`; `model`;
  32 блока `proptest!` на общих стратегиях; первые модели на редактировании текста и стеке
  `Router`; та же модель как ночная фаззинг-цель.
- **Проверка:** подложенный дефект воспроизводится по напечатанному зерну и сжимается за 1 024
  прогона.

### TP22. Умолчания и удаление

- **Что:** проверки завершения и аудит `Unnamed` включены по умолчанию; флаги `flui test`;
  `widgets.rs`, `widgets/`, `replay.rs`, `control_cost.rs` удалены; запись в `docs/gaps.md` снята;
  «Current forms» получает по строке на глубину.
- **Опыт:** E9 до включения.
- **Проверка:** `rg 'gaps.md#one-tester'` = 0;
  `rg 'LaidOut|lay_out\(|harness::mount|control_cost|PointerScript|pump_report|find_by_render_type'`
  по `crates`, `packages`, `docs`, `book` и `AGENTS.md` = 0.
