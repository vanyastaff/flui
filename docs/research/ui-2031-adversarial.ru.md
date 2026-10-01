# UI до 2031 года: проверка безопасности, стоимости и границ API

Исследование на 30 сентября 2026 года. Прогноз до 2031 года — условный, а не
обещание рынка или принятый ADR. Роль записки: проверять привлекательные
архитектурные предложения на отказ, неверное использование и цену. Источники
найдены через Keenable; статья Chrome о fonts повторно прочитана через Firecrawl.

## Тезис

Сильный универсальный API не обязан обещать одинаковый набор эффектов на всех
устройствах. Он обязан сохранять смысл поддерживаемых операций, явно отклонять
неподдерживаемые, ограничивать стоимость и оставаться пригодным после отказа.
Стабильный Scene и один wgpu lowering уменьшают сложность, но не доказывают
безопасность. Generational ID доказывает идентичность живого объекта, но не право
вызвать действие над ним. Эти выводы — инженерная оценка на основе приведённых
ниже исторических случаев и контрактов.

## Что уже доказала история

| Случай | Подтверждённый факт | Урок для FLUI |
|---|---|---|
| Dawn: raw pointer при предполагаемом удержании чужой reference | Chromium описывает use-after-free, где путь ClearBuffer нарушил предположения о lifetime; исправление добавило ref counting [1] | Источник image/texture/font должен жить столько же, сколько его identity или записанная ссылка; комментарий о внешнем владельце недостаточен |
| Dawn: device loss между установкой callback и его очисткой | Отчёт описывает dangling pointer на границе Lost и destroy/callback [1] | Проверять loss одновременно с pending map/upload/callback, а не только успешный restart; отмена обязана завершить обязательства ровно один раз |
| SwiftShader JIT: integer overflow | Chromium описывает overflow при объединении alloca и повторяющиеся варианты compiler bugs [1] | Валидный shader source и safe Rust host не заменяют ограничения compiler input и стоимость compilation |
| FreeType/web fonts | Chrome описывает эксплуатацию CVE-2020-15999, проблемы layout/hinting/table combinations и переход web fonts на Fontations в Chrome 133 [2] | Mature parser полезен, но trust boundary, обновления и malformed corpus остаются; memory safety не гарантирует правильность или bounded work |
| Лимит decoder не означает жёсткую квоту | image crate явно различает strict width/height и best-effort max_alloc, который некоторые decoders не поддерживают [3] | Проверять поддержку конкретного ограничения; после decode независимо проверять размер/объём результата; не рекламировать мягкий лимит как гарантию |

Это случаи из других систем, а не утверждение, что FLUI использует FreeType,
SwiftShader или имеет соответствующие уязвимости. Chromium Rule of 2 отдельно
различает untrusted input, unsafe implementation и high privilege; Rust unsafe
subset требует локально проверяемых инвариантов [4]. WebGPU security considerations
независимо подтверждают необходимость validation до драйвера и fallible global
GPU allocations [5]. Поэтому доверие к wgpu не устраняет ответственность FLUI за
его собственные данные, бюджеты и последствия ошибок.

## Инварианты, не зависящие от сценария будущего

**Валидность и допустимая стоимость различаются.** Геометрически корректная Scene
может быть слишком дорогой для устройства. Host/engine согласуют budget:
snapshot bytes, ops, path segments, glyph misses, upload bytes, pipelines in flight,
offscreen peak bytes и effect passes. Ограничения engine не следует незаметно
вшивать в универсальную грамматику Scene. Iterative traversal устраняет stack
overflow, но десять тысяч вложенных blur/saveLayer всё ещё могут исчерпать работу.
Оценка bounds, transient lifetimes и стоимости должна предшествовать allocation.

**Ни один счётчик не является полным budget.** Sixteen cached textures при 4K RGBA8
могут удерживать около 506 MiB, хотя count невелик. Width/height не ограничивают
число изображений. GPU memory budget не ограничивает tessellation CPU time.
Per-frame budget не ограничивает бесконечную очередь compile/decode. Нужны
отдельные peak, retained и pending budgets, с checked arithmetic и проверкой
device limits. WebGPU допускает memory limiting/watchdog, но не обещает, что они
сохранят отзывчивость конкретного UI в нужные миллисекунды [5].

**Отказ не оплачивает работу, которая не завершилась.** BudgetExceeded,
Unsupported, NotReady, device loss и validation failure не должны стирать damage,
считать новую сцену показанной или уничтожать единственный источник wake debt.
NotReady нужен лишь при реальном deferred consumer; его очередь ограничена,
работа cancellable по realm/surface generation, wake debt переживает замену hook.
Полностью отклонённый кадр оставляет предыдущую согласованную презентацию.
Не следует сначала показывать половину кадра, затем выдавать успешный fallback.

