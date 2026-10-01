# Геометрические clips и coverage для FLUI

Дата проверки: 1 октября 2026. Это исследование контракта и способов реализации; GPU-код не менялся, измерений и сборок не было. Renderer остаётся pure wgpu. Внешний raster engine или перенос Flutter tests не предлагаются.

## Вопросы и способ проверки

1. Какие правила геометрической принадлежности нужны для intersection, difference и path fill rules?
2. Какие analytic/stencil/MSAA/mask способы сохраняют повторное пересечение и где возникают ошибки AA?
3. Какие механизмы предоставляет используемый wgpu 30, а какие ограничения и владение должен задать FLUI?

Использованы две поисковые выдачи Keenable и шесть чтений страниц Keenable/Firecrawl. Основные страницы прочитаны целиком; WebGPU — большой документ, проверены соответствующие разделы. Поисковые snippets ACM служат только указателями: содержимое платных статей не прочитано и не используется как доказательство. Публикация в 2026 не делает старый алгоритм новым; ниже отдельно указаны действующий API, старые технические основы и наши рекомендации.

## Источники и установленные факты

- **[CSS Masking Level 1](https://www.w3.org/TR/css-masking-1/), §§5, 6.1, 6.2.** Получена опубликованная Candidate Recommendation Draft от 5 августа 2021, не окончательный стандарт. Ancestor clips задают cumulative intersection; silhouette отдельных детей одного SVG clipPath объединяется логическим OR. Геометрия каждого ребёнка концептуально задаёт 1-bit membership с оговоркой AA на границе. Clip не меняет inherent geometry. Это полезная спецификация семантики, но FLUI не обязан воспроизводить CSS effect ordering.
- **[MDN clip-rule](https://developer.mozilla.org/en-US/docs/Web/CSS/clip-rule).** Независимая редакционная проверка W3C: nonzero считает ориентированные пересечения луча, evenodd — чётность. Два вложенных контура одинакового направления дают заполненный центр для nonzero и отверстие для evenodd; противоположные направления дают отверстие в обоих случаях. Это правила path membership, а не opacity.
- **[wgpu 30 MultisampleState](https://docs.rs/wgpu/30.0.0/wgpu/struct.MultisampleState.html).** Реальные поля `count: u32`, `mask: u64`, `alpha_to_coverage_enabled: bool`. Sample mask ограничивает затрагиваемые samples. Alpha-to-coverage создаёт дополнительную mask, AND с primitive coverage; гарантированы крайние значения alpha=0 и alpha=1. Документация не обещает аналитически точную площадь для промежуточной alpha.
- **[wgpu 30 StencilState](https://docs.rs/wgpu/30.0.0/wgpu/struct.StencilState.html).** Реальные поля `front`, `back`, `read_mask`, `write_mask`; stencil является механизмом pipeline state, а не готовым деревом clips. Собственный алгоритм должен управлять stencil reference, reset и ограниченной разрядностью.
- **[WebGPU specification](https://www.w3.org/TR/webgpu/), multisample/depth-stencil/render-pass sections.** Независимая проверка нижнего API: stencil front/back compare и операции keep/zero/replace/invert/increment/decrement; sample mask, multisample count и resolve target. В браузерном API sample count ограничен 1 или 4; native wgpu capability может быть шире, поэтому нельзя распространять native режим на web без проверки. Полученный документ содержит изменения 2026 и остаётся CRD. Также есть ограничения sample builtins для compatibility режима: нельзя обещать одинаковую доступность shader sample mask/sample index на каждом GLES/WebGPU пути.
- **[NVIDIA GPU Gems 3, chapter 25](https://developer.nvidia.com/gpugems/gpugems3/part-iv-image-effects/chapter-25-rendering-vector-art-gpu), §§25.4–25.5.** Историческая техническая основа: stencil invert triangle fans дают odd parity и holes; implicit curves позволяют shader membership; approximate signed distance AA имеет ограничения около triangle boundaries, где применяется MSAA. MSAA использует аппаратно зависимый sample pattern. Это объясняет варианты, но не является современным benchmark и не доказывает производительность FLUI.

## Вывод: объединять геометрию до сглаживания

Это наше математическое следствие контракта intersection, а не цитата о готовом API. Пусть membership в sample `s` равна `A(s)` и `B(s)` из {0,1}. Intersection — `A(s) && B(s)`, difference — `A(s) && !B(s)`. Coverage вычисляется после булевой операции как среднее/интеграл результата по pixel footprint. Тогда `C ∩ C = C` и `C − C = ∅` выполняются до любой выбранной дискретизации.

Перемножение отдельно resolved coverage теряет пространственную корреляцию: при coverage(C)=0.5 повторный clip даёт 0.25 вместо 0.5. Аналогично `cA*(1-cB)` для одинаковых shapes даёт 0.25 вместо пустоты. `min(cA,cB)` восстанавливает идемпотентность, но не общую геометрию: два непересекающихся участка одного pixel по 0.5 имеют intersection 0, а min даёт 0.5. Поэтому ни product, ни min нельзя объявлять точной общей композицией геометрических clips.

Scalar R8 mask допустима как **окончательный resolved результат всего выражения**, либо как намеренно полупрозрачная user mask. Нельзя затем трактовать две независимо resolved R8 textures как точный geometric intersection. Повторное linear sampling окончательной mask тоже меняет фильтр: нужны согласованные pixel centers, integer device origin и определённое правило textureLoad/nearest; offscreen rebases не должны сдвигать mask на полпикселя.

## Сравнение способов — рекомендации, не обещания реализации

| Способ | Где подходит | Что обязательно ограничить |
|---|---|---|
| Scissor | Axis-aligned device rect HardEdge | Rotated/skewed rect AABB лишь conservative rejection; intersection bounds не заменяет membership |
| Analytic shader expression | Rect, полные elliptical RRect radii, ограниченные простые chains | Membership всего expression до AA; approximation AA имеет явный tolerance; depth/operation budget; finite/invertible transform policy |
| Stencil + MSAA | Tessellated paths и булевы операции на общей sample сетке | Fill rule, self-intersections, reset, wrap/overflow, attachment sample-count matching; fixed samples не означают точный area integral |
| Supersampled membership + resolve | Общий bounded expression, переносимый quality path | Общая sample lattice для всех nodes; device-space tolerance при flatten curves; CPU/GPU work и mask bytes до allocation |
| Resolved mask | Повторное использование готового полного clip expression | Cache key включает geometry/fill rule/transform/device origin/DPR/AA policy/epoch; immutable lease до queue completion |

Не выбирать stencil как универсальный механизм до сравнения attachment/pass costs с bounded supersampled mask. Не включать alpha-to-coverage как универсальную замену геометрического AA: он смешивает material alpha и sample membership, а промежуточный mapping не гарантирует требуемую area coverage. Сначала один корректный fallback, затем fast paths с одинаковым контрактом. Существующий lyon (manifest minimum 1.0.16, lockfile 1.0.19 и lyon_tessellation 1.0.22) можно использовать для подготовки paths; tessellation сама по себе не предоставляет lifecycle clip chain, AA boolean composition или resource policy.

## Rust-shaped граница для текущего engine

Предлагается приватный immutable `ClipNode { parent, operation, geometry, captured_transform, behavior }`. Owned geometry сохраняет path fill rule и оба радиуса каждого угла. `Save/Restore` меняет head, command CTM override меняет только CTM; сохранённый clip не следует за последующими transforms. Seal делает node payload read-only; draw run захватывает clip reference и resource leases. Lowering выбирает All/Empty/Scissor/Analytic/Mask без изменения painter order.

Текущие точки замены: `state_stack.rs::clip_rect`, `resolve_rrect_clip`, `active_clip`; `painter/transform_clip.rs::clip_path`; `layer_dispatcher.rs` recording и `replay/` consumption. Сейчас AABB path и один rounded slot не могут выразить общий контракт. Новый контракт должен доходить до каждого solid/gradient/image/glyph/tessellation и offscreen composite; новая публичная abstraction без production caller не нужна.

Effect boundary хранит отдельно input clip и final composite clip, с явным владельцем AA coverage. Для destructive blend требуется `out = lerp(dst, blend(src,dst), coverage)`, а не просто умножение alpha src: Clear игнорирует src alpha и иначе стирает весь pixel. Это собственный compositing contract FLUI; dual-source либо destination-read fallback выбираются capability policy. Один clip не должен сглаживаться на child draws и повторно при group composite случайно. Filter bounds inflation и clip ordering должны быть описаны как engine operations, без автоматического принятия CSS порядка.

Собственные обязательства: ограничить node count/depth/path complexity/flatten output/mask bytes/pass count до роста; first typed failure отменяет весь frame, следующий frame восстанавливается; mask allocations удерживаются до GPU completion и не возвращаются в pool раньше. Это не предоставляется wgpu или lyon.

## Приёмка через поведение FLUI

Все строки — дополнения существующих публичных readback families, не перенос чужих tests. Внутренний sampling oracle не должен повторять новый shader predicate.

1. Rotated rect: translate(32,32), rotate45°, clip локальный [-16,-16,32,32], обратный rotate, fill. Sample (12,12) лежит внутри AABB и вне diamond: backdrop сохраняется.
2. Nested rounded: outer [0,0,64,64] radius32, inner тот же bounds radius0. (4,4) сохраняет backdrop; inner не заменяет outer.
3. Elliptical corner rx20/ry5: point (2,8) внутри прямой боковой части; нельзя превращать радиусы в circle radius20.
4. Path triangle (0,0),(64,0),(0,64): (50,50) вне path, хотя внутри bounds. Две same-direction вложенные closed contours различают nonzero/evenodd в центре; reversed inner даёт hole обоим.
5. Difference rectangle: центр отверстия остаётся backdrop, вне отверстия внутри parent виден fill. Повторение AA shape дважды совпадает с одним clip; `C−C` сохраняет backdrop даже на boundary.
6. Два geometry clips, покрывающих противоположные половины одного pixel: intersection пуст; этот случай отвергает ложную min-coverage реализацию. Для точного edge oracle использовать заранее заданные axis-aligned площади; для MSAA — выбранный дискретный контракт, без угадывания аппаратных sample positions.
7. Clip при CTM A, рисование при CTM B, SaveLayer inner clip + RestoreLayer и sibling Picture: captured clip, state restoration и list isolation проверяются отдельно.
8. AA Clear group над opaque red с half-pixel границей: boundary примерно [128,0,0,128] при area-based policy, внешний pixel остаётся red. Для другого AA kernel заранее фиксируется его reference tolerance; полного clear на boundary быть не должно.
9. Mask reuse после offscreen origin rebase и два encodes до одной submit дают те же interior/edge samples. Quota failure, two failures в хронологической конкуренции, recovery и completion retirement проверяются через результат frame и pixels.

## Неопределённости и следующий выбор

Не измерены fast-path crossover, VRAM, p50/p99 record/encode/GPU, AMD/Intel/NVIDIA/native/web coverage, capability fallback. Источники не доказывают оптимальный выбор для FLUI. До реализации general path AA нужно выбрать: математический area contract либо дискретный samples contract с quality/tolerance; максимальную device-space flatten error; разрешённые affine/projective/singular transforms; memory/work caps. Эти решения нельзя скрывать словом «exact».

Нужен небольшой pure-wgpu prototype в существующем engine: axis rect oracle, correlated clips, path holes и destructive composite, затем сравнение stencil-MSAA против общего supersampled mask при одинаковой quality цели. До таких измерений рекомендации о скорости остаются гипотезами. Существующие фиксы CTM/list/SaveLayer scope не зависят от выбора raster strategy и могут поставляться отдельно.
