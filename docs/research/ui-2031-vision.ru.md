# Видение FLUI до 2031 года

Основание: состояние checkout после обновления origin/main до `84fc88220`,
исследование источников на 30 сентября 2026 года и независимые разборы трёх
агентов. Горизонт — пять лет. Документ предлагает направление, не утверждает
принятый ADR или неизбежность будущего. «Лучший» здесь означает наиболее
устойчивый к рассмотренным сценариям, а не доказанно оптимальный для любого UI.

## Вывод

FLUI стоит развивать как Rust UI с явным владением, точной семантикой сцены,
ограниченными ресурсами и проверяемыми действиями. Сложность будущего должна
менять host adapters и внутреннее исполнение, сохраняя понятный контракт UI.
Синхронный frame path, realm ownership и wgpu lowering уже подходят этому
направлению. Их следует завершить и проверить, а не менять из-за прогноза.

Особенно ценное улучшение — одинаковая сцена и одинаковые правила clip,
alpha, blend и effects для окна, capture и теста. После этого внутренний
план GPU-проходов может выбирать более дешёвое исполнение, не меняя видимый
результат. Semantics и авторизованные действия остаются самостоятельным
контрактом над renderer. Универсальность означает переносимость смысла и
явное согласование возможностей, а не молчаливое приближение на слабом GPU.

## Арена: предложения и взаимная критика

| Позиция | Сильное предложение | Возражение другой позиции | Принятый результат |
|---|---|---|---|
| Развитие UI | Готовить structured semantic actions для совместной работы пользователя и AI; не связывать действие с пикселями | Stable target не даёт permission; snapshots могут раскрывать данные и удерживать ресурсы; remote Scene требует доставки fonts/images | Развивать существующий protocol только под реальный workflow; авторизация и preconditions у host/application; ограниченные opt-in diagnostics |
| Чистота архитектуры | Узкая GPU-free Scene, один lowering, обратимые caches и pass plan | GPU-free не означает безопасную сериализацию; итеративный tree walk не ограничивает сложность paths, effects и uploads | Scene остаётся локальным контрактом; до lowering проверять количественные бюджеты; transport и custom shaders не добавлять заранее |
| Безопасность и стоимость | Проверяемые descriptors и outcomes, bounded memory/work, отсутствие silent fallbacks | Публичный автомат из пяти стадий кадра усложняет обычное приложение; реальные display callbacks могут не прийти | Использовать существующий PresentDisposition и Result; receipt не называть scanout confirmation. Отдельный completion API — только потребителю с timeout/cancellation |

Фундамент должен допускать игровые/3D surfaces и AI runtime: владение GPU,
ресурсами, временем, input и внешними сервисами необходимо согласовать сейчас.
Выбор конкретного spatial backend или inference provider требует отдельной
проверки этих контрактов. Работа AI runtime доставляет результаты к frame
boundary; синхронный paint не должен ждать inference или network.
Также не принят вариант «сначала только FPS»: быстрый неверный кадр не
становится качественным UI.

Полные позиции и первичные источники:
[развитие UI](ui-2031-innovation.ru.md),
[архитектура](ui-2031-architecture.ru.md),
[критическая проверка](ui-2031-adversarial.ru.md).
Отдельно проверены [Flame, Flutter GPU, AI Toolkit и GenUI](flutter-extensibility-lessons.ru.md):
это существующие пути расширения, которые FLUI должен учитывать сейчас.
[Desktop MCP и тестируемость FLUI](desktop-mcp-flui-testing.ru.md) описывают,
как использовать уже существующий инструмент для semantic и pixel проверок,
не создавая второй automation framework.
[Экосистема wgpu](wgpu-ecosystem-adoption.ru.md) проверена по опубликованным
версиям и dependency requirements; для новых зависимостей нужна проверка
реального сокращения собственной реализации и стоимости frame path.
[Цветовой фундамент](color-foundation-adoption.ru.md) определяет конкретную
миграцию от u8 sRGB к tagged float values и единым alpha/working/output правилам.

## Что вероятно изменится

