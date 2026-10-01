# FLUI 2026–2031: сценарии и архитектурные возможности

Дата основания прогноза: 30 сентября 2026 года. Горизонт: 2031 год.
Это исследовательское предложение, не ADR и не обещание roadmap.
Вероятности в процентах не назначаются: имеющиеся источники не позволяют
достоверно калибровать их для FLUI.

## Тезис

Наиболее устойчивое преимущество FLUI — проверяемые контракты состояния,
семантики, действий и представления кадра. Богатый renderer необходим,
но увеличение числа эффектов само по себе не делает framework готовым
к новым способам взаимодействия. Синхронный wgpu тракт стоит сохранить;
автоматизацию, inference, голос и внешние протоколы размещать над ним.

Прочитаны [аудит возможностей](engine-capability-audit.ru.md),
[исследование растеризаторов](engine-rasterizer-references.ru.md) и
[Rust конкуренты](engine-rust-competitors.ru.md). Их найденные ошибки порядка,
clips и ресурсов — действующие задачи; прогноз не должен отодвигать их.

## Подтверждённые основания

Chrome 10 февраля 2026 года объявил ранний preview WebMCP: website может
описать структурированные действия вместо raw DOM actuation [1]. Прочитанный
30 сентября draft WebMCP описывает tools, schemas, pending executions,
cancellation, exposed origins и consequential/untrusted-content hints.
Сам документ явно говорит: это не W3C Standard и не Standards Track [2].
Значит, structured interaction — реальное направление разработки, а
универсальный стабильный протокол 2031 года из этого не следует.

Gartner в июне 2025 года одновременно прогнозировал рост применения agentic
AI и отмену многих проектов из-за стоимости, неясной ценности и слабых risk
controls [3]. Это прогноз аналитической компании, не измеренная доля рынка
2026 года и не доказательство успеха FLUI. Важна противоположность сигналов:
разумно подготовить недорогую границу расширения, дорогое AI ядро — рано.

W3C XAUR перечисляет пользовательские потребности XR: альтернативные input
modalities, согласование устройств, customization и взаимодействие без
обязательных движений тела [4]. Это Working Group Note 2021 года, не норматив
и не прогноз продаж XR. Основание для переносимой модели действий сильнее,
чем основание превращать 2D renderer FLUI в 3D engine.

## Три сценария

| Сценарий к 2031 | Уверенность и основание | Наблюдаемые сигналы | Что опровергнет прогноз |
|---|---|---|---|
| Desktop/mobile/web остаются основой; adaptive UI улучшает привычные окна | Высокая уверенность в необходимости поддерживать эти среды, умеренная в их относительном доминировании. Существующие Rust framework решения обслуживают эти среды; источники не доказывают исчезновение привычного UI. | Потребители FLUI требуют IME, accessibility, DPI, web limits, battery и долгие стабильные сессии; именно эти сценарии определяют поддержку. | Большинство реальных FLUI продуктов требует другого presentation host, а традиционные окна перестают быть существенным workload. |
| AI выполняет ограниченные задачи внутри UI вместе с пользователем | Умеренная уверенность в росте спроса; низкая в конкретном победившем протоколе. Preview Chrome [1] и draft [2] показывают движение, Gartner [3] — серьёзные риски внедрения. | Несколько независимых клиентов требуют discovery typed actions, cancellation, stale-target errors и аудит; появляются interoperable implementations и повторяемые продуктовые результаты. | К 2028–2029 structured action APIs не получают независимых внедрений, integrations остаются дешевле screenshot/DOM automation или прямых backend APIs, спрос FLUI отсутствует. |
| Spatial/multimodal UI растёт в отдельных продуктах | Низкая уверенность в массовости, умеренная в полезности modality-independent actions. XAUR [4] подтверждает потребности, а не масштаб рынка. | Реальные приложения требуют voice/switch/gaze input, host composition, безопасные target selection и fallback на 2D. | Spatial спрос ограничивается отдельными 3D engines; ни один FLUI потребитель не требует этого host, а input adapters решают все задачи без новых core contracts. |

Это сценарии, которые могут сосуществовать. Нельзя складывать их уверенность
или использовать таблицу как статистическую модель рынка.

## Решение с ценностью во всех сценариях

### Уже существует в FLUI

[ADR-0080](../adr/ADR-0080-agent-protocol-desktop-contract.md) фиксирует wire
vocabulary, typed replies, ошибки и never-rebound handles.
[ADR-0095](../adr/ADR-0095-agent-protocol-schema-crate.md) уже добавил
`flui-protocol` с ElementId, WindowId, Node, Tree, ReadQuery, ActionRequest;
`SemanticsAgent` читает и действует через owner inbox. Контракт
ElementId в `crates/flui-protocol/src/tree.rs` прямо запрещает переназначение
handle другому элементу: исчезнувший target отвечает `gone`; in-process
backend использует generational accessibility identity.

`packages/flui-devtools/src/agent/mod.rs` уже реализует optional agent server:
debug-only local named pipe/Unix socket, launch token, bounded request lines,
`windows`/`read`/`act`. Действия исполняет owner на следующем drain/frame
boundary. `crates/flui-app/src/app/dev_agent.rs` связывает его через DevAgentHook,
не добавляя зависимость приложения от devtools package. Desktop/iOS hook
поддерживают; Android/web пока не вызывают его. Закрытое окно отвечает `gone`.
Логи уже исключают labels, values и request lines. Token не защищает от других
процессов того же пользователя — это явно документированный debug threat model.

Следовательно, семантический action adapter не отсутствует и создавать вторую
параллельную модель действий не нужно. Предлагается оценить и при необходимости
расширить существующий контракт ADR-0080/0095 под измеренные потребности.

### Возможные расширения после проверки потребителя

