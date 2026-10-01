# Damage, clips и эффекты: проверенные границы корректности

Дата проверки: 2026-10-01. FLUI: `e8909cfca5e9676d9fd12d8de5dfe5cdababba71`.
Исследование через Keenable Web и Firecrawl; прочитаны полные страницы, а не только поисковые выдержки. Production не менялся, cargo/GPU не запускались. Это рекомендации к плану, не доказательство реализации.

## Что подтверждают источники

| Источник | Проверенный факт | Статус и ограничение |
|---|---|---|
| [EGL_EXT_buffer_age](https://developer.nvidia.com/docs/drive/drive-os/6.0.9/public/drive-os-linux-sdk/api_reference/EGL_EXT_buffer_age.html) | Age 0 означает undefined contents; age N требует учитывать изменения за соответствующую историю кадров. Memory pressure, power events и resize могут уничтожать пригодность прежних contents. | Complete, version 12, 2013-06-13; текст спецификации в SDK NVIDIA 6.0.9. Это EGL, не обещание wgpu surface. |
| [EGL_KHR_partial_update](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_partial_update.txt) | Surface damage сравнивает последовательные изображения поверхности; buffer damage описывает изменения относительно последнего использования конкретного буфера. Это разные множества. | Полный текст Khronos extension; применим как различение понятий, не как API FLUI. |
| [Chromium DamageTracker](https://chromium.googlesource.com/chromium/src/+/main/cc/trees/damage_tracker.cc) | Старые и новые bounds, удалённые surfaces и masks входят в damage. Неизменившиеся layers всё равно рисуются при пересечении damage. Backdrop dependencies учитываются в draw order; pixel-moving backdrop может расширять damage. | Прочитан main, blob `e11eef324eb8c17777d56292b0f092593182f992`; mutable branch, не закреплённая версия Chrome. C++ renderer, не архитектурный шаблон для Rust. |
| [Chromium 41471914](https://issues.chromium.org/41471914) | При partial rendering backdrop читал за пределами damage ранее отфильтрованный результат. Повторная фильтрация переносила его в damage и давала тёмные края. Зафиксированное исправление расширило damage на intersecting pixel-moving backdrop render-pass outputs до отрисовки. | Исторический отчёт 2019, первоначально Chrome 76/macOS; содержит гипотезы обсуждения и итоговый commit, их нельзя смешивать. |
| [CSS Transforms Level 1](https://www.w3.org/TR/css-transforms-1/) | Для non-invertible CTM объект и его содержимое не отображаются; переход через singular scale во время анимации определён явно. | Candidate Recommendation 2019-02-14, не требование совместимости FLUI с CSS. |
| [Filter Effects Level 2](https://drafts.csswg.org/filter-effects-2/) | Описаны paint-order backdrop image, inverse transforms, output clipping и backdrop root boundaries. Для blur указан mirror edge у clipped transformed border box. | Editor’s Draft 2026-01-23; определение Backdrop Root прямо не имеет WG consensus. Нельзя объявлять этот draft единственно правильным контрактом FLUI. |
| [glam Mat4 0.33.7](https://docs.rs/glam/0.33.7/glam/f32/struct.Mat4.html) | `inverse` возвращает invalid matrix для non-invertible input; при `glam_assert` zero determinant может panic. `try_inverse` возвращает Option; `is_finite` — отдельная проверка. | Перепроверена lockfile версия 0.33.7; наличие try_inverse подтверждено также в установленном DMat4 source. Библиотечная математика не заменяет политику допуска геометрии. Дополнительно прочитан 0.30.9, но он не основание для текущей версии FLUI. |
| [lyon FillTessellator 1.0.22](https://docs.rs/lyon_tessellation/1.0.22/lyon_tessellation/struct.FillTessellator.html), [crate docs](https://docs.rs/lyon_tessellation/1.0.19/lyon_tessellation/) | Tessellator не обрабатывает NaN в inputs. Flattening curves зависит от tolerance, определяющей максимальное отклонение приближения. | NaN ограничение перепроверено по полной странице lockfile версии 1.0.22; tolerance по установленному source. Crate overview первоначально прочитан для 1.0.19. Это не доказательство ограниченности CPU для любого конечного path. Поиск нашёл `GeometryBuilderError::TooManyVertices`, но сам enum не устанавливает общий work budget. |

## Независимая перепроверка ключевых выводов

Требование известного предыдущего изображения подтверждается двумя отдельными первичными текстами EGL: buffer age определяет сохранность, partial update различает buffer и surface damage. Отсюда не следует, что wgpu surface имеет buffer-age API: FLUI должен продолжать опираться на свой committed retained target и полное копирование в surface.

Риск повторного backdrop filtering подтверждён и итоговым историческим исправлением Chromium, и текущим DamageTracker, который требует порядка зависимостей и expansion. Это сильнее первоначальной гипотезы автора issue. Commit исправления: [9ec375f2c2485af85d873791e9a2a32b71d6f903](https://chromium.googlesource.com/chromium/src/+/9ec375f2c2485af85d873791e9a2a32b71d6f903), review [1847675](https://chromium-review.googlesource.com/c/chromium/src/+/1847675). Сам commit отдельно не загружался: его описание прочитано в полном issue.

CSS устанавливает одну явную singular-transform политику; glam независимо подтверждает, что inverse не предоставляет безопасную автоматическую политику. Это не обосновывает универсальный determinant epsilon: uniform scale `1e-4` остаётся invertible, хотя determinant `1e-8`. Маленький determinant и плохая обусловленность — разные свойства.

## Решения для FLUI до реализации

**Первый представленный partial кадр должен равняться fresh full.** Следующий full repaint не исправляет факт неверной презентации. В текущем `renderer.rs` проверка `has_advanced_shape_straddling` происходит после записи; `FrameProtocol::run` blit/commit допускает текущий результат и только выставляет full-next. Это надо заменить доказуемым preflight или повторной записью до commit/present. Full promotion корректна как оптимизационный fallback, если происходит до представления ошибочного кадра.

**Развести четыре области:** локальную геометрию, output coverage, input dependency эффекта и presentation damage. AABB подходит для conservative work culling/damage; он не заменяет clip coverage. Effect bounds должны исходить из того же kernel/transform/boundary policy, что actual lowering. Compose складывает зависимости в порядке passes; viewport-affecting transparent-black filters имеют отдельный охват. Неизменившийся effect может требовать перерасчёта при изменении своего input.

**Не предполагать, что backdrop всегда обязан читать halo вне output rect.** Текущий `apply_backdrop_blur` копирует output rect и использует clamp; CSS draft выбирает другую edge policy, но тоже задаёт boundary. Нужно выбрать FLUI контракт: откуда backdrop читается, где изолируется, как обрабатывает край, какие clips применяются до фильтра и какие после. После выбора damage должен покрывать все input pixels, которые реально читаются, и все output pixels, которые реально записываются. Double-filtering prior retained output недопустимо при любом выборе boundary.

**Validation перед lowering и allocations.** Входные числа и накопленный transform проверяются отдельно: конечные операнды могут переполниться при композиции, при f64→f32 или при inversion. NaN/Inf нельзя передавать lyon или превращать saturating cast в пустой clip/маленькую texture. Рекомендуемая политика: invalid/nonrepresentable geometry — typed frame error; finite singular affine transform — явно определённый empty subtree либо typed unsupported error; projective geometry — поддерживаемая homogeneous модель с определённым поведением у `w=0` либо явный отказ. Identity inverse fallback не доказательство правильного изображения. Использовать имеющийся glam::DMat4::try_inverse и отдельную finite/precision validation, а не новый ручной inverse.

**Обусловленность проверять по ошибке отображения, а не одному epsilon.** Для clip-local mapping нужны finite inverse и сохранение требуемой пиксельной точности при viewport/DPR/translation. Проверять очень малые uniform scales отдельно от anisotropic/sheared почти вырожденных матриц. Точный порог и схема нормализации остаются инженерным решением; приведённые источники не дают универсального численного порога. Не добавлять новый ручной линейный algebra stack вместо зрелого glam.

**Geometry budget ограничивает работу, а не только итоговую память.** Это наша рекомендация из свойства tolerance-dependent flattening, не обещание lyon: лимиты input commands, clip/effect nesting, flattened segments, generated vertices/indices, pass count и суммарных temporary pixels; checked arithmetic до резервирования. Ограниченный geometry builder с typed rejection полезен, но итоговый vertex cap сам по себе не ограничивает предшествующий flattening/intersection CPU. Кооперативный work counter в допустимых точках обработки предпочтительнее обещания deadline у синхронной функции. Если dependency не имеет прерываемого API, это явное остаточное ограничение.

**Упростить отказ, не изображение.** Unknown/unsupported effect или exact clip должен давать typed outcome, а не незаметно меняться на bounding rect или identity transform. Независимые caller-owned raw wgpu hazards и полный GPU lifetime ledger — другая работа; этот документ не объявляет их решёнными.

## Приёмочные readbacks и отказоустойчивость

Все сравнения partial-vs-full выполняются по всему viewport сразу после первого partial frame. Sample-only проверки достаточно для различения конкретной геометрии, но недостаточно для утверждения полной partial эквивалентности.

| Семейство существующих FLUI тестов | Новые входы и наблюдаемая проверка |
|---|---|
| `damage_readback_tests` | Translucent advanced draw пересекает damage; первый present равен fresh full вне и внутри damage. Сценарий должен различать double blending, не только opacity 1. |
| `damage_readback_tests` | Изменение перед backdrop, внутри выбранного input support; изменение после backdrop в draw order. Два соседних/вложенных effects, удаление старого output и propagation через несколько dependencies. |
| `damage_readback_tests` | Изменение только в blur halo, shrinking и magnifying transform, fractional offset/DPR; не смешивать work AABB с exact output clip. Сохранить `a_change_in_a_shrunk_blurs_halo_matches_a_full_frame` и существующие effect/clip removal cases. |
| `clip_layers_read_back_as_the_clip_contract_specifies` | Вложенные rrect/rrect и rrect/squircle; elliptical corners, rotated/skewed rect, concave path и holes/fill rule. Pixel inside обеих AABB, но outside exact intersection отличает фикс от приближения. |
| Existing failure-path families | NaN/Inf input, finite composition overflow, failed inverse, geometry budget, затем следующий valid full/partial. Старые committed pixels сохраняются, damage debt не теряется, первый typed failure authoritative. |

Существующий `a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape` сейчас честно фиксирует approximation. При exact-contract migration его ожидаемое поведение меняется намеренно; нельзя оставить leakage assertion и одновременно объявить exact clips готовыми.

Geometry rejection и transient resource backpressure различаются: невозможный scene не должен бесконечно просить retry. Для отказа после промежуточных submits candidate rollback сохраняет committed image; уже submitted ресурсы продолжают свой completion lifetime. Для двух конкурирующих ошибок нужно отдельное наблюдение первого outcome и следующей успешной операции.

## Ограничения исследования

Поиски: четыре начальных запроса; четыре полных чтения затем адресная перепроверка. Rate-limit/retry loops не было. `https://www.w3.org/TR/filter-effects-2/` вернул 404; старый FXTF draft сообщил перенос в CSSWG, прочитан новый адрес. Docs.rs страницы могут быть обрезаны заданным max_chars; нужные секции проверены, но весь glam reference не объявляется прочитанным целиком.

Нет независимого Rust renderer proof для всей retained/backdrop topology: Rust sources здесь подтверждают реальные input constraints математической и tessellation dependency; Chromium/EGL дают отдельное доказательство repaint hazards. Нет benchmark, GPU запуска, численной оценки condition threshold или универсального complexity bound. Эти ограничения должны оставаться в плане, а не превращаться в обещание готовности.
