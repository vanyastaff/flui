# Архитектура FLUI на 2026–2031: сохранить контракты, менять механизмы

Проверено 30 сентября 2026 года. Роль в обсуждении — консервативный
архитектор. Прогнозы ниже являются сценариями, а не установленными будущими
фактами. Документ не меняет принятые ADR и не вводит публичные API.

## Тезис

Самое устойчивое устройство UI — узкий контракт наблюдаемого результата,
явное владение состоянием и возможность менять механизм выполнения.
Сохранить GPU-free scene, один wgpu lowering и синхронную транзакцию realm;
форматы цвета, lifetime ресурсов, зависимости эффектов и capability fallback
сделать проверяемыми. Новая технология заслуживает публичный API только
после конкретного потребителя, который не помещается в существующий контракт.

Это не довод против retained rendering, GPU compute или remote UI. Это
требование сначала назвать работу, которую они устраняют, и поведение,
которое обязаны сохранить. Backend abstraction, public render graph и
асинхронные методы на каждой render node не являются страховкой от будущего.

## Исторические ошибки: доказательства и пределы выводов

| Опасный выбор | Первичное свидетельство | Вывод для FLUI |
|---|---|---|
| Специализировать pipeline на каждое сочетание draw/clip | Google описывает у Ganesh взрыв вариантов, compilation jank и GL-centric технический долг; Graphite сокращает pipelines [1] | Измерять число комбинаций; shader/pipeline cache key должен содержать необходимые состояния, без случайного разнообразия |
| Считать callback обработки input подтверждением видимого UI | Chromium исправлял scroll→click race: ScrollEnd приходил раньше compositor frame, и click использовал старую hit-test geometry [2] | Различать принятие действия, commit состояния, submission и фактическое подтверждение presentation |
| Исправить race бесконечным ожиданием display | Та же правка была отменена из-за удержанного callback; reland очищает его при reset и разрешает завершение для невидимой вкладки [2] | У каждого wait должен быть cancellation/lifecycle outcome; hidden window не должен держать obligation навсегда |
| Свести IME к keypress и push-строке | TSF требует pull read/edit под документным lock, включая reentrant asynchronous upgrade [3]; AppKit требует marked range, selected range и character geometry [4] | TextStore и frame commit gate — контракт документа; key events не заменяют composition, reconversion и range queries |
| Считать per-draw alpha дешёвой заменой group alpha | Compose прямо показывает разные результаты на overlapping content [5] | Оптимизация только при доказанной эквивалентности; прозрачность группы нельзя распределить без анализа overlap |
| Заменить формат на FP16 и назвать результат HDR | Skia отделяет transfer/gamut/alpha преобразования [6]; SwiftUI явно задаёт working color mode [7] | End-to-end color contract предшествует HDR; storage format не определяет значение компонент |
| Добавить multiwindow через конкретный backend | Историческое предложение egui отмечает, что API через backend привязывает docking libraries к integration [8] | Window intent и realm ownership держать в host/contracts; scene не должна владеть platform window |
| Называть частичный redraw безопасным без истории буферов | Khronos определяет age=0 как undefined и требует объединять damage истории для старых buffers [9] | Retained target или проверенный buffer-age protocol; узкий UI diff сам по себе не сохраняет swapchain pixels |

Таблица не доказывает, что retained или immediate mode «победил». Обе модели
могут содержать state caches, display lists и изолированные compositing
groups. Публикация egui [8] — историческое обсуждение constraints, а не
описание современной полноты реализации. Статья Chromium о critical path
описывает старый GL/SkPicture-era pipeline и полезна для причинности
layout/paint/compositing, но не служит картой Chromium 2026 [10].

В FLUI эти выводы согласуются с owner-affine realms (ADR-0027), pull TextStore
(ADR-0090) и separation scene/record/replay. Не перенести внутреннюю иерархию
Graphite, AppKit или Flutter: сохранить их проверенные случаи как tests.

Agent protocol уже существует: ADR-0080/ADR-0095, `flui-protocol` и локальный
authenticated debug server официального devtools package. Agent handles
никогда не перепривязываются после исчезновения элемента; in-process backend
использует generational accessibility identity. Следующие предложения
расширяют эти контракты только при новом production workload и не требуют
параллельного протокола идентификаторов.

## Минимальные инварианты API и типов

Ниже предложения для следующей итерации дизайна; названия типов — смысловые
эскизы, не запрос немедленно добавить новые `pub` items.

