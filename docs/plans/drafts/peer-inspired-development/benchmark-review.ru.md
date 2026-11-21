# Опубликованные бенчмарки и заявления соседних проектов

Дата проверки источников: 2 ноября 2026 года. Это обзор опубликованных авторами данных и исходников, не результаты наших запусков. Он дополняет [исследование](research.ru.md) и служит входом в [план сравнений FLUI](benchmark-plan.ru.md).

## Как читать доказательства

- Опубликованные числа с кодом и условиями можно воспроизводить, но они не становятся независимым подтверждением.
- Benchmark harness без результатов подтверждает наличие способа измерения.
- Заявление README без сравнительного набора остаётся заявлением автора.
- Демо, число компонентов, размер исходника и FPS не заменяют друг друга. CPU ms/frame, GPU duration, wall frame interval и input-to-present имеют разные границы.

## Raven

В [README, Performance](https://github.com/oscar-soirey/Raven/blob/c185cb5f869c5b83bf87dccaf6f91470942e48f0/README.md#performance) опубликован Release-прогон на двухъядерной виртуальной машине для 12 000 widgets / 27 800 primitives:

| Операция | Опубликованное время |
|---|---:|
| Первый layout | 2–3 мс |
| Полный repaint | 1,3–2,3 мс |
| Layout после изменения одного label | 0,03–0,05 мс |

[tests/benchmark.c](https://github.com/oscar-soirey/Raven/blob/c185cb5f869c5b83bf87dccaf6f91470942e48f0/tests/benchmark.c) по умолчанию использует null backend. Измеряет первый layout, первый paint с rasterization glyphs, 30 повторных repaint и relayout одного label. Для repaint выводятся best и average; диапазон README не следует переименовывать в p95. Headless viewport сделан большим, чтобы ничего не отсекалось. Vulkan-режим использует другое окно и отдельно сообщает render-thread time: это не автоматически та же нагрузка.

Автор пишет, что parallel paint почти не изменил результат на его двух ядрах. Glass example: около 250 мс на кадр, 1100×720, SwiftShader на CPU; аппаратный GPU не измерен. «Один draw call» в null-сценарии не означает измеренную GPU-стоимость.

Другие заявленные достоинства: stripped shared library около 1,3 MB с двумя шрифтами, XML/designer, языковые bindings, docking, node graph и timeline. Размер библиотеки не равен размеру установленного приложения со всеми зависимостями.

**Урок для FLUI:** сравнивать полный repaint и локальное изменение раздельно; проверять parallel mode на разных нагрузках, не приписывать ему ускорение заранее.

## Knots

Найдены [методика и исходники](https://knotsui.com/benchmarks/), [JSON результатов](https://knotsui.com/benchmarks/results.json) и [авторское объяснение](https://knotsui.com/blogs/why-knots.html#performance). Ссылки сайта изменяемые; перед воспроизведением сохранить их содержимое и контрольные суммы. Knots dependency в опубликованном benchmark build.zig.zon закреплена на `71d16770b8f9e28e1fb1374039575267c0751726`; это не HEAD исходников, использованный в общем исследовании.

Условия: Ryzen 5 7600X, Radeon RX 6750 XT, Mesa 26.2.3, Linux/Wayland; окно 1600×1000. Header, scrolling sidebar и grid. Каждая кнопка подаётся каждый кадр, реализации могут отсекать невидимое. 120 warmup frames; 1000 измеряемых кадров для маленьких сцен, 300 для больших; пять повторов. Таблица ниже — медиана средних process CPU ms/frame по повторам, не p50 latency отдельного кадра.

| Кнопок | Knots, CPU мс/кадр | egui | Dear ImGui |
|---:|---:|---:|---:|
| 100 | 0,131 | 0,413 | 0,074 |
| 1 000 | 0,619 | 1,701 | 0,167 |
| 10 000 | 3,585 | 11,237 | 0,803 |
| 53 000 | 9,193 | 56,703 | 3,417 |

При 53 000 кнопок peak RSS: 147,54 MiB / 140,61 MiB / 42,09 MiB соответственно. Все три сообщили 0,0% idle CPU при указанной длительности и точности наблюдения; это не доказательство нулевых wakeups или нулевой мощности.

В JSON пересборки:

| Режим | Knots | egui | Dear ImGui |
|---|---:|---:|---:|
| Обычная debug-пересборка | 2,314 с | 0,669 с | 0,564 с |
| Resident watch + incremental | 0,081 с | Не измерен отдельной строкой | Не измерен отдельной строкой |

Ограничения:

- Авторская витрина сравнивает лучший режим каждого инструмента. Watch-изменение меняет строку, обычный rebuild runner добавляет комментарий: для нашего сравнения нужна одинаковая содержательная правка.
- В коде Knots accessibility отключена; настройки всех адаптеров необходимо сверить до сравнения.
- CPU-время выбрано потому, что egui на этом Wayland host ограничен частотой display даже при запросе vsync off. Отношение CPU не равно отношению FPS.
- У Knots нет сложного shaping, bidi, fallback и IME по [README](https://github.com/knots-ui/knots/blob/a6b840794194fa58fb0aaf5b34d3a86d48cbcc6b/README.md). ASCII-grid не проверяет эту работу.
- Dear ImGui здесь быстрее Knots, а на большой button-сцене Knots занимает больше памяти, чем egui. Эти результаты сохраняются вместе с выигрышами.
- Сводный JSON содержит агрегаты и per-repeat medians, но не полный массив frame samples. Для наших p99 и интервалов сохранять все наблюдения.

**Урок для FLUI:** взять форму воспроизводимого сравнения и расширить её retained/local-update и correctness-проверками, а не принуждать FLUI пересоздавать всё дерево каждый кадр.

## Rosin

[README](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/README.md) заявляет layout около 10 000 nodes за 1,8 мс на M1 MacBook Air и примерно 2,3× относительно Yoga/Taffy; также часто менее 1 мс CPU на кадр, сотни FPS с Vello и возможность уменьшить приложение ниже 5 MB.

В просмотренном дереве найдены [microbench реактивных переменных](https://github.com/sailbrush/rosin/blob/574c09e2e09656d575092ab85d52baa3ac2e520b/rosin-core/examples/bench_vars.rs), stylesheet parsing и selector matching benches. Воспроизводимый комплект именно Rosin/Yoga/Taffy со сценами и raw results не найден. Это ограничение поиска, не доказательство отсутствия материалов где-либо ещё.

**Статус:** layout-цифры — авторское заявление с указанной машиной; коэффициент не подтверждён нами. Проверять на общей геометрической семантике: constraint layout и flexbox поддерживают разные наборы правил.

## WaterUI / Cherenkov

[Cross-engine suite](https://github.com/water-rs/waterui/blob/63aa76bc199a1ff8b8b885dfe5418a7401238580/graphics/cherenkov/bench/README.md) содержит адаптеры Cherenkov, Vello classic/hybrid/CPU и Skia. Документирует CPU encode/submit/wait, настоящие GPU timestamps, completion intervals, p50/p90/p99, capacity sweep, energy/frame, CPU affinity, GPU clocks и thermal conditions.

Особенно полезные правила: не складывать перекрывающиеся этапы в «frame time»; offscreen completion не называть presentation; недоступную метрику помечать причиной; чередовать base/head; различать GPU time и shader-core cycles. Capacity threshold 8,333 мс — заданный бюджет для 120 Hz, не опубликованный достигнутый результат.

[Apple backend comparison harness](https://github.com/water-rs/waterui/blob/63aa76bc199a1ff8b8b885dfe5418a7401238580/backends/apple/benches/issue-282/README.md) прямо сообщает, что принятых результатов ещё нет. Наличие harness не доказывает ускорение WaterUI.

**Урок для FLUI:** перенять определение измеряемых границ, артефакты и контроль условий; отдельно сравнивать whole-app WaterUI и renderer Cherenkov.

## Blinc, Ply, Tessera

| Проект | Чем выделяет себя | Найденные измерительные средства и пределы |
|---|---|---|
| [Blinc](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/README.md) | SDF/glass, springs, custom shaders, 40+ компонентов и 40+ web demos | [Performance guide](https://github.com/project-blinc/Blinc/blob/8bfcc1286d2c9b3724de12747d2b7c1098368d61/docs/book/src/advanced/performance.md): headless cn_profile для build/paint/tab switch, traces и compositor fast-path constraints. Сравнительных опубликованных результатов в просмотренных материалах не найдено |
| [Ply](https://github.com/TheRedDeveloper/ply-engine/blob/52be132d71f8f6af3f55c6b27a584636f71cddf3/README.md) | Простой builder, встроенный inspector, shader playground и live docs | README приводит playground 207 строк, todo 242, Snake 295. Это размер примеров, не скорость; benchmark suite по именам файлов не найден |
| [Tessera](https://github.com/tessera-ui/tessera/blob/b8a29c077b6ee6e20be3d0a3e65cdc88135d83e4/README.md) | Custom shaders, constraint layout, handle-based measurement и profiling | Есть profiler и инструменты анализа. Опубликованная сравнительная таблица в просмотренных README/дереве не найдена |

## Что нельзя заключить

Из этих материалов не следует, что какой-либо проект быстрее FLUI: FLUI в опубликованных таблицах не участвует. Числа Raven без GPU, Rosin layout и Knots process CPU нельзя складывать в общий рейтинг. Сравнение допустимо только после одинаковых сценариев, правильного результата, зафиксированных версий и собственных повторных прогонов.
