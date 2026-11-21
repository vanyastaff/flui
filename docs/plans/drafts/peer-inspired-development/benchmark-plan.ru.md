# План сравнительных бенчмарков FLUI

Статус: широкая программа остаётся предложением; владелец отдельно разрешил первый FLUI-only этап. Общая сцена и wall-time adapter реализованы, [baseline и границы доказательств](benchmark-baseline.ru.md) сохранены в этом draft PR. Native/peer adapters не реализованы, performance budgets не приняты. Дата: 2 ноября 2026 года. [Обзор upstream-результатов](benchmark-review.ru.md) объясняет происхождение кандидатов. Ниже указаны условия измерения, а не обещанный выигрыш FLUI.

## Цель и существующая основа

Ответить на четыре вопроса: сколько стоит UI данного поведения; сколько работы вызывает маленькое изменение; остаётся ли ввод отзывчивым при тяжёлом rendering; сколько времени и ресурсов нужно автору приложения для изменения и доставки этого UI.

На базе FLUI `9a5df98b49b25d267a1cf1d8db3c400bdd6492b7` уже есть:

- [xtask bench-collect](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/tools/xtask/src/bench.rs): собирает Criterion targets, требует свежие samples, разделяет unavailable и feature-gated targets.
- [xtask perf](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/tools/xtask/src/perf.rs) и [widget scenarios](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/crates/flui-widgets/tests/perf.rs): детерминированные counts, не timings. Baseline включает full reassemble, idle, локальный text change, прокрутку 10k списка и slide transition.
- [Engine bench targets](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/crates/flui-engine/Cargo.toml): render/text throughput, offscreen resource cache и raster backpressure. [render_throughput](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/crates/flui-engine/benches/render_throughput.rs) содержит GPU encode/submit workload и отдельные CPU allocation/cache microbenchmarks. raster_backpressure использует NoOpBackend; text throughput имеет реальный GPU и ожидание завершения, shaping вынесен из timed loop. Ни один из этих путей сам по себе не доказывает native input-to-present latency.
- [Performance protocol](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/crates/flui-protocol/src/perf.rs) и [утверждённые требования driver](https://github.com/vanyastaff/flui/blob/9a5df98b49b25d267a1cf1d8db3c400bdd6492b7/docs/plans/specs/flui-driver/requirements.md#performance): распределения, input journal, per-target budgets, unknown refresh и cannot-verify. Наличие DTO не означает готовую native instrumentation.

Использовать эти механизмы и существующие testing kits. Не создавать второй frame driver, clock или протокол. Расширения implementation scope записываются в профильные specs; этот документ не ведёт requirement/task IDs.

## Участники и порядок подключения

| Группа | Участники | Зачем | Граница сравнения |
|---|---|---|---|
| Первая общая desktop-сцена | FLUI, Knots, egui, Dear ImGui | Воспроизводимая отправная точка с известным upstream workload | Одинаковый видимый результат и события; UI internals различаются |
| Retained и локальные изменения | FLUI, Raven; далее Rosin, Blinc, Tessera, WaterUI | Проверить цену invalidation, layout/placement и animation | Только подтверждённые возможности каждого adapter |
| Layout engines | FLUI, Rosin, Taffy, Yoga | Отделить стоимость solver от renderer/window | Общая семантика constraints, размеров и выравнивания; Taffy/Yoga не whole-app конкуренты |
| Effects и rendering | FLUI, Raven, Blinc, Tessera; затем Cherenkov/Skia/Vello | Цена blur/material/text и caching | Сначала UI-сцены; renderer-only corpus отдельным отчётом |
| Продуктовый путь | FLUI, WaterUI, Ply; затем Knots/Blinc | Build, edit-to-visible, startup, package footprint | Один небольшой editor/list application, одинаковые правки и состояние |

Rosin whole-app требует пригодного macOS пути; не эмулировать отсутствующую Linux/Windows поддержку. Knots включается в Linux Wayland cohort, не X11. Flutter/Slint/GPUI — возможное последующее расширение после первого полного отчёта, без обязательства поддерживать все адаптеры сразу.

Перед запуском у каждого участника фиксируются точный SHA/release, lockfiles, compiler, backend и licenses. Данные исторической страницы Knots воспроизводятся отдельной legacy-конфигурацией; новые версии всех проектов идут в новую конфигурацию. Нельзя сравнить сегодняшний FLUI с чужой старой таблицей как с новым прогоном.

## Сценарии

Ни один сценарий не включается в рейтинг без проверки результата. Число logical items, созданных nodes и реально видимых элементов записывать раздельно.

| Сценарий | Фиксированная нагрузка и изменения | Участники | Основная метрика и проверка |
|---|---|---|---|
| Большая сетка | 100/1k/10k/53k buttons и отдельно labels; 1600×1000; header 48px, sidebar 260px, grid cells 14px; общий набор текста/цветов | Первая desktop-группа, затем Raven | CPU process time / completed frame; screenshot, количество items и hit-test выбранных cells |
| Первое построение и layout | Wide: 1k/10k/50k leaves при глубине 4; deep: глубина 32/128/512 при 1k leaves; text-free и text-heavy раздельно | FLUI/Raven; solver subset FLUI/Rosin/Taffy/Yoga | First build и layout wall time раздельно; проверка rects, constraints, overflow и baselines |
| Локальная invalidation | Дерево 10k items: менять один label, 1% labels, только цвет, opacity, transform, ширину, alignment; размеры текста равные и меняющие перенос | FLUI/Raven, затем Rosin/Blinc/Tessera/WaterUI | CPU/update плюс counts внутри FLUI; верные pixels, hit-test, semantics и отсутствие stale state |
| Scroll и virtualization | 10k/100k строк; viewport 800×600; отдельные fixed-height 20px и variable-height 20–80px; scroll на экран, jump в конец, reverse | FLUI/egui/Raven/Knots, затем WaterUI/Ply | Native p95 input-to-present и resident memory; правильный видимый диапазон, focus и state после reorder |
| Animation / placement | 1/100/1000 объектов; 10 секунд фиксированной траектории; transform/opacity отдельно от изменения layout size | FLUI/Tessera/Blinc/Rosin, затем Raven | p99 completion interval, counts measure/layout; одна траектория без независимых spring defaults, правильные clipping/hit-test |
| Текст | ASCII отдельно; mixed Latin/Cyrillic, Arabic bidi, CJK, emoji fallback; 1k/10k glyphs; cold и warm font/atlas | FLUI/Rosin/WaterUI; Raven/Blinc/Ply после проверки поддержки; Knots только ASCII cohort | Shaping CPU и raster GPU отдельно; golden wrap/selection/ink checks; отсутствующие возможности UNSUPPORTED |
| Effects | 1/4/16 backdrop panels; 1280×720 и 1920×1080 physical pixels, scale 1/2 отдельными runs; blur sigma 4/12/24; static и moving background | FLUI/Raven/Blinc/Tessera; renderer cohort позже | GPU duration и memory при одинаковом результате; linear/sRGB, alpha, clip, blur footprint. Refraction — отдельный capability cohort |
| Raster и конкуренция | Тот же editor, resize, два окна; дозированная UI- и GPU-нагрузка отдельно; быстрые state updates, hide/close во время работы | Сначала FLUI baseline/candidate; Raven как внешний ориентир | Input p95/p99, queue occupancy/age, accepted/coalesced/presented counts, shutdown; итоговый state и frame generation корректны |
| Images/resources | 100 локальных изображений 1024×1024; последовательные и повторные запросы, 4 concurrent decodes, отмена половины; холодный/тёплый decoded cache | FLUI/WaterUI/Blinc, затем остальные с аналогичным API | Decode CPU, bytes copied, peak memory, input latency; одинаковые hashes, отсутствие stale delivery. HTTP только отдельным локальным сервером |
| Idle / startup / footprint | Один экран и 1k items; no-input idle 60 секунд; fresh-process startup; отдельные cached и cold asset режимы | Все готовые whole-app adapters | CPU%, wakeups/s, RSS/private bytes, GPU allocations, launch-to-first-correct-frame; idle не прекращает обработку следующего input |
| Developer iteration | Counter и editor; правки текста, цвета, функции и state shape; одинаковое содержательное изменение | FLUI/WaterUI/Ply/Knots/egui/Blinc по доступности | Cold build, warm rebuild, edit-to-visible, состояние после reload/restart, peak build RAM/disk, packaged bytes; проверка изменённого изображения |

### Два режима честного сравнения UI

1. **Одинаковая работа:** невиртуализированная grid-сцена, каждый logical item существует, одинаковая последовательность событий и viewport. Retained framework имеет право переиспользовать дерево; ему нельзя навязывать full reassemble ради имитации immediate API. Отдельный forced-rebuild stress маркируется как искусственная нагрузка и не заменяет основной результат.
2. **Рекомендуемый путь приложения:** virtualization, retained caching и штатные property bindings включены. Модель, видимое содержимое и скрипт событий одинаковы; сравнивается достигнутая пользовательская задача. В отчёте явно указано, какая работа была устранена.

ASCII-grid не доказывает скорость rich text. Нельзя сравнивать виртуализированные 30 видимых строк с 100k созданных widgets без отдельного режима. Число внутренних render nodes между frameworks не является общей единицей работы.

## Метрики и точки времени

| Метрика | Определение | Что нельзя подставлять вместо неё |
|---|---|---|
| CPU/frame | Process CPU delta на измеренное число completed frames; owner-thread CPU при доступности отдельно | Wall time или сумма CPU и GPU |
| Phase wall time | Build/layout/paint/encode/submit с явными границами и потоком; diagnostic instrumentation отдельным режимом | Сумма перекрывающихся фаз как полный frame time |
| GPU time | Реальные timestamp queries вокруг заявленного набора passes, с query availability/disjoint handling | Submit duration, оценка по FPS |
| Completion interval | Разница completion timestamp соседних кадров; тип сигнала и его верхняя граница указаны | Present-to-present или photon latency |
| Input latency | Journal/OS delivery → первый presentation, отражающий именно этот event/state; input/processing/presentation отдельно | Synthetic injection → submit без указания границ |
| Frame pacing | Refresh period, deadline misses и janky frames отдельно; dropped/coalesced не смешивать | Универсальное «60 FPS» при неизвестной частоте |
| Memory/energy | Steady/peak RSS или OS-equivalent, allocator bytes, VRAM раздельно; joules/frame при фиксированном Hz | RSS одной ОС как точный аналог private bytes другой; CPU% как энергия |
| Iteration/startup | От edit/save или process launch до указанного наблюдаемого результата | Успешное завершение compile как edit-to-visible |

Headless controlled-clock tests дают counts и корректность, не wall-clock latency. CPU solver microbench может использовать monotonic wall clock вне frame-driver и маркируется как отдельный microbenchmark. Offscreen GPU completion никогда не выдаётся за native present. Если OS present timestamp недоступен, публикуется input-to-submit/completion с именем границы, а input-to-present = UNAVAILABLE. Физическую photon latency измерять только отдельным устройством/камерой.

Существующий flui-driver контракт остаётся каноническим: пять итераций по умолчанию, median per-run percentile, per-target budgets и cannot-verify при недоступных данных. Дополнительные cross-framework статистические отчёты не меняют его verdict.

## Условия воспроизводимости и эквивалентности

- На одну сравнительную таблицу — одинаковая физическая машина, ОС, power mode, backend-class и размер поверхности. Windows/DX12, Linux/Wayland/Vulkan, macOS/Metal и browser/WebGPU — отдельные cohorts. Если Raven использует Vulkan/MoltenVK, это записывается и не называется одинаковым native backend.
- Первый host для FLUI instrumentation — доступная Windows-машина. Первое четырёхстороннее сравнение с Knots — отдельный Linux Wayland host. macOS нужен для Rosin whole-app; Android/Apple mobile подключаются позже, только с реальными устройствами.
- У каждой машины записать CPU/GPU, RAM, driver, OS/compiler, monitor Hz/DPI, viewport/physical resolution, affinity, clocks/governor, thermal state, power source. Не угадывать модели устройств или уже выполненные прогоны.
- Pin fonts/assets и их лицензии, locale, shaping corpus, seed и input trace. Default font/theme не считается одинаковой нагрузкой.
- Production release settings, target CPU, LTO, debug symbols, allocator и features фиксируются. Report как минимальный common-feature режим, так и рекомендуемые production defaults. Accessibility off/on и screen-reader attached — отдельные capabilities/runs.
- В benchmark process нет inspector, verbose logs и screenshot readback, если они не предмет сценария. Correctness capture проводится до/после timed span. Измерить overhead instrumentation отдельным A/A.
- Исключить сеть из runtime-корпуса, кроме явно названного resource-сценария; downloads отделены от cold build. Build caches изолированы, cached dependencies и truly fresh build имеют разные названия. Очистка касается только принадлежащих benchmark directories.
- Hot reload, resident watch и обычный rebuild публикуются разными строками; одинаковая правка должна менять поведение/изображение. State shape change проверяет restart semantics.
- Warm/cold glyphs, shader compilation и decoded assets фиксируются раздельно. Нельзя спрятать startup/JIT в warmup и затем назвать результат cold.
- Без общей text/effect семантики — capability report, не место в рейтинге. Общие layout rects проверять с заранее выбранным допуском 0,5 logical px; где pixel snapping отличается, допуск и причина фиксируются до timing. Image-error threshold задаётся отдельно для сцены и не разрешает пропавшие glyphs/effects.
- Копирование upstream harness/scene требует проверки лицензии и attribution. Если лицензия конкретного файла неясна, написать независимый adapter по описанию сценария, сохранив источник методики.

## Прогоны и статистика

1. Сначала feature/correctness parity и instrument validation. Намеренно лишний rebuild, задержка кадра и пропущенный результат должны обнаруживаться counters, timing и outcome checks соответственно; artificial control не становится оптимизацией framework.
2. Провести A/A одинакового бинарника, отдельный процесс на каждый повтор. До просмотра A/B зафиксировать primary metric каждого сценария, practical threshold и допустимый шум.
3. Для runtime: минимум 120 warmup frames, затем пять независимых повторов по 10 000 измеренных completed frames для заявлений о p99. Более короткий pilot разрешён, но маркируется exploratory без p99 gate. Cold start не имеет warmup; 30 отдельных launches в каждом из пяти блоков, без заявления OS cache eviction, если она не сделана.
4. Сравниваемые implementations запускать последовательно; чередовать порядок по фиксированному seed, для FLUI base/candidate использовать сбалансированные AB/BA блоки. Не запускать параллельные Cargo builds, GPU jobs и benchmarks.
5. Сохранять все frame samples, event IDs и process repeats. Публиковать median per-run mean/percentile с точным названием; не выдавать объединение миллионов коррелированных кадров за независимые эксперименты.
6. Для A/B рассчитывать paired bootstrap 95% interval по независимым блокам/запускам; frame-block resampling допускается с указанным размером блока. Пять повторов — минимум, не гарантия точности. Если интервал широк, итог INCONCLUSIVE; не повторять до первого выгодного p-value. Число дополнительных повторов фиксируется до нового batch.
7. Предлагаемый initial practical threshold для runtime CPU/latency: max(5% от baseline, 0,1 мс), для памяти 5%, для build/startup max(5%, 10 мс). Для sub-ms solver/kernel threshold задаётся отдельно после A/A и до A/B; общий 0,1 мс его скроет. Это кандидаты для утверждения, не текущие release gates.
8. Если весь интервал изменения хуже +threshold — REGRESSION; лучше −threshold — IMPROVEMENT; весь внутри ±threshold — EQUIVALENT WITHIN BUDGET; иначе INCONCLUSIVE. Feature failure/invalid samples не проходят статистическую проверку как быстрый результат. Не сводить множество разных метрик в один score.
9. Inter-run distributions, min/max и ухудшения публикуются вместе с median. Primary comparisons перечислены заранее; secondary findings отмечены exploratory, массовые comparisons требуют отдельной коррекции/подтверждения.
10. Hard product budgets привязываются к конкретному target и interaction. Периоды 16,67/8,33 мс объясняют 60/120 Hz, но сами по себе не назначают input latency и не доказывают no-drops. Принятые driver budgets имеют приоритет.

Для idle пять 60-секундных окон после одинакового settling, затем контрольное действие. Для energy минимум пять 60-секундных окон при фиксированном Hz, измеренный system baseline рядом; данные без power sensor = UNAVAILABLE. Для build/reload пять независимых повторов на класс правки с точным cache state; хвосты p99 по пяти build observations не публикуются.

## Артефакты одного запуска

Будущий harness должен выдавать версионированный manifest и отдельные данные:

- `manifest.json`: scenario/version, baseline/candidate SHA, adapter SHA, lock/toolchain hashes, build commands/settings, host/backend, feature-parity matrix, assets/seed, warmup/sample policy, заранее выбранные metrics/thresholds.
- `samples.jsonl`: run/frame/event IDs, clocks/units, CPU/GPU/phase/completion samples, unavailable reasons; source event → observed state mapping.
- `outcomes.json`: correctness, capability support, captures/checksums и причины исключений.
- `summary.json`: per-run distributions, intervals, comparison verdict и noise controls.
- `report.md`: итоговая таблица, scope, ограничения, команды воспроизведения, полный список плохих/недоступных результатов.

Большие traces и raw outputs — immutable CI/release artifacts с checksums и retention policy, не тысячи generated файлов в source tree. Manifest и исходники адаптеров должны позволять воспроизведение независимо от доступа к машине автора. Шаблон результата не содержит выдуманных нулей: NOT_RUN, UNSUPPORTED, UNAVAILABLE, INVALID, INCONCLUSIVE различаются.

## Последовательность реализации и область ответственности

| Порядок | Работа | Ответственная область | Критерий завершения |
|---|---|---|---|
| Начальная фиксация | Инвентарь capabilities, scenes, source pins, machine manifest; выбор primary metrics | testing/tooling и владельцы adapters | У каждой строки есть условия, correctness oracle и причина выбора peer; все результаты NOT_RUN |
| Базовый FLUI | Переиспользовать counted scenarios и Criterion; добавить timed common grid/local update | flui-widgets/testing, tools/xtask | Публичный consumer, A/A, полные samples; counts не переименованы в timings |
| Первое внешнее сравнение | Grid/labels 1k, 10k, 53k; FLUI/Knots/egui/ImGui на Linux; idle/memory | tooling + adapter owners | Один воспроизводимый отчёт с parity и ограничениями; худшие результаты сохранены |
| Retained/layout | Raven local-update/paint; Rosin/Taffy/Yoga solver subset, затем placement peers | rendering/view/widgets | Геометрия совпадает; cold/warm/local/full разделены; доступность платформ проверена |
| GPU и native latency | Effects/text corpus; timestamps/completion; native driver input/presentation | engine/app/native, существующий driver scope | Pixel correctness, source-correlated latency; отсутствие timestamp не маскируется estimate |
| Raster experiment | FLUI inline против candidate на тех же сценах; Raven внешний ориентир | app/engine/runtime | Ограниченная очередь, recovery/shutdown и p95/p99; ускорение не оплачено ростом input lag |
| Продуктовый отчёт | WaterUI/Ply/Blinc iteration, packaging; mobile energy после desktop | CLI/packages/platform | Одинаковые edits, сохранность state, правильный install/startup и artifact footprint |

Новые framework API не открывать только ради измерения: использовать публичный consumer и существующий testing seam. Детерминированные counters могут блокировать CI через существующий perf gate. Machine-dependent timings сначала advisory на закреплённом runner; публичное сравнение не измеряется на случайном общем CI host.

## Первые действия после согласования

1. Проверить версии FLUI и peers перед реализацией, записать общий workload и capability matrix; не изменять исторические upstream-результаты.
2. Выбрать доступный Windows target для instrumentation и Linux Wayland target для общей таблицы, зафиксировать baseline без чужих jobs.
3. Реализовать только первый вертикальный путь: одинаковая grid/labels сцена → correctness → samples → report, плюс FLUI local-update control.
4. Прогнать A/A и зафиксировать thresholds до A/B. Расширять adapters после подтверждения измерительного пути.
5. Обновить профильные specs доказательствами; публиковать сравнительные утверждения только после повторного запуска по инструкции.

Успех первой итерации — достоверный сравнительный отчёт, даже если FLUI проигрывает. Его результат должен определять следующую оптимизацию, а не подтверждать заранее выбранный лозунг.