1. **Realm и presentation владеют состоянием.** Focus, IME, scheduler и keys
   принадлежат realm/presentation. Только typed `Send` сообщения и immutable
   snapshots переходят между executors. Surface/device generation входит
   в каждую отложенную GPU obligation; закрытое окно не воскресает из worker
   completion. Surface и handle source освобождаются в установленном порядке.
2. **Geometry имеет единицы.** Logical/device coordinates и DPR не
   смешиваются. Transform, clip, damage и offscreen rebase потребляют одну
   согласованную систему координат; coverage bounds округляются наружу.
3. **Alpha и color имеют значение.** Straight и premultiplied textures не
   подменяются друг другом; источник и working color space определены.
   Color conversion не умножает alpha повторно. Group effects потребляют
   упорядоченное целое поддерево и composited once согласно своему blend.
4. **Записанная сцена не содержит window/GPU ownership.** Asset reference
   отличается от GPU allocation. Texture/glyph handles не становятся
   пользовательскими идентификаторами. Единственный backend сегодня — wgpu;
   GPU-free test seam проверяет frame host без плагинной архитектуры.
5. **Валидность сцены отличается от доступности выполнения.** Перед lowering
   оценивать operations/path segments/glyph count, uploads, offscreen bytes,
   passes и pending compilations. Host задаёт бюджет; engine возвращает
   обозначенный отказ/NotReady, не молча выключает clip/blur. Числа лимитов
   зависят от workload/device policy, а не навечно зашиваются в язык сцены.
6. **Транзакция имеет конечный исход.** Accepted, committed, submitted,
   displayed, superseded, unavailable и cancelled различимы там, где host
   реально способен их подтвердить. Если backend не даёт display feedback,
   не переименовывать queue submission в displayed. Failed/NotReady не
   стирает damage и wake debt; закрытие владельца завершает ожидающих.
   В engine уже есть исчерпывающий `PresentDisposition` с `Presented`,
   `NoDamage`, `NotShown`; использовать его для существующего pacing.
   Новую стадию добавлять только потребителю, которому нужна дополнительная
   гарантия, и не дублировать outcome в соседнем API. `Presented` фиксирует
   достижение `present()`, а не новый универсальный OS display-ack protocol.
7. **Text input не вторгается в GPU engine.** TextStore отвечает на read/edit,
   selection/composition/range geometry в realm. Engine рисует shaped runs.
   UTF-8/UTF-16 offsets конвертируются на platform boundary; временное
   отсутствие геометрии обозначается, а не заменяется rect=(0,0,0,0).

Ownership-типы предотвращают часть ошибок, но не являются доказательством
ресурсной безопасности. Итеративная прогулка по 10 000 слоям защищает стек;
10 000 nested blur/saveLayer всё ещё способны исчерпать память и время.
Pool и pipeline cache также требуют eviction/cancellation budgets.

## Три будущих мира

| Сценарий до 2031 | Что вероятно потребует изменения механизма | Контракт, который сохраняется | Условие внедрения |
|---|---|---|---|
| Мощный desktop GPU, сложные editors и charts | Compute paths, GPU culling, parallel pure recording | Paint order, exact clips, shaped text, isolated group alpha | Hardware benchmark показывает bottleneck tessellation/overdraw; compute fallback совпадает по поведению |
| Mobile/Web с ограничением памяти, энергии и bandwidth | Bounded intermediates, replay простых recordings вместо raster cache, меньше copies | Полное содержимое фильтра, halo и damage debt | Измерены peak VRAM, battery/thermal proxy и tail latency; изменение памяти не обрезает картинку |
| HDR, remote и agent-mediated UI | Color-managed targets, video transport, semantic action protocol | Realm ownership, asset lifetime, explicit action/presentation outcomes | Есть конкретный HDR display или remote action workload; privacy/resource policy и cross-version tests готовы |

Это три стресс-сценария, не прогноз долей рынка. Один механизм может
обслуживать несколько сценариев. Scene semantics нужны во всех трёх;
конкретная cache strategy или AA implementation — нет.

## Что обратимо и что дорого менять

**Обратимые решения:** внутренний pass plan, bounds analysis, texture pool
policy, compiled-pipeline warmup, выбор stencil/mask/depth для clip,
выбор AA и способ video transport. Pass plan оправдан, если вычисляет
ресурсные lifetimes, dependencies и cost; второй экземпляр дерева с новыми
аллокациями без этих фактов усложняет систему.