Использовать существующие действия и ошибки; дополнительные business actions,
target preconditions, cancellation или presentation acknowledgments сначала
проверить на существующем owner inbox и протоколе. Никакие из них не объявляются
отсутствующими дефектами на основании одного прогноза. Publication adapter
для нового transport оправдан только production caller и отдельным ADR, если
меняет границу доверия debug-only local server или принятый MCP contract.

Типизированный запрос должен вести к той же проверяемой команде, что keyboard/touch/AT.
Application logic остаётся единственной authority. Renderer рисует
результирующую сцену и не знает об LLM или происхождении запроса.

Rust API должен выражать ownership и допустимые действия: capability handle
получается в lifecycle, observation snapshot неизменяем, write request
валидируется при доставке. Транспорт не получает raw pointers, TextureId,
GlyphKey или права на произвольные callbacks. Realm-scoped opaque targets
имеют generation; повторное использование slab slot не оживляет старое право.
Wire schema требует runtime validation: Rust типы не проверяют внешний JSON.

Прототип оправдан конкретным тестовым workflow: прочитать semantics, изменить
UI, вызвать прежнюю action. Удалённый/заменённый target уже обязан вернуть
`gone`, а не действие для нового узла. Не вводить новый wire код stale-target
без необходимости: ADR-0080 уже задаёт vocabulary. Target-specific precondition
лучше глобального «revision изменилась — отказ»: изменение часов не должно
ломать независимую кнопку. Закрытие realm отменяет pending requests.

Необходимы разные состояния результата: request принят, изменение committed,
кадр displayed. Семантическая транзакция может быть воспроизводимой без
битовой идентичности raster pixels на разных GPU и host fonts. Проверка
displayed нужна только операции, контракт которой зависит от видимого кадра.

## Исторические ошибки, которые легко повторить

1. **Слишком общая абстракция заранее.** Универсальный renderer plugin,
   spatial graph или AI runtime без клиента добавляет lifetime и compatibility
   обязательства. Расширение должно иметь production caller и bounded scope.
2. **Считать визуальное дерево смыслом приложения.** Координаты и shader
   resources не объясняют намерение и право на действие. Semantics и typed
   actions должны быть независимы от batching и GPU allocations.
3. **Выдавать annotations за защиту.** consequential hint помогает клиенту,
   но не заменяет authorization, проверку параметров и правила приложения.
   Untrusted descriptions и content остаются данными, не инструкциями.
4. **Делать запись бесконечной.** Provenance хранить ограниченно и transient;
   не захватывать password, пользовательский текст, images или fonts по
   умолчанию. Opt-in diagnostics имеют budget и явное удаление.
5. **Отождествлять универсальность с одинаковым поведением любой ценой.**
   Mobile/web adapters имеют разные capabilities. Явный отказ лучше молчаливого
   Paint fallback; accessibility alternative лучше обязательного gesture.

## Возражения архитектурной и ресурсной проверки

Ключевое возражение принято: GPU-free Scene ownership не означает готовность
к remote scene transport. Для replay потребуются fonts/images distribution,
версии shaping, color agreement и lifetime protocol; это дополнительная
стоимость privacy, лицензий и совместимости. Начальный remote прототип может
использовать redacted semantics; raster stream согласуется отдельно, поскольку
pixels раскрывают данные и требуют bandwidth/energy. Ни один канал не включать
по умолчанию. Scene transport открывать только при измеренной проблеме
bandwidth/offline playback и наличии точных ресурсов на клиенте.

Generational token и revision проверяют identity/freshness, но не permission
или намерение пользователя. Host отдельно авторизует principal/action class и
при исполнении проверяет enabled/editable/current value. Semantic label и
видимые pixels не дают полномочий.

Agent IDs не должны быть стабильными «навсегда». Стабильность business action
name не равна праву обращаться к старому runtime target. Realm generation и
target-specific validation должны предотвращать действие после reconcile,
window close и slot reuse. Retention snapshots ограничивается budget; аудит
не должен удерживать texture allocations или font blobs на время сессии.

## Последовательность проверки идеи

Сначала закрыть текущие renderer correctness gaps. Затем проверить имеющиеся
agent workflow tests и дополнить только непокрытую таблицу контрактов: inspect, reconcile, validate action,
commit, explicit result. Только после него попробовать один transport
adapter к имеющемуся протоколу без изменений engine. Метрики: correctness, stale rejection,
cancellation, snapshot size, retained memory и latency до commit/display.

Решение продолжать принимается по независимым production consumers и
измеренному преимуществу перед существующим AT/test API. Если преимущество
не найдено, удалить experimental adapter и сохранить улучшенные semantics
контракты. Будущий сценарий не является оправданием вечного unwired pub API.

## Источники

1. [Chrome: WebMCP early preview, 10 февраля 2026](https://developer.chrome.com/blog/webmcp-epp).
2. [WebMCP Draft Community Group Report, прочитан 30 сентября 2026](https://webmachinelearning.github.io/webmcp/). Metadata revision: d61d0e6d297ddb6bff3510b1330dbb215c6ef43c; draft изменяемый.
3. [Gartner: forecast о canceled agentic projects, 25 июня 2025](https://www.gartner.com/en/newsroom/press-releases/2025-06-25-gartner-predicts-over-40-percent-of-agentic-ai-projects-will-be-canceled-by-end-of-2027).
4. [W3C XR Accessibility User Requirements, Working Group Note, 25 августа 2021](https://www.w3.org/TR/2021/NOTE-xaur-20210825/).

Источники найдены/прочитаны через Keenable и Firecrawl. Нормативные свойства,
заявленные направления разработки, аналитические прогнозы и собственные
рекомендации разделены. Числа adoption из Gartner не экстраполируются на 2031.
