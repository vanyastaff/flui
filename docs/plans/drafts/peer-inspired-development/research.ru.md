# Архитектурные и продуктовые уроки для FLUI

2 ноября 2026 года. Разбор Raven, Blinc, Rosin, WaterUI, Ply, Knots и Tessera относительно FLUI `bb618a7f7f1510ff11093109094ba02c937d1970`.

У этих проектов полезно перенимать способ превращать сложность GUI в понятную возможность для автора приложения. Сильное решение снимает с потребителя конкретную обязанность: управлять лишней пересборкой, связывать несколько платформенных проектов, искать причину неверного размера, восстанавливать состояние после правки или встраивать второй event loop. Наличие более короткого примера или красивой демонстрации само по себе этого не доказывает.

Для FLUI наиболее обоснованы четыре направления: законченный путь разработки и доставки приложения; визуальная диагностика поверх существующих наблюдений; согласованный preview и testing; измеримый контроль цены изменений UI. Embedding и расширение GPU-пайплайнов заслуживают отдельных экспериментов с реальными потребителями. Все семь чужих архитектур внедрять не требуется.

Основа сравнения — исходники, существующие тесты, документация и авторские анонсы. Тесты и приложения в этом разборе не запускались: это исследование механизмов и возможностей, без сравнительных замеров скорости и без сертификата платформенной готовности. Там, где автор не объяснял исходную мотивацию, боль сформулирована как наша интерпретация продукта.

## Как читать сравнение

У каждого проекта рассматриваются пять вещей: пользовательская боль, механизм решения, потребитель этого механизма, цена решения и применимость к FLUI. Новизна оценивается по полезному отличию, а не возрасту GitHub-репозитория.

| Проект | Боль или продуктовая задача | Сильное решение | Основной урок для FLUI |
|---|---|---|---|
| Raven | Собирать desktop-инструмент приходится из разрозненных toolkit, designer и preview | Designer и приложение используют общий документ, схему и живые widgets | Инспекция и preview должны видеть настоящее приложение |
| Blinc | Много ручной работы между состоянием и законченным визуальным интерфейсом | Изменения свойств несут информацию о нужной работе pipeline | Стоимость изменения должна определяться в одном месте |
| Rosin | Редактору нужны быстрые изменения UI и короткий цикл разработки | Зависимости разделены по фазам; reload имеет несколько режимов | Точно объяснять invalidation и сохранение состояния |
| WaterUI | Декларативный UI и платформенная интеграция требуют слишком много обвязки | Общие semantic actions, тесты, preview и инструменты; CLI владеет backend-проектами | Одна модель должна обслуживать несколько пользовательских путей |
| Ply | Простой экран требует слишком многих понятий и действий | Небольшой набор конструкций, доступный inspector, живые примеры | Измерять путь до полезного результата |
| Knots | UI нужно соединить с приложением, которое уже владеет GPU и окном | Независимый packet и разделённые prepare/encode | Проверять расширение настоящим внешним host |
| Tessera | Перемещение и нестандартные эффекты требуют лишней работы или изменения renderer | Placement отдельно от measurement; типизированные pipelines | Разделять работу только там, где это даёт измеримый выигрыш |

## Raven

### Общая модель для автора приложения и инструментов

Наша интерпретация боли Raven: разработчику desktop-редактора нужен целый рабочий набор — widgets, docking, темы, документ интерфейса, preview и редактор свойств. Если инструменты моделируют UI отдельно от runtime, они постепенно расходятся с тем, что увидит пользователь.

Raven делает designer потребителем своего публичного API. `Rxml_document` представляет редактируемый документ; `Rxml_get_tag_*` и `Rxml_get_attribute_*` дают схему для палитры и property editor; `Rwidget_pick` и `Rwidget_get_xml_node` связывают живой widget с документом. Сам designer написан на Raven. Это конкретный потребитель механизма, а не только описание будущей архитектуры. [Designer](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/tools/designer/main.c), [публичный API](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/include/raven/raven_xml.h).