**Дорогие контракты:** meaning цветов, text offsets, identity/lifetime
action targets, ownership realm, promises о completion, публичная wire
scene grammar и совместимость replay ресурсов. Их нужно закрепить прежде
публичного API. Не вводить общую backend trait лишь потому, что к 2031
могут появиться другие GPU APIs: wgpu уже служит этой переносимой границей
и native pipeline cache сам имеет backend limits [11].

## Возражения инновационному направлению и минимальный ответ

**Stable semantic identities.** Это уже обеспечивается протоколом выше engine.
Минимальный workload для проверки или нового расширения:
agent читает snapshot, UI reconciles, agent вызывает прежнее действие. Target
нуждается в существующем невозвратном handle и проверяемой precondition.
Если внутренний слот reused, action обязан получить протокольный `gone`,
а не воздействовать на другой control.
Смена любой snapshot revision не обязана отвергать action: target-specific
precondition может безопасно подтвердить неизменившуюся цель.

**Action provenance.** Не нужен бесконечный журнал всех text/image данных.
Нужны bounded transient records с источником действия, outcome и target;
чувствительный текст исключается по умолчанию. Если никто не потребляет эти
records для конкретного audit/debug workload, публичный tracing protocol
подождёт. Transport identity не даёт разрешения выполнять action.

**Deterministic frame transactions.** Полезны порядок действий, committed
snapshot и воспроизводимые errors. Побайтовая детерминированность native
GPU/font rasterization между устройствами не следует из этого. Не делать
presentation barrier обязательной для всех действий: background/hidden
target требует конечный outcome и cancellation, как показывает [2].

**Remote scene replay.** Scene-only transport требует fonts/images,
шaping/version/color соглашения и resource lifetime protocol. GPU-free scene
не означает безопасный переносимый wire формат. Начать с video плюс semantics;
scene replay оправдать измеренной bandwidth/offline задачей и наличием точных
resources у клиента. Не публиковать TextureId/GlyphKey как durable automation
identity. Не отдавать весь scene/resource stream без policy на содержимое.

## Как опровергнуть этот дизайн

- Если реальный второй production renderer нельзя подключить без нарушения
  scene semantics и GPU-free границы, пересмотреть backend seam по ADR.
- Если synchronized realm transaction измеримо ограничивает throughput при
  чистом независимом workload, вынести именно эту работу; не распараллеливать
  mutable lifecycle/layout по предположению.
- Если budget preflight стоит дороже предотвращённой работы, заменить
  expensive estimation на conservative accounting при recording.
- Если video+semantics не выдерживает измеренный remote latency/bandwidth,
  сравнить scene replay на том же workload с полным resource/privacy budget.
- Если retained target bandwidth превосходит full redraw на целевых mobile
  GPUs, переключать plan по измеренному порогу, сохраняя damage correctness.

## Источники и границы проверки

Исследование использовало Keenable и Firecrawl. Первичный rasterizer обзор
и актуальные platform caveats записаны также в
[engine-rasterizer-references.ru.md](engine-rasterizer-references.ru.md).

1. [Google: Skia Graphite architecture](https://blog.google/chromium/introducing-skia-graphite-chromes/), 2025.
2. [Chromium: synthetic input waits on compositor display, pinned commit](https://chromium.googlesource.com/chromium/src/+/b37e23cbd52ba447ab89c6d18f4ea0ecdc3fa5c0).
3. [Microsoft: TSF Document Locks](https://learn.microsoft.com/en-us/windows/win32/tsf/document-locks).
4. [Apple: NSTextInputClient](https://developer.apple.com/documentation/appkit/nstextinputclient).
5. [Android: compositing strategies](https://developer.android.com/develop/ui/compose/graphics/draw/modifiers).
6. [Skia: Color Management](https://skia.org/docs/user/color/).
7. [Apple: drawingGroup working color mode](https://developer.apple.com/documentation/swiftui/view/drawinggroup(opaque:colormode:)).
8. [egui: историческое обсуждение multiple viewports](https://github.com/emilk/egui/discussions/3087).
9. [Khronos: EGL_EXT_buffer_age](https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_buffer_age.txt).
10. [Chromium: The Rendering Critical Path](https://www.chromium.org/developers/the-rendering-critical-path/).
11. [wgpu 30.0.1: PipelineCache](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCache.html).

Проверка источников подтверждает исторические сценарии и внешние контракты.
Она не доказывает исчерпывающую безопасность FLUI, не является benchmark
и не предсказывает фактический стек 2031 года. Здесь не запускались сборки,
live IME, native multiwindow или remote transport experiments.
