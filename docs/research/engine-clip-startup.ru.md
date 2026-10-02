# Стоимость первого clip на WARP

Проверка выполнена после `e3ae05ce9` с той же clip-family, без удаления строк
или ослабления pixel assertions. В CI `37051862629` engine GPU suite заняла
2252,133 с, из них clip-family — 402,534 с. Это время тестов после компиляции,
а не восстановление cache. Windows workspace job отдельно сообщает cache miss.

Локальный probe явно запрашивал fallback adapter DX12: Microsoft Basic Render
Driver, Windows driver 10.0.26100.9549. Три painter на одном device рисовали
64x64 rect с AA clip; отдельно измерялись encode и submit/poll completion.
Старый shader: 9,55 / 12,62 / 13,75 с на completion. Один цикл с uniform sample
count вместо фиксированных 8x8 циклов: примерно 3,2–3,5 с. Итоговая специализация
rect/curves/general: 5,55 / 5,51 / 5,38 мс. Это диагностические образцы,
не статистический benchmark CI; нагрузка хоста и версия WARP отличаются от
GitHub runner. Отложенная компиляция драйвером может входить в submit/completion;
эти числа нельзя называть чистым GPU time.

Семантика сохраняется: прежняя 8x8 сетка для AA, один center sample для hard,
тот же центр для hard листьев в смешанной цепочке. Удалена дублирующая hard
ветка shader. Специализация ограничена тремя вариантами геометрии, с общими
layout/module; неиспользуемые path/corner ветки удаляются compiler constants.
Новых глобальных caches, хранения между тестами или backend-specific raster
путей нет. Лимиты membership work и uniform payload остаются прежними.
Admission учитывает три общих GPU объекта и каждый новый pipeline отдельно.

Создание eager effect pipelines заняло около 0,49 с на первом painter и
0,06–0,08 с на повторных. Изменения этого механизма не включены: они не объясняют
основную воспроизведённую задержку и расширили бы исправление.

В существующий benchmark добавлена группа `clip_first_use_prepare_submit_wait`:
новый painter на каждый sample, общий device, явный fallback по
`FLUI_BENCH_FALLBACK=1`, полное завершение GPU work, варианты rect hard/AA,
curve, path и mixed. Driver caches могут быть тёплыми: это не cold-device startup
и не обычный повторный кадр. Benchmark не заменяет pixel readback tests.

Cache fingerprint (включая лишние установленные Rust toolchains) и отсутствие
Windows cache producer — отдельные проблемы закрытого без merge PR #1409.
Очистка старых cache generations не исправляет их и не объясняет медленное
выполнение уже скомпилированных тестов. Общий CI speedup требует нового
сопоставимого Actions run; локальная проверка этого не доказывает.

Сравнение одной и той же clip-family на локальном WARP (forced fallback только
для диагностического запуска, production adapter policy восстановлена):
старый код 143,196 с, исправленный 32,400 с — примерно в 4,4 раза быстрее в этих
двух запусках. Обе версии прошли все прежние assertions. Это отдельные измерения,
без доверительного интервала; общую длительность CI из них не экстраполируем.

## Сравнение решений

