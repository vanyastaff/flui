# План развития FLUI на основе сравнительного исследования

Дата: 2 ноября 2026 года. База FLUI: `bb618a7f7f1510ff11093109094ba02c937d1970`.

Цель — усилить FLUI на уровне исполнения, рендеринга, компонентов, прикладных возможностей и инструментов, используя идеи, исходники и планы Raven, Blinc, Rosin, WaterUI, Ply, Knots и Tessera. Результатом каждого направления становится работающий пользовательский сценарий с доказательствами, а не только новый внутренний механизм.

Это план приоритетов и проверок. Утверждённые feature specs и ADR остаются источником контрактов. Новые архитектурные решения ниже — конкретные кандидаты для отдельной спецификации и эксперимента. План не разрешает публикацию, не меняет автоматически принятый scope и не объявляет чужие реализации быстрее или надёжнее FLUI.

Сопоставление основано на чтении кода и тестов; исполнения и замеров на этой базе в рамках подготовки плана не было. «Есть» означает найденный production-путь или API; успешный прогон указывается отдельно в feature-spec. Полный исходный разбор: [архитектурные и продуктовые уроки](research.ru.md).

## Что уже есть и что действительно нужно усилить

| Область | Подтверждённая исходная точка | Следующий результат |
|---|---|---|
| Владение UI | Один owner thread обслуживает UI runtimes; `!Send` state и изоляция runtime закреплены принятой ADR-0266 | Сохранить этот контракт, вынести подходящую тяжёлую работу за его пределы |
| Многопоточность | Native IO/compute-пулы и host executors; owned SceneSnapshot и raster protocol. Production RasterLane пока inline, Web compute выполняется последовательно | Проверенный dedicated raster mode на допустимых hosts; неблокирующие resource workloads и ясные fallback-режимы |
| Рендеринг | wgpu, glyph atlas, GPU blur, backdrop filters/grouping, shader masks, SDF clipping, effect/resource caches | Доступные приложению материалы и подтверждённые сценарии custom rendering; оптимизация по профилю |
| Layout и реактивность | Constraint layout, cached child geometry, signals в build, explicit transitions без rebuild | Измерить пользу более общих property bindings и разделения measurement/placement |
| Компоненты | Базовый widget catalog, Material/Cupertino, формы, scrolling, анимации, текстовый ввод | Связный набор для сложного desktop-приложения и завершённые визуальные возможности существующих каталогов |
| Текст и платформы | Shaping, editable text, IME и accessibility contracts; зрелость и прогоны различаются | Закрыть утверждённую приёмку реального ввода, focus и screen reader; развить прикладную локализацию |
| Приложение | Notes acceptance app с persistence/keyboard/teardown сценариями | Надёжный внешний consumer и публичный showcase из той же реализации |
| Инструменты | CLI, hot reload, headless testing, typed inspection и части runtime agent | Довести утверждённый flui-driver; затем использовать его для preview и связной визуальной диагностики |
| Доставка | Сборка нескольких target’ов; полноценная дистрибуция отмечена как unaudited | Воспроизводимая сборка и установка release-артефакта на выбранной платформе |

