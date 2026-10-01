# Rust UI rendering: решения для FLUI

Дата исследования: 30 сентября 2026 года. Это ограниченное исследование
архитектуры, а не рейтинг скорости. FLUI и конкуренты не запускались на общей
машине с общей сценой; сравнительных чисел FPS, памяти и задержки здесь нет.

## Вопросы и метод

1. Как отделяются состояние UI, описание сцены и исполнение GPU команд?
2. Как сокращаются повторная работа, память и стоимость больших сцен?
3. Как закрепляются контракты цвета, текста, DPI и воспроизводимых снимков?

Поиск выполнен Keenable и Firecrawl; прочитаны первичные статьи Linebender,
исходники GPUI, egui и Iced, документация Slint. Датированные статьи ниже
подтверждают факты 2026 года. Ссылки на main/master и latest — изменяемые
снимки, прочитанные во время исследования, а не доказательство состояния
конкретного релиза на заданную дату. Документация Slint сама предупреждает
об unreleased next version; её возможности не объявляются выпущенными.

## Что показывают источники

| Проект | Проверенный подход | Что полезно FLUI |
|---|---|---|
| Vello | В январской статье перечислены compute, CPU и hybrid реализации; в апреле Hybrid назван примерно beta quality. Оптимизации включают rect fast paths, glyph caching, gradients, opaque full-tile images и SrcOver layers [1][2]. | Измерять отдельно геометрию, coverage, shading и композицию. Непрозрачные области могут сокращать работу, но не дают права менять порядок полупрозрачных draws. |
| Xilem/Masonry | Декларативные views обновляют retained widgets; апрельская статья сообщает переход Masonry с жёсткого Vello classic на imaging [1][2]. | Отделение UI от рисования оправдано. Подменяемый renderer для FLUI потребует самостоятельного решения и ADR: чужой переход не отменяет текущий контракт wgpu-only. |
| GPUI | Scene содержит paint operations, отдельные массивы primitive kinds, bounds tree и draw order; finish сортирует массивы, BatchIterator объединяет их с учётом порядка [3]. | Порядок — часть IR. Оптимизация batch должна доказывать отсутствие изменения перекрытий, а не полагаться на фиксированный порядок типов. |
| Iced | Geometry Cache повторно использует geometry при одинаковых bounds; clear явно инвалидирует её. Group позволяет совместное использование rendering storage [4]. | Кэш должен иметь ключ зависимости и явную invalidation. Размеры, scale, stroke, shader, clips и generation нельзя заменять одним хешем bytes или адресом без владения. |
| egui | RendererOptions::PREDICTABLE отключает dithering, задаёт один sample и включает software texture filtering для предсказуемых snapshots [5]. | Нужны два режима проверки: точные детерминированные fixtures и production GPU readbacks с ограниченными допусками. Само совпадение golden не доказывает работу обычного sampler. |
| Slint | Документация разделяет backend ОС и renderer; software renderer поддерживает partial и line-by-line rendering, одновременно перечисляя ограничения transform, shadow, clip и текста [6]. | Матрица возможностей должна быть честной и составной: поддержка rect не означает поддержку того же rect с transform, filter и clip. CPU fallback — отдельный продуктовый выбор. |

Текущий README Vello уже описывает CPU, GPU без обязательных compute shaders и
compute renderer в research; CPU/GPU разделяют Sparse Strips и инфраструктуру
[7]. Это наблюдение изменяемого upstream, не основание задним числом менять
названия апрельской статьи или утверждать зрелость конкретного сентябрьского
релиза. Выбирать алгоритм по слову «GPU» недостаточно: важны возможности
целевых адаптеров, стоимость подготовки и workload.

## Цвет и текст

egui discussion объясняет различие между sRGB-encoded bytes, UNorm sampling
и аппаратными sRGB conversions [8]. Это подтверждает необходимость явного
color contract, но обсуждение issue не является стандартом цвета. Для FLUI
нужно сохранить уже принятое encoded-space поведение до отдельного решения:
замена format на sRGB сама по себе не улучшает цвет и может добавить второе
кодирование. В readbacks следует различать загрузку текстуры, gradient
interpolation, alpha premultiplication, offscreen composition и presentation.

