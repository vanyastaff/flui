# Растеризация UI: внешние контракты и направления для FLUI

Дата проверки источников: 30 сентября 2026 года. Горизонт рекомендаций —
2026 год и дальнейшее развитие. Это исследовательская записка, а не принятый
ADR и не обещание совместимости со Skia, Impeller, Compose или SwiftUI.
FLUI сохраняет собственный синхронный тракт кадра и wgpu как GPU-слой.

## Вопросы исследования

1. Как устранить непредсказуемую стоимость создания шейдеров и GPU pipelines,
   сохранив переносимость через wgpu?
2. Какие контракты групповой прозрачности, clip, цвета и промежуточных targets
   должны предшествовать оптимизации offscreen rendering?
3. Как отделить инвалидирование UI, повторную запись команд и damage repaint,
   чтобы ускорение сохраняло изображение?

Keenable использован для поиска и чтения первичных страниц; Firecrawl — для
независимого поиска и извлечения Apple/Khronos. Результаты поиска сторонних
блогов исключены из оснований рекомендаций. Документация описывает заявленные
контракты; она не заменяет измерение FLUI на конкретном GPU.

## Шейдеры и предсказуемое время кадра

**Факты.** Impeller заявляет offline-компиляцию шейдеров и reflection,
предварительное создание pipeline state objects, явные caches и маркировку
GPU-ресурсов. Текущая документация отражает Flutter 3.47 и датирована
21 августа 2026 года. Поддержка платформ неоднородна: Android API 29+
использует Impeller по умолчанию с fallback при отсутствии Vulkan; web
продолжает использовать Skia [1].

Graphite уменьшает число специализированных pipelines, чтобы их можно было
подготовить при запуске. Google описывает независимые Recorders и submission
на GPU main thread; Chromium использует Dawn вместо самостоятельных native
backends. Приведённые улучшения Motionmark относятся к Chrome на M3, а не
к произвольному UI-движку [2].

**Перекрёстная проверка.** В wgpu 30.0.1 driver machine-code generation остаётся
дорогой операцией. `PipelineCache` помогает при последующих запусках, требует
совместимого устройства и сейчас работает только на Vulkan. Большинство
desktop drivers уже имеют собственные caches; приложение не может спросить,
есть ли такой cache [3]. Поэтому проверка WGSL при сборке, offline-перевод
шейдера и подготовка native GPU pipeline — три разных действия.

**Рекомендация для FLUI.** Сначала измерять отдельно создание shader modules,
pipelines, первое выполнение эффекта и повторное выполнение. Подготавливать
частые сочетания формата/coverage/blend до первого интерактивного кадра;
редкие сочетания готовить на разрешённой инфраструктурной границе. Сохранять
Vulkan pipeline cache через backend/device/version key и атомарную замену
файла. Cache miss должен менять время подготовки, а не изображение. Не
обещать отсутствие shader jank на всех backends и не вводить второй backend
ради буквального повторения Impeller.

## Группы, промежуточные targets и порядок команд

**Факты.** Compose различает `Auto`, принудительный `Offscreen` и
`ModulateAlpha`. Последний применяет alpha к каждой команде и может давать
другое изображение для перекрывающихся элементов. Offscreen создаёт texture
размера области рисования и по умолчанию ограничивает содержимое этой
областью. `Clear` без изолированной группы может очистить общий window buffer
вместо локального содержимого [4]. SwiftUI `drawingGroup` сводит поддерево
в offscreen image и явно задаёт рабочий color mode. `opaque=true` требует
alpha=1 [5]. На WWDC26 Apple отдельно поясняет: compositing group определяет
визуальный результат эффектов группы; это не универсальная оптимизация [6].

**Перекрёстная проверка.** У Compose и SwiftUI изоляция группы — семантика.
Следовательно, замена group opacity на per-draw alpha допустима только при
доказанной эквивалентности. Ни один из этих источников не доказывает, что
каждая группа должна получать framebuffer размером всего окна.

**Рекомендация для FLUI.** У всех эффектов должен быть общий контракт входа:
упорядоченное содержимое группы, затем обработка, затем composite. Граф
проходов внутри engine может хранить зависимости чтения/записи, размер
target, origin, halo и срок последнего использования. Это собственный Rust
план кадра, а не новая публичная иерархия классов. Аллокации брать из pool при
replay и освобождать после последнего потребителя; opaque/identity shortcuts
разрешать после pixel-equivalence tests.

Вложенные filters сначала корректно выполнять в общей системе координат;
после этого добавлять bounds analysis и cropping. Для cropped target одной
переносимой операцией менять геометрию, scissors, clip mapping и UV. Проверять
изображение на перекрытии, sibling до и после эффекта, nested opacity,
advanced blend и границе blur halo. Бюджет памяти и число render passes
измерять вместе с качеством, особенно на tile-based mobile GPU.

## Clip, premultiplied alpha и цвет

**Факты.** В Impeller источники blending и обычно sampled textures имеют
premultiplied alpha. Pipeline blends используют аппаратные blend factors;
advanced blends читают backdrop. Framebuffer fetch снижает стоимость, но
fallback требует завершения pass и промежуточных операций [7]. Graphite
описывает depth-only clip draws и отделение shader program от clip-stack
state; opaque draws можно переупорядочивать с depth, translucent сохраняют
порядок [2]. Это конкретная архитектура Graphite, а не обязательный дизайн.