| Сценарий 2031 | Уверенность | Сигнал, который стоит наблюдать | Реакция FLUI |
|---|---|---|---|
| Обычные окна/mobile/web продолжают требовать сложный текст, доступность, mixed DPI и долгие сессии | Высокая для необходимости этих контрактов; ниже для доли рынка | Реальные пользователи, ошибки IME/focus, target GPU limits, memory plateau | Завершать существующий framework, ограничивать память, проверять каждую платформу |
| AI чаще выполняет ограниченные действия через structured interface | Умеренная; победивший протокол неизвестен | Независимые внедрения WebMCP-подобных инструментов и production-запросы FLUI | Существующий agent protocol, typed requests, stale rejection и host policy; никакого LLM в renderer |
| Различия в display colour, refresh cadence и энергобюджете становятся заметнее | Высокая для неоднородности платформ; HDR для всех не установлен | Клиенты требуют HDR/media, 90/120 Hz, battery и variable cadence | Сквозной color contract по ADR и real host timing; SDR fallback, idle не рендерит |
| Spatial/remote/multimodal UI растёт в отдельных продуктах | Низкая для массового замещения, умеренная для нужды в разных input modalities | Конкретный host и потребитель, измеренная польза, доступность альтернатив | Адаптеры действий и presentation; отдельное решение о 3D/remote, без изменения 2D ядра заранее |

