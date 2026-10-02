# Границы GPU-эффектов: внешние источники и решения для FLUI

Дата чтения: 2026-10-01. Исследование не является GPU-воспроизведением дефектов FLUI. Первичные страницы прочитаны через Keenable Web и Firecrawl; поисковые сниппеты не используются как единственное доказательство. Код, API и тесты других движков не переносятся.

## Вопросы

1. Как различать область конечного результата, необходимый вход фильтра и физическую область offscreen allocation?
2. В каком пространстве выполнять анизотропный blur, маску и вложенный backdrop при вращении/неравномерном scale?
3. Когда прозрачная область группы всё равно должна изменять destination, и что это означает для clip/damage?

## Первичные источники и их статус

| Источник | Версия / статус при чтении | Проверенное содержание |
|---|---|---|
| [SkImageFilter_Base.h](https://skia.googlesource.com/skia/+/refs/heads/main/src/core/SkImageFilter_Base.h) | Подвижная main; blob `8beea3df6e4dd6c076150575c0870b5c734a2729`; полный файл | `getInputBounds` и `getOutputBounds` имеют разные направления; blur требует входной margin; фильтры, меняющие transparent black, нельзя ограничивать непрозрачным content bounds. |
| [SkBlurImageFilter.cpp](https://skia.googlesource.com/skia/+/refs/heads/main/src/effects/imagefilters/SkBlurImageFilter.cpp) | Подвижная main; blob `459ff48f2e4ada8190e89e2874cef1b04902cba5`; полный файл | Отдельные sigma по осям; нулевой sigma допустим для одномерного blur; desired output расширяется для child input. Конкретная реализация использует ceil(3 sigma), это не универсальный размер любого blur. |
| [SkImageFilterTypes.h](https://skia.googlesource.com/skia/+/refs/heads/main/src/core/SkImageFilterTypes.h) | Подвижная main; blob `c3ee71b69bfeeb47079752330b7ebb791f27fd7e`; прочитан фрагмент до 28000 символов | Parameter/Layer/Device пространства различаются типами; Mapping раскладывает CTM с учётом возможностей фильтра. Не заявляем чтение остатка файла. |
| [Impeller Gaussian blur](https://github.com/flutter/engine/blob/main/impeller/entity/contents/filters/gaussian_blur_filter_contents.cc) | Историческая engine/main, не обещание актуального Flutter monorepo; прочитана страница исходника | `CalculateBlurInfo` различает scaled source space, padding, local padding; rotation применяется к результату, sigma/radius остаются двухкомпонентными. |
| [Filter Effects Level 2](https://drafts.fxtf.org/filter-effects-2/) | Editor’s Draft от 2026-01-23, нестабильный CSS-документ | §2.1: backdrop копируют, фильтруют, затем ограничивают output clip; transforms учитываются обратным отображением; операции фильтров выполняются последовательно. Backdrop Root — отдельное определение доступного источника. |
| [Compositing and Blending Level 1](https://www.w3.org/TR/compositing-1/) | Candidate Recommendation Draft от 2024-03-21 | Porter–Duff формулы, isolated group с пустым начальным backdrop; группы и blending имеют разные этапы. Это математическая проверка, не принятие CSS stacking-context правил FLUI. |

Поиск WebRender через Firecrawl developer search не дал достаточно сильного первичного результата; его не включаем как подтверждение. Ограничение: это шесть страниц, а не исчерпывающий сравнительный аудит GPU-бэкендов.

## Факты, проверенные независимо

**Вход не равен output clip.** Skia прямо расширяет desired output при запросе child input; Filter Effects 2 независимо описывает filtering перед output clipping. Следовательно, копирование ровно output bounds с clamp sampler меняет результат у края. Исключение возможно только как отдельно объявленный контракт input cropping/edge mode, а не как следствие размера texture.

**Sigma по осям не заменяется средним.** Skia допускает x=0 или y=0 и вычисляет bounds по каждой оси; Impeller также хранит Vector2 sigma/radius/padding. Это независимо подтверждает необходимость двумерных параметров. Но одинаковый параметр sigma не гарантирует побайтно одинаковые Gaussian/Kawase алгоритмы.

**CTM не является одним числом resolution scale.** Skia Mapping различает пространства и capability фильтра; Impeller выполняет blur в scaled source space, сохраняя rotation для результата. Поэтому device-AABB capture — допустимый вариант для маски, но для blur локальных осей нельзя автоматически заменить его изотропным blur в device space. Для affine CTM ковариация Gaussian преобразуется как A·diag(sigma_x²,sigma_y²)·Aᵀ; это наше математическое следствие, не цитата API этих движков.

**Transparent source не всегда означает no-op.** В Porter–Duff copy результат равен source; для clear оба фактора нулевые; source-in имеет нулевой destination factor. Значит пустая часть ограниченной группы может очистить destination. Bounds только нарисованных opaque детей недостаточны для покрытия composite. Coverage clip остаётся отдельным ограничителем; при coverage c итог должен смешивать полный результат оператора с прежним destination, а не просто уменьшать alpha source для любого оператора.

## Предлагаемые решения FLUI — собственные, не внешние требования

В существующем приватном IR явно различить: local effect parameters; выбранное пространство вычисления; mapping в parent target; integer allocation origin/extent; desired output; required input footprint; composite coverage. Не добавлять публичный Scene hook и не создавать второй CPU renderer.

Общий GPU traversal должен принимать текущий target/effect context и обслуживать window и публичный capture. Вложенный effect получает доступ к тому же исполнителю и DeviceDomain, а не временный dispatcher с отключёнными handlers. Backdrop читает уже скомпозированную последовательность своего разрешённого parent target: root surface нельзя молча использовать внутри opacity/mask isolation. Точный предел backdrop visibility группы требуется закрепить FLUI-контрактом до реализации; CSS Backdrop Root не принимается автоматически.

Для shader mask допустим integer cover преобразованных bounds в device space. Child raster CTM перебазируется на allocation origin; shader samples через inverse mapping в local mask coordinates. Сингулярный/projective CTM требует явного поведения, без max-scale approximation. Для blur выбрать локальное/промежуточное пространство, сохраняющее обе оси; затем перенести результат через полную матрицу. Footprint вычислять для реального GPU kernel, включая downsample/upsample taps и округление; не копировать Skia 3 sigma в Dual-Kawase без доказательства.

Clip при растеризации входа и clip при composite — разные решения. Output damage не должен обрезать требуемый input halo. Destructive blend coverage определяется границами группы и composite clip, а не union видимых детей. Фильтр color matrix с alpha bias может создать цвет из transparent black; оптимизация по content bounds должна учитывать это свойство.

До первого прохода резервировать известные allocations и metadata; уже submitted работа остаётся собственностью completion owner. После частичного отказа candidate target не становится committed. Следующий допустимый кадр должен завершиться; shared traversal не отменяет текущий протокол first-error/epoch quarantine.

## Различающие GPU-сценарии для FLUI

| Сценарий | Что измерять, чтобы тест не был ложно зелёным |
|---|---|
| Маска 100×20, rotation45°, градиент alpha вдоль local x | Две внутренние точки у противоположных концов длинной оси плюс прозрачный угол device AABB. Constant mask один только crop проверит, но не shader mapping. |
| Backdrop output x=32..64; красная полоса x=28..31 на чёрном фоне | При выбранном source-halo contract пиксель x=33 получает red вклад, x=27 остаётся исходным. При bounded-input contract ожидается выбранный edge mode. Сначала закрепить политику; не считать один вариант универсальным требованием. |
| Blur sigma(8,0) над маленьким белым прямоугольником | Вклад по горизонтали вне rect, отсутствие вклада по вертикали на той же дистанции. Повторить под rotation90° и scale(2,1). |
| Mask → backdrop и backdrop → mask | Background с двумя различными полосами; сравнить shared capture/window content path и явно выбрать источник вложенного backdrop, не только nonblank. |
| Opacity0.5 с двумя перекрывающимися белыми детьми | Перекрытие и одиночная область должны иметь одинаковую group opacity; backdrop внутри группы не должен преждевременно выпустить детей в root. |
| Src/Clear группа с дырой под rounded clip | Пиксель дырки внутри coverage должен измениться, внешний corner остаться прежним; fractional edge должен сохранять непрерывное покрытие. |
| Изменение stripe только вне output, внутри input halo | Partial repaint и full repaint совпадают внутри эффекта; проверять также пиксели вне damage. |
| Отказ allocation вложенного эффекта после ранней submission | Последняя committed картинка сохраняется, first error не заменяется cleanup error; следующий простой валидный кадр даёт ожидаемый green пиксель. |

Это проектируемые регрессии; здесь они не запускались. Эталон Gaussian не должен получаться копией production helper: для axes/coverage использовать аналитические zero/nonzero и точные Porter–Duff ожидания, для общего фильтра — независимый full-frame контроль и mutation proof.

## Проверка общего исполнения эффектов — 2026-10-02

После выбора ordered IR дополнительно прочитаны:

- [Impeller EntityPass, строки 467–499](https://codebrowser.dev/flutter_engine/flutter_engine/flutter/impeller/entity/entity_pass.cc.html): backdrop получает `pass_context.GetTexture()`, а активный pass завершается до чтения. Это исторический снимок engine, не утверждение об актуальном Flutter. Проверяет порядок и источник чтения; не задаёт API FLUI.
- [iced_wgpu 0.14.0, lib.rs](https://docs.rs/iced_wgpu/0.14.0/src/iced_wgpu/lib.rs.html): `present` вызывает `draw` (строка 171), screenshot вызывает тот же `draw` с offscreen view (строка 243); `draw` выполняет общие `prepare` и `render` (146–147). Это аргумент за общий путь представления и capture, но не доказательство поддержки вложенных фильтров в Iced.

Для FLUI выбран общий recording visitor и эффекты внутри существующего ordered IR.
ShaderMask — изолированная группа, затем shader-source blend с child-destination,
затем SrcOver в родителя. Backdrop — чтение текущего replay target в своей позиции
порядка. Такой выбор устраняет временный backend без эффектов и преждевременную
отправку GPU-команд при обходе дерева. Отдельная shader-mask программа не нужна,
если обычный shader lowering сохраняет координаты и полностью покрывает attachment.
Это условие проверяется readback, включая аффинные преобразования и границы.

Текущий этап не объявляет произвольный affine blur реализованным: separable blur
по device x/y корректен при сохраняющем оси преобразовании; вращение локальных
анизотропных осей требует направленных проходов либо другого пространства фильтра.
До такой реализации корректный результат API — типизированный отказ, а не
усреднение sigma или неявная замена направления.

## Проверка wgpu API — 2026-10-02

Live [crates.io API](https://crates.io/api/v1/crates/wgpu) сообщил
`max_stable_version = newest_version = 30.0.1`; workspace lock содержит
wgpu, wgpu-core и naga 30.0.1. [Release notes](https://github.com/gfx-rs/wgpu/releases/tag/v30.0.1)
описывают исправления Vulkan acquire fence, Metal color constants и WebGPU
requestAdapter failure. Обновление dependency для этой работы не требуется.

Сверены [CommandEncoder](https://docs.rs/wgpu/30.0.1/wgpu/struct.CommandEncoder.html)
и [Limits](https://docs.rs/wgpu/30.0.1/wgpu/struct.Limits.html), а также локальные
исходники exact locked версии. Backdrop завершает предшествующий pass, копирует
matching single-sample COPY_SRC attachment в отдельный COPY_DST input и только
затем фильтрует/композитит в том же encoder. Вложенного queue.submit нет;
prepared resources следуют существующему completion owner.

Аффинный gradient ABI сначала увеличивал число attributes до 15. Стандартный
requested limit равен 16, но adapter maximum не заменяет requested device limit.
Соседние geometry fields упакованы в один Float32x4, stop count/offset — Uint32x2:
сохранены прежние 13 attributes вместе с quad, stride 176 и offsets Rust ABI.
GPU recovery fixture запрашивает limit 13; readbacks проверяют все три градиента.
Это проверка требований API, а не свидетельство исполнения на Metal/WebGPU/mobile.