Skia color management отделяет alpha от преобразования цвета: unpremultiply,
linearize, преобразование gamut, destination encode, затем premultiply.
Совпадение пространств позволяет убрать обратные операции [8]. SwiftUI
также явно отделяет working color mode от того, что группа rasterized [5].

**Рекомендация для FLUI.** Зафиксировать типом/контрактом, где texture и shader
output straight или premultiplied. Для premultiplied цвета mask coverage
должна менять RGB и alpha согласованно; белая mask обязана сохранять
полупрозрачный белый источник. Нельзя выбрать straight-alpha blend pipeline
только потому, что opaque тест выглядит правильно.

Clip — пересечение всех активных областей, включая transform и difference;
один SDF slot достаточен лишь для одного clip. Сравнить стоимость mask atlas,
stencil и depth-based подхода на сценах с nested clips, прежде чем выбрать
один. Радиусы rx/ry сохранять независимо. Bounds approximation должна быть
явно обозначена как ограничение, а не считаться полной реализацией path clip.

Нынешнее encoded-space blending FLUI — самостоятельный контракт. Переход
к linear blending/HDR потребует отдельного ADR для всей цепочки: входные
цвета и images, filters, offscreen format, tone mapping и presentation.
Выбор FP16 surface сам по себе не реализует HDR. Проверять dark/midtone
swatches, alpha gradients и colored blur на opaque и transparent backgrounds.

## Инвалидирование, damage и переносимость

**Факты.** Compose отслеживает чтения state по composition/layout/drawing
и повторяет необходимые фазы; изменение state, прочитанного при drawing,
может повторять только drawing [9]. Apple WWDC26 описывает независимое
dependency tracking отдельных views и влияние частых environment updates
на область инвалидирования [6]. Это UI dependency tracking, не контракт
содержимого swapchain image.

Khronos `EGL_EXT_buffer_age` определяет возраст back buffer как число frames
с момента его определения. Age=0 означает undefined contents; при age=N
нужно учитывать изменения за N кадров. Memory pressure и resize способны
сделать содержимое непригодным [10]. Это EGL extension, а не функция wgpu.

**Перекрёстная проверка.** Узкая область UI invalidation не доказывает,
что свежеполученный swapchain image содержит предыдущий кадр. Поэтому
retained target FLUI остаётся корректным решением без достоверного buffer age.
Graphite рассматривает повторное выполнение recordings с translation как
направление развития; статья не обещает бесплатный retained rendering [2].

**Рекомендация для FLUI.** Разделить счётчики build/layout/record/replay,
present и repaint pixels. При движении учитывать старые и новые bounds;
при filters — halo и зависимости backdrop; при ошибке сохранять damage debt.
Сравнивать последовательность partial frames с последовательностью full
frames побайтно вне допустимой AA tolerance, включая failed present,
superseded scene, resize и восстановление device.

Переносимость доказывать матрицей Vulkan/Metal/DX12/WebGPU, а не названием
API. Каждую optional GPU capability проверять и иметь поведенчески заданный
fallback. До оптимизации общего replay объединить scene lowering для окна
и readback: одинаковая scene должна давать одинаковые mask/backdrop/follower
результаты. Native surface/presentation smoke tests остаются отдельными.

## Приоритет и доказательства завершения

| Порядок | Работа | Доказательство |
|---|---|---|
| Сначала | Общая семантика scene и ordered effect input; alpha/clip correctness | Public scene readbacks, тесты красные при возврате дефектного production hunk |
| Затем | Единый plan проходов и bounded intermediates | Nested-effects golden matrix, peak VRAM, число passes/copies и эквивалентность flat/nested |
| Затем | Warmup pipelines и backend cache | First-use и steady-state p50/p95/p99 на hardware; сохранение изображения при cache miss |
| После этого | Phase-specific invalidation и retained replay | Счётчики реально пропущенной работы плюс full-vs-partial sequence readbacks |
| Отдельное решение | Linear color/HDR или compute path rasterization | ADR, portability/fallback matrix и оценка quality/cost на desktop и mobile |

## Первичные источники

1. [Flutter: Impeller rendering engine](https://docs.flutter.dev/perf/impeller).
2. [Google: Introducing Skia Graphite, 8 июля 2025](https://blog.google/chromium/introducing-skia-graphite-chromes/).
3. [wgpu 30.0.1: PipelineCache](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCache.html).
4. [Android: Graphics modifiers и compositing strategies](https://developer.android.com/develop/ui/compose/graphics/draw/modifiers).
5. [Apple: drawingGroup(opaque:colorMode:)](https://developer.apple.com/documentation/swiftui/view/drawinggroup(opaque:colormode:)).
6. [Apple: SwiftUI Group Lab, WWDC26](https://developer.apple.com/videos/play/wwdc2026/8006/), dependency tracking и раздел о groups в 56:13.
7. [Flutter source: Impeller color blending](https://github.com/flutter/flutter/blob/main/docs/engine/impeller/docs/blending.md).
8. [Skia: Color Management](https://skia.org/docs/user/color/).
9. [Android: Jetpack Compose phases](https://developer.android.com/develop/ui/compose/phases).
10. [Khronos: EGL_EXT_buffer_age](https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_buffer_age.txt).

Предел проверки: источники подтверждают внешние контракты и направления,
но не наличие этих возможностей в FLUI и не выигрыш их переноса. Это не
полный обзор GPU-растеризации. Прогнозы Graphite из статьи 2025 года
помечены как направления; их фактическое внедрение в 2026 году здесь не
проверено. Версионно незакреплённый Flutter main может измениться.