Ключевые источники: [актуальная модель UI threads](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/adr/ADR-0266-one-owner-thread-ui-runtimes-isolate.md), [production raster lane](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-app/src/app/raster_lane.rs#L1), [engine architecture](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-engine/ARCHITECTURE.md), [transitions](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-widgets/src/transitions/mod.rs#L1), [layout cache](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-rendering/src/pipeline/owner/subtree_arena.rs#L1034), [каталоги и примеры](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/examples/README.md), [документированные пробелы](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/gaps.md).

### Две существенные поправки к общему обзору

Owned SceneSnapshot означает готовую границу передачи данных, но сам по себе не означает параллельное выполнение raster. В текущем приложении `RasterOwner::pump` выполняется синхронно. ADR-0045 о dedicated raster остаётся Proposed. Более новая ADR-0266 также отменяет прежнюю идею параллельно исполняемых UI runtimes: изоляция state не гарантирует изоляцию времени кадра.

Inspector и унификация проверок уже спроектированы и утверждены в [flui-driver](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/flui-driver/tasks.md). Новый план должен ускорять и дополнять эту работу. Runtime jobs, transports, driver core, capture, CLI/MCP, reporting и Inspector имеют существующий граф зависимостей; отдельный новый протокол или inspector backend не нужен.

## Как заимствовать идеи и код

Для каждого кандидата сохраняется компактная карточка: боль, peer и точная ревизия, конкретный механизм/исходник, статус «реализован / пример / roadmap», аналог в FLUI, production consumer, цена интеграции, эксперимент и решение по его результату.

Есть четыре разных способа переноса:

1. **Прямое переиспользование зависимости**, если её API, сопровождение, лицензия и ownership подходят. Проверить public API closure и объём зависимостей.
2. **Адаптация алгоритма или части кода**, сохранив происхождение, условия лицензии и attribution. Тестировать наши lifecycle, arithmetic и platform cases, а не только upstream happy path.
3. **Перенос архитектурного принципа** в наши owner-local types и слои. Например, кластеризация paint work не требует глобального job pool из Raven.
4. **Перенос продуктового решения**: inspector, библиотека материалов, graph editor, ready-to-use forms, живые примеры или способ организации документации.

Roadmap других проектов — источник гипотез о будущих потребностях. Невыпущенная feature не считается проверенным решением. В наш план она попадает с задачей и consumer, а не из-за наличия строки у конкурента.

## Порядок выполнения

| Очерёдность | Направление | Результат, после которого можно двигаться дальше |
|---|---|---|
| Сначала | Сверка состояния и воспроизводимые нагрузки | Код, specs и известные прогоны сведены; baseline сохранён |
| Ближайший основной поток | Завершение текущих lifecycle, persistence, input и driver задач | Notes проходит полный публичный сценарий; driver даёт достоверные действия и наблюдения |
| Параллельный системный поток | Raster thread, загрузка ресурсов и фоновые вычисления | UI остаётся отзывчивым под тяжёлой работой; очередь, recovery и shutdown проверены |
| Следующий видимый результат | Материалы и сложные компоненты на одном editor consumer | Пользователь собирает выразительное рабочее окно без внутреннего engine API |
| После baseline и instrumentation | Property binding, placement и paint parallelism experiments | Измерен выигрыш; принят или отклонён конкретный механизм |
| По графу flui-driver | Диагностика, Inspector и preview | Один элемент связан с деревом, причиной работы, pixels и проверкой |
| По готовности consumer | Публикация framework и доставка приложения | Внешний проект и installable artifact проверены на точном SHA |
| При появлении host consumer | Embedding и расширяемый GPU pipeline | FLUI работает внутри чужого окна/render loop с полным lifecycle |

Это порядок зависимостей, а не обещание завершить всё до календарной даты. Утверждённый scope flui-driver сохраняется целиком. Эксперименты с новыми механизмами не становятся дополнительными релизными блокерами без отдельного решения владельца.

## Исходная точка и измерения

Дополнение от 2 ноября: [опубликованные результаты соседних проектов](benchmark-review.ru.md) и [конкретный план сравнительных бенчмарков](benchmark-plan.ru.md). В них раздельно определены CPU/layout, GPU/completion, native input latency и developer iteration; цифры upstream не считаются измерениями FLUI.

**Сделать:** сверить профильные `tasks.md`, текущий HEAD и тесты по направлениям этой таблицы. Общий [release/tasks.md](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/release/tasks.md) содержит устаревшие сведения от 5 октября: например, текущие acceptance fixtures и части driver уже присутствуют. Исправление реестра должно ссылаться на профильные доказательства; статус PR или checkbox не заменяет прогон.

Подготовить три фиксированных workload:

- Notes: текст, IME, focus, 10 000 строк, загрузка/сохранение, ошибка и Retry, restart.
- Визуальный editor: панели, большой canvas, zoom/pan, выделение, анимация и backdrop material.
- Resource stress: большие изображения, повторные запросы одного ресурса, отмена, resize, два окна, скрытие и закрытие во время работы.

Зафиксировать hardware, backend, build profile, viewport, scale, seed, dataset и SHA. Собирать build/layout/paint/raster/present, число измерений детей, input-to-visible latency, queue occupancy, память, cold start и idle wakeups. Достаточность числа повторов и бюджет фиксируются до сравнения кандидатов; существующие performance requirements flui-driver имеют приоритет.

**Готово, когда:** каждый вывод «нужна оптимизация» имеет baseline и воспроизводимую нагрузку; «не проверено» отделено от failure и PASS. Все предложения ниже имеют отмеченный consumer и владельца области.

## Надёжное приложение и единый driver

**Что усиливаем:** уже утверждённые [persistence](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/persistence/tasks.md), [teardown](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/teardown/tasks.md), [focus/keyboard](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/focus-keyboard/tasks.md), [text/IME](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/text-ime/tasks.md) и [flui-driver](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/flui-driver/tasks.md).

**Работы:**

- Довести owner jobs, synthetic input, agent server и driver core по принятому графу. Завершить миграцию старого devtools agent и front ends в предусмотренную архитектуру; старые paths удалять вместе с миграцией потребителей.
- Довести Notes через реальные storage/background/close пути. Сохранить одну реализацию приложения для внешнего consumer, учебного примера и acceptance.
- Связать headless, synthetic и native сценарии с одним отчётом, явно указывающим путь ввода. Нативные прогоны следуют готовности соответствующего driver lane.

**Проверка результата:** заметка, черновик, route и scroll переживают restart; ошибка записи не показывает ложный Saved; Retry применяет актуальное состояние; stale load не заменяет новые данные; close во время сохранения следует принятому контракту. Focus возвращается к правильному элементу, IME-композиция проходит путь поля и commit, screen-reader проверка выполняется отдельно от чтения accessibility tree. Panic, отмена и shutdown оставляют либо работоспособное следующее действие, либо установленный контрактом terminal exit.

**Владельцы:** `flui-view`, `flui-runtime`, `flui-app`, `flui-native`, widget/catalog owners, `flui-protocol` и будущий driver core. Изменения shared files планируются по существующим feature specs.

## Многопоточность и ресурсы

### Dedicated raster как первый кандидат

**Идея peer:** изучить Raven render owner, очередь и передачу paint commands. **Точка FLUI:** inline raster уже принимает owned SceneSnapshot; есть proposed raster ADR и mailbox protocol. Поток исполнения можно менять через эту границу, не делая UI state общим между потоками.

**Порядок:**

1. Проверить актуальные surface, present и OS-thread ограничения для закреплённых wgpu/backend версий; документировать допустимые hosts и inline fallback.
2. Реализовать эксперимент dedicated owner за существующим protocol на одном допустимом desktop backend; сравнить с inline на одинаковых нагрузках.
3. Проверить production pacing: UI не должен производить бесконечную очередь, пока raster медленнее. Telemetry может теряться по контракту; control/recovery/shutdown — имеют отдельную судьбу.
4. При подтверждённой пользе завершить решение ADR-0045, мигрировать runners и проверить каждый затронутый backend. Публичное приложение сохраняет owner-local интерфейс.

**Условия принятия:** input обслуживается во время занятости raster; нет UI join/wait на обычном submit; память и количество in-flight работ ограничены; старое surface generation отвергается; resize, device loss, pause/resume и shutdown не теряют control obligations. Результаты чтения пикселей одинаковы в пределах существующего допуска. Изменения p95/p99 latency и throughput подтверждены измерениями; рост среднего FPS при ухудшении интерактивности не считается успехом.

**Отдельная ветка:** параллельная запись paint возможна только для доказанно независимой работы и требует отдельного ADR по ADR-0027 §10: независимые subtrees, отсутствие process-wide text-shaping lock, измерения на deep tree, wide list и realistic text. User callbacks, mutable render objects и UI borrows не переносятся в workers автоматически. Parallel paint, build/layout и thread-per-runtime не включаются под видом включения raster thread; каждое изменение проходит собственное решение относительно ADR-0266/ADR-0027.

**Что именно читать у Raven:** owner-thread API check в [context.c](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/core/context.c#L23); разбиение paint на jobs и восстановление порядка в [paint.c](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/widget/paint.c#L121); helping wait в [jobs.c](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/core/jobs.c#L152); render command ownership в [renderer.c](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/render/renderer.c#L89). У него widgets/layout mutations остаются на UI owner; paint chunks с UI callbacks тоже остаются там. Job queue растёт, render command queue не имеет общего лимита в submit, а producer throttling блокирует при лимите frames. Полезную декомпозицию адаптируем к ограниченным lanes FLUI и nonblocking owner contract. Числовые пороги разбиения и ограничения кадров не являются переносимыми константами производительности.

### Загрузка без скрытого блокирования

В [gaps](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/gaps.md#L467) уже описаны три конкретные задачи: Web compute выполняется inline; одинаковые изображения декодируются повторно; network image зависит от Tokio reactor конкретной IO lane.

**Работы:** Web worker strategy с явными требованиями к deployment и fallback; bounded decoded-image cache у правильного runtime owner с dedup in-flight; platform HTTP capability, которая не требует от приложения собственного reactor. Сначала воспроизвести каждый наблюдаемый недостаток через публичный путь.

**Критерии:** large decode не блокирует input/frame на заявленном background target; два одновременных запроса одного ресурса используют одну работу; исчезновение одного consumer не отменяет нужную другому; retired runtime не принимает старый результат; eviction держит бюджет; HTTP image работает в headless и host-executor сценариях. Отсутствие capability не маскируется как доказанная фоновая обработка.

## GPU возможности и визуальные компоненты

**Что изучать:** Blinc glass/refraction и material shaders; Raven backdrop effects; Tessera typed pipeline lifecycle; Knots host-owned render pass. Для каждого выбранного эффекта сравнить алгоритм, samples/passes, bounds, cache keys, colour space и recovery.

FLUI уже имеет blur, BackdropFilter/BackdropGroup и SDF clipping. Конкретная незавершённость: [CupertinoNavigationBar](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/packages/flui-cupertino/src/nav_bar.rs#L40) не использует translucent blur, а нужные widgets ещё не доступны через SDK этой package. Начальный перенос — довести существующую возможность до production catalog consumer.

**Порядок:**

1. Доступность текущих effects через допустимый SDK и один законченный компонент, например translucent navigation surface.
2. Higher-level material с параметрами tint/blur/contrast и доступным opaque fallback. Refraction/noise вводить только после сравнения с существующим effect pipeline.
3. Один внешний custom effect через типизированный extension contract, если существующие paint primitives его не выражают. Не публиковать произвольный wgpu API до решения API-closure и lifecycle.

**Критерии:** GPU readback различает исходный фон, blur, tint и clip; overlap и backdrop grouping корректны; resize/DPI/transform не меняют область эффекта ошибочно; pooled textures не протекают между кадрами; reduced transparency/high contrast дают понятный fallback; профили показывают количество passes, texture memory и цену кадра. Новый render object получает harness row, а код компонента работает через SDK.

**Исходники-кандидаты:** [Blinc glass.wgsl](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_gpu/src/shaders/glass.wgsl) и [resource setup](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_gpu/src/renderer/glass.rs); [Raven backdrop capture](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/render/vulkan/vk_glass.c#L234); [WaterUI Filtrate contracts](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/graphics/filtrate/core/src/lib.rs). У Filtrate полезно различие color и spatial effects, footprint области чтения, animated parameters и reference executor. Это направление развития существующего `ImageFilter`, а не второй filter engine. Первая проверка — blur→tint на прозрачном cropped source: optimized и reference paths совпадают, damage расширен на footprint, изменение uniform не пересобирает pipeline.

## Прикладные компоненты и функции

**Consumer:** после Notes использовать небольшой editor как второй тип нагрузки. Он проверяет функции, которые counter и формы не раскрывают: workspace layout, canvas, selection, масштабирование, инструменты и продолжительные жесты. Это может быть отдельный демонстрационный graph editor; миграция Nebula или другого продукта автоматически не предполагается.

**Порядок каталога:**

1. Изучить и сопоставить имеющиеся split panes, resize handles, tree/list selection и workspace state. Закрывать недостающее поверх текущих layout/input/focus механизмов.
2. Docking с устойчивой identity и восстановлением layout, если он нужен выбранному editor. Reorder, hide/remove, перенос панели и смена scale должны сохранять корректный focus и state.
3. Node graph: zoom/pan, selection, typed node/port identity, hit-testing связей, keyboard alternatives и undoable operations. Graph algorithm и document model принадлежат приложению, reusable interaction/layout — framework или официальному package по текущим правилам слоёв.
4. Timeline, charts и media controls добавлять следующими конкретными consumers. Не объединять их в новый универсальный engine до появления общей ответственности.

Raven полезен реализацией editor widgets; Blinc — готовыми interaction compositions и themes; WaterUI — прикладными controls и reactive projections; Ply — небольшим набором базовых понятий и непосредственным доступом к результату.

**Критерии:** один полезный editor работает через публичную поверхность; удаление/перестановка сохраняет identity нужных объектов; drag cancel не оставляет capture; закрытие панели освобождает subscriptions; операции доступны с клавиатуры и отражены в semantics. Сериализация session не сохраняет runtime handles. Пример, документация и тест используют одну реализацию компонентов.

При scoped-поиске в текущих crates/packages готовые Dock/SplitPane/TreeView/NodeGraph/CodeEditor-компоненты не обнаружены; primitives для их композиции существуют. Devtools Timeline — запись профиля, не редактор временной шкалы. Сначала инвентарь и consumer, затем публичные типы.

**Docking:** изучить [Raven dock_area.c](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/widgets/dock_area.c#L1152) и [tear/redock tests](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/tests/test_dock_tearing.c). Перенос панели в отдельное окно — следующая задача после docking внутри окна: edit/IME, focus, DPI, shared document и close имеют отдельную приёмку. Ремаунт presentation не должен означать потерю domain state или двойной dispose.

**Graph и timeline:** [Raven user link](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/widgets/node_graph.c#L1284) разделяет совместимость pins и domain veto приложения, после callback проверяет liveness повторно; [timeline snapping](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/widgets/timeline.c#L452) переводит экранный порог в время с учётом zoom. У FLUI один drag должен давать одну undo transaction, stale/rejected connect не менять модель, а snap distance оставаться одинаковым в экранных единицах.

### Проекции форм и property editor

Высокий приоритет внутри editor-направления: [WaterUI projected form](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/examples/form/src/lib.rs) связывает поля aggregate state с controls и live theme. У FLUI `Form` и `FormField<T>` уже есть. Сначала сравнить ручную типизированную projection поверх существующего lifecycle, затем derive после двух реальных consumers — например, settings и property editor.

**Проверка:** последовательные изменения двух полей не теряют первое; перестановка controls не переносит field state; validation/reset/save имеют один owner; reentrant field update не пишет устаревшую копию всей структуры. Тип `Color` может получать цветовой control, но domain validation остаётся у приложения. Это уменьшение binding boilerplate с сохранением сильной типизации.

### Media и native content

Среднесрочная ветка после появления потребителя: изучить разделение [WaterUI PlaybackSession и controls](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/components/multimedia/video/src/session.rs). FLUI `Texture` и external registry дают кадры, но не готовый player. Начальный контракт — source, play/pause, seek, phase, failure и capabilities; decoder, audio sync и platform integration имеют собственную реализацию. Критерии: поздний seek не меняет новый source, disposal останавливает работу, pause не расходует decode впустую, unsupported путь явно отказан.

Карты/WebView/native video рассматриваются после одного настоящего hosted-view consumer. У FLUI `PlatformViewLayer` не доказывает готовую host integration; изучить [WaterUI placement contract](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/backends/hydrolysis/src/platform_view.rs). Проверяются clip, z-order, scroll, overlays, focus/IME и removal внутри callback. Готовые charts в этой исследовательской итерации не проверялись: до обещания их в release нужна отдельная карточка задачи и consumer.

## Реактивность, layout, animation и локализация

**Property updates:** сравнить Blinc effect metadata и Rosin phase dependencies с текущими signals и explicit transitions. Проверить цвет, opacity, transform, alignment, размер, текст и reorder. Расширять typed bindings, если они снимают реальные rebuilds или обязанности consumer; не вводить глобальный reactive registry.

**Placement:** сравнить Tessera с cached child geometry FLUI. Условие принятия — уменьшение измерений и стоимости workload при корректных intrinsic/baseline, hit-test и semantics. Отсутствие выигрыша означает сохранение текущей формы, а не неудачу проекта.

**Animation и темы:** изучить state transitions, choreography и semantic motion/shape roles Blinc. У FLUI уже есть springs, transitions и catalog motion; цель — согласованный прикладной контракт interruption, retargeting, reduced motion и theme switching. Прерванная анимация не прыгает в старое начало, замена owner прекращает callbacks, смена темы сохраняет состояние.

**Локализация:** исследовать Rosin Fluent/reactive arguments как кандидат поверх текущих Localizations. Сначала реальный consumer с plural/select и runtime locale switching; затем решение о зависимости или adapter. Критерии: смена locale/arguments обновляет нужный текст, не ломает selection/focus, кеш учитывает locale и revision, fallback и missing message имеют определённое поведение. Реальное shaping/bidi/font fallback проверяется отдельно от правильного перевода строки.

Исходники: [Blinc semantic motion tokens](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_theme/src/tokens/animation.rs), [typed state transitions](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_core/src/fsm.rs), [Rosin localization](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/src/localization.rs). Первый motion consumer переключает согласованную политику button/menu/sheet/navigation без настройки четырёх controllers. Первый localization consumer проверяет «1/2/5 документов», en/ru/ar, смену аргумента, missing key и reload перевода. Hierarchical/parallel states у Blinc помечены будущим развитием, а Rosin не даёт полного locale negotiation/fallback: эти части проектируем отдельно, не приписываем их существующему коду.

## Как использовать чужие планы развития

В [roadmap Blinc](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/README.md#roadmap) есть новые controls, DSL/live editing, accessibility и visual inspector; [Rosin](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/README.md) отмечает расширение widgets, платформ, introspection и UI fuzzing. Эти планы полезны для поиска повторяющихся пользовательских болей и будущих точек сравнения.

Раз в завершённый milestone обновлять только отобранные карточки: что upstream реализовал, как изменился contract, какие новые tests/consumer появились и изменило ли это нашу оценку. Кандидат повышается в приоритете, если появился наш реальный сценарий и уменьшилась стоимость интеграции. Число чужих stars, размер roadmap или появление shader сами по себе не меняют порядок FLUI.

## Инструменты и опыт разработки

**Принятый путь:** typed Inspect → runtime jobs/transports → driver → capture/report/CLI → Inspector. Работы source locations, tracing, layout facts и budgets уже описаны в flui-driver. Inspector является приложением-потребителем driver; собственную вторую модель наблюдаемого UI он не заводит.

**Критерии:** выбор элемента связывает widget/render/semantics/layer через typed identity; видны constraints, размер, clipping и причина работы; stale selection после unmount выдаёт определённый результат. Инспекция с пользовательским callback не держит опасный guard и не ломает следующее действие при panic. Production build соблюдает принятую политику отсутствия dev endpoint.

**Дополнение:** preview одного экрана через существующий runtime, rendered capture и управляемые environment/time. CLI-синтаксис ещё не утверждён. Сначала сравнить отдельную команду preview с preset/target существующего driver; выбрать вариант с меньшей новой ответственностью.

**Критерии preview:** один screen consumer используется в приложении и проверке, нет preview-only UI engine; frame/scale/theme/locale и capture source записаны; semantic тест не выдаётся за проверку pixels. Ошибка build не уничтожает последний пригодный preview.

**Hot reload:** сохранить текущие сильные стороны host/types/logic split. Из Rosin/Raven перенять явную классификацию правок: styling, совместимый code reload, несовместимый state. Для каждой определить focus, selection, scroll, draft и in-flight work; restart/отказ отображать явно.

## Доставка и продвижение

Продолжить existing release scope: внешний consumer по registry/staging artifacts, корректный package graph, docs.rs и повторяемый tutorial. App distribution — отдельный результат: release package, identity, permissions, установка и удаление на чистой машине. Сертификаты и publisher credentials становятся внешней зависимостью только на шаге подписи; их отсутствие не означает успешно подписанный релиз.

Для первого полного native маршрута разумно сохранить Windows из существующего решения release scope; уточнение матрицы выполняется при сверке актуальных specs. Код и проверки остальных target’ов продолжаются по утверждённому графу. Поддержку каждой платформы заявлять по её собственному датированному прогону.

После каждого завершённого результата публиковать материал, который его воспроизводит: raster latency до/после на workload; editor с docking; native IME-путь; диагностика лишнего rebuild; восстановление несохранённого документа. Каждая демонстрация содержит source, SHA, platform, build recipe и известные ограничения. Это последовательное накопление доказательств полезности FLUI.

## Общий критерий завершения любой задачи

- Есть production consumer и наблюдаемое снятие обязанности с него.
- Новое поведение проверяется публичным test; для исправления сохранён RED без production fix. Кейс различает правильное и сломанное поведение.
- Проверены применимые reentrancy, panic/Drop, stale identity, cancellation, full queue, retirement, числовые пределы и следующая операция после сбоя.
- Изменены все затронутые features, packages, examples и tooling; устаревшие consumers мигрированы, старый путь удалён либо имеет согласованный срок удаления.
- Пройдены scoped проверки по правилам репозитория: `cargo fmt --all`, clippy/nextest через `cargo heli` для изменённых crates и зависимых при public API change; doctests отдельно, GPU readback в один поток, native runs на соответствующих устройствах. CI выполняет Linux/wasm32; compile не заменяет native run.
- В профильном `tasks.md` записаны SHA, команды, результаты и ограничения; public-facing изменения имеют changelog fragment. Platform parity обновляется только в соответствии с фактическими прогонами.

## Первые конкретные действия

1. Сверить статусы release и профильных specs по текущему коду, особенно driver, persistence, focus и IME; сохранить единый реестр доказательств в их существующих `tasks.md`.
2. Зафиксировать три workload и измерить inline raster/resource baseline. Это вход и для системных оптимизаций, и для публичных performance claims.
3. Продолжить готовые по зависимости driver/lifecycle задачи; параллельно оформить scoped эксперимент dedicated raster через существующий protocol.
4. Оформить consumer-driven работу над translucent catalog surface через существующий BackdropFilter и SDK.
5. Уточнить второй consumer — небольшой editor — и составить инвентарь необходимых workspace/graph компонентов относительно текущего widget catalog.
6. По результатам source studies выбрать следующий один алгоритм или API для переноса: property binding, paint partitioning, reactive localization либо typed custom effect. Остальные сохраняют мотивированную позицию в каталоге кандидатов.

План будет успешен, когда каждое выбранное заимствование превратится в более отзывчивое приложение, более выразительный компонент или более простой проверяемый путь разработки — с измерениями и реальными consumers.