**Тип ресурса несёт смысл.** Texture reference связывает device generation,
dimensions, sample mode, color interpretation, straight/premultiplied alpha и
lifetime. Неподдерживаемая texture usage/format даёт ошибку на границе регистрации.
Разрозненные bool и свободные размеры позволяют описать невозможные сочетания.
Однако carrier type не должен притворяться, что универсально импортирует любой
native video surface: это отдельная capability platform/host.

**Синхронный frame не означает синхронный decode/compile.** Подготовка данных
может быть на IO/tooling edges, выдавая готовый immutable результат к следующему
кадру. Apple подтверждает stalls от streaming allocations на render thread и
необходимость измерять thermal sustained performance [6]; отдельно указывает
runtime shader compilation как источник stutter и описывает offline generation
[7]. Это аргумент за bounded preflight и наблюдаемость cache misses, а не за async
layout/paint или копирование Metal-specific cache API в публичный FLUI API.

**Pixels, semantics и authorization независимы.** Semantic snapshot и typed
actions находятся над renderer. Opaque realm + generation + snapshot revision
защищают от stale target; host дополнительно проверяет principal, capability
действия и live preconditions: enabled/editable, relevant value/version и scope.
Label «Удалить» и увиденная кнопка не дают права удаления. Provenance фиксирует
решение, но не создаёт разрешения. Сохранение text, screenshot и scene refs
должно иметь собственные count/bytes/retention limits и правила redaction.

## Проверка предложений других направлений

FLUI уже имеет agent-protocol под ADR-0095: `flui-protocol` гарантирует, что
element handle не переназначается другому элементу; исчезнувший target отвечает
`gone`. `AgentServer` в `flui-devtools` работает через локальный endpoint с launch
token, feature по умолчанию выключена, release build оставляет server inert.
Это прочитано в [tree.rs](../../crates/flui-protocol/src/tree.rs) и
[agent/mod.rs](../../packages/flui-devtools/src/agent/mod.rs).
Рекомендации ниже касаются расширения
этого контракта для будущих production/remote consumers, а не создания второго
agent adapter и не обвинения существующего debug-only server в отсутствии
предусмотренной им защиты. Token authentication не следует автоматически
переименовывать в fine-grained authorization для будущих внешних действий.

### Durable semantic identity, provenance и replay

Одобряется optional adapter над scene/rendering: snapshots, типизированное
действие, ограниченные доказательства исполнения. Отклоняется process-global
identity и бессрочная история «на будущее». Local generational token с revision
достаточен для действия в живом realm. Durable identity оправдан только
потребителем reconnect/undo и отдельным контрактом переидентификации. Повторно
проигранное действие не должно повторять внешнюю запись автоматически.

Scene replay и action replay — разные операции. Первый восстанавливает рисунок;
второй может изменить файлы, деньги или аккаунт. Нужны отдельные ошибки и
idempotency policy host, а не общий метод `replay()`.

Video streaming не является безопасным дешёвым default: pixels могут содержать
секреты, а framebuffer copies/encode/network расходуют ресурсы. Scene transport
также способен удерживать большие изображения/fonts и раскрывать текст.
Нужно отдельно выбирать redacted semantics, raster stream или scene transport,
с явным scope; renderer не должен самостоятельно принимать это решение.

### Узкий Scene и обратимый внутренний pass plan

Одобряется один wgpu lowering без публичной гипотетической backend hierarchy.
Pass plan оправдан, если вычисляет проверяемые lifetime, dependency regions,
peak memory и стоимость, позволяя отказать до allocation. Если он только
дублирует tree и создаёт ещё один Vec на каждый узел, пользу нужно доказать
профилем. Resource-aware plan остаётся internal, пока нет двух настоящих
потребителей общего публичного контракта.

Capability negotiation не может заканчиваться silent bypass clipping/blur:
«картинка есть» не равно правильной композиции. Разрешённые деградации должны
именоваться и проверяться; секретное fallback-поведение делает consumer API
непредсказуемым. Искусственная универсальность ценой CPU rasterizer, plugin layer
или async machinery не нужна без сценария потребителя и измерения всей стоимости.

## Какие API изменения оправданы сейчас

1. Проверка supported subset дополнительных GPU features и dimensions/usages до
   создания ресурса; Result с конкретной причиной отказа на реальных границах.
2. Texture descriptor, сохраняющий существующие alpha/sampling/dimensions contracts.
3. Правдивость уже существующего frame result: GPU validation error не должен
   становиться successful present и погашать damage.
4. Byte budget transient/retained resources с admission и reclaim; метрики misses,
   compile/allocation stalls и ошибки с рабочим production consumer.

