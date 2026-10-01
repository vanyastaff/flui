# Точная композиция clips и effects

Основание: аудит production paths после merge resource contract в
`e8909cfca`. Breaking changes разрешены до 1.0. Это уточнение третьей поставки
[engine foundation plan](engine-foundation-implementation.ru.md), а не обещание
уже работающей точной композиции. Контракты и fixtures принадлежат FLUI.

## Найденные failure scenarios

| Граница | Сценарий и ожидаемый результат | Текущий путь |
|---|---|---|
| Command transform и clip lifetime | Установить clip при translate(10,10), сменить transform и рисовать за его device bounds: пиксели остаются исключёнными до Restore | `LayerDispatcher::with_transform` сохраняет всю state, затем восстанавливает вместе с clip |
| Inline picture replay | Picture с root clip и без Save/Restore вставить через Canvas::draw_picture, затем рисовать за clip: явно определить, изолируется ли replay | Команды flatten-ятся без отдельного scope; balanced depth сам по себе не предотвращает утечку root clip |
| Nested analytic clips | Внешний rrect с большим radius и внутренний с меньшим при одинаковых bounds: внешний corner остаётся исключённым | Один SDF slot заменяется внутренним clip |
| Rotated rect и произвольный path | Diamond AABB corner, notch или even-odd hole остаются исключёнными | Scissor AABB пропускает эти пиксели |
| Elliptical corners | rx=20, ry=5 сохраняют эллипс, не круг radius20 | `resolve_rrect_clip` сводит rx/ry через max |
| Difference | Rect difference удаляет центр из текущего region | Dispatcher предупреждает и не устанавливает clip |
| Window/capture | Одна LayerTree с mask, backdrop и linked follower имеет одинаковую семантику | Публичный capture использует generic visitor без GPU diversion handlers |
| Nested masks | Внутренняя mask выполняется внутри capture внешней | Cached child dispatcher не имеет offscreen context |
| Capture domain | Mask 100x20 при rotation45 требует transformed device extent и обратное отображение shader coordinates | Texture extent вычисляется из local size и максимального scale |
| Backdrop footprint | Явно выбрать input boundary/edge policy; после этого изменения во всём required input влияют на output; sigma_x/y сохраняются | Копируется output region с clamp без отдельного контракта; sigma усредняются |
| Destructive blend coverage | Clear на полупокрытом AA edge частично стирает destination | Texture composite fixed-function Clear игнорирует coverage source |
| Partial presentation | Первый partial frame совпадает с fresh full по всей поверхности | Straddling advanced shapes исправляются только следующим full repaint |

Это результаты чтения исходников. Каждый сценарий требует различающего
readback до признания production defect исправленным; warnings и последующий
исправный кадр не доказывают правильность первого результата.

Первый scope fix уже проверен публичным Canvas → SceneBuilder → HeadlessRenderer:
смена CTM не снимает захваченный clip, SaveLayer возвращает прежний clip,
а sibling Picture не наследует состояние предыдущего display list. Отдельное
удаление каждого исправления ломает соответствующий pixel readback. Это не
доказательство точной path geometry, всего effect traversal или unwind safety.
Canvas::draw_picture встраивает команды в тот же список и этой гарантии не
получает. Его scope contract требует отдельного решения на границе painting/engine
и проверок replay через публичный API; текущий changelog говорит о picture layers.

## Что изменило внешнее исследование

Первичные источники и ограничения собраны в четырёх memo:
[clip coverage](../research/engine-clip-coverage-research.ru.md),
[effect footprints](../research/engine-effect-footprints-research.ru.md),
[damage и validation](../research/engine-damage-validation-research.ru.md),
[GPU resources](../research/engine-mask-resource-research.ru.md).

- AA resolve выполняется после boolean membership в общем sample domain.
  Product нарушает C∩C=C; min не различает непересекающиеся половины одного
  pixel. R8 допустима как финальная coverage, не универсальный операнд цепочки.
- Перед выбором общего raster path сравнить stencil/MSAA и supersampled mask
  на одинаковом quality contract. «Exact» означает точную геометрию принятой
  модели, но не обещание математически точной площади любой кривой.
- Effect allocation, required input, output coverage и presentation damage
  получают отдельные значения. Device AABB не заменяет inverse mapping маски
  и не задаёт оси анизотропного blur.