Проверенные источники: [PipelineCompilationOptions wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCompilationOptions.html),
[PipelineCache wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCache.html),
[Graphite в Chromium](https://blog.google/chromium/introducing-skia-graphite-chromes/),
[разбор shader permutations](https://therealmjp.github.io/posts/shader-permutations-part2/),
[обсуждение управления loop unrolling в WGSL](https://github.com/gpuweb/gpuweb/issues/4110),
[wgpu: precompiled shaders](https://github.com/gfx-rs/wgpu/issues/9052).
Последние два источника — обсуждения разработки, не обещание доступного API.

| Решение | Что даёт | Ограничение и решение для FLUI |
| --- | --- | --- |
| Три specialization constants варианта | Удаление неиспользуемых geometry branches через штатный wgpu API | Выбрано для текущего исправления; число pipelines ограничено независимо от числа clip nodes |
| Persistent PipelineCache | Повторное использование результатов компиляции | В wgpu 30.0.1 реализован только для Vulkan; не исправляет DX12/WARP. Отдельно оценивать для Android/Vulkan |
| Управление unrolling | Может ограничить разрастание shader | Портативного стандартного WGSL атрибута для этого нет; uniform bound не гарантирует одинаковую оптимизацию всеми драйверами |
| Кэш готовых clip masks | Убирает повторную растеризацию неизменной цепочки | Не решает first use; требует budget, invalidation и ключа с transform, DPR, target origin/extent и режимом AA |
| Scissor для hard axis-aligned intersection | Позволяет вообще не строить membership mask | Перспективный следующий эксперимент; доказать pixel-center boundaries, transform/reflection, damage и rebased scratch coordinates |
| Depth/stencil clips по мотивам Graphite | Отделяет состояние clip от content pipelines | Большая смена архитектуры; эквивалентность нашей AA Boolean coverage и difference не установлена |
| Precompiled/passthrough shaders | Может уменьшить startup work | Tracking issue не является готовым переносимым контрактом; обход проверки WGSL расширяет обязанности по безопасности и совместимости |

Graphite полезен как источник принципа ограничивать число pipelines и отделять clip
от content shader. Его устройство не доказывает возможность заменить наш membership
pass без изменения пикселей. Исторический разбор permutations объясняет tradeoffs,
но доступность API проверяется по документации используемой версии wgpu.

Следующее уменьшение работы стоит искать в доказуемом scissor fast path и повторном
использовании mask, а не в росте числа shader variants. Сначала нужны отдельные
замеры первого использования и устойчивого кадра, затем readback cases, различающие
ошибочные pixel boundaries. Нужны проверки на DX12 hardware, Vulkan и Metal;
локальный WARP не выбирает универсального победителя.

## Воспроизводимый benchmark

Команда: `cargo bench -p flui-engine --bench offscreen_resource_cache --features testing --profile dev --locked -- clip_first_use_prepare_submit_wait --quick`
при `FLUI_BENCH_FALLBACK=1`. Это короткий Criterion запуск в dev profile,
не полноценная статистическая серия release benchmark.

| Сценарий | Наблюдаемый интервал |
| --- | --- |
| Rect hard | 8,87–9,15 мс |
| Rect AA | 7,67–7,88 мс |
| Curve AA | 2,057–2,071 с |
| Path AA | 2,823–2,896 с |
| Mixed AA | 2,798–2,827 с |

Curve/path остаются дорогими на WARP. Исправление не устраняет всю стоимость
сложного membership shader; ускорение прямоугольника нельзя переносить на них.
Существующая полная GPU suite с обычной adapter policy прошла: 57 tests,
0 skipped, 98,387 с. WARP family comparison выше проверяет сохранение поведения
отдельно от benchmark, который не выполняет readback assertions.

## Следующий эксперимент перед выбором архитектуры

1. Для curve/path измерить один painter с повторными кадрами и новые painter на
   том же device; отдельно записать encode, submission/completion и создание
   pipeline. Это различит стоимость подготовки и повторной работы, насколько
   позволяет отложенная работа драйвера.
2. Проверить scissor-only представление для пересечения hard axis-aligned rects.
   Допускать fast path только при доказанной эквивалентности pixel-center тесту;
   остальные цепочки сохраняют общий membership path.
3. Если повторные маски остаются значимой стоимостью, прототипировать bounded
   cache с явной принадлежностью device и budget. Проверить изменение clip,
   transform, DPR, attachment origin и освобождение ресурсов после ошибки.
4. Сравнить результат на software и hardware adapters. Принимать оптимизацию
   только с неизменными readback assertions и отдельно показанной стоимостью
   памяти; изменение shader не должно просто переносить задержку на первый frame.

Это порядок проверки гипотез, а не утверждение, что перечисленные fast paths уже
реализованы или что cache гарантированно ускорит приложения.