Публичный enum из пяти стадий recorded/validated/submitted/completed/displayed
пока не оправдан без потребителя. Submit не доказывает физического показа;
GPU completion также не означает наблюдаемого отображения. Существующее receipt
следует называть по событию, которое реально наблюдается. Новый async completion
protocol потребует cross-crate ADR и жизненного цикла cancellation.

У engine уже есть исчерпывающий [PresentDisposition](../../crates/flui-engine/src/raster.rs) с `Presented`, `NoDamage`
и `NotShown`; `Presented` документирован как достижение `present()`, а не optical
display confirmation. Следует сохранить и уточнять этот работающий контракт,
добавляя новую стадию только вместе с реальным потребителем и проверкой всех
pacing callers. Предложение не означает, что текущий renderer возвращает bool.

## Условный прогноз и опровергающие проверки

| До 2031 года | Уверенность | Что может опровергнуть/изменить рекомендацию |
|---|---|---|
| Ограничения памяти, времени и энергии останутся существенными | Высокая: fallible GPU heaps, thermal/bandwidth и ARR уже документированы [5][6][8] | Измерения всех целевых устройств покажут отсутствие bottleneck; тогда конкретный budget смягчается, но checked limits остаются |
| AI/remote actions увеличат ценность bounded typed semantic adapter | Средняя; это продуктовый прогноз | Отсутствие production consumer, выигрыша над обычным automation или невозможность обеспечить authorization; тогда adapter не включать |
| Offline/prepared pipelines полезнее compilation в frame | Высокая для дорогих pipelines [7], средняя для FLUI без замеров | Cold/warm latency и память показывают, что подготовки дороже и misses укладываются в deadline; оставить lazy cache |
| Новый публичный backend abstraction не окупится автоматически | Средняя, вывод из текущего одного lowering | Второй реальный backend/consumer с тем же контрактом и measurable product need; тогда пересмотреть ADR |
| Raw shader extension не следует давать недоверенному контенту по умолчанию | Высокая для trust boundary [1][5] | Реальная sandbox/cost model с bounded compilation/execution и fuzzed interface; можно дать отдельную negotiated capability |

Год 2031 не задаёт численный FPS, обязательный HDR или единственный compositor.
Инварианты выше выдерживают разные устройства и UI сценарии; конкретные thresholds
выбираются профилем, а не прогнозом маркетинга.

## Проверки, которые могут опровергнуть красивый дизайн

- Malformed image/font, маленький compressed input с большим decoded output;
  decoder soft limit unsupported; несколько одновременных задач.
- Длинный допустимый path, много glyph misses и вложенные effects; отказ до
  allocation; следующий небольшой кадр успешно показывается.
- Device loss между upload/map/callback; два отказа подряд; cancellation при
  resize/realm close; первый failure остаётся authoritative.
- Stale semantic revision, reuse slot, hidden/disabled action, изменение value
  перед выполнением; никаких действий после отказа authorization.
- Cold pipeline cache, theme/effect combinations, recovery cache invalidation;
  p95/p99 CPU/GPU и memory peak, sustained thermal load, idle energy.

Это матрица adversarial tests для плана; записка не утверждает, что тесты уже
написаны или что перечисленные сценарии уже ломают FLUI.

## Источники

1. [Chromium — WebGPU Technical Report](https://chromium.googlesource.com/chromium/src/+/main/docs/security/research/graphics/webgpu_technical_report.md).
2. [Chrome — Memory safety for web fonts](https://developer.chrome.com/blog/memory-safety-fonts?hl=en), 19 марта 2025; прочитано Keenable и Firecrawl.
3. [image crate — Limits](https://docs.rs/image/latest/image/struct.Limits.html), документация библиотеки; latest меняется, перед реализацией сверить pinned dependency.
4. [Chromium — The Rule Of 2](https://chromium.googlesource.com/chromium/src/+/main/docs/security/rule-of-2.md).
5. [W3C — WebGPU, 14 июля 2026](https://www.w3.org/TR/2026/CRD-webgpu-20260714/), прочитаны security considerations, особенно 2.1.9–2.1.13; это non-normative security discussion спецификации.
6. [Apple — Delivering Optimized Metal Apps and Games](https://developer.apple.com/videos/play/wwdc2019/606/), streaming, memory bandwidth, thermals.
7. [Apple — Target and optimize GPU binaries with Metal 3](https://developer.apple.com/videos/play/wwdc2022/10102/), официальные индексированные excerpts об offline compilation; полный transcript этой страницы отдельно не прочитан.
8. [Android — Adaptive refresh rate](https://source.android.com/docs/core/graphics/arr).

Источники не доказывают безопасность FLUI, наличие driver vulnerability на
конкретной машине или показатели latency будущих устройств. Исследование не
включает сборок, fuzz runs, device tests или benchmark. Внешние исторические
инциденты использованы как проверочные контрпримеры, не как обвинение текущего кода.