Основания: [платформенные требования](ui-requirements-2026.ru.md),
[Chrome WebMCP preview](https://developer.chrome.com/blog/webmcp-epp),
[draft WebMCP](https://webmachinelearning.github.io/webmcp/),
[W3C XR accessibility requirements](https://www.w3.org/TR/2021/NOTE-xaur-20210825/).
Draft и Working Group Note не выдаются за стандарт или прогноз продаж.
Прогноз AI необходимо проверять также против
[предупреждения Gartner о стоимости и провалах проектов](https://www.gartner.com/en/newsroom/press-releases/2025-06-25-gartner-predicts-over-40-percent-of-agentic-ai-projects-will-be-canceled-by-end-of-2027).

## Сильный API: что уже есть и что улучшать

В FLUI уже есть LifecycleContext capabilities, generational IDs, realm state,
immutable scene vocabulary и общий flui-protocol. ADR-0095 принял часть схемы
и in-process SemanticsAgent; devtools AgentServer — local endpoint с token и
только для debug. Это не пустое место для второго agent API.
[ADR-0095](../adr/ADR-0095-agent-protocol-schema-crate.md) явно отличает
реализованное от Proposed; будущую работу следует вести через этот контракт.

Ниже требования к развитию API, а не новые публичные типы в этом изменении:

- Невалидный размер, неподдерживаемый формат или превышенный бюджет отклоняется
  до wgpu; ошибка доступна через Result, следующий кадр может продолжить работу.
- Texture resource описывается вместе с alpha interpretation, colour space,
  sampling и ownership. Нельзя зарегистрировать nearest и получить linear,
  или передать premultiplied pixels в straight pipeline без явного преобразования.
- Внешние JSON/IPC requests проверяются при исполнении: Rust-типы не валидируют
  wire input автоматически. Realm/element generation проверяет свежесть;
  principal/action policy и live preconditions проверяют право на действие.
- Clip/effect decisions должны выражать поддержанную семантику. Не превращать
  параметр API в обещание, которое renderer принимает и игнорирует.
- Progress и failure debt принадлежит владельцу операции. Accepted request,
  committed state и вызов present различаются; предоставлять новые стадии
  только при нужде клиента, с cancel/timeout и поведением при occlusion.
- Пользователь пакета получает достаточный flui-sdk surface с проверенными
  контрактами. Не создавать микрокрейты, runtime global registries или
  trait hierarchy ради гипотетической переносимости.

## Оптимизация и масштабирование

Сначала сделать корректным ordered replay и lifetime frame resources. Затем
измерять workload, а не оптимизировать число вызовов само по себе:

| Нагрузка | Ограничение | Полезная метрика | Возможная оптимизация после проверки |
|---|---|---|---|
| Большие списки и сложные деревья | CPU build/layout/record и invalidation | Повторная работа и время фаз, размер Scene | Narrow invalidation и reuse на правильной границе |
| Смешанные gradients/images/text | Painter order и upload lifetime | Draw/pass count, upload bytes, frame p95/p99 | Ordered batching, stable stop slices и buffer pools |
| Nested clips/filters | Coverage correctness, live offscreen bytes | Peak memory, extent, copies и bandwidth | Cropped targets с проверенным rebase, reuse pass resources |
| Long-lived text/images и resize | Накопление ресурсов | CPU/GPU plateau, atlas occupancy, eviction/reupload | Byte budgets, reclaimed caches и font/resource ownership |
| HiDPI, rotation, animation | Affine coordinates и fractional edges | Pixel readbacks и quality/time при DPR 1/1.25/1.5/2/3 | Correct quad transforms, scale-aware AA, deliberate static snapping |
| Mobile/web и variable cadence | Power, optional features, deadlines | Idle frames, missed deadlines, supported fallback quality | Host cadence, capability negotiation, fewer unnecessary passes |

Рендер Graphite/Impeller, Sparse Strips и pipeline caches полезны как варианты,
но чужой benchmark не даёт FLUI выигрыша. Основания и ограничения описаны в
[исследовании растеризаторов](engine-rasterizer-references.ru.md) и
[Rust-конкурентов](engine-rust-competitors.ru.md).

## Ошибки, которых стоит опасаться

1. **Недооценить взаимодействие функций.** Отдельно работающие clip, alpha и
   filter могут вместе терять содержимое. Составная матрица важнее счётчика
   pub methods; аудит FLUI уже дал пять различающих regression cases.
2. **Перепутать завершение состояния и отображение.** Это реальная гонка в
   browser automation, а не чисто теоретическое опасение; кейс Chromium с
   re-land и callback cleanup приведён в архитектурной позиции.
3. **Забыть, что слабый GPU и память — обычный пользователь.** Count limit
   не ограничивает bytes. Тысячи paths, glyphs, resizes или nested effects
   способны перегрузить синхронный тракт без единого unsafe.
4. **Выдать метаданные за security boundary.** Semantics label, action hint,
   generation и screenshot не дают разрешения. Prompt injection и stale
   targets не лечатся новым renderer.
5. **Подменить контракт оптимизацией.** ModulateAlpha не всегда эквивалентен
   group opacity; новый colour format не делает pipeline colour-managed;
   dirty UI region не гарантирует содержимое swapchain buffer.
6. **Заморозить неправильный API.** До 1.0 исправлять неверный shape дешевле.
   Unwired public hooks и универсальные параметры создают долг совместимости.
   Компилятор должен запрещать неверное владение и arity; runtime — проверять
   external input и реальные capabilities.
7. **Закрыть развитие из-за неопределённого прогноза.** Нельзя считать 3D,
   AI runtime, HDR или второй rasterizer ненужными из-за отсутствия текущего
   потребителя. Проверить фундамент на таких расширениях необходимо до 1.0;
   выбор реализации опирается на прототип, метрики и межкрейтовый ADR.

## Порядок работы на пять лет

Это ориентир с критериями допуска к следующей работе, не обещание сроков.

До 1.0 необходимо принять и проверить фундамент из
[плана архитектуры](flui-foundation-architecture.ru.md): владение состоянием,
геометрия, цвет и alpha, зависимости effects, ресурсные бюджеты, исходы кадра
и расширение SDK. Отложенная реализация HDR или remote host допустима только
после проверки, что существующий контракт позволяет её добавить без замены
ядра. Отсутствие сегодняшнего потребителя не оправдывает закрепление
заведомо ограниченного формата данных или неявного владения.

- **2026–2027:** закрыть текущие ordering/clip/image/mask gaps, общий window/
  capture path, ограниченные GPU resources и честные errors. До широкой
  стабилизации API — composition readbacks, native smoke и memory stress.
  Уже сейчас реализовать и проверить первые потребители расширений:
  [GPU composition](embedded-gpu-scene.ru.md) и
  [AI provider streaming](ai-runtime-demonstration.ru.md). Managed GPU widget,
  runtime tools и generated widget catalog проверяются отдельно; первый
  пример не означает, что все эти контракты уже завершены.
- **2027–2028:** измеренные performance budgets, mobile/web capability matrix,
  cadence/idle, доступность, live IME и regression artefacts. Оптимизации
  принимаются вместе с image-equivalence и воспроизводимой методикой.
- **2028–2029:** при реальном спросе завершать accepted/proposed границы
  agent protocol, typed application actions, cancellation и policy. Проверить
  пользу независимо от выбранного AI vendor; diagnostics остаются opt-in.
- **2029–2031:** по запросу продукта — реализация HDR presentation, remote или новый
  host; изменения через ADR и эксперимент с ограниченным scope. Ежегодно
  пересматривать сценарии по adoption и измерениям, удалять неподтверждённые
  эксперименты. Не ждать этих дат для требований уже существующего потребителя.

Критерий хорошей архитектуры в 2031 году: новую возможность можно добавить на
нужном слое, цена понятна, неподдерживаемый случай явен, а прежнее приложение
сохраняет поведение. Это проверяемая цель; «гениальность» без таких
доказательств не помогает принять инженерное решение.