При изменении XML runtime сопоставляет существующие widgets: сначала по `Name`, затем по совпадающему поддереву, затем по tag и порядку. Он применяет изменившиеся attributes, сохраняя неизменённые widgets. Это позволяет не терять часть локального состояния при визуальной правке. Но смена типа, отдельные изменения attributes и замена nodes имеют другую судьбу; публичный контракт требует обновлять handles заменённых widgets. [Алгоритм patch](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/src/xml/xml_watch.c#L331-L438).

Цена подхода — runtime-схема, правила сопоставления и синхронизация source document с живым деревом. В standalone designer custom C tags могут заменяться stand-ins, так что равенство preview и приложения имеет границы. Красивый Bevy editor тоже нельзя считать доказательством готовой GPU-интеграции: его README прямо отмечает, что Bevy backend не был скомпилирован в среде создания, а передача изображения устроена через readback и обновление текстуры. [Stand-ins](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/tools/designer/main.c#L613-L695), [Bevy example](https://github.com/oscar-soirey/Raven/blob/2bb1a8c0f457b89e867c8a3e6a89b3bb9b412981/examples/bevy_editor/README.md).

**Применение к FLUI.** Связать runtime identity, author identifiers, diagnostics и source location, чтобы выбор элемента на экране объяснял его устройство. Динамическая identity остаётся типизированной; source location служит навигации. Первый полезный результат — inspector и preview существующего Rust UI. Новый XML DSL и большой визуальный designer пока не имеют достаточного обоснования.

**Проверка ценности:** пользователь редактирует оформление поля с введённым текстом; preview отражает изменение, а focus, selection и scroll сохраняются согласно явному контракту. При замене несовместимого элемента инструмент сообщает, что пересоздано.

## Blinc

### Стоимость изменения свойства известна системе

Blinc объединяет декларативный builder, signals, анимации, GPU-эффекты и библиотеку компонентов. Наша интерпретация: он сокращает число самостоятельных решений между первой моделью состояния и выразительным экраном. Галерея и music-player example демонстрируют этот продуктовый путь; они не доказывают независимое production adoption. [Введение](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/docs/book/src/introduction.md), [music player](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/examples/blinc_app_examples/examples/music_player.rs).

Особенно полезен `PropertyId::side_effects()`: изменения различают visual, layout, text и другие последствия. После пачки обновлений нужная работа объединяется. Интеграционные тесты отдельно проверяют opacity-only update, width и изменение текста с повторным измерением. Общий формат метаданных позволяет runner объединять последствия пачки, а `PropertyId::side_effects()` задаёт стандартную классификацию. Однако `PartialPropertyUpdate` принимает и явно заданное поле `effects`, которое тесты заполняют вручную: эти проверки не доказывают автоматическое применение классификации всеми производителями updates. [Классификация](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_layout/src/property.rs), [тесты обновлений](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_layout/tests/property_updates.rs).

`reactive_text` умеет обновлять bound text без пересборки окружающего дерева. При этом унификация не полная: соседний `reactive_node` документирует, что `Computed` в этом пути читается однократно. Чем больше способов обновлять дерево, тем важнее одинаковые и понятные правила для приложения. [Реализация и ограничения](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/crates/blinc_cn/src/reactive_props.rs).

Глобальные reactive registry, синхронизированные очереди и spring-примеры с `Arc` и locks не подходят owner-local модели FLUI. Переносить следует распределение ответственности за последствия mutation, сохраняя наши правила владения. [Reactive architecture](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/docs/book/src/architecture/reactive-state.md), [spring API](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/docs/book/src/animation/springs.md).

**Применение к FLUI.** Исследовать, можно ли распространить удобство существующих специализированных render subscriptions на обычные bound properties без второй reactive-системы. Сначала сравнить API и фактическую работу для цвета, opacity, размера и текста.

**Проверка ценности:** изменение цвета не вызывает ненужный build/layout, текст корректно переизмеряется, смесь обновлений не теряет нужную инвалидизацию, retired owner не принимает запоздалое обновление. Критерий — и меньше работы, и меньше обязанностей у автора приложения.

## Rosin

### Реактивность учитывает фазу чтения

Rosin строится для Sailbrush, редактора 2D-анимации и compositing. Это явно названный потребитель, который задаёт реальные требования к iteration speed, layout и native desktop API; зрелость самого Sailbrush здесь не проверялась. Готовая платформенная интеграция пока только macOS. [README](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/README.md).

`Viewport` хранит отдельные dependency maps для build, style, layout и draw. Чтения записываются вокруг конкретной фазы, затем изменение выбирает нужную работу. Данные, от которых зависит только рисование, не обязаны запускать build. Реализация также допускает дополнительный style/layout, когда после build изменились реальные hover bounds: механика не обещает один проход при любых обстоятельствах. [Выбор фазы](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/src/viewport.rs#L1109), [layout scope](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/src/viewport.rs#L1250).

Второй полезный контракт касается hot reload. Совместимый type hash позволяет заменить библиотеку и callbacks; несовместимая форма приводит к сохранению state и описаний окон, затем перезапуску. Пользовательское обещание определяется классом правки. Этот подход стоит изучать отдельно от безопасности конкретного ABI-механизма. [Reload и restart](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin/src/mac/hot.rs#L235).

У reactive scopes есть unwind cleanup: снимается scope и продолжается исходный panic. Запись обновляет версию и освобождает guard перед уведомлением. Это полезный образец конкретных invariants, близких правилам FLUI. Но `Var<T>` использует глобальный registry, `Send + Sync` и `RwLock`; такое хранение переносить не следует. [Reactive implementation](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/src/reactive.rs), [scope tests](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/src/tests/reactive.rs).

**Применение к FLUI.** Показывать причины и минимально необходимые фазы работы, а у hot reload публиковать понятную таблицу: что сохраняется при изменении оформления, кода и формы state. Phase-aware reactive engine — только один из вариантов; специализированные owner-local bindings могут решить нужные сценарии проще.

**Проверка ценности:** изменение draw-only данных, layout данных и структуры даёт разные счётчики; после panic следующий update работает; reload не теряет состояние молча и не применяет результат старой операции к новому owner.

## WaterUI

### Семантика становится общим интерфейсом инструментов

В авторском анонсе мотив прямо назван: декларативность и типобезопасность SwiftUI за пределами Apple, fine-grained updates и native look-and-feel. Отсюда смешанные backends — системные widgets и собственный renderer. Эта продуктовая цель отличается от FLUI и не требует менять нашу базовую модель. [Анонс автора](https://users.rust-lang.org/t/waterui-a-swiftui-inspired-cross-platform-ui-framework-for-rust-with-cross-platform-native-rendering/133717).

`Query<R>` ищет узлы по role, label и identifier. Семантические операции вызывают accessibility actions, а не напрямую callback приложения. Геометрические операции доступны только у runtime с layout; semantic-only тест не может через тот же тип притвориться проверкой hit-testing. Handles проверяют ревизию дерева, а драйвер умеет управляемое время. [Query](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/testing/src/query.rs), [action dispatch](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/testing/src/app.rs#L1643), [driver](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/testing/src/driver.rs).

Эта модель обслуживает тестирование, preview и инструменты. Собственный Inspector написан на WaterUI; sidebar tests выбирают строки и проверяют detail view. При этом один сценарий использует координаты из-за отсутствующей семантики back control — полезное напоминание, что сильная архитектура не равна полному покрытию всех widgets. [Inspector](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/components/devtools/inspector/app/src/ui/mod.rs), [его tests](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/components/devtools/inspector/app/tests/sidebar.rs).

CLI концентрирует генерацию backend-проектов, разрешение toolchain, сборку и упаковку; пользователь описывает приложение в Rust и manifest. Preview запускает настоящий generated binary и проверяет результат. Стоимость этого удобства переходит к сопровождающим framework: им принадлежат platform generation, assets, signing и package lifecycle. [CLI](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/cli/README.md), [package](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/cli/src/terminal/commands/package.rs), [preview](https://github.com/water-rs/waterui/blob/06cb3f31f871084c8a49cf1a88dd14f902c1fcc6/cli/src/preview/run.rs).

**Применение к FLUI.** Соединить существующие semantics, testing, agent endpoint и devtools в путь «экран → preview → проверка → диагностика». Author identifiers должны помогать всем инструментам, а tests сохранять различие semantic action, pointer dispatch, GPU output и настоящего OS accessibility bridge.

**Проверка ценности:** один экран без специальной тестовой копии запускается в приложении, находится инструментом по identifier, проверяется headless-сценарием и снимается в rendered preview. Semantic success не засчитывается как доказательство поведения screen reader.

## Ply

### Первый полезный экран важнее количества возможностей

Автор хотел сделать multiplayer board game и столкнулся с boilerplate, глубокими ECS queries и многословной раскладкой. Поэтому короткий путь до приложения — исходная задача Ply. [Авторский анонс](https://users.rust-lang.org/t/ply-1-0-a-dead-simple-cross-platform-app-engine-with-live-wasm-docs/138740).

Небольшой набор понятий — element, sizing, layout, children, text — подкреплён состояниями builder `NoId`, `WithId`, `Detached`. Inspector включается обычной публичной настройкой и показывает выбранный элемент, layout и свойства на основании сохранённой раскладки. Живой Todo-пример соединяет ввод, состояние, добавление, удаление и scrolling. [Builder и debug mode](https://github.com/TheRedDeveloper/ply-engine/blob/52be132d71f8f6af3f55c6b27a584636f71cddf3/src/lib.rs), [inspector](https://github.com/TheRedDeveloper/ply-engine/blob/52be132d71f8f6af3f55c6b27a584636f71cddf3/src/engine.rs#L9197), [живые примеры](https://plyx.iz.rs/examples/).

Цена отличается от FLUI: frame loop, async show, собственный fork macroquad и inspector внутри engine. Краткость примера не является основанием перенести эти решения в синхронный on-demand pipeline. Состояние Todo в thread-local и identity строк по индексам также не становятся подходящим примером для нашего owner-scoped state.

**Применение к FLUI.** Обычный автор приложения должен решать задачу, не изучая crate DAG. Следует сравнивать количество необходимых решений, setup-правок и обращений к внутреннему source. Типы, защищающие lifecycle, сохраняют ценность; их сложность нужно оценивать на реальном сценарии, а не по числу строк counter.

**Проверка ценности:** новый разработчик создаёт editable list с validation, находит причину overflow в inspector, добавляет тест и получает распространяемый артефакт. Фиксируются время, ручные действия и точки, где документации не хватило.

## Knots

### Расширяемость подтверждается чужим владельцем ресурсов

Knots возник из MPV watch-party клиента Shaven. В раннем анонсе автор фокусировался на standalone desktop apps и даже исключал встраивание в чужой renderer из целей. В текущем коде такой consumer уже есть: это показывает, как изменение реального сценария меняет полезную границу библиотеки. [Анонс автора](https://ziggit.dev/t/knots-a-library-for-building-guis-with-zig/15244).

`ui.Context` производит независимый от graphics API `render.Packet`. Независимость не абсолютна: packet задаёт протокол geometry, glyphs, clipping и shaders. `validateExtensions` отказывает неподдерживаемым extensions до интерпретации erased data. `Painter.prepare` и `Painter.encode` отделяют подготовку UI от записи в render pass владельца. [Packet](https://github.com/knots-ui/knots/blob/a6b840794194fa58fb0aaf5b34d3a86d48cbcc6b/src/render/Packet.zig), [contract](https://github.com/knots-ui/knots/blob/a6b840794194fa58fb0aaf5b34d3a86d48cbcc6b/src/render/Contract.zig).

Embedded example записывает один подготовленный UI в offscreen и onscreen targets, оставляя submission и presentation у host. Consumer tests проверяют, в частности, stale frame handles и abort незавершённого frame. Цена — borrowed packet data и обязанности host по lifetime GPU-ресурсов. Это не замена owned cross-thread snapshot. [Consumer](https://github.com/knots-ui/knots/blob/a6b840794194fa58fb0aaf5b34d3a86d48cbcc6b/examples/embedded/src/main.zig), [tests](https://github.com/knots-ui/knots/blob/a6b840794194fa58fb0aaf5b34d3a86d48cbcc6b/tests/embedded_view_consumer.zig).

**Применение к FLUI.** Если выбираем рынок embedded UI для существующих редакторов и движков, нужен один настоящий foreign-host consumer. Проверяются window, device, event loop, input, focus, IME, accessibility и teardown вместе. Renderer trait без такого consumer ещё не снимает интеграционную сложность приложения.

**Проверка ценности:** host встраивает FLUI без второго GPU device/event loop и без доступа к внутренним render nodes; отказ extension и потеря/замена surface имеют явную судьбу. Сохраняем owned `SceneSnapshot` и наши границы потоков.

## Tessera

### Измерение и размещение имеют разную цену

Наша интерпретация задачи Tessera: дать автору UI менять положение и создавать нестандартные эффекты, не оплачивая лишнее измерение и изменения центрального renderer.

`MeasureScope::placement` включает фазу `Place`; чтения state внутри неё должны инвалидировать placement, не measurement. `PlacementScope` использует готовые размеры детей. `LayoutChild::probe` кэширует измерение под constraints, а intrinsic probes изолируют metadata и snapshots. Это содержательный механизм, но он добавляет тонкие зависимости и требования к корректности cache. [Probe и intrinsic](https://github.com/tessera-ui/tessera/blob/a54736922f98066d0fd624eb9e12fa9158251eed/tessera-ui/src/layout.rs#L81-L140), [placement](https://github.com/tessera-ui/tessera/blob/a54736922f98066d0fd624eb9e12fa9158251eed/tessera-ui/src/layout.rs#L264-L275).

Для graphics extensions типизированные `DrawCommand` маршрутизируются в `DrawablePipeline<T>` с frame/pass/draw lifecycle и batches. Собственные компоненты используют этот же контракт. Однако plugin по-прежнему связан с GPU lifecycle; clipping, opacity и композиция требуют проверки. [Pipeline lifecycle](https://github.com/tessera-ui/tessera/blob/a54736922f98066d0fd624eb9e12fa9158251eed/tessera-ui/src/renderer/drawer/pipeline.rs#L204-L319), [реальный rectangle pipeline](https://github.com/tessera-ui/tessera/blob/a54736922f98066d0fd624eb9e12fa9158251eed/tessera-components/src/pipelines/simple_rect/pipeline.rs).

**Применение к FLUI.** Отдельную placement-фазу оценивать относительно наших caches и explicit transitions. Для расширений renderer проверить один полезный внешний эффект через допустимую SDK-поверхность; публичный wgpu нельзя раскрывать в обход существующей API policy.

**Проверка ценности:** сравнить фактические child measures при alignment, scrolling и reorder; отдельно проверить hit-testing и semantics. Paint transform и layout placement решают разные задачи. При отсутствии выигрыша дополнительная фаза не нужна.

## Что у FLUI уже есть

Обзор соседей не означает, что FLUI требуется заново построить их возможности.

| Область | Подтверждённая реализация FLUI | Реальная следующая задача |
|---|---|---|
| Запуск | CLI, templates, doctor, run, devices, build | Упростить и проверить весь внешний consumer path |
| Reactive UI | Signals подписывают elements в build; explicit transitions обновляют render objects напрямую | Исследовать удобство обычных property bindings и диагностировать стоимость |
| Layout | Clean child с неизменными constraints возвращает cached geometry | Измерить, что добавит отдельная placement-фаза |
| Hot reload | Host/types/logic split, state preservation, отдельный hook | Сделать режимы и судьбу state понятными и проверяемыми |
| Testing | Настоящий runtime pump, a11y queries, semantics actions, replay и controlled clock | Соединить с preview и инструментами без второй реализации runtime |
| Devtools | Phase profiling, timeline, counters, gesture overlay, semantics agent и MCP | Видимый widget-tree inspector с объяснением причин |
| Rendering | Owned SceneSnapshot и raster seam | При выбранном рынке — полный foreign-host consumer |
| Приложение | Notes acceptance app, native wrapper и fault variants | Сделать из существующего consumer доступный showcase и учебный путь |

Источники: [CLI](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-cli/src/main.rs#L108), [signals](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-view/src/reactive/mod.rs), [transitions](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-widgets/src/transitions/mod.rs), [тест отсутствия rebuild](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-widgets/tests/transitions.rs#L170), [layout cache](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-rendering/src/pipeline/owner/subtree_arena.rs#L1034), [testing facade](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/src/testing.rs), [devtools limits](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/packages/flui-devtools/README.md#L206), [raster ownership](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/crates/flui-engine/src/raster_owner.rs), [examples](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/examples/README.md).

Две конкретные внешние границы ещё открыты: полноценная дистрибуция — существующие unsigned/debug артефакты не закрывают release signing и установку; add-to-app — foreign render surface не решает lifecycle, input, IME и accessibility внутри чужого окна. [Distribution gap](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/gaps.md#L351-L359), [add-to-app](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/gaps.md#37-add-to-app-no-flui-view-inside-an-existing-native-app).

## Предлагаемый порядок развития

Это кандидаты для решений владельца, а не новая принятая roadmap. Существующие specs сохраняют силу. В частности, release requirements уже описывают Notes, внешний consumer, публикацию, native evidence и первый час разработчика; документ помечен draft, поэтому его календарь нельзя выдавать за подтверждённый текущий статус. [Release requirements](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/docs/plans/specs/release/requirements.md).

### Сначала один законченный пользовательский путь

Использовать Notes как общее приложение для tutorial, тестов, screenshots, profiling и device evidence. Довести выбранную платформу от чистой среды до запуска, редактирования, сохранения, восстановления и распространяемого артефакта. Платформенный охват растёт после повторяемого результата.

Собирать фактические данные: время до первого запуска; число ручных platform-правок; место первой ошибки; время изменения текста/стиля до результата; сохранность черновика после отказа; успешность установки артефакта на другой машине. Исходные значения пока не измерены, обещать ускорение в процентах нельзя.

### Затем видимая диагностика

Создать widget inspector как consumer официального SDK и существующих наблюдений. Выбор элемента должен показывать incoming constraints, получившийся size, padding/transform/clip, hit-test и semantics identity, причину последнего rebuild/layout/paint. В первой версии достаточно объяснения состояния; live editing добавляет отдельные вопросы владения и восстановления.

Проверка — пользователь находит причину overflow или неожиданного rebuild без чтения renderer source. Сам inspector не должен менять lifecycle приложения, удерживать retired nodes или запрашивать production-only public exports без потребителя.

### Связать preview и проверки

Один и тот же экран использовать для приложения, headless tests и rendered preview. Явно различать семантические проверки, pointer dispatch, GPU readback и платформенные прогоны. CLI preview должен переиспользовать текущий runtime; новый упрощённый preview engine создаст расхождение, которого мы хотим избежать.

### Провести ограниченный эксперимент с invalidation

На одной editor-сцене менять цвет, opacity, transform, alignment, width, text и порядок keyed rows. Считать build, layout, child measure и paint; проверять визуальный результат, hit-test, semantics и state. Сравнить существующий idiomatic FLUI с возможным typed binding. Результат эксперимента может быть улучшением diagnostics, расширением специализированного API или отказом от нового механизма.

Для каждой новой оптимизации дополнительно нужны unmount, stale update и panic-сценарии. Снижение числа вызовов не оправдывает потерю accepted work или повреждение identity.

### Embedding и custom rendering привязать к потребителю

Если появляется редактор/движок с существующим GPU loop, выбрать его как второй production consumer и проверить контракт целиком. До этого не расширять public API ради гипотетического рынка. Это более дорогая ветка: вместе с draw pass она включает focus, текстовый ввод, accessibility, surface recovery и shutdown.

## Продвижение через проверяемый результат

Публичная стартовая страница FLUI сейчас начинает с устройства деревьев и архитектурных решений. Это полезно сопровождающему; новый пользователь раньше должен увидеть приложение, которое сможет построить, и условия, на которых оно работает. [Текущая книга](https://vanyastaff.github.io/flui/), [README](https://github.com/vanyastaff/flui/blob/bb618a7f7f1510ff11093109094ba02c937d1970/README.md).

Предлагаемая подача:

1. Показать Notes или небольшой редактор в действии: ввод, список, сохранение, восстановление, клавиатура. Рядом — source, точная версия и проверенная платформа.
2. Дать короткий путь изменения этого же приложения и его запуска. Документация, шаблон и демонстрация должны ссылаться на одну реализацию.
3. Объяснить по одной наблюдаемой пользе: почему scroll остаётся плавным; как inspector находит лишнюю работу; что сохраняет hot reload; как тест ловит потерю черновика.
4. Подкреплять техническую публикацию небольшим воспроизводимым сценарием. Для производительности указывать hardware, сборку, workload и фазы; для надёжности — конкретный отказ и следующее успешное действие.
5. Отдельно показывать экспериментальные платформы и неподтверждённые пути. Общая архитектура сама по себе не доказывает native IME или screen-reader parity.

Рабочая гипотеза позиционирования: FLUI позволяет писать сложные интерфейсы на Rust с предсказуемым layout, явным lifecycle и доступной диагностикой. Чтобы это стало обещанием продукта, каждый пункт должен быть виден в приложении и доказательствах. Формулировки «быстрее всех», «полностью native» и «production-ready везде» из этого исследования не следуют.

Наиболее ценное заимствование у этих проектов — постоянная связь между архитектурным решением, обязанностью, снятой с автора приложения, и демонстрацией результата. Для FLUI эту связь можно усилить уже на существующем фундаменте.