- Backdrop crop/clamp не объявляется доказанным дефектом сам по себе:
  CSS draft выбирает boundary policy и не имеет consensus по Backdrop Root.
  FLUI сначала определяет собственные границы источника и edge mode.
- Partial promotion выполняется до commit/present; фильтрация уже
  отфильтрованного retained результата не является допустимым input recovery.
- CPU finish и GPU completion различаются; временные masks следуют существующему
  DeviceDomain. Transient attachment применим только к discard-only attachments,
  а не sampled effect textures. Производительность требует измерения на GPU.
- Новые GPU API проверяются до production по всей цепочке adapter → wgpu backend
  → lowering. Например, браузерные immediates уже описаны Chrome, но web backend
  используемого wgpu release ещё не исполняет set_immediates. Native fast path
  не должен становиться обязательным cross-platform контрактом.

Для горизонта 2026–2031 это сценарии развития, а не достоверный прогноз рынка:
больше вложенных эффектов, разных consumers, AI-generated сцен и external GPU
content повышают требования к bounded work, typed refusal и capabilities.
Capture/tooling должны исполнять тот же production contract. Будущие 3D/AI
расширения должны подключаться через явное владение ресурсами и пространствами,
а не через глобальное состояние или незаметный fallback. Их runtime и полный
публичный API этим исследованием ещё не спроектированы.

## Целевая модель

Command transform меняет только CTM. Clip захватывает геометрию и transform
в момент своей команды; смена CTM не восстанавливает clip. Save/Restore
управляют scope и clip ownership явно.

Immutable ClipNode содержит parent, Intersect/Difference, shape, fill rule,
validated transform и AA policy. Rect, RRect с полными rx/ry, RSuperellipse
и owned Path не сводятся к общему bounding rectangle. Saved state хранит
head chain; scissor остаётся только ускорением консервативных bounds.

Lowered coverage различает All, Empty, Scissor, Analytic и owned MaskLease.
Общие masks вычисляют boolean membership каждого sample до resolve coverage:
повторный Intersect(C,C) должен быть идемпотентным. Умножение уже сглаженных
mask alpha нарушает это свойство. Difference использует complement membership
в том же sample domain. Geometry conditioning, projective cases, nonfinite и
f64-to-f32 overflow получают явную policy до upload/allocation. Для inverse
использовать имеющийся glam::DMat4::try_inverse с проверкой finite/precision,
а не новый собственный algebra stack.

Effect input footprint, expanded output и composite clip различаются.
Coverage owner определяет, где clip применяется: content или group composite,
без непреднамеренного двойного AA. Destructive blends используют корректную
coverage interpolation через поддерживаемый GPU path. Недоступная комбинация
возвращает typed error, не silently заменяется bounds или unmasked content.

Mask allocations, chain metadata, tessellation, pass count и submitted retention
учитываются до роста/создания. Один submission owner сохраняет leases до
completion; pool return не доказывает допустимость reuse. Failure не повреждает
scope, первый error сохраняется, следующий кадр реально выполняется.

## Порядок реализации и приёмка

1. Разделить command CTM и clip lifetime; проверить смену на identity и другой
   transform, все текущие clip shapes, Save/Restore и ambient layer offset.
2. Внедрить immutable chain и validated geometry; убрать single-slot overwrite.
   Проверить intersection, idempotence, difference, elliptical corners,
   rotated/skewed shapes, fill rules и fractional/DPR coordinates.
3. Подключить общий window/capture traversal и effect context с вложенными
   captures; один output descriptor и resource owner для всех consumers.
4. Развести effect footprints, capture mappings и composite coverage;
   backdrop halo, anisotropic blur и destructive blends имеют отдельные pixels.
5. Выполнить partial/full equivalence на первом кадре, admission/refusal,
   competing failures и next-frame progress. Измерить mask passes, peak live
   bytes и frame p95/p99; отличать payload budget от driver VRAM.

Public API подключается к production caller в той же поставке. Изменение
cross-crate Scene/paint semantics получает ADR; private engine ownership
фиксируется Mapping decision. Для каждого исправления negative mutation должна
сломать его behavior test. Используются существующие readback families;
перенос тестов другого framework и сравнение 1:1 не являются приёмкой.