В апреле Linebender описывает Glifo как отделение glyph rendering и font
outlines от text layout, с отдельной сложностью color emoji и atlas caching;
проект ещё находится в разработке [1]. Это поддерживает существующее
разделение shaping и rasterization FLUI, а не перенос shaping в engine.
Исследовать Glifo как будущий reusable компонент можно после проверки
контрактов, лицензии и зрелости. Проверки текста должны включать fallback,
emoji, переменный scale, atlas eviction и различие logical/device baseline.

## Предлагаемый порядок улучшений FLUI

Это рекомендации по будущей работе, а не сведения о реализованных изменениях.

1. **Корректный IR и ресурсы кадра.** Сначала закрепить painter order между
   primitives и gradients, неизменяемые slices gradient stops каждого draw,
   lifetime uploaded images и frame-local buffers. GPUI сохраняет порядок
   независимо от primitive storage [3], Iced явно задаёт reuse/invalidation
   [4]. Вместе эти независимые проекты подкрепляют приоритет корректности
   перед batching. Приёмка: смешанные overlapping draws и два разных
   gradient segments должны падать при откате исправления.
2. **Составная матрица команд.** Для shape/image/text указать transform,
   blend, opacity, clip, shader/tile mode и effects. Неподдерживаемая операция
   должна давать диагностируемый результат, а не молча принимать параметры.
   Перечень ограничений Slint [6] и реальные fixes Vello для transformed
   filters/gradient clips [2] независимо показывают важность комбинаций.
3. **Воспроизводимые изображения.** Зафиксировать fonts, device scale,
   target format и fixture assets. Детерминированный режим egui [5] полезен
   как пример, но FLUI также нужны production GPU readbacks. Сохранять
   expected/actual/diff, заранее определять допустимые отклонения границ,
   проверять discriminating pixels и повторное использование renderer.
4. **Измеримые бюджеты памяти и времени.** Собирать CPU record/replay,
   GPU timestamps, upload bytes, allocation high-water mark, atlas occupancy,
   offscreen extent, pass count и retained-target bandwidth. Сценарии:
   длинный список, много градиентов, CJK/emoji text, вложенные clips/filters,
   многократный resize/DPI и смена image contents. Оптимизации Vello [1][2]
   служат гипотезами для измерений, не готовыми обещаниями ускорения FLUI.
5. **Алгоритмы после базовой корректности.** Сравнить имеющийся SDF/lyon/SSAA
   путь с более тесной path caching, tile culling и opaque coverage tracking.
   Sparse Strips рассматривать как исследовательский вариант [7]. Не
   подменять engine Vello/Skia до ADR и общего benchmark набора.

Никакой источник здесь не доказывает, что FLUI быстрее, медленнее или экономнее
другого проекта. Для такого вывода нужны одинаковые сцены, платформы, fonts,
color settings, ограничения качества и опубликованная методика.

## Источники

1. [Linebender in 2026 Q1, 19 апреля 2026](https://linebender.org/blog/tmil-25/).
2. [Linebender in December 2025, 15 января 2026](https://linebender.org/blog/tmil-24/).
3. [GPUI Scene и BatchIterator, изменяемый main](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/scene.rs).
4. [Iced geometry Cache, изменяемый master](https://github.com/iced-rs/iced/blob/master/graphics/src/geometry/cache.rs).
5. [egui wgpu renderer и PREDICTABLE options, изменяемый main](https://github.com/emilk/egui/blob/main/crates/egui-wgpu/src/renderer.rs).
6. [Slint Backends & Renderers, документация с предупреждением unreleased](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/).
7. [Vello README, изменяемый main](https://github.com/linebender/vello).
8. [egui: обсуждение sRGB attachment и encoded-space blending](https://github.com/emilk/egui/issues/6863).
